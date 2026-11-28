//! Page-focused visual PDF comparison using the bounded Hayro preview renderer.
//!
//! This route compares rasterized pages only. It intentionally does not fold in
//! the separate text-layer or document-metadata checks from `pdf_compare`.

use crate::{render_viewer_batch, PagePixels};
use localwright_core::{Error, ErrorKind, Job, Result};
use std::path::{Path, PathBuf};

const MAX_PARAMETER_BYTES: usize = 4096;
const GUTTER_PIXELS: u32 = 12;
const MAX_COMPOSITE_PIXELS: u32 = crate::pdf_workspace::PREVIEW_MAX_PIXELS * 3;

#[derive(Debug)]
struct PageRequest {
    second_pdf: PathBuf,
    first_page: usize,
    second_page: usize,
}

pub(crate) fn compare(
    first_pdf: &Path,
    parameter: &str,
    job: &Job,
) -> Result<(image::RgbaImage, serde_json::Value)> {
    if parameter.len() > MAX_PARAMETER_BYTES {
        return Err(Error::new(
            ErrorKind::Limit,
            "Page comparison parameters exceed 4 KiB",
        ));
    }
    let value: serde_json::Value = serde_json::from_str(parameter)
        .map_err(|error| Error::input(format!("Expected page comparison JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::input("Page comparison parameters must be a JSON object"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "second_pdf" | "first_page" | "second_page"))
    {
        return Err(Error::input(
            "Only second_pdf, first_page and second_page are supported",
        ));
    }
    let second_pdf = object
        .get("second_pdf")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| Error::input("second_pdf must be a non-empty local path"))?;
    let first_page = object
        .get("first_page")
        .and_then(serde_json::Value::as_u64)
        .and_then(|page| usize::try_from(page).ok())
        .ok_or_else(|| Error::input("first_page must be a positive integer"))?;
    let second_page = object
        .get("second_page")
        .and_then(serde_json::Value::as_u64)
        .and_then(|page| usize::try_from(page).ok())
        .ok_or_else(|| Error::input("second_page must be a positive integer"))?;
    let request = PageRequest {
        second_pdf: PathBuf::from(second_pdf),
        first_page,
        second_page,
    };
    if request.second_pdf.as_os_str().is_empty()
        || !request
            .second_pdf
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return Err(Error::input("second_pdf must name a local .pdf file"));
    }
    if request.first_page == 0 || request.second_page == 0 {
        return Err(Error::input(
            "Page numbers are 1-based and must be positive",
        ));
    }
    job.check()?;
    let first = render_viewer_batch(first_pdf, request.first_page, &[], job)?.preview;
    job.check()?;
    let second = render_viewer_batch(&request.second_pdf, request.second_page, &[], job)?.preview;
    let first_image = to_image(first)?;
    let second_image = to_image(second)?;

    let width = first_image
        .width()
        .checked_add(GUTTER_PIXELS)
        .and_then(|width| width.checked_add(second_image.width()))
        .ok_or_else(|| Error::input("Visual comparison width overflow"))?;
    let height = first_image.height().max(second_image.height());
    let output_pixels = width
        .checked_mul(height)
        .filter(|pixels| *pixels <= MAX_COMPOSITE_PIXELS)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Limit,
                "Side-by-side comparison exceeds the 4.5 million pixel limit",
            )
        })?;
    let mut composite =
        image::RgbaImage::from_pixel(width, height, image::Rgba([224, 228, 232, 255]));
    image::imageops::replace(&mut composite, &first_image, 0, 0);
    image::imageops::replace(
        &mut composite,
        &second_image,
        i64::from(first_image.width() + GUTTER_PIXELS),
        0,
    );

    let same_dimensions = first_image.width() == second_image.width()
        && first_image.height() == second_image.height();
    let (changed_pixels, mean_absolute_rgb_difference) = if same_dimensions {
        let (changed, mean) = pixel_difference(&first_image, &second_image, job)?;
        (Some(changed), Some(mean))
    } else {
        (None, None)
    };
    let visually_identical = same_dimensions && changed_pixels == Some(0);
    let report = serde_json::json!({
        "schema_version": 1,
        "comparison": "selected PDF pages rendered side by side with the bundled Hayro renderer",
        "first": {"page": request.first_page, "render_pixels": {"width": first_image.width(), "height": first_image.height()}},
        "second": {"page": request.second_page, "render_pixels": {"width": second_image.width(), "height": second_image.height()}},
        "result": if visually_identical { "pixel-identical-render" } else { "rendered-pages-differ" },
        "same_render_dimensions": same_dimensions,
        "changed_pixels": changed_pixels,
        "mean_absolute_rgb_difference": mean_absolute_rgb_difference,
        "composite_pixels": output_pixels,
        "limitations": [
            "The PNG is a side-by-side comparison of bounded raster previews; it does not compare text, metadata, annotations, forms, tags, signatures or complete PDF structure.",
            "A pixel-identical result means identical output from this Hayro rendering path only. Unsupported PDF features may be omitted or differ from other viewers.",
            "Parsing and rendering run in-process. Bounds are not an OS sandbox or a hard CPU/memory quota."
        ]
    });
    job.check()?;
    Ok((composite, report))
}

fn to_image(pixels: PagePixels) -> Result<image::RgbaImage> {
    let expected_bytes = pixels
        .width
        .checked_mul(pixels.height)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| Error::input("Rendered PDF page pixel count overflow"))?;
    if pixels.width == 0
        || pixels.height == 0
        || pixels.width * pixels.height > crate::pdf_workspace::PREVIEW_MAX_PIXELS
        || pixels.pixels.len() != expected_bytes as usize
    {
        return Err(Error::engine(
            "PDF renderer returned an invalid or over-budget pixel buffer",
        ));
    }
    image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.pixels)
        .ok_or_else(|| Error::engine("Could not construct a page comparison image"))
}

fn pixel_difference(
    first: &image::RgbaImage,
    second: &image::RgbaImage,
    job: &Job,
) -> Result<(u64, f64)> {
    let total_pixels = first.width() * first.height();
    let mut changed_pixels = 0u64;
    let mut rgb_difference = 0u64;
    for (index, (left, right)) in first.pixels().zip(second.pixels()).enumerate() {
        if index % 16_384 == 0 {
            job.check()?;
        }
        let difference = left.0[..3]
            .iter()
            .zip(&right.0[..3])
            .map(|(left, right)| u64::from(left.abs_diff(*right)))
            .sum::<u64>();
        changed_pixels += u64::from(difference != 0);
        rgb_difference += difference;
    }
    let channels = u64::from(total_pixels) * 3;
    Ok((
        changed_pixels,
        if channels == 0 {
            0.0
        } else {
            rgb_difference as f64 / channels as f64
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use localwright_core::{Job, Request};
    use lopdf::{dictionary, Document, Object};
    use std::path::Path;
    use tempfile::tempdir;

    fn pdf_with_pages(path: &Path, page_contents: &[(&str, i64)]) {
        let mut document = Document::with_version("1.7");
        let parent = document.new_object_id();
        let mut pages = Vec::new();
        let mut max_width = 0;
        for (paint, width) in page_contents {
            max_width = max_width.max(*width);
            let content = document.add_object(lopdf::Stream::new(
                dictionary! {},
                format!("{paint} 0 0 {width} 300 re f").into_bytes(),
            ));
            pages.push(document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => parent,
                "Contents" => content,
            }));
        }
        document.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => pages.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => pages.len() as i64,
                "MediaBox" => vec![0.into(), 0.into(), max_width.into(), 300.into()],
            }),
        );
        let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => parent });
        document.trailer.set("Root", root);
        document.save(path).unwrap();
    }

    fn one_page_pdf(path: &Path, paint: &str, width: i64) {
        pdf_with_pages(path, &[(paint, width)]);
    }

    #[test]
    fn selected_page_comparison_renders_identical_and_modified_pages() {
        let directory = tempdir().unwrap();
        let first = directory.path().join("first.pdf");
        let identical = directory.path().join("identical.pdf");
        let modified = directory.path().join("modified.pdf");
        pdf_with_pages(&first, &[("0 0 0 rg", 200), ("0 0 0 rg", 200)]);
        one_page_pdf(&identical, "0 0 0 rg", 200);
        one_page_pdf(&modified, "1 0 0 rg", 200);

        let same_output = directory.path().join("same.png");
        let same_parameter = serde_json::json!({
            "second_pdf": identical.to_str().unwrap(),
            "first_page": 2,
            "second_page": 1,
        })
        .to_string();
        let same_outcome = crate::handle(
            Request {
                operation: "pdf-page-compare".into(),
                input: first.clone(),
                output: same_output.clone(),
                parameter: same_parameter,
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        let same_report: serde_json::Value = serde_json::from_str(&same_outcome.preview).unwrap();
        assert_eq!(same_report["result"], "pixel-identical-render");
        assert_eq!(same_report["changed_pixels"], 0);
        assert_eq!(same_report["first"]["page"], 2);
        assert!(same_output.exists());
        let same_image = image::open(&same_output).unwrap().to_rgba8();
        assert!(same_image.width() > 0 && same_image.height() > 0);

        let changed_output = directory.path().join("changed.png");
        let changed_parameter = serde_json::json!({
            "second_pdf": modified.to_str().unwrap(),
            "first_page": 1,
            "second_page": 1,
        })
        .to_string();
        let changed_outcome = crate::handle(
            Request {
                operation: "pdf-page-compare".into(),
                input: first.clone(),
                output: changed_output.clone(),
                parameter: changed_parameter,
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        let changed_report: serde_json::Value =
            serde_json::from_str(&changed_outcome.preview).unwrap();
        assert_eq!(changed_report["result"], "rendered-pages-differ");
        assert!(changed_report["changed_pixels"].as_u64().unwrap() > 0);
        let changed_image = image::open(&changed_output).unwrap().to_rgba8();
        assert_eq!(changed_image.width(), same_image.width());
        assert_ne!(changed_image.as_raw(), same_image.as_raw());

        let different_geometry = directory.path().join("different-geometry.pdf");
        one_page_pdf(&different_geometry, "0 0 0 rg", 240);
        let geometry_output = directory.path().join("geometry.png");
        let geometry_outcome = crate::handle(
            Request {
                operation: "pdf-page-compare".into(),
                input: first.clone(),
                output: geometry_output.clone(),
                parameter: serde_json::json!({
                    "second_pdf": different_geometry.to_str().unwrap(),
                    "first_page": 1,
                    "second_page": 1,
                })
                .to_string(),
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        let geometry_report: serde_json::Value =
            serde_json::from_str(&geometry_outcome.preview).unwrap();
        assert_eq!(geometry_report["same_render_dimensions"], false);
        assert_eq!(geometry_report["changed_pixels"], serde_json::Value::Null);
        assert!(image::open(geometry_output).is_ok());

        assert!(crate::handle(
            Request {
                operation: "pdf-page-compare".into(),
                input: first,
                output: same_output,
                parameter: serde_json::json!({
                    "second_pdf": identical.to_str().unwrap(),
                    "first_page": 1,
                    "second_page": 1,
                })
                .to_string(),
                ..Request::default()
            },
            Job::new(),
        )
        .is_err());
    }

    #[test]
    fn invalid_selection_and_oversized_parameter_are_rejected() {
        let directory = tempdir().unwrap();
        let pdf = directory.path().join("first.pdf");
        one_page_pdf(&pdf, "0 0 0 rg", 200);
        let zero_page = serde_json::json!({
            "second_pdf": pdf.to_str().unwrap(),
            "first_page": 0,
            "second_page": 1,
        })
        .to_string();
        assert!(compare(&directory.path().join("first.pdf"), &zero_page, &Job::new()).is_err());
        let out_of_range = serde_json::json!({
            "second_pdf": pdf.to_str().unwrap(),
            "first_page": 2,
            "second_page": 1,
        })
        .to_string();
        assert!(compare(&pdf, &out_of_range, &Job::new()).is_err());
        assert!(compare(&pdf, &"x".repeat(MAX_PARAMETER_BYTES + 1), &Job::new()).is_err());
    }
}
