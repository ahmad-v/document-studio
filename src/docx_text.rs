use localwright_core::{read_bounded, Error, ErrorKind, Job, Result};
use quick_xml::{events::Event, name::ResolveResult, reader::NsReader, XmlVersion};
use std::{
    io::{Cursor, Read},
    path::Path,
};

const MAX_ARCHIVE: u64 = 16 * 1024 * 1024;
const MAX_DOCUMENT: u64 = 8 * 1024 * 1024;

pub fn extract(path: &Path, job: &Job) -> Result<String> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
    {
        return Err(Error::input("Expected .docx input"));
    }
    let bytes = read_bounded(path, MAX_ARCHIVE)?;
    job.check()?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(super::err)?;
    if archive.len() > 4096 {
        return Err(Error::new(
            ErrorKind::Limit,
            "DOCX archive entry limit exceeded",
        ));
    }
    let mut document = archive.by_name("word/document.xml").map_err(super::err)?;
    if document.size() > MAX_DOCUMENT {
        return Err(Error::new(
            ErrorKind::Limit,
            "DOCX document XML exceeds the byte limit",
        ));
    }
    let mut xml = Vec::new();
    let mut chunk = [0; 32 * 1024];
    loop {
        job.check()?;
        let read = document.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if (xml.len() + read) as u64 > MAX_DOCUMENT {
            return Err(Error::new(
                ErrorKind::Limit,
                "DOCX document XML exceeds the byte limit",
            ));
        }
        xml.extend_from_slice(&chunk[..read]);
    }
    if xml.len() as u64 != document.size() {
        return Err(Error::input(
            "DOCX document XML does not match its declared size",
        ));
    }
    let xml =
        std::str::from_utf8(&xml).map_err(|_| Error::input("DOCX document XML must be UTF-8"))?;
    extract_body(xml, job)
}

fn extract_body(xml: &str, job: &Job) -> Result<String> {
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut body_depth = None;
    let mut text_depth = None;
    let mut found_body = false;
    let mut found_root = false;
    let mut text = String::new();

    loop {
        job.check()?;
        let (namespace, event) = reader.read_resolved_event().map_err(super::err)?;
        let is_word = matches!(namespace, ResolveResult::Bound(ns)
            if matches!(ns.as_ref(),
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                | "http://purl.oclc.org/ooxml/wordprocessingml/main"));
        match event {
            Event::Start(element) => {
                depth += 1;
                if depth > 128 {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "DOCX XML nesting exceeds 128 levels",
                    ));
                }
                if text_depth.is_some() {
                    return Err(Error::input("Unexpected nested element in DOCX text"));
                }
                let name = element.local_name();
                if depth == 1 {
                    if found_root || !is_word || name.as_ref() != "document" {
                        return Err(Error::input("Expected a WordprocessingML document"));
                    }
                    found_root = true;
                }
                if is_word {
                    if depth == 2 && name.as_ref() == "body" {
                        if found_body {
                            return Err(Error::input("DOCX has more than one document body"));
                        }
                        found_body = true;
                        body_depth = Some(depth);
                    } else if body_depth.is_some() {
                        match name.as_ref() {
                            "t" => text_depth = Some(depth),
                            "tab" => text.push('\t'),
                            "br" | "cr" => text.push('\n'),
                            _ => {}
                        }
                    }
                }
            }
            Event::End(element) => {
                if body_depth.is_some() && is_word && element.local_name().as_ref() == "p" {
                    text.push('\n');
                }
                if text_depth == Some(depth) {
                    text_depth = None;
                }
                if body_depth == Some(depth) {
                    body_depth = None;
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::input("Unexpected DOCX closing tag"))?;
            }
            Event::Text(value) if text_depth.is_some() => {
                text.push_str(&value.xml_content(XmlVersion::Implicit1_0));
            }
            Event::CData(value) if text_depth.is_some() => {
                text.push_str(&value.xml_content(XmlVersion::Implicit1_0));
            }
            Event::GeneralRef(reference) if text_depth.is_some() => {
                if let Some(ch) = reference.resolve_char_ref().map_err(super::err)? {
                    text.push(ch);
                } else {
                    let entity = reference.as_ref();
                    text.push_str(
                        quick_xml::escape::resolve_xml_entity(entity)
                            .ok_or_else(|| Error::input("Unsupported DOCX XML entity"))?,
                    );
                }
            }
            Event::DocType(_) => {
                return Err(Error::input("DOCX XML document types are unsupported"))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !found_body || depth != 0 {
        return Err(Error::input("Incomplete DOCX document body"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_namespace_bound_body_text_and_standard_entities() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.docx");
        let output = dir.path().join("result.txt");
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:x="urn:other">
            <w:body><w:p><w:r><x:t>ignored</x:t><w:t>Caf&#233; &amp; tea</w:t><w:tab/><w:t>second</w:t><w:br/><w:t>end</w:t></w:r></w:p>
            <w:p><w:r><w:t>Body two</w:t></w:r></w:p></w:body>
        </w:document>"#;
        write_docx(&input, xml);
        let request = localwright_core::Request {
            operation: "docx-text".into(),
            input: input.clone(),
            output: output.clone(),
            ..Default::default()
        };
        let result = crate::handle(request.clone(), Job::new()).unwrap();
        assert_eq!(result.preview, "Café & tea\tsecond\nend\nBody two\n");
        assert_eq!(std::fs::read_to_string(&output).unwrap(), result.preview);
        assert!(crate::handle(request, Job::new()).is_err());

        let hostile = dir.path().join("external-entity.docx");
        write_docx(
            &hostile,
            &format!(r#"<!DOCTYPE w:document SYSTEM "file:///arbitrary">{xml}"#),
        );
        assert!(extract(&hostile, &Job::new()).is_err());
    }

    fn write_docx(path: &Path, xml: &str) {
        use std::io::Write as _;
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
}
