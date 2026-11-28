#![forbid(unsafe_code)]
mod citations;
mod compare;
mod docx_text;
mod epub;
#[cfg(feature = "ocr")]
mod ocr;
#[cfg(feature = "gui")]
mod pdf_viewer;
mod pdf_workspace;
use localwright_core::*;
use lopdf::{dictionary, Document, Object};
use pdf_workspace::{
    document_within_limits, MAX_DOCUMENT_BYTES, PREVIEW_MAX_EDGE, PREVIEW_MAX_PIXELS,
    THUMBNAIL_MAX_EDGE, VISIBLE_THUMBNAILS,
};
use std::{io::Write, path::Path};
use zip::{write::SimpleFileOptions, ZipWriter};
pub const OPERATIONS:&[Operation]=&[
 Operation{id:"markdown-docx",label:"Markdown to DOCX",hint:"Basic headings and plain paragraphs only. Choose new .docx output."},
 Operation{id:"text-docx",label:"Plain text to DOCX",hint:"Convert UTF-8 text to literal DOCX paragraphs. Choose a new .docx output."},
 Operation{id:"docx-text",label:"DOCX body text",hint:"Extract plain text from a DOCX document body. Optional new .txt output."},
 Operation{id:"epub-inspect",label:"Inspect EPUB package",hint:"Inspect bounded EPUB container, metadata and spine structure. Optional new .json report. This is not a complete EPUB conformance validator or layout renderer."},
 Operation{id:"epub-text",label:"Extract EPUB text",hint:"Extract UTF-8 XHTML body text in reading order. Optional new .txt output. Styles, images, scripts, navigation and page layout are not rendered."},
 Operation{id:"compare-text",label:"Compare text files",hint:"Enter the second UTF-8 text file path as the parameter. Optional new .txt report."},
 Operation{id:"citation-convert",label:"Convert citation records",hint:"Parameter format pair: csl-json->ris, ris->csl-json, csl-json->bibtex, or ris->bibtex. Choose a new output with .json, .ris, or .bib extension."},
 Operation{id:"citation-validate",label:"Validate CSL JSON bibliography",hint:"Validate one CSL JSON object or array (up to 10,000 records). Optional new .json report."},
 Operation{id:"pdf-info",label:"PDF information",hint:"Show the PDF version and page count. Optional new JSON output."},
 Operation{id:"pdf-preview",label:"Render PDF page preview",hint:"Render one 1-based page to a bounded PNG. Enter the page number (default 1) and choose a new .png output. Raster preview only; rendering fidelity and unsupported PDF features may vary."},
 Operation{id:"pdf-text",label:"PDF text",hint:"Extract text from PDF text layer. Optional new .txt output. No OCR."},
 Operation{id:"pdf-split",label:"PDF page range",hint:"Parameter inclusive page range, e.g. 2-4. New .pdf output. Experimental; not sanitization."},
 Operation{id:"pdf-rotate",label:"Rotate PDF pages",hint:"Enter start-end,degrees, e.g. 2-4,90. Use 90, 180 or 270 clockwise. Choose a new PDF."},
 Operation{id:"pdf-reorder",label:"Reorder PDF pages",hint:"Enter every page once, in the desired order, e.g. 3,1,2. Choose a new .pdf output."},
 Operation{id:"pdf-delete-pages",label:"Delete PDF pages",hint:"Enter one or more unique 1-based page numbers separated by commas. Preserves the remaining page order. Choose a new .pdf output. This is not sanitization."},
 Operation{id:"pdf-merge",label:"Merge PDFs",hint:"Input first PDF; parameter second PDF path. New .pdf output. Experimental page-only merge."},
 #[cfg(feature="ocr")]
 Operation{id:"ocr-png",label:"Experimental PNG OCR",hint:"Recognize English text using the private OCR pack. Optional new .txt output."},
 #[cfg(feature="ocr")]
 Operation{id:"ocr-png-pdf",label:"Searchable PDF from PNG",hint:"Create a new one-page PDF with the original image and a recognized English text layer."}];
fn err(e: impl std::fmt::Display) -> Error {
    Error::input(e.to_string())
}
fn xml(s: &str) -> Result<String> {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c == '\t'
                || c == '\n'
                || c == '\r'
                || (c >= '\u{20}' && c != '\u{fffe}' && c != '\u{ffff}') =>
            {
                out.push(c)
            }
            _ => return Err(Error::input("Invalid XML character")),
        }
    }
    Ok(out)
}
fn docx(p: &Path, source: &str, markdown: bool, j: &Job) -> Result<()> {
    let mut body = String::new();
    for line in source.lines() {
        j.check()?;
        let hashes = line.bytes().take_while(|&c| c == b'#').count();
        let heading =
            markdown && (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ');
        let content = if heading { &line[hashes + 1..] } else { line };
        let style = if heading {
            format!("<w:pPr><w:outlineLvl w:val=\"{}\"/></w:pPr>", hashes - 1)
        } else {
            String::new()
        };
        let run_style = if heading {
            "<w:rPr><w:b/><w:sz w:val=\"32\"/></w:rPr>"
        } else {
            ""
        };
        body.push_str(&format!(
            "<w:p>{style}<w:r>{run_style}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
            xml(content)?
        ));
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );
    write_new(p, |f| {
        let mut z = ZipWriter::new(f);
        let opt = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            ("word/document.xml", document.as_str()),
        ] {
            j.check()?;
            z.start_file(name, opt).map_err(err)?;
            z.write_all(data.as_bytes())?;
        }
        z.finish().map_err(err)?;
        Ok(())
    })
}
fn load(p: &Path) -> Result<Document> {
    let bytes = read_bounded(p, MAX_DOCUMENT_BYTES)?;
    load_pdf_bytes(&bytes)
}
fn load_pdf_bytes(bytes: &[u8]) -> Result<Document> {
    let d = Document::load_mem(bytes).map_err(err)?;
    if d.is_encrypted() {
        return Err(Error::input("Encrypted PDF not supported"));
    }
    if !document_within_limits(bytes.len() as u64, d.get_pages().len(), d.objects.len()) {
        return Err(Error::input("PDF object or page limit exceeded"));
    }
    Ok(d)
}

#[derive(Debug)]
pub(crate) struct PagePixels {
    pub(crate) page: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct ViewerBatch {
    pub(crate) page_count: usize,
    pub(crate) properties: PdfProperties,
    pub(crate) page_width_points: f32,
    pub(crate) page_height_points: f32,
    pub(crate) preview: PagePixels,
    pub(crate) thumbnails: Vec<PagePixels>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PdfProperties {
    pub(crate) version: String,
    pub(crate) byte_length: u64,
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) subject: Option<String>,
    pub(crate) creator: Option<String>,
    pub(crate) producer: Option<String>,
}

fn pdf_metadata_text(document: &Document, key: &[u8]) -> Option<String> {
    let info = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| match object {
            Object::Reference(id) => document.get_object(*id).ok(),
            value => Some(value),
        })?;
    let value = info.as_dict().ok()?.get(key).ok()?;
    let decoded = lopdf::decode_text_string(value).ok()?;
    let cleaned = decoded
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
        .take(512)
        .collect::<String>();
    let cleaned = cleaned.trim().to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn pdf_properties(document: &Document, byte_length: u64) -> PdfProperties {
    PdfProperties {
        version: document.version.clone(),
        byte_length,
        title: pdf_metadata_text(document, b"Title"),
        author: pdf_metadata_text(document, b"Author"),
        subject: pdf_metadata_text(document, b"Subject"),
        creator: pdf_metadata_text(document, b"Creator"),
        producer: pdf_metadata_text(document, b"Producer"),
    }
}

fn render_page_bitmap(
    page: &hayro::hayro_syntax::page::Page<'_>,
    page_number: usize,
    max_edge: u16,
    job: &Job,
) -> Result<PagePixels> {
    let (page_width, page_height) = page.render_dimensions();
    let page_area = page_width * page_height;
    if !page_width.is_finite()
        || !page_height.is_finite()
        || !page_area.is_finite()
        || page_width <= 0.0
        || page_height <= 0.0
        || page_area <= 0.0
    {
        return Err(Error::input("PDF page has invalid render dimensions"));
    }
    let edge = f32::from(max_edge.max(1).min(PREVIEW_MAX_EDGE as u16));
    let max_pixels = PREVIEW_MAX_PIXELS.min(u32::from(max_edge.max(1)).pow(2));
    let pixel_scale = (edge / page_width.max(page_height))
        .min((max_pixels as f32 / page_area).sqrt())
        .min(1.0);
    let width = (page_width * pixel_scale).round().clamp(1.0, edge) as u16;
    let height = (page_height * pixel_scale).round().clamp(1.0, edge) as u16;
    if u32::from(width) * u32::from(height) > max_pixels {
        return Err(Error::input("Rendered preview exceeds the pixel budget"));
    }
    job.check()?;
    let pixmap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::render(
            page,
            &hayro::RenderCache::new(),
            &hayro::hayro_interpret::InterpreterSettings::default(),
            &hayro::RenderSettings::default(),
            &hayro::PixmapSettings {
                x_scale: pixel_scale,
                y_scale: pixel_scale,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            },
        )
    }))
    .map_err(|_| Error::engine("PDF renderer rejected the page while rendering"))?;
    job.check()?;
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    if width * height > max_pixels {
        return Err(Error::engine(
            "PDF renderer exceeded the preview pixel budget",
        ));
    }
    Ok(PagePixels {
        page: page_number,
        width,
        height,
        pixels: pixmap.data_as_u8_slice().to_vec(),
    })
}

pub(crate) fn render_viewer_batch(
    path: &Path,
    page_number: usize,
    thumbnail_pages: &[usize],
    job: &Job,
) -> Result<ViewerBatch> {
    if thumbnail_pages.len() > VISIBLE_THUMBNAILS {
        return Err(Error::input(
            "Thumbnail batch exceeds the bounded page window",
        ));
    }
    let bytes = read_bounded(path, MAX_DOCUMENT_BYTES)?;
    let byte_length = bytes.len() as u64;
    let document =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load_pdf_bytes(&bytes)))
            .map_err(|_| {
                Error::input("PDF parser stopped unexpectedly while reading the document")
            })??;
    let page_count = document.get_pages().len();
    if page_number == 0 || page_number > page_count {
        return Err(Error::input("Choose an existing 1-based PDF page number"));
    }
    if thumbnail_pages
        .iter()
        .any(|page| *page == 0 || *page > page_count)
    {
        return Err(Error::input("Thumbnail page selection is out of range"));
    }
    job.check()?;
    let pdf = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::hayro_syntax::Pdf::new(bytes)
    }))
    .map_err(|_| Error::input("PDF renderer stopped unexpectedly while parsing the document"))?
    .map_err(|_| Error::input("PDF renderer could not parse the document"))?;
    let pages = pdf.pages();
    let page = pages
        .get(page_number - 1)
        .ok_or_else(|| Error::input("PDF renderer could not find the requested page"))?;
    let (page_width_points, page_height_points) = page.render_dimensions();
    let properties = pdf_properties(&document, byte_length);
    let preview = render_page_bitmap(page, page_number, PREVIEW_MAX_EDGE as u16, job)?;
    let mut thumbnails = Vec::with_capacity(thumbnail_pages.len());
    for thumbnail_page in thumbnail_pages {
        job.check()?;
        let Some(page) = pages.get(thumbnail_page - 1) else {
            continue;
        };
        if let Ok(thumbnail) = render_page_bitmap(page, *thumbnail_page, THUMBNAIL_MAX_EDGE, job) {
            thumbnails.push(thumbnail);
        }
        job.check()?;
    }
    Ok(ViewerBatch {
        page_count,
        properties,
        page_width_points,
        page_height_points,
        preview,
        thumbnails,
    })
}

fn render_page_rgba(
    path: &Path,
    page_number: usize,
    job: &Job,
) -> Result<(u32, u32, Vec<u8>, usize)> {
    let batch = render_viewer_batch(path, page_number, &[], job)?;
    Ok((
        batch.preview.width,
        batch.preview.height,
        batch.preview.pixels,
        batch.page_count,
    ))
}

fn render_pdf_preview(path: &Path, page_number: usize, output: &Path, job: &Job) -> Result<String> {
    extension(output, "png")?;
    let (width, height, pixels, page_count) = render_page_rgba(path, page_number, job)?;
    let rgba = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| Error::engine("PDF renderer returned an invalid pixel buffer"))?;
    write_new(output, |file| {
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(file, image::ImageFormat::Png)
            .map_err(err)
    })?;
    Ok(format!("Rendered PDF page {page_number} of {page_count} to {width}×{height} PNG ({PREVIEW_MAX_PIXELS} pixel maximum)"))
}

/// Open the native Document Studio viewer on a rendered PDF page.
#[cfg(feature = "gui")]
pub fn run_pdf_preview(path: &Path, initial_page: usize) -> std::result::Result<(), String> {
    pdf_viewer::run(path.to_path_buf(), initial_page)
}
fn extension(p: &Path, ext: &str) -> Result<()> {
    if p.extension().and_then(|s| s.to_str()) != Some(ext) {
        return Err(Error::input(format!("Expected .{ext} output")));
    }
    Ok(())
}
// Materialize inheritable attributes before replacing each page's parent.
fn inherited(doc: &Document, id: lopdf::ObjectId, key: &[u8]) -> Result<Option<Object>> {
    let mut next = id;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..64 {
        if !seen.insert(next) {
            return Err(Error::input("Cyclic page tree"));
        }
        let d = doc.get_dictionary(next).map_err(err)?;
        if let Ok(v) = d.get(key) {
            return Ok(Some(v.clone()));
        }
        match d.get(b"Parent") {
            Ok(v) => next = v.as_reference().map_err(err)?,
            Err(_) => return Ok(None),
        }
    }
    Err(Error::input("Page tree too deep"))
}
fn merge(mut a: Document, mut b: Document, j: &Job) -> Result<Document> {
    a.renumber_objects_with(1);
    b.renumber_objects_with(
        a.max_id
            .checked_add(1)
            .ok_or_else(|| Error::input("Object ID overflow"))?,
    );
    let a_order = a.get_pages().into_values().collect();
    let b_order = b.get_pages().into_values().collect();
    rebuild_pages([(a, a_order), (b, b_order)], "1.7", j)
}

fn page_range(
    parameter: &str,
    pages: &std::collections::BTreeMap<u32, lopdf::ObjectId>,
) -> Result<(u32, u32)> {
    let (start, end) = parameter
        .split_once('-')
        .ok_or_else(|| Error::input("Expected start-end"))?;
    let (start, end): (u32, u32) = (
        start.trim().parse().map_err(err)?,
        end.trim().parse().map_err(err)?,
    );
    if start == 0 || start > end || !pages.contains_key(&start) || !pages.contains_key(&end) {
        return Err(Error::input("Invalid page range"));
    }
    Ok((start, end))
}

fn rotate_pages(doc: &mut Document, parameter: &str, job: &Job) -> Result<()> {
    let (range, angle) = parameter
        .split_once(',')
        .ok_or_else(|| Error::input("Expected start-end,degrees"))?;
    let angle: i64 = angle.trim().parse().map_err(err)?;
    if !matches!(angle, 90 | 180 | 270) {
        return Err(Error::input("Choose 90, 180 or 270 degrees clockwise"));
    }
    let pages = doc.get_pages();
    let (start, end) = page_range(range, &pages)?;
    for (_, &id) in pages.range(start..=end) {
        job.check()?;
        let previous = match inherited(doc, id, b"Rotate")? {
            Some(Object::Reference(reference)) => doc
                .get_object(reference)
                .and_then(Object::as_i64)
                .map_err(err)?,
            Some(value) => value.as_i64().map_err(err)?,
            None => 0,
        };
        if previous % 90 != 0 {
            return Err(Error::input(
                "Existing PDF rotation is not a multiple of 90 degrees",
            ));
        }
        let rotation = (previous.rem_euclid(360) + angle) % 360;
        doc.get_object_mut(id)
            .and_then(Object::as_dict_mut)
            .map_err(err)?
            .set("Rotate", rotation);
    }
    Ok(())
}

fn reorder(d: Document, order: &str, j: &Job) -> Result<Document> {
    let mut remaining = d.get_pages();
    let mut ids = Vec::with_capacity(remaining.len());
    for entry in order.split(',') {
        j.check()?;
        let page = entry.trim().parse::<u32>().map_err(err)?;
        let id = remaining
            .remove(&page)
            .ok_or_else(|| Error::input(format!("Page {page} is repeated or out of range")))?;
        ids.push(id);
    }
    if !remaining.is_empty() {
        return Err(Error::input("The order must include every page"));
    }
    let version = d.version.clone();
    rebuild_pages([(d, ids)], &version, j)
}

fn delete_pages(d: Document, selection: &str, j: &Job) -> Result<Document> {
    if selection.len() > 16 * 1024 {
        return Err(Error::input("Page selection exceeds the limit"));
    }
    let pages = d.get_pages();
    let mut remove = std::collections::HashSet::new();
    for item in selection.split(',') {
        j.check()?;
        let page = item.trim().parse::<u32>().map_err(err)?;
        if page == 0 || !pages.contains_key(&page) || !remove.insert(page) {
            return Err(Error::input("Choose unique existing 1-based page numbers"));
        }
    }
    if remove.is_empty() || remove.len() >= pages.len() {
        return Err(Error::input("PDF must retain at least one page"));
    }
    let keep = pages
        .into_iter()
        .filter_map(|(number, id)| (!remove.contains(&number)).then_some(id))
        .collect();
    let version = d.version.clone();
    rebuild_pages([(d, keep)], &version, j)
}

fn rebuild_pages(
    sources: impl IntoIterator<Item = (Document, Vec<lopdf::ObjectId>)>,
    version: &str,
    j: &Job,
) -> Result<Document> {
    let mut out = Document::with_version(version);
    let mut ids = vec![];
    for (mut d, order) in sources {
        for id in order {
            j.check()?;
            let attrs = [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"]
                .iter()
                .map(|key| Ok((*key, inherited(&d, id, key)?)))
                .collect::<Result<Vec<_>>>()?;
            let page = d
                .get_object_mut(id)
                .map_err(err)?
                .as_dict_mut()
                .map_err(err)?;
            for (k, v) in attrs {
                if let Some(v) = v {
                    page.set(k, v);
                }
            }
            // Drop interactive annotations when rebuilding the page tree.
            page.remove(b"Annots");
            page.remove(b"AA");
            ids.push(id);
        }
        out.max_id = out.max_id.max(d.max_id);
        out.objects.append(&mut d.objects);
    }
    let parent = out.new_object_id();
    for id in &ids {
        out.get_object_mut(*id)
            .map_err(err)?
            .as_dict_mut()
            .map_err(err)?
            .set("Parent", Object::Reference(parent));
    }
    out.objects.insert(parent,Object::Dictionary(dictionary!{"Type"=>"Pages","Kids"=>Object::Array(ids.iter().copied().map(Object::Reference).collect()),"Count"=>ids.len() as i64}));
    let root = out.add_object(dictionary! {"Type"=>"Catalog","Pages"=>parent});
    out.trailer.set("Root", root);
    out.prune_objects();
    Ok(out)
}
pub fn handle(r: Request, j: Job) -> Result<Outcome> {
    j.check()?;
    let mut written = None;
    let mut warnings = vec![];
    let preview = match r.operation.as_str() {
        "markdown-docx" | "text-docx" => {
            extension(&r.output, "docx")?;
            let input = String::from_utf8(read_bounded(&r.input, 2 * 1024 * 1024)?).map_err(err)?;
            let markdown = r.operation == "markdown-docx";
            docx(&r.output, &input, markdown, &j)?;
            written = Some(r.output);
            if markdown {
                warnings.push("Only ATX headings and literal paragraphs; inline markup, tables, images and layout are not interpreted.".into());
            }
            "Created basic DOCX".into()
        }
        #[cfg(feature = "ocr")]
        "ocr-png" => {
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "txt")?;
            }
            let recognized = ocr::recognize(&r.input, &j)?;
            if !r.output.as_os_str().is_empty() {
                j.check()?;
                write_bytes(&r.output, recognized.as_bytes())?;
                written = Some(r.output);
            }
            warnings.push(
                "Experimental English OCR. Review the recognized text against the original.".into(),
            );
            recognized.chars().take(24000).collect()
        }
        #[cfg(feature = "ocr")]
        "ocr-png-pdf" => {
            extension(&r.output, "pdf")?;
            let pdf = ocr::searchable_pdf(&r.input, &j)?;
            j.check()?;
            write_bytes(&r.output, &pdf)?;
            written = Some(r.output);
            warnings.push(
                "Experimental English OCR. Review the searchable text against the original image."
                    .into(),
            );
            "Created a one-page searchable PDF".into()
        }
        "docx-text" => {
            let text = docx_text::extract(&r.input, &j)?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "txt")?;
                j.check()?;
                write_bytes(&r.output, text.as_bytes())?;
                written = Some(r.output);
            }
            warnings.push("Body text only. Layout, list markers, headers, images and annotations are omitted. Hidden body text may appear.".into());
            text.chars().take(24000).collect()
        }
        "epub-inspect" => {
            let report = epub::inspect(&r.input, &j)?;
            let report = serde_json::to_string_pretty(&report)?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "json")?;
                j.check()?;
                write_bytes(&r.output, report.as_bytes())?;
                written = Some(r.output);
            }
            warnings.push("Structural inspection only. The package is not checked against every EPUB conformance rule and is not rendered.".into());
            report
        }
        "epub-text" => {
            let text = epub::extract_text(&r.input, &j)?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "txt")?;
                j.check()?;
                write_bytes(&r.output, text.as_bytes())?;
                written = Some(r.output);
            }
            warnings.push("Plain text only, in EPUB spine order. CSS, page layout, images, tables as layout, scripts, navigation, annotations and DRM-protected content are not converted.".into());
            let mut preview = text.chars().take(24000).collect::<String>();
            if preview.len() < text.len() {
                preview.push_str(
                    "\n[Preview truncated. Save a text report to see the extracted content.]",
                );
            }
            preview
        }
        "compare-text" => {
            let report = compare::compare(&r.input, Path::new(&r.parameter), &j)?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "txt")?;
                j.check()?;
                write_bytes(&r.output, report.as_bytes())?;
                written = Some(r.output);
            }
            let mut preview = report.chars().take(24000).collect::<String>();
            if preview.len() < report.len() {
                preview.push_str("\n[Preview truncated. Save a report to see all changes.]");
            }
            preview
        }
        "citation-convert" => {
            let (from, to) = r
                .parameter
                .split_once("->")
                .ok_or_else(|| Error::input("Enter source->target format"))?;
            let ext = match to {
                "csl-json" => "json",
                "ris" => "ris",
                "bibtex" => "bib",
                _ => {
                    return Err(Error::input(
                        "Supported output formats are csl-json, ris and bibtex",
                    ))
                }
            };
            extension(&r.output, ext)?;
            let input = read_bounded(&r.input, 8 * 1024 * 1024)?;
            let bytes = citations::convert(&input, from, to, &j)?;
            j.check()?;
            write_bytes(&r.output, &bytes)?;
            written = Some(r.output);
            warnings.push("Interchange preserves only the fields supported by the selected formats. No citation style is rendered; review the bibliography before publication.".into());
            String::from_utf8_lossy(&bytes)
                .chars()
                .take(24000)
                .collect()
        }
        "citation-validate" => {
            let input = read_bounded(&r.input, 8 * 1024 * 1024)?;
            let count = citations::validate(&input)?;
            let report = serde_json::to_string_pretty(
                &serde_json::json!({"schema_version":1,"format":"CSL JSON","valid":true,"references":count}),
            )?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "json")?;
                j.check()?;
                write_bytes(&r.output, report.as_bytes())?;
                written = Some(r.output);
            }
            report
        }
        "pdf-info" => {
            let document = load(&r.input)?;
            let properties =
                pdf_properties(&document, std::fs::metadata(&r.input).map_err(err)?.len());
            let report = serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "pdf_version": properties.version,
                "pages": document.get_pages().len(),
                "file_bytes": properties.byte_length,
                "title": properties.title,
                "author": properties.author,
                "subject": properties.subject,
                "creator": properties.creator,
                "producer": properties.producer,
            }))?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "json")?;
                j.check()?;
                write_bytes(&r.output, report.as_bytes())?;
                written = Some(r.output);
            }
            report
        }
        "pdf-preview" => {
            let page_number = if r.parameter.trim().is_empty() {
                1
            } else {
                r.parameter
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| Error::input("Enter a positive 1-based page number"))?
            };
            let report = render_pdf_preview(&r.input, page_number, &r.output, &j)?;
            written = Some(r.output);
            #[cfg(feature = "gui")]
            if !cfg!(test) && !std::env::args_os().any(|argument| argument == "--cli") {
                let launched = std::env::current_exe()
                    .map_err(|error| error.to_string())
                    .and_then(|executable| {
                        std::process::Command::new(executable)
                            .arg("--preview-pdf")
                            .arg(&r.input)
                            .arg(page_number.to_string())
                            .spawn()
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    });
                if let Err(error) = launched {
                    warnings.push(format!(
                        "The PNG was saved, but its native preview window could not open: {error}"
                    ));
                }
            }
            warnings.push("Raster preview only. PDF rendering is incomplete: unsupported blending, transparency groups, annotations, forms, and other PDF features may be missing or differ. This is not a structural edit, redaction, or security review.".into());
            report
        }
        "pdf-text" => {
            let d = load(&r.input)?;
            let mut s = String::new();
            for page in d.get_pages().keys() {
                j.check()?;
                s.push_str(&d.extract_text(&[*page]).map_err(err)?);
                s.push('\n');
                if s.len() > 8 * 1024 * 1024 {
                    return Err(Error::input("Extracted text limit exceeded"));
                }
            }
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "txt")?;
                write_bytes(&r.output, s.as_bytes())?;
                written = Some(r.output);
            }
            warnings.push(
                "Text layer only; reading order, fonts and encoding can affect extraction.".into(),
            );
            s.chars().take(24000).collect()
        }
        "pdf-split" | "pdf-merge" | "pdf-reorder" | "pdf-delete-pages" | "pdf-rotate" => {
            extension(&r.output, "pdf")?;
            let mut d = load(&r.input)?;
            if r.operation == "pdf-split" {
                let pages = d.get_pages();
                let (a, b) = page_range(&r.parameter, &pages)?;
                let remove = pages
                    .keys()
                    .copied()
                    .filter(|n| *n < a || *n > b)
                    .collect::<Vec<_>>();
                d.delete_pages(&remove);
                d.prune_objects();
            } else if r.operation == "pdf-rotate" {
                rotate_pages(&mut d, &r.parameter, &j)?;
            } else {
                d = if r.operation == "pdf-reorder" {
                    reorder(d, &r.parameter, &j)?
                } else if r.operation == "pdf-delete-pages" {
                    delete_pages(d, &r.parameter, &j)?
                } else {
                    merge(d, load(Path::new(&r.parameter))?, &j)?
                };
                warnings.push(
                    "Page-only output omits annotations, bookmarks, forms and document metadata."
                        .into(),
                );
            }
            let count = d.get_pages().len();
            j.check()?;
            write_new(&r.output, |f| {
                d.save_to(f).map_err(err)?;
                Ok(())
            })?;
            written = Some(r.output);
            warnings.push("Experimental structural manipulation. Not redaction; hidden or active PDF content may survive. Do not open untrusted output without a safe PDF viewer.".into());
            format!("Wrote {count} pages")
        }
        _ => return Err(Error::input("Unknown operation")),
    };
    Ok(Outcome {
        preview,
        written,
        warnings,
        ..Outcome::default()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pdf_properties_decode_bounded_unicode_metadata_and_file_size() {
        let mut document = Document::with_version("1.7");
        document.trailer.set(
            "Info",
            Object::Dictionary(dictionary! {
                "Title" => lopdf::text_string("Résumé\u{0007} study"),
                "Author" => lopdf::text_string("Ada Lovelace"),
                "Producer" => lopdf::text_string("Local PDF producer"),
            }),
        );
        let properties = pdf_properties(&document, 1234);
        assert_eq!(properties.version, "1.7");
        assert_eq!(properties.byte_length, 1234);
        assert_eq!(properties.title.as_deref(), Some("Résumé study"));
        assert_eq!(properties.author.as_deref(), Some("Ada Lovelace"));
        assert_eq!(properties.producer.as_deref(), Some("Local PDF producer"));
        assert!(properties.subject.is_none());
        assert!(properties.creator.is_none());
    }

    #[test]
    fn docx_parts() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.docx");
        docx(&p, "# Café & science\nA < B", true, &Job::new()).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(p).unwrap()).unwrap();
        let mut s = String::new();
        use std::io::Read;
        z.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert!(s.contains("Café &amp; science"));
        assert!(s.contains("A &lt; B"));
    }
    #[test]
    fn text_docx_preserves_literal_heading_characters() {
        use std::io::Read as _;
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.txt");
        let output = dir.path().join("output.docx");
        std::fs::write(&input, "# literal").unwrap();
        handle(
            Request {
                operation: "text-docx".into(),
                input,
                output: output.clone(),
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("# literal"));
        assert!(!xml.contains("outlineLvl"));
    }

    #[test]
    fn rotates_only_selected_pages_relative_to_inherited_angle() {
        let mut doc = Document::load("tests/fixtures/two-pages.pdf").unwrap();
        let pages = doc.get_pages();
        let first = pages[&1];
        let second = pages[&2];
        for id in [first, second] {
            doc.get_object_mut(id)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .remove(b"Rotate");
        }
        let parent = doc
            .get_dictionary(first)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(parent)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Rotate", -90);
        rotate_pages(&mut doc, "2-2,180", &Job::new()).unwrap();
        assert_eq!(
            inherited(&doc, first, b"Rotate")
                .unwrap()
                .unwrap()
                .as_i64()
                .unwrap(),
            -90
        );
        assert_eq!(
            inherited(&doc, second, b"Rotate")
                .unwrap()
                .unwrap()
                .as_i64()
                .unwrap(),
            90
        );
        assert!(rotate_pages(&mut doc, "0-2,90", &Job::new()).is_err());
        assert!(rotate_pages(&mut doc, "1-2,45", &Job::new()).is_err());
    }

    #[test]
    fn pdf_single_page_extraction_writes_new_output_and_preserves_source() {
        let directory = tempfile::tempdir().unwrap();
        let input = Path::new("tests/fixtures/two-pages.pdf").to_path_buf();
        let output = directory.path().join("page-two.pdf");
        handle(
            Request {
                operation: "pdf-split".into(),
                input: input.clone(),
                output: output.clone(),
                parameter: "2-2".into(),
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        assert_eq!(load(&input).unwrap().get_pages().len(), 2);
        assert_eq!(load(&output).unwrap().get_pages().len(), 1);
        assert!(handle(
            Request {
                operation: "pdf-split".into(),
                input,
                output,
                parameter: "1-1".into(),
                ..Request::default()
            },
            Job::new(),
        )
        .is_err());
    }

    #[test]
    fn pdf_merge_writes_a_new_page_only_copy_and_preserves_both_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let first = Path::new("tests/fixtures/two-pages.pdf").to_path_buf();
        let second = directory.path().join("second.pdf");
        let output = directory.path().join("merged.pdf");
        std::fs::copy(&first, &second).unwrap();
        let second_parameter = second.to_str().unwrap().to_owned();
        handle(
            Request {
                operation: "pdf-merge".into(),
                input: first.clone(),
                output: output.clone(),
                parameter: second_parameter.clone(),
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        assert_eq!(load(&first).unwrap().get_pages().len(), 2);
        assert_eq!(load(&second).unwrap().get_pages().len(), 2);
        assert_eq!(load(&output).unwrap().get_pages().len(), 4);

        assert!(handle(
            Request {
                operation: "pdf-merge".into(),
                input: first.clone(),
                output: first.clone(),
                parameter: second_parameter,
                ..Request::default()
            },
            Job::new(),
        )
        .is_err());
        assert_eq!(load(&first).unwrap().get_pages().len(), 2);
    }

    #[test]
    fn reorder_preserves_page_order_and_inherited_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.pdf");
        let output = dir.path().join("output.pdf");
        let mut document = Document::with_version("1.7");
        let parent = document.new_object_id();
        let first = document.add_object(dictionary! {"Type" => "Page", "Parent" => parent});
        let second =
            document.add_object(dictionary! {"Type" => "Page", "Parent" => parent, "Rotate" => 90});
        document.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(first), Object::Reference(second)],
                "Count" => 2,
                "MediaBox" => vec![Object::Integer(0), 0.into(), 200.into(), 300.into()],
            }),
        );
        let root = document.add_object(dictionary! {"Type" => "Catalog", "Pages" => parent});
        document.trailer.set("Root", root);
        document.save(&input).unwrap();
        let request = |parameter: &str| Request {
            operation: "pdf-reorder".into(),
            input: input.clone(),
            output: output.clone(),
            parameter: parameter.into(),
            ..Request::default()
        };
        for invalid in ["1", "1,1", "0,2", "1,3", "1,2,"] {
            assert!(handle(request(invalid), Job::new()).is_err());
            assert!(!output.exists());
        }
        handle(request("2, 1"), Job::new()).unwrap();
        let result = load(&output).unwrap();
        let pages = result.get_pages();
        assert_eq!(pages.len(), 2);
        let first = result.get_dictionary(pages[&1]).unwrap();
        let second = result.get_dictionary(pages[&2]).unwrap();
        assert_eq!(first.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
        assert!(second.get(b"Rotate").is_err());
        for page in [first, second] {
            assert_eq!(
                page.get(b"MediaBox").unwrap().as_array().unwrap(),
                &vec![Object::Integer(0), 0.into(), 200.into(), 300.into()]
            );
        }
        let original = load(&input).unwrap();
        let original_pages = original.get_pages();
        assert_eq!(original_pages.len(), 2);
        assert!(original
            .get_dictionary(original_pages[&1])
            .unwrap()
            .get(b"Rotate")
            .is_err());
        assert_eq!(
            original
                .get_dictionary(original_pages[&2])
                .unwrap()
                .get(b"Rotate")
                .unwrap()
                .as_i64()
                .unwrap(),
            90
        );
        assert!(handle(request("1,2"), Job::new()).is_err());
    }

    #[test]
    fn pdf_preview_renders_selected_page_with_bounded_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.pdf");
        let output = dir.path().join("preview.png");
        let mut document = Document::with_version("1.7");
        let parent = document.new_object_id();
        let content = document.add_object(lopdf::Stream::new(
            dictionary! {},
            b"0 0 0 rg 0 0 200 300 re f".to_vec(),
        ));
        let first = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => parent,
            "Contents" => content,
        });
        let second = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => parent,
        });
        document.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(first), Object::Reference(second)],
                "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            }),
        );
        let root = document.add_object(dictionary! {"Type" => "Catalog", "Pages" => parent});
        document.trailer.set("Root", root);
        document.save(&input).unwrap();

        let request = |page: &str, output: &Path| Request {
            operation: "pdf-preview".into(),
            input: input.clone(),
            output: output.to_path_buf(),
            parameter: page.into(),
            ..Request::default()
        };
        assert!(handle(request("0", &output), Job::new()).is_err());
        assert!(handle(request("3", &output), Job::new()).is_err());
        assert!(!output.exists());
        let result = handle(request("1", &output), Job::new()).unwrap();
        assert!(result.preview.contains("page 1 of 2"));
        assert!(result.warnings[0].contains("Raster preview only"));
        let rendered = image::open(&output).unwrap().to_rgba8();
        assert_eq!(rendered.dimensions(), (200, 300));
        assert!(rendered.get_pixel(100, 150).0[0] < 10);
        let checksum = rendered
            .as_raw()
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        println!("page 1/2: 200x300 RGBA; black center pixel; FNV-1a64={checksum:016x}");

        let second_output = dir.path().join("page-2.png");
        let second_result = handle(request("2", &second_output), Job::new()).unwrap();
        assert!(second_result.preview.contains("page 2 of 2"));
        let second = image::open(second_output).unwrap().to_rgba8();
        assert_eq!(second.dimensions(), (200, 300));
        assert_eq!(second.get_pixel(100, 150).0, [255, 255, 255, 255]);
    }

    #[test]
    fn xml_control() {
        assert!(xml("\u{1}").is_err());
    }

    #[test]
    fn citation_routes_export_and_validate_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("refs.json");
        let output = dir.path().join("refs.ris");
        std::fs::write(
            &input,
            r#"{"id":"unit","type":"article-journal","title":"Unicode café"}"#,
        )
        .unwrap();
        let request = Request {
            operation: "citation-convert".into(),
            input: input.clone(),
            output: output.clone(),
            parameter: "csl-json->ris".into(),
            ..Request::default()
        };
        let result = handle(request.clone(), Job::new()).unwrap();
        assert!(result.preview.contains("Unicode café"));
        let exported = std::fs::read_to_string(&output).unwrap();
        assert!(exported.contains("TI  - Unicode café"));
        assert!(handle(request, Job::new()).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), exported);

        let report = dir.path().join("validation.json");
        let checked = handle(
            Request {
                operation: "citation-validate".into(),
                input,
                output: report.clone(),
                ..Request::default()
            },
            Job::new(),
        )
        .unwrap();
        assert!(checked.preview.contains("\"valid\": true"));
        assert!(std::fs::read_to_string(report)
            .unwrap()
            .contains("\"references\": 1"));
    }
}
