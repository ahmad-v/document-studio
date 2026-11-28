//! Bounded EPUB package inspection and plain-text extraction.
//!
//! This module validates the container/OPF references needed for extraction. It is
//! not a complete EPUB conformance validator or a layout renderer.

use localwright_core::{read_bounded, Error, ErrorKind, Job, Result};
use quick_xml::{
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
    XmlVersion,
};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Seek},
    path::Path,
};
use zip::{CompressionMethod, ZipArchive};

const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;
const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PACKAGE_XML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SPINE_ITEMS: usize = 2000;
const MAX_EXTRACTED_BYTES: usize = 8 * 1024 * 1024;
const EPUB_MIMETYPE: &str = "application/epub+zip";
const CONTAINER_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:container";
const OPF_NS: &str = "http://www.idpf.org/2007/opf";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

#[derive(Debug)]
struct Item {
    path: String,
    media_type: String,
}

#[derive(Debug)]
struct Package {
    title: Option<String>,
    creators: Vec<String>,
    language: Option<String>,
    spine: Vec<Item>,
    members: HashSet<String>,
    archive: ZipArchive<Cursor<Vec<u8>>>,
}

struct OpfMetadata {
    title: Option<String>,
    creators: Vec<String>,
    language: Option<String>,
    spine: Vec<Item>,
}

#[derive(Debug, Clone)]
pub(crate) struct EpubBook {
    pub(crate) title: Option<String>,
    pub(crate) creators: Vec<String>,
    pub(crate) language: Option<String>,
    pub(crate) chapters: Vec<EpubChapter>,
}

#[derive(Debug, Clone)]
pub(crate) struct EpubChapter {
    pub(crate) label: String,
    pub(crate) text: String,
}

pub fn inspect(path: &Path, job: &Job) -> Result<serde_json::Value> {
    let Package {
        title,
        creators,
        language,
        spine,
        ..
    } = load(path, job)?;
    Ok(serde_json::json!({
        "schema_version": 1,
        "format": "EPUB",
        "title": title,
        "creators": creators,
        "language": language,
        "spine_items": spine.len(),
        "extracted_text": false,
        "limitations": "Structural package inspection only. Not a complete EPUB conformance validator; no layout, CSS, images, scripts, or interactive content are rendered."
    }))
}

pub fn extract_text(path: &Path, job: &Job) -> Result<String> {
    let book = read_book(path, job)?;
    let mut output = String::new();
    if let Some(title) = &book.title {
        append_bounded(&mut output, title, job)?;
        append_bounded(&mut output, "\n\n", job)?;
    }
    for (index, chapter) in book.chapters.iter().enumerate() {
        job.check()?;
        if index > 0 && !output.ends_with("\n\n") {
            append_bounded(&mut output, "\n", job)?;
        }
        append_bounded(&mut output, &chapter.text, job)?;
        if !output.ends_with('\n') {
            append_bounded(&mut output, "\n", job)?;
        }
    }
    Ok(output)
}

pub(crate) fn read_book(path: &Path, job: &Job) -> Result<EpubBook> {
    let Package {
        title,
        creators,
        language,
        spine,
        members,
        mut archive,
        ..
    } = load(path, job)?;
    let mut extracted_bytes = title
        .as_ref()
        .map_or(0, |value| value.len().saturating_add(2));
    let mut chapters = Vec::with_capacity(spine.len());
    for (index, item) in spine.iter().enumerate() {
        job.check()?;
        if item.media_type != "application/xhtml+xml" {
            continue;
        }
        if !members.contains(&item.path) {
            return Err(Error::input("EPUB spine references a missing content item"));
        }
        let bytes = read_member(&mut archive, &item.path, MAX_ENTRY_BYTES, job)?;
        let xhtml = std::str::from_utf8(&bytes)
            .map_err(|_| Error::input("EPUB XHTML content must be UTF-8"))?;
        let chapter = extract_xhtml(xhtml, job)?;
        if chapter.trim().is_empty() {
            continue;
        }
        extracted_bytes = extracted_bytes
            .checked_add(chapter.len())
            .and_then(|size| size.checked_add(2))
            .ok_or_else(|| Error::input("EPUB extracted text size overflow"))?;
        if extracted_bytes > MAX_EXTRACTED_BYTES {
            return Err(Error::new(
                ErrorKind::Limit,
                "EPUB extracted text exceeds 8 MiB",
            ));
        }
        let label = item
            .path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .strip_suffix(".xhtml")
            .or_else(|| item.path.rsplit('/').next())
            .filter(|name| !name.is_empty())
            .map(|name| name.replace(['_', '-'], " "))
            .unwrap_or_else(|| format!("Chapter {}", index + 1));
        chapters.push(EpubChapter {
            label,
            text: chapter,
        });
    }
    Ok(EpubBook {
        title,
        creators,
        language,
        chapters,
    })
}

fn load(path: &Path, job: &Job) -> Result<Package> {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
    {
        return Err(Error::input("Expected .epub input"));
    }
    let bytes = read_bounded(path, MAX_INPUT_BYTES)?;
    job.check()?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(super::err)?;
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(Error::new(
            ErrorKind::Limit,
            "EPUB entry count is outside the supported limit",
        ));
    }
    let mut members = HashSet::with_capacity(archive.len());
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        job.check()?;
        let entry = archive.by_index(index).map_err(super::err)?;
        let name = entry.name().map_err(super::err)?.into_owned();
        let normalized = normalize_member(&name)?;
        if entry.is_symlink() {
            return Err(Error::input(
                "EPUB packages containing symbolic links are unsupported",
            ));
        }
        if !members.insert(normalized) {
            return Err(Error::input("EPUB package contains duplicate paths"));
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(Error::new(
                ErrorKind::Limit,
                "EPUB member exceeds the 16 MiB limit",
            ));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or_else(|| Error::input("EPUB expanded size overflow"))?;
        if expanded > MAX_EXPANDED_BYTES {
            return Err(Error::new(
                ErrorKind::Limit,
                "EPUB expanded size exceeds the 128 MiB limit",
            ));
        }
    }
    let first = archive.by_index(0).map_err(super::err)?;
    if first.name().map_err(super::err)?.as_ref() != "mimetype"
        || first.compression() != CompressionMethod::Stored
        || first.size() != EPUB_MIMETYPE.len() as u64
    {
        return Err(Error::input(
            "EPUB must begin with an uncompressed mimetype entry",
        ));
    }
    drop(first);
    let mimetype = read_member(&mut archive, "mimetype", 64, job)?;
    if mimetype != EPUB_MIMETYPE.as_bytes() {
        return Err(Error::input("Invalid EPUB mimetype"));
    }
    let container = read_member(
        &mut archive,
        "META-INF/container.xml",
        MAX_PACKAGE_XML_BYTES,
        job,
    )?;
    let rootfile = parse_container(&container, job)?;
    if !members.contains(&rootfile) {
        return Err(Error::input(
            "EPUB container references a missing package document",
        ));
    }
    let opf = read_member(&mut archive, &rootfile, MAX_PACKAGE_XML_BYTES, job)?;
    let OpfMetadata {
        title,
        creators,
        language,
        spine,
    } = parse_opf(&opf, &rootfile, job)?;
    if spine.is_empty() || spine.len() > MAX_SPINE_ITEMS {
        return Err(Error::new(
            ErrorKind::Limit,
            "EPUB spine is empty or exceeds 2,000 items",
        ));
    }
    for item in &spine {
        job.check()?;
        if !members.contains(&item.path) {
            return Err(Error::input("EPUB spine references a missing content item"));
        }
        if item.media_type != "application/xhtml+xml" {
            return Err(Error::input("EPUB spine contains a non-XHTML item; this text route does not convert other media"));
        }
    }
    Ok(Package {
        title,
        creators,
        language,
        spine,
        members,
        archive,
    })
}

fn read_member<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
    job: &Job,
) -> Result<Vec<u8>> {
    let mut file = archive.by_name(name).map_err(super::err)?;
    let declared = file.size();
    if declared > limit {
        return Err(Error::new(
            ErrorKind::Limit,
            "EPUB XML or content member exceeds its read limit",
        ));
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    let mut chunk = [0u8; 32 * 1024];
    loop {
        job.check()?;
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) as u64 > limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "EPUB member exceeds its decompression limit",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    if bytes.len() as u64 != declared {
        return Err(Error::input(
            "EPUB member size does not match its directory record",
        ));
    }
    Ok(bytes)
}

fn normalize_member(value: &str) -> Result<String> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(Error::input("EPUB contains an unsafe member path"));
    }
    let mut parts = Vec::new();
    for part in value.trim_end_matches('/').split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            return Err(Error::input("EPUB contains an unsafe member path"));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(Error::input("EPUB contains an empty member path"));
    }
    Ok(parts.join("/"))
}

fn resolve_href(base_file: &str, href: &str) -> Result<String> {
    let href = href.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(href)?;
    if decoded.is_empty()
        || decoded.starts_with('/')
        || decoded.contains('\\')
        || decoded.contains('\0')
        || decoded.contains(':')
    {
        return Err(Error::input(
            "EPUB manifest contains an unsafe content path",
        ));
    }
    let mut components = base_file.split('/').collect::<Vec<_>>();
    components.pop();
    for component in decoded.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(Error::input("EPUB manifest path escapes the package"));
                }
            }
            value if value.contains(':') => {
                return Err(Error::input(
                    "EPUB manifest contains a drive or scheme path",
                ))
            }
            value => components.push(value),
        }
    }
    normalize_member(&components.join("/"))
}

fn percent_decode(value: &str) -> Result<String> {
    let input = value.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            if index + 2 >= input.len() {
                return Err(Error::input("Malformed EPUB URL escape"));
            }
            let high =
                hex(input[index + 1]).ok_or_else(|| Error::input("Malformed EPUB URL escape"))?;
            let low =
                hex(input[index + 2]).ok_or_else(|| Error::input("Malformed EPUB URL escape"))?;
            let byte = high * 16 + low;
            if matches!(byte, b'/' | b'\\' | 0) {
                return Err(Error::input(
                    "Encoded separators are unsupported in EPUB paths",
                ));
            }
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| Error::input("EPUB URL path is not valid UTF-8"))
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn attr(element: &BytesStart<'_>, name: &str) -> Result<Option<String>> {
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.map_err(super::err)?;
        if attribute.key.local_name().as_ref() == name {
            return attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|value| Some(value.into_owned()))
                .map_err(super::err);
        }
    }
    Ok(None)
}

fn bound(namespace: &ResolveResult<'_>, expected: &str) -> bool {
    matches!(namespace, ResolveResult::Bound(value) if value.as_ref() == expected)
}

fn parse_container(bytes: &[u8], job: &Job) -> Result<String> {
    let xml =
        std::str::from_utf8(bytes).map_err(|_| Error::input("EPUB container XML must be UTF-8"))?;
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut found_root = false;
    let mut rootfile = None;
    loop {
        job.check()?;
        let (namespace, event) = reader.read_resolved_event().map_err(super::err)?;
        match event {
            Event::Start(element) => {
                depth += 1;
                if depth > 64 {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "EPUB container XML nesting exceeds 64 levels",
                    ));
                }
                let name = element.local_name();
                if depth == 1 {
                    if !bound(&namespace, CONTAINER_NS) || name.as_ref() != "container" {
                        return Err(Error::input("Invalid EPUB container root"));
                    }
                    found_root = true;
                }
                if bound(&namespace, CONTAINER_NS) && name.as_ref() == "rootfile" {
                    let path = attr(&element, "full-path")?
                        .ok_or_else(|| Error::input("EPUB rootfile is missing full-path"))?;
                    let media_type = attr(&element, "media-type")?;
                    if rootfile.is_none()
                        && media_type
                            .as_deref()
                            .is_none_or(|value| value == "application/oebps-package+xml")
                    {
                        rootfile = Some(normalize_member(&percent_decode(&path)?)?);
                    }
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::input("Unexpected EPUB container closing tag"))?;
            }
            Event::DocType(_) => {
                return Err(Error::input("EPUB XML document types are unsupported"))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !found_root || depth != 0 {
        return Err(Error::input("Incomplete EPUB container document"));
    }
    rootfile.ok_or_else(|| Error::input("EPUB container has no package root"))
}

fn parse_opf(bytes: &[u8], opf_path: &str, job: &Job) -> Result<OpfMetadata> {
    let xml =
        std::str::from_utf8(bytes).map_err(|_| Error::input("EPUB package XML must be UTF-8"))?;
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut found_root = false;
    let mut metadata_depth = None;
    let mut manifest_depth = None;
    let mut spine_depth = None;
    let mut text_target: Option<(usize, String)> = None;
    let mut title = None;
    let mut creators = Vec::new();
    let mut language = None;
    let mut manifest = BTreeMap::<String, Item>::new();
    let mut spine_ids = Vec::new();
    loop {
        job.check()?;
        let (namespace, event) = reader.read_resolved_event().map_err(super::err)?;
        match event {
            Event::Start(element) => {
                depth += 1;
                if depth > 128 {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "EPUB package XML nesting exceeds 128 levels",
                    ));
                }
                let name = element.local_name();
                if depth == 1 {
                    if !bound(&namespace, OPF_NS) || name.as_ref() != "package" {
                        return Err(Error::input("Invalid EPUB package document root"));
                    }
                    found_root = true;
                }
                if bound(&namespace, OPF_NS) {
                    match name.as_ref() {
                        "metadata" => metadata_depth = Some(depth),
                        "manifest" => manifest_depth = Some(depth),
                        "spine" => spine_depth = Some(depth),
                        "item" if manifest_depth.is_some() => {
                            let id = attr(&element, "id")?.ok_or_else(|| {
                                Error::input("EPUB manifest item is missing an id")
                            })?;
                            let href = attr(&element, "href")?.ok_or_else(|| {
                                Error::input("EPUB manifest item is missing href")
                            })?;
                            let media_type = attr(&element, "media-type")?.ok_or_else(|| {
                                Error::input("EPUB manifest item is missing media-type")
                            })?;
                            let path = resolve_href(opf_path, &href)?;
                            if manifest.insert(id, Item { path, media_type }).is_some() {
                                return Err(Error::input("EPUB manifest contains duplicate ids"));
                            }
                        }
                        "itemref" if spine_depth.is_some() => {
                            let idref = attr(&element, "idref")?
                                .ok_or_else(|| Error::input("EPUB spine item is missing idref"))?;
                            spine_ids.push(idref);
                            if spine_ids.len() > MAX_SPINE_ITEMS {
                                return Err(Error::new(
                                    ErrorKind::Limit,
                                    "EPUB spine exceeds 2,000 items",
                                ));
                            }
                        }
                        _ => {}
                    }
                } else if bound(&namespace, DC_NS)
                    && metadata_depth.is_some()
                    && matches!(name.as_ref(), "title" | "creator" | "language")
                {
                    text_target = Some((depth, String::new()));
                }
            }
            Event::Text(value) if text_target.is_some() => {
                let (_, text) = text_target.as_mut().expect("checked target");
                text.push_str(&value.xml_content(XmlVersion::Implicit1_0));
                if text.len() > 4096 {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "EPUB metadata text is too long",
                    ));
                }
            }
            Event::GeneralRef(reference) if text_target.is_some() => {
                let (_, text) = text_target.as_mut().expect("checked target");
                if let Some(character) = reference.resolve_char_ref().map_err(super::err)? {
                    text.push(character);
                } else {
                    text.push_str(
                        quick_xml::escape::resolve_xml_entity(reference.as_ref())
                            .ok_or_else(|| Error::input("Unsupported EPUB XML entity"))?,
                    );
                }
            }
            Event::End(element) => {
                let local = element.local_name();
                if text_target
                    .as_ref()
                    .is_some_and(|(target_depth, _)| *target_depth == depth)
                {
                    let (_, value) = text_target
                        .take()
                        .ok_or_else(|| Error::input("EPUB metadata parser lost its text state"))?;
                    let text = value.trim().to_owned();
                    match local.as_ref() {
                        "title" if title.is_none() => title = Some(text),
                        "creator" if creators.len() < 64 => creators.push(text),
                        "language" if language.is_none() => language = Some(text),
                        _ => {}
                    }
                }
                if metadata_depth == Some(depth) {
                    metadata_depth = None;
                }
                if manifest_depth == Some(depth) {
                    manifest_depth = None;
                }
                if spine_depth == Some(depth) {
                    spine_depth = None;
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::input("Unexpected EPUB package closing tag"))?;
            }
            Event::DocType(_) => {
                return Err(Error::input("EPUB XML document types are unsupported"))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !found_root || depth != 0 {
        return Err(Error::input("Incomplete EPUB package document"));
    }
    let mut spine = Vec::with_capacity(spine_ids.len());
    for id in spine_ids {
        spine.push(
            manifest
                .remove(&id)
                .ok_or_else(|| Error::input("EPUB spine references a missing manifest id"))?,
        );
    }
    Ok(OpfMetadata {
        title,
        creators,
        language,
        spine,
    })
}

fn extract_xhtml(xml: &str, job: &Job) -> Result<String> {
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut body_depth = None;
    let mut ignore_depth = None;
    let mut found_root = false;
    let mut found_body = false;
    let mut output = String::new();
    loop {
        job.check()?;
        let (namespace, event) = reader.read_resolved_event().map_err(super::err)?;
        match event {
            Event::Start(element) => {
                depth += 1;
                if depth > 128 {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "EPUB XHTML nesting exceeds 128 levels",
                    ));
                }
                let name = element.local_name();
                if depth == 1 {
                    if !bound(&namespace, XHTML_NS) || name.as_ref() != "html" {
                        return Err(Error::input("EPUB spine document is not XHTML"));
                    }
                    found_root = true;
                }
                if bound(&namespace, XHTML_NS) {
                    if depth == 2 && name.as_ref() == "body" {
                        body_depth = Some(depth);
                        found_body = true;
                    }
                    if body_depth.is_some() && matches!(name.as_ref(), "script" | "style") {
                        ignore_depth = Some(depth);
                    }
                    if body_depth.is_some()
                        && ignore_depth.is_none()
                        && matches!(name.as_ref(), "br" | "hr")
                    {
                        append_bounded(&mut output, "\n", job)?;
                    }
                }
            }
            Event::Text(value) if body_depth.is_some() && ignore_depth.is_none() => {
                let text = value.xml_content(XmlVersion::Implicit1_0);
                append_bounded(&mut output, &text, job)?;
            }
            Event::GeneralRef(reference) if body_depth.is_some() && ignore_depth.is_none() => {
                if let Some(character) = reference.resolve_char_ref().map_err(super::err)? {
                    let text = character.to_string();
                    append_bounded(&mut output, &text, job)?;
                } else {
                    append_bounded(
                        &mut output,
                        quick_xml::escape::resolve_xml_entity(reference.as_ref())
                            .ok_or_else(|| Error::input("Unsupported XHTML entity"))?,
                        job,
                    )?;
                }
            }
            Event::CData(value) if body_depth.is_some() && ignore_depth.is_none() => {
                let text = value.xml_content(XmlVersion::Implicit1_0);
                append_bounded(&mut output, &text, job)?;
            }
            Event::End(element) => {
                let name = element.local_name();
                if ignore_depth == Some(depth) {
                    ignore_depth = None;
                }
                if body_depth.is_some()
                    && ignore_depth.is_none()
                    && bound(&namespace, XHTML_NS)
                    && matches!(
                        name.as_ref(),
                        "p" | "div"
                            | "li"
                            | "section"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "blockquote"
                            | "tr"
                    )
                    && !output.ends_with('\n')
                {
                    append_bounded(&mut output, "\n", job)?;
                }
                if body_depth == Some(depth) {
                    body_depth = None;
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::input("Unexpected XHTML closing tag"))?;
            }
            Event::DocType(_) => {
                return Err(Error::input("EPUB XHTML document types are unsupported"))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !found_root || !found_body || depth != 0 {
        return Err(Error::input("Incomplete EPUB XHTML document"));
    }
    Ok(output)
}

fn append_bounded(output: &mut String, text: &str, job: &Job) -> Result<()> {
    job.check()?;
    if output.len().saturating_add(text.len()) > MAX_EXTRACTED_BYTES {
        return Err(Error::new(
            ErrorKind::Limit,
            "EPUB extracted text exceeds 8 MiB",
        ));
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    const CONTAINER: &str = r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    const OPF: &str = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Sample &amp; Book</dc:title><dc:creator>Author</dc:creator><dc:language>en</dc:language></metadata><manifest><item id="one" href="chapter%201.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="cover.png" media-type="image/png"/></manifest><spine><itemref idref="one"/></spine></package>"#;
    const XHTML: &str = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>hidden title</title><script>ignored()</script></head><body><h1>Hello</h1><p>Fish &amp; chips</p><script>do not execute</script><p>Final.</p></body></html>"#;

    fn fixture(path: &Path, opf: &str, xhtml: &str) {
        let mut archive = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        archive
            .start_file(
                "mimetype",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        archive.write_all(EPUB_MIMETYPE.as_bytes()).unwrap();
        for (name, data) in [
            ("META-INF/container.xml", CONTAINER),
            ("OPS/package.opf", opf),
            ("OPS/chapter 1.xhtml", xhtml),
            ("OPS/cover.png", "test cover"),
        ] {
            archive
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .unwrap();
            archive.write_all(data.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn inspects_epub_metadata_in_spine_order_and_extracts_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        fixture(&path, OPF, XHTML);
        let report = inspect(&path, &Job::new()).unwrap();
        assert_eq!(report["title"], "Sample & Book");
        assert_eq!(report["creators"][0], "Author");
        assert_eq!(report["language"], "en");
        assert_eq!(report["spine_items"], 1);
        assert_eq!(report["extracted_text"], false);
        let text = extract_text(&path, &Job::new()).unwrap();
        assert!(text.contains("Hello\nFish & chips\nFinal."));
        assert!(!text.contains("do not execute"));
        assert!(!text.contains("hidden title"));
        let book = read_book(&path, &Job::new()).unwrap();
        assert_eq!(book.title.as_deref(), Some("Sample & Book"));
        assert_eq!(book.creators, vec!["Author".to_owned()]);
        assert_eq!(book.language.as_deref(), Some("en"));
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].label, "chapter 1");
        assert!(book.chapters[0].text.contains("Fish & chips"));
    }

    #[test]
    fn native_operation_routes_write_new_metadata_and_text_reports() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("book.epub");
        let metadata = dir.path().join("book.json");
        let text = dir.path().join("book.txt");
        fixture(&input, OPF, XHTML);
        let inspected = crate::handle(
            localwright_core::Request {
                operation: "epub-inspect".into(),
                input: input.clone(),
                output: metadata.clone(),
                ..Default::default()
            },
            Job::new(),
        )
        .unwrap();
        assert_eq!(inspected.written.as_deref(), Some(metadata.as_path()));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&inspected.preview).unwrap()["format"],
            "EPUB"
        );
        #[cfg(feature = "gui")]
        {
            let reader = crate::handle(
                localwright_core::Request {
                    operation: "epub-reader".into(),
                    input: dir.path().join("book.epub"),
                    ..Default::default()
                },
                Job::new(),
            )
            .unwrap();
            assert!(reader.preview.contains("native EPUB reader"));
        }
        let extracted = crate::handle(
            localwright_core::Request {
                operation: "epub-text".into(),
                input,
                output: text.clone(),
                ..Default::default()
            },
            Job::new(),
        )
        .unwrap();
        assert_eq!(extracted.written.as_deref(), Some(text.as_path()));
        assert!(std::fs::read_to_string(&text)
            .unwrap()
            .contains("Fish & chips"));
        assert!(crate::handle(
            localwright_core::Request {
                operation: "epub-text".into(),
                input: dir.path().join("book.epub"),
                output: text,
                ..Default::default()
            },
            Job::new(),
        )
        .is_err());
    }

    #[test]
    fn rejects_manifest_traversal_doctypes_and_wrong_mimetype() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        let traversal = OPF.replace("chapter%201.xhtml", "%2e%2e/%2e%2e/secret.xhtml");
        fixture(&path, &traversal, XHTML);
        assert!(inspect(&path, &Job::new()).is_err());
        fixture(&path, OPF, &XHTML.replace("<html", "<!DOCTYPE html><html"));
        assert!(extract_text(&path, &Job::new()).is_err());

        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        archive
            .start_file("mimetype", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"not-an-epub").unwrap();
        archive.finish().unwrap();
        assert!(inspect(&path, &Job::new()).is_err());
    }

    #[test]
    fn rejects_unsafe_archive_member_paths() {
        for path in ["../outside", "/absolute", "C:/drive", "a\\b", "a//b"] {
            assert!(normalize_member(path).is_err(), "{path}");
        }
    }
}
