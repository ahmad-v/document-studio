//! Bounded structural Markdown to EPUB 3 conversion.
//!
//! This is an offline text/structure conversion, not a page-layout converter.
//! Remote links and image resources are intentionally omitted so generated books
//! never fetch content when opened by another reader.

use localwright_core::{read_bounded, Error, Job, Result};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};
use std::{
    io::{Cursor, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const MAX_MARKDOWN_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EPUB_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct Metadata {
    title: Option<String>,
    language: Option<String>,
}

/// Build one self-contained EPUB 3 chapter from a Markdown source.
///
/// `metadata_json` may contain `title` and `language`. Unknown fields are
/// rejected so typos do not silently produce unintended book metadata.
pub(crate) fn convert(path: &Path, metadata_json: &str, job: &Job) -> Result<Vec<u8>> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
    {
        return Err(Error::input("Expected .md or .markdown input"));
    }
    let metadata = parse_metadata(metadata_json)?;
    let source = read_bounded(path, MAX_MARKDOWN_BYTES)?;
    let source =
        std::str::from_utf8(&source).map_err(|_| Error::input("Markdown input must be UTF-8"))?;
    job.check()?;

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let mut body = String::new();
    let mut cancelled = false;
    {
        let events = Parser::new_ext(source, options)
            .take_while(|_| {
                cancelled = job.check().is_err();
                !cancelled
            })
            .filter_map(|event| {
                match event {
                    // Keep visible text but discard navigation and image URLs. Markdown
                    // raw HTML is rendered as escaped text, never interpreted as markup.
                    Event::Start(Tag::Link { .. })
                    | Event::End(TagEnd::Link)
                    | Event::Start(Tag::Image { .. })
                    | Event::End(TagEnd::Image) => None,
                    Event::Html(text) | Event::InlineHtml(text) => Some(Event::Text(text)),
                    other => Some(other),
                }
            });
        html::push_html(&mut body, events);
    }
    if cancelled {
        job.check()?;
    }
    let title = metadata.title.unwrap_or_else(|| default_title(path));
    let language = metadata.language.unwrap_or_else(|| "und".to_owned());
    let title_xml = xml_escape(&title);
    let language_xml = xml_escape(&language);
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{language_xml}\" lang=\"{language_xml}\"><head><title>{title_xml}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head><body>{body}</body></html>"
    );
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{language_xml}\" lang=\"{language_xml}\"><head><title>Contents</title></head><body><nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol><li><a href=\"chapter-001.xhtml\">{title_xml}</a></li></ol></nav></body></html>"
    );
    let mut identity = Vec::with_capacity(source.len() + title.len() + language.len());
    identity.extend_from_slice(source.as_bytes());
    identity.extend_from_slice(title.as_bytes());
    identity.extend_from_slice(language.as_bytes());
    let identifier = format!(
        "urn:document-studio:markdown:{:016x}",
        stable_hash(&identity)
    );
    let package = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"book-id\" xml:lang=\"{language_xml}\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"book-id\">{identifier}</dc:identifier><dc:title>{title_xml}</dc:title><dc:language>{language_xml}</dc:language></metadata><manifest><item id=\"chapter\" href=\"chapter-001.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/><item id=\"style\" href=\"style.css\" media-type=\"text/css\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>"
    );
    let container = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" version=\"1.0\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let compressed = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("mimetype", stored)
        .map_err(|error| Error::engine(error.to_string()))?;
    writer
        .write_all(b"application/epub+zip")
        .map_err(|error| Error::engine(error.to_string()))?;
    for (name, bytes) in [
        ("META-INF/container.xml", container.as_bytes()),
        ("OEBPS/content.opf", package.as_bytes()),
        ("OEBPS/nav.xhtml", nav.as_bytes()),
        ("OEBPS/chapter-001.xhtml", content.as_bytes()),
        (
            "OEBPS/style.css",
            b"body { font-family: serif; line-height: 1.45; margin: 5%; } img { max-width: 100%; } table { border-collapse: collapse; } th, td { border: 1px solid; padding: 0.25em; }" as &[u8],
        ),
    ] {
        job.check()?;
        writer
            .start_file(name, compressed)
            .map_err(|error| Error::engine(error.to_string()))?;
        writer
            .write_all(bytes)
            .map_err(|error| Error::engine(error.to_string()))?;
    }
    let bytes = writer
        .finish()
        .map_err(|error| Error::engine(error.to_string()))?
        .into_inner();
    if bytes.len() > MAX_EPUB_BYTES {
        return Err(Error::input("Generated EPUB exceeds 32 MiB output limit"));
    }
    job.check()?;
    Ok(bytes)
}

fn parse_metadata(json: &str) -> Result<Metadata> {
    if json.trim().is_empty() {
        return Ok(Metadata::default());
    }
    if json.len() > 4096 {
        return Err(Error::input("EPUB metadata parameter exceeds 4 KiB"));
    }
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| Error::input(format!("Expected metadata JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::input("EPUB metadata must be a JSON object"))?;
    if object.keys().any(|key| key != "title" && key != "language") {
        return Err(Error::input(
            "Only title and language EPUB metadata are supported",
        ));
    }
    let title = object
        .get("title")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::input("title must be a string"))
        })
        .transpose()?
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if title
        .as_ref()
        .is_some_and(|value| value.chars().count() > 200)
    {
        return Err(Error::input("EPUB title exceeds 200 characters"));
    }
    let language = object
        .get("language")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::input("language must be a string"))
        })
        .transpose()?
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if language.as_ref().is_some_and(|value| {
        value.len() > 35
            || value.starts_with('-')
            || value.ends_with('-')
            || value.split('-').any(|part| {
                part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
    }) {
        return Err(Error::input("language must be a BCP 47-style language tag"));
    }
    Ok(Metadata { title, language })
}

fn default_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Untitled")
        .to_owned()
}

fn xml_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            character if character == '\t' || character == '\n' || character >= ' ' => {
                output.push(character)
            }
            _ => output.push('\u{fffd}'),
        }
    }
    output
}

fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
