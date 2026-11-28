//! Bounded structural conversion through the application-private Pandoc runtime.
//!
//! This route does not preserve page layout and deliberately does not render PDF.

use localwright_core::{
    engine::{EngineInvocation, EngineRuntime},
    read_bounded, write_bytes, Capability, Error, ErrorKind, Job, Outcome, Result,
};
use serde::Deserialize;
use std::{io::Cursor, path::Path};
use zip::ZipArchive;

const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PARAMETER_BYTES: usize = 2 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_CONTAINER_ENTRIES: usize = 4096;
const MAX_CONTAINER_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CONTAINER_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversionMetadata {
    title: Option<String>,
    author: Option<String>,
    language: Option<String>,
}

fn input_format(path: &Path) -> Result<&'static str> {
    match extension(path).as_deref() {
        Some("txt" | "md" | "markdown" | "mkd") => Ok("markdown"),
        Some("html" | "htm") => Ok("html"),
        Some("docx") => Ok("docx"),
        Some("odt") => Ok("odt"),
        Some("epub") => Ok("epub"),
        Some("tex" | "latex") => Ok("latex"),
        _ => Err(Error::input(
            "Pandoc conversion input must be TXT, Markdown, HTML, DOCX, ODT, EPUB or LaTeX",
        )),
    }
}

fn output_format(path: &Path) -> Result<&'static str> {
    match extension(path).as_deref() {
        Some("txt") => Ok("plain"),
        Some("md" | "markdown") => Ok("markdown"),
        Some("html" | "htm") => Ok("html5"),
        Some("docx") => Ok("docx"),
        Some("odt") => Ok("odt"),
        Some("epub") => Ok("epub3"),
        Some("tex" | "latex") => Ok("latex"),
        Some("pdf") => Err(Error::input(
            "Pandoc does not render PDF in this offline route; PDF output needs a separate typesetting engine",
        )),
        _ => Err(Error::input(
            "Pandoc conversion output must use TXT, Markdown, HTML, DOCX, ODT, EPUB or LaTeX",
        )),
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

fn metadata(parameter: &str) -> Result<ConversionMetadata> {
    if parameter.len() > MAX_PARAMETER_BYTES {
        return Err(Error::new(
            ErrorKind::Limit,
            "Conversion metadata exceeds 2 KiB",
        ));
    }
    if parameter.trim().is_empty() {
        return Ok(ConversionMetadata::default());
    }
    let values: ConversionMetadata = serde_json::from_str(parameter)?;
    for value in [&values.title, &values.author].into_iter().flatten() {
        if value.trim().is_empty()
            || value.chars().count() > 256
            || value.chars().any(char::is_control)
        {
            return Err(Error::input(
                "Document title and author must contain 1–256 printable characters",
            ));
        }
    }
    if let Some(language) = &values.language {
        if language.is_empty()
            || language.len() > 35
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || language.starts_with('-')
            || language.ends_with('-')
        {
            return Err(Error::input(
                "Language must be a short BCP 47-style ASCII tag such as en or en-US",
            ));
        }
    }
    Ok(values)
}

fn preflight_container(bytes: &[u8], source_format: &str, job: &Job) -> Result<()> {
    if !matches!(source_format, "docx" | "odt" | "epub") {
        return Ok(());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::input("Office and EPUB inputs must be valid ZIP containers"))?;
    if archive.is_empty() || archive.len() > MAX_CONTAINER_ENTRIES {
        return Err(Error::new(
            ErrorKind::Limit,
            "Office or EPUB archive entry count is outside the supported limit",
        ));
    }
    let mut expanded = 0_u64;
    for index in 0..archive.len() {
        job.check()?;
        let entry = archive
            .by_index(index)
            .map_err(|_| Error::input("Office or EPUB archive contains an invalid entry"))?;
        if entry.is_symlink() || entry.enclosed_name().is_none() {
            return Err(Error::input(
                "Office or EPUB archive contains a symlink or unsafe path",
            ));
        }
        if entry.size() > MAX_CONTAINER_ENTRY_BYTES {
            return Err(Error::new(
                ErrorKind::Limit,
                "Office or EPUB archive entry exceeds the 64 MiB limit",
            ));
        }
        expanded = expanded.checked_add(entry.size()).ok_or_else(|| {
            Error::new(
                ErrorKind::Limit,
                "Archive expanded-size accounting overflow",
            )
        })?;
        if expanded > MAX_CONTAINER_EXPANDED_BYTES {
            return Err(Error::new(
                ErrorKind::Limit,
                "Office or EPUB archive exceeds the 256 MiB expanded-size limit",
            ));
        }
    }
    Ok(())
}

fn invocation_arguments(
    source_format: &str,
    destination_format: &str,
    metadata: &ConversionMetadata,
) -> Vec<String> {
    let mut args = vec![
        format!("--from={source_format}"),
        format!("--to={destination_format}"),
        "--standalone".into(),
        "--wrap=none".into(),
        "-".into(),
    ];
    for (key, value) in [
        ("title", &metadata.title),
        ("author", &metadata.author),
        ("lang", &metadata.language),
    ] {
        if let Some(value) = value {
            args.insert(args.len() - 1, format!("--metadata={key}:{value}"));
        }
    }
    args
}

pub(crate) fn convert(input: &Path, output: &Path, parameter: &str, job: &Job) -> Result<Outcome> {
    job.check()?;
    let source_format = input_format(input)?;
    let destination_format = output_format(output)?;
    if output.file_name().is_none() || output.exists() {
        return Err(Error::input(
            "Choose a new output path; conversion never replaces an existing file",
        ));
    }
    let metadata = metadata(parameter)?;
    let bytes = read_bounded(input, MAX_INPUT_BYTES)?;
    preflight_container(&bytes, source_format, job)?;
    job.check()?;

    let runtime = EngineRuntime::discover_current_executable().map_err(|error| {
        Error::engine(format!(
            "Private Pandoc engine pack is unavailable: {}",
            error.message
        ))
    })?;
    let result = runtime.run(
        "document-pandoc",
        EngineInvocation {
            args: invocation_arguments(source_format, destination_format, &metadata),
            stdin: bytes,
            ..EngineInvocation::default()
        },
        job,
    )?;
    if result.stdout.is_empty() {
        return Err(Error::engine("Pandoc produced an empty document"));
    }
    if result.stdout.len() > MAX_OUTPUT_BYTES {
        return Err(Error::new(
            ErrorKind::Limit,
            "Converted document exceeds the 64 MiB output limit",
        ));
    }
    job.check()?;
    let output_bytes = result.stdout.len();
    write_bytes(output, &result.stdout)?;

    let mut warnings = vec![
        "Structural conversion only: page layout, unsupported elements, embedded media, annotations and application-specific features may change or be omitted. PDF rendering is not part of this route.".into(),
    ];
    if !result.stderr.is_empty() {
        let details = String::from_utf8_lossy(&result.stderr);
        warnings.push(format!(
            "Pandoc reported conversion notes: {}",
            details.trim().chars().take(2048).collect::<String>()
        ));
    }
    Ok(Outcome {
        preview: format!(
            "Converted {source_format} to {destination_format} structurally ({output_bytes} bytes)"
        ),
        warnings,
        written: Some(output.to_path_buf()),
        draft: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_supported_structural_formats() {
        assert_eq!(input_format(Path::new("input.MD")).unwrap(), "markdown");
        assert_eq!(input_format(Path::new("input.txt")).unwrap(), "markdown");
        assert_eq!(input_format(Path::new("input.docx")).unwrap(), "docx");
        assert_eq!(input_format(Path::new("input.epub")).unwrap(), "epub");
        assert_eq!(output_format(Path::new("output.odt")).unwrap(), "odt");
        assert_eq!(output_format(Path::new("output.tex")).unwrap(), "latex");
        assert!(output_format(Path::new("output.pdf")).is_err());
        assert!(input_format(Path::new("input.exe")).is_err());
    }

    #[test]
    fn conversion_metadata_is_bounded_and_passed_as_arguments() {
        let values =
            metadata(r#"{"title":"Lab report","author":"A. Researcher","language":"en-GB"}"#)
                .unwrap();
        let args = invocation_arguments("markdown", "docx", &values);
        assert!(args
            .iter()
            .any(|value| value == "--metadata=title:Lab report"));
        assert!(args
            .iter()
            .any(|value| value == "--metadata=author:A. Researcher"));
        assert!(args.iter().any(|value| value == "--metadata=lang:en-GB"));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(metadata(r#"{"language":"en--US"}"#).is_err());
        assert!(metadata(r#"{"external_filter":"/tmp/filter"}"#).is_err());
        assert!(metadata(&format!("{{\"title\":\"{}\"}}", "x".repeat(300))).is_err());
    }
}
