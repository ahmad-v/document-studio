//! Bounded CommonMark to self-contained, offline HTML conversion.

use crate::markdown_epub::{
    default_title, parse_metadata, render_safe_html_body, xml_escape, MAX_MARKDOWN_BYTES,
};
use localwright_core::{read_bounded, Error, Job, Result};
use std::path::Path;

const MAX_HTML_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn convert(path: &Path, metadata_json: &str, job: &Job) -> Result<String> {
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
    let bytes = read_bounded(path, MAX_MARKDOWN_BYTES)?;
    let source =
        std::str::from_utf8(&bytes).map_err(|_| Error::input("Markdown input must be UTF-8"))?;
    let body = render_safe_html_body(source, job)?;
    let title = metadata.title.unwrap_or_else(|| default_title(path));
    let language = metadata.language.unwrap_or_else(|| "und".to_owned());
    let html = format!(
        "<!doctype html>\n<html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; script-src 'none'; connect-src 'none'; base-uri 'none'; form-action 'none'\"><title>{}</title><style>body{{font-family:serif;line-height:1.5;margin:2rem auto;max-width:72rem;padding:0 1rem}}table{{border-collapse:collapse}}th,td{{border:1px solid #777;padding:.35em .6em}}pre{{overflow:auto;padding:1em;background:#f3f3f3}}blockquote{{border-left:.25em solid #888;margin-left:0;padding-left:1em}}</style></head><body><main>{}</main></body></html>\n",
        xml_escape(&language),
        xml_escape(&title),
        body
    );
    if html.len() > MAX_HTML_BYTES {
        return Err(Error::input("Generated HTML exceeds 32 MiB output limit"));
    }
    job.check()?;
    Ok(html)
}
