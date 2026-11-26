#![forbid(unsafe_code)]
#[cfg(feature = "ocr")]
mod ocr;
use localwright_core::*;
use lopdf::{dictionary, Document, Object};
use std::{io::Write, path::Path};
use zip::{write::SimpleFileOptions, ZipWriter};
pub const OPERATIONS:&[Operation]=&[
 Operation{id:"markdown-docx",label:"Markdown to DOCX",hint:"Basic headings and plain paragraphs only. Choose new .docx output."},
 Operation{id:"pdf-info",label:"PDF information",hint:"Show the PDF version and page count. Optional new JSON output."},
 Operation{id:"pdf-text",label:"PDF text",hint:"Extract text from PDF text layer. Optional new .txt output. No OCR."},
 Operation{id:"pdf-split",label:"PDF page range",hint:"Parameter inclusive page range, e.g. 2-4. New .pdf output. Experimental; not sanitization."},
 Operation{id:"pdf-merge",label:"Merge PDFs",hint:"Input first PDF; parameter second PDF path. New .pdf output. Experimental page-only merge."},
 #[cfg(feature="ocr")]
 Operation{id:"ocr-png",label:"Experimental PNG OCR",hint:"Recognize English text using the private OCR pack. Optional new .txt output."}];
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
fn docx(p: &Path, source: &str, j: &Job) -> Result<()> {
    let mut body = String::new();
    for line in source.lines() {
        j.check()?;
        let hashes = line.bytes().take_while(|&c| c == b'#').count();
        let heading = (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ');
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
    let mut out = Document::with_version("1.7");
    let mut ids = vec![];
    for mut d in [a, b] {
        for (_, id) in d.get_pages() {
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
            } // Drop interactive annotations from merged pages; this is not redaction.
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
        "markdown-docx" => {
            extension(&r.output, "docx")?;
            let input = String::from_utf8(read_bounded(&r.input, 2 * 1024 * 1024)?).map_err(err)?;
            docx(&r.output, &input, &j)?;
            written = Some(r.output);
            warnings.push("Only ATX headings and literal paragraphs; inline markup, tables, images and layout are not interpreted.".into());
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
        "pdf-split" | "pdf-merge" => {
            extension(&r.output, "pdf")?;
            let mut d = load(&r.input)?;
            if r.operation == "pdf-split" {
                let (a, b) = r
                    .parameter
                    .split_once('-')
                    .ok_or_else(|| Error::input("Expected start-end"))?;
                let (a, b): (u32, u32) = (a.parse().map_err(err)?, b.parse().map_err(err)?);
                let pages = d.get_pages();
                if a == 0 || a > b || !pages.contains_key(&a) || !pages.contains_key(&b) {
                    return Err(Error::input("Invalid page range"));
                }
                let remove = pages
                    .keys()
                    .copied()
                    .filter(|n| *n < a || *n > b)
                    .collect::<Vec<_>>();
                d.delete_pages(&remove);
                d.prune_objects();
            } else {
                d = merge(d, load(Path::new(&r.parameter))?, &j)?;
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
        docx(&p, "# Café & science\nA < B", &Job::new()).unwrap();
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
    fn xml_control() {
        assert!(xml("\u{1}").is_err());
    }
}
