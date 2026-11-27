#![forbid(unsafe_code)]
mod citations;
mod compare;
mod docx_text;
#[cfg(feature = "ocr")]
mod ocr;
use localwright_core::*;
use lopdf::{dictionary, Document, Object};
use std::{io::Write, path::Path};
use zip::{write::SimpleFileOptions, ZipWriter};
pub const OPERATIONS:&[Operation]=&[
 Operation{id:"markdown-docx",label:"Markdown to DOCX",hint:"Basic headings and plain paragraphs only. Choose new .docx output."},
 Operation{id:"text-docx",label:"Plain text to DOCX",hint:"Convert UTF-8 text to literal DOCX paragraphs. Choose a new .docx output."},
 Operation{id:"docx-text",label:"DOCX body text",hint:"Extract plain text from a DOCX document body. Optional new .txt output."},
 Operation{id:"compare-text",label:"Compare text files",hint:"Enter the second UTF-8 text file path as the parameter. Optional new .txt report."},
 Operation{id:"citation-convert",label:"Convert citation records",hint:"Parameter format pair: csl-json->ris, ris->csl-json, csl-json->bibtex, or ris->bibtex. Choose a new output with .json, .ris, or .bib extension."},
 Operation{id:"citation-validate",label:"Validate CSL JSON bibliography",hint:"Validate one CSL JSON object or array (up to 10,000 records). Optional new .json report."},
 Operation{id:"pdf-info",label:"PDF information",hint:"Show the PDF version and page count. Optional new JSON output."},
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
    let bytes = read_bounded(p, 32 * 1024 * 1024)?;
    let d = Document::load_mem(&bytes).map_err(err)?;
    if d.is_encrypted() {
        return Err(Error::input("Encrypted PDF not supported"));
    }
    if d.get_pages().len() > 1000 || d.objects.len() > 100000 {
        return Err(Error::input("PDF object or page limit exceeded"));
    }
    Ok(d)
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
            let report = serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "pdf_version": document.version,
                "pages": document.get_pages().len(),
            }))?;
            if !r.output.as_os_str().is_empty() {
                extension(&r.output, "json")?;
                j.check()?;
                write_bytes(&r.output, report.as_bytes())?;
                written = Some(r.output);
            }
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
        assert!(handle(request("1,2"), Job::new()).is_err());
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
