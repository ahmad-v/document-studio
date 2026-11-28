//! Bounded structural and low-resolution visual comparison of two PDF files.
//!
//! The raster comparison uses Document Studio's existing Hayro renderer. It is
//! useful for finding visible differences, but inherits that renderer's fidelity
//! limits and is not a pixel-perfect archival or legal comparison.

use crate::{load_pdf_bytes, pdf_properties, render_page_bitmap, PagePixels};
use localwright_core::{read_bounded, Error, ErrorKind, Job, Result};
use std::path::Path;

const MAX_COMPARE_PAGES: usize = 100;
const MAX_COMPARE_TEXT_BYTES: usize = 8 * 1024 * 1024;
const COMPARE_RENDER_EDGE: u16 = 224;
const POINT_TOLERANCE: f32 = 0.01;

pub(crate) fn compare(
    before_path: &Path,
    after_path: &Path,
    job: &Job,
) -> Result<serde_json::Value> {
    let before_bytes = read_bounded(before_path, crate::pdf_workspace::MAX_DOCUMENT_BYTES)?;
    job.check()?;
    let after_bytes = read_bounded(after_path, crate::pdf_workspace::MAX_DOCUMENT_BYTES)?;
    let before_file_bytes = before_bytes.len() as u64;
    let after_file_bytes = after_bytes.len() as u64;
    job.check()?;
    let before = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        load_pdf_bytes(&before_bytes)
    }))
    .map_err(|_| Error::input("PDF parser stopped unexpectedly on the first comparison input"))??;
    let after = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        load_pdf_bytes(&after_bytes)
    }))
    .map_err(|_| {
        Error::input("PDF parser stopped unexpectedly on the second comparison input")
    })??;
    let before_pages = before.get_pages();
    let after_pages = after.get_pages();
    let before_count = before_pages.len();
    let after_count = after_pages.len();
    let page_count = before_count.max(after_count);
    if page_count == 0 || page_count > MAX_COMPARE_PAGES {
        return Err(Error::new(
            ErrorKind::Limit,
            format!("PDF comparison accepts 1 through {MAX_COMPARE_PAGES} pages per input"),
        ));
    }

    let before_render = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::hayro_syntax::Pdf::new(before_bytes)
    }))
    .map_err(|_| Error::input("PDF renderer stopped unexpectedly on the first input"))?
    .map_err(|_| Error::input("The first PDF could not be rendered"))?;
    let after_render = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::hayro_syntax::Pdf::new(after_bytes)
    }))
    .map_err(|_| Error::input("PDF renderer stopped unexpectedly on the second input"))?
    .map_err(|_| Error::input("The second PDF could not be rendered"))?;
    let before_render_pages = before_render.pages();
    let after_render_pages = after_render.pages();
    if before_render_pages.len() != before_count || after_render_pages.len() != after_count {
        return Err(Error::input(
            "PDF parser and renderer disagree about one of the page counts",
        ));
    }

    let mut before_text_bytes = 0usize;
    let mut after_text_bytes = 0usize;
    let mut pages = Vec::with_capacity(page_count);
    let mut changed_pages = 0usize;
    let mut added_pages = 0usize;
    let mut removed_pages = 0usize;

    for number in 1..=page_count {
        job.check()?;
        match (
            before_pages.get(&(number as u32)),
            after_pages.get(&(number as u32)),
        ) {
            (Some(_), Some(_)) => {
                let before_text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    before.extract_text(&[number as u32])
                }))
                .map_err(|_| {
                    Error::input(format!(
                        "PDF text extraction stopped unexpectedly on first page {number}"
                    ))
                })?
                .map_err(|error| {
                    Error::input(format!(
                        "Could not extract text from first PDF page {number}: {error}"
                    ))
                })?;
                let after_text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    after.extract_text(&[number as u32])
                }))
                .map_err(|_| {
                    Error::input(format!(
                        "PDF text extraction stopped unexpectedly on second page {number}"
                    ))
                })?
                .map_err(|error| {
                    Error::input(format!(
                        "Could not extract text from second PDF page {number}: {error}"
                    ))
                })?;
                before_text_bytes = before_text_bytes
                    .checked_add(before_text.len())
                    .filter(|bytes| *bytes <= MAX_COMPARE_TEXT_BYTES)
                    .ok_or_else(|| {
                        Error::new(ErrorKind::Limit, "PDF comparison text exceeds 8 MiB")
                    })?;
                after_text_bytes = after_text_bytes
                    .checked_add(after_text.len())
                    .filter(|bytes| *bytes <= MAX_COMPARE_TEXT_BYTES)
                    .ok_or_else(|| {
                        Error::new(ErrorKind::Limit, "PDF comparison text exceeds 8 MiB")
                    })?;

                let before_page = before_render_pages
                    .get(number - 1)
                    .ok_or_else(|| Error::input("First PDF renderer omitted a page"))?;
                let after_page = after_render_pages
                    .get(number - 1)
                    .ok_or_else(|| Error::input("Second PDF renderer omitted a page"))?;
                let before_dimensions = before_page.render_dimensions();
                let after_dimensions = after_page.render_dimensions();
                let dimensions_equal = point_dimensions_equal(before_dimensions, after_dimensions);
                let before_pixels =
                    render_page_bitmap(before_page, number, COMPARE_RENDER_EDGE, job)?;
                let after_pixels =
                    render_page_bitmap(after_page, number, COMPARE_RENDER_EDGE, job)?;
                let pixel = pixel_metrics(&before_pixels, &after_pixels);
                let render_dimensions_equal = before_pixels.width == after_pixels.width
                    && before_pixels.height == after_pixels.height;
                let text_equal = before_text == after_text;
                let changed = !text_equal
                    || !dimensions_equal
                    || !render_dimensions_equal
                    || pixel.changed_pixels.is_some_and(|count| count > 0);
                changed_pages += usize::from(changed);
                pages.push(serde_json::json!({
                    "page": number,
                    "status": if changed { "changed" } else { "unchanged" },
                    "text_equal": text_equal,
                    "page_dimensions_equal": dimensions_equal,
                    "before_page_points": {"width": before_dimensions.0, "height": before_dimensions.1},
                    "after_page_points": {"width": after_dimensions.0, "height": after_dimensions.1},
                    "before_render_pixels": {"width": before_pixels.width, "height": before_pixels.height},
                    "after_render_pixels": {"width": after_pixels.width, "height": after_pixels.height},
                    "render_dimensions_equal": render_dimensions_equal,
                    "changed_pixels": pixel.changed_pixels,
                    "mean_absolute_rgb_difference": pixel.mean_absolute_rgb_difference,
                }));
            }
            (Some(_), None) => {
                removed_pages += 1;
                changed_pages += 1;
                pages.push(serde_json::json!({"page": number, "status": "removed"}));
            }
            (None, Some(_)) => {
                added_pages += 1;
                changed_pages += 1;
                pages.push(serde_json::json!({"page": number, "status": "added"}));
            }
            (None, None) => return Err(Error::input("PDF page sequence is inconsistent")),
        }
        job.check()?;
    }

    let before_properties = pdf_properties(&before, before_file_bytes);
    let after_properties = pdf_properties(&after, after_file_bytes);
    let metadata_equal = before_properties.version == after_properties.version
        && before_properties.title == after_properties.title
        && before_properties.author == after_properties.author
        && before_properties.subject == after_properties.subject
        && before_properties.creator == after_properties.creator
        && before_properties.producer == after_properties.producer;
    Ok(serde_json::json!({
        "schema_version": 1,
        "comparison": "bounded PDF text, page geometry and rendered raster preview",
        "before": {
            "pages": before_count,
            "file_bytes": before_properties.byte_length,
            "pdf_version": before_properties.version,
            "title": before_properties.title,
            "author": before_properties.author,
            "subject": before_properties.subject,
            "creator": before_properties.creator,
            "producer": before_properties.producer,
        },
        "after": {
            "pages": after_count,
            "file_bytes": after_properties.byte_length,
            "pdf_version": after_properties.version,
            "title": after_properties.title,
            "author": after_properties.author,
            "subject": after_properties.subject,
            "creator": after_properties.creator,
            "producer": after_properties.producer,
        },
        "summary": {
            "page_count_equal": before_count == after_count,
            "document_metadata_equal": metadata_equal,
            "changed_pages": changed_pages,
            "added_pages": added_pages,
            "removed_pages": removed_pages,
            "render_edge_limit": COMPARE_RENDER_EDGE,
            "text_byte_limit_per_document": MAX_COMPARE_TEXT_BYTES,
        },
        "pages": pages,
        "limitations": [
            "Visual results are low-resolution raster comparisons using the bundled Hayro renderer; unsupported PDF features can be omitted or differ.",
            "Text comparison covers extracted text-layer content and does not compare fonts, tags, annotations, forms, bookmarks, attachments, signatures, scripts or full object structure.",
            "PDF parsing and rendering run in-process. Input and output bounds are not an OS sandbox or a hard CPU/memory quota.",
        ],
    }))
}

struct PixelMetrics {
    changed_pixels: Option<u64>,
    mean_absolute_rgb_difference: Option<f64>,
}

fn point_dimensions_equal(left: (f32, f32), right: (f32, f32)) -> bool {
    left.0.is_finite()
        && left.1.is_finite()
        && right.0.is_finite()
        && right.1.is_finite()
        && (left.0 - right.0).abs() <= POINT_TOLERANCE
        && (left.1 - right.1).abs() <= POINT_TOLERANCE
}

fn pixel_metrics(before: &PagePixels, after: &PagePixels) -> PixelMetrics {
    if before.width != after.width
        || before.height != after.height
        || before.pixels.len() != after.pixels.len()
        || before.pixels.len() % 4 != 0
    {
        return PixelMetrics {
            changed_pixels: None,
            mean_absolute_rgb_difference: None,
        };
    }
    let mut changed = 0u64;
    let mut difference = 0u64;
    for (left, right) in before
        .pixels
        .chunks_exact(4)
        .zip(after.pixels.chunks_exact(4))
    {
        let pixel_difference = left[..3]
            .iter()
            .zip(&right[..3])
            .map(|(left, right)| u64::from((*left).abs_diff(*right)))
            .sum::<u64>();
        if pixel_difference > 0 {
            changed += 1;
        }
        difference += pixel_difference;
    }
    let channels = (before.width as u64) * (before.height as u64) * 3;
    PixelMetrics {
        changed_pixels: Some(changed),
        mean_absolute_rgb_difference: Some(if channels == 0 {
            0.0
        } else {
            difference as f64 / channels as f64
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Document, Object};

    fn one_page_pdf(path: &Path, color: &str, width: i64) {
        let mut document = Document::with_version("1.7");
        let parent = document.new_object_id();
        let content = document.add_object(lopdf::Stream::new(
            dictionary! {},
            format!("{color} 0 0 {width} 300 re f").into_bytes(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => parent,
            "Contents" => content,
        });
        document.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
                "MediaBox" => vec![0.into(), 0.into(), width.into(), 300.into()],
            }),
        );
        let root = document.add_object(dictionary! {"Type" => "Catalog", "Pages" => parent});
        document.trailer.set("Root", root);
        document.save(path).unwrap();
    }

    #[test]
    fn report_detects_visual_and_geometry_changes_and_is_repeatable() {
        let directory = tempfile::tempdir().unwrap();
        let before = directory.path().join("before.pdf");
        let visual_change = directory.path().join("visual-change.pdf");
        let geometry_change = directory.path().join("geometry-change.pdf");
        one_page_pdf(&before, "0 0 0 rg", 200);
        one_page_pdf(&visual_change, "1 0 0 rg", 200);
        one_page_pdf(&geometry_change, "0 0 0 rg", 240);

        let visual_report = compare(&before, &visual_change, &Job::new()).unwrap();
        assert_eq!(visual_report["summary"]["changed_pages"], 1);
        assert!(visual_report["summary"]["page_count_equal"]
            .as_bool()
            .unwrap());
        assert_eq!(visual_report["pages"][0]["page_dimensions_equal"], true);
        assert_eq!(visual_report["pages"][0]["status"], "changed");
        assert!(visual_report["pages"][0]["changed_pixels"].is_number());

        let geometry_report = compare(&before, &geometry_change, &Job::new()).unwrap();
        assert_eq!(geometry_report["summary"]["changed_pages"], 1);
        assert_eq!(geometry_report["pages"][0]["page_dimensions_equal"], false);
        assert_eq!(
            geometry_report["pages"][0]["changed_pixels"],
            serde_json::Value::Null
        );

        let repeat = compare(&before, &before, &Job::new()).unwrap();
        assert_eq!(repeat["summary"]["changed_pages"], 0);
        assert!(repeat["pages"][0]["text_equal"].as_bool().unwrap());
        assert_eq!(repeat["pages"][0]["changed_pixels"], 0);
    }

    #[test]
    fn comparison_rejects_over_page_limit_before_rendering() {
        assert_eq!(MAX_COMPARE_PAGES, 100);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("too-many-pages.pdf");
        let mut document = Document::with_version("1.7");
        let parent = document.new_object_id();
        let mut children = Vec::new();
        for _ in 0..=MAX_COMPARE_PAGES {
            children.push(Object::Reference(document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => parent,
            })));
        }
        document.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => children,
                "Count" => (MAX_COMPARE_PAGES + 1) as i64,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            }),
        );
        let root = document.add_object(dictionary! {"Type" => "Catalog", "Pages" => parent});
        document.trailer.set("Root", root);
        document.save(&path).unwrap();
        let error = compare(&path, &path, &Job::new()).unwrap_err();
        assert!(error.message.contains("accepts 1 through 100 pages"));
    }
}
