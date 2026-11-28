//! Experimental PNG OCR through an application-private Tesseract executable.

use localwright_core::{
    engine::{EngineInvocation, EngineRunOptions, EngineRuntime, EngineSpec},
    Capability,
};
use localwright_core::{read_bounded, Error, ErrorKind, Job, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::{
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_TEXT: u64 = 4 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
const MAX_PDF_PAGES: usize = 20;
const MAX_SEARCHABLE_PDF_BYTES: usize = 128 * 1024 * 1024;

fn validate_pdf_page_count(pages: usize) -> Result<()> {
    if pages == 0 || pages > MAX_PDF_PAGES {
        return Err(Error::new(
            ErrorKind::Limit,
            format!("OCR accepts between 1 and {MAX_PDF_PAGES} PDF pages per job"),
        ));
    }
    Ok(())
}

fn component(root: &Path, name: &str, pinned: Option<&str>, limit: u64) -> Result<PathBuf> {
    let expected = pinned.ok_or_else(|| Error::engine("OCR build has no pinned engine pack"))?;
    let path = root.join(name);
    let canonical = path
        .canonicalize()
        .map_err(|_| Error::engine(format!("Private OCR pack is missing {name}")))?;
    if canonical != path {
        return Err(Error::engine("OCR pack paths must not redirect"));
    }
    let bytes = read_bounded(&path, limit)?;
    if !format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(expected) {
        return Err(Error::engine(format!(
            "OCR pack checksum mismatch for {name}"
        )));
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn private_libraries(root: &Path, job: &Job) -> Result<Option<std::ffi::OsString>> {
    let directory = root.join("lib");
    let Some(pin) = option_env!("DOCUMENT_OCR_LIBRARIES_SHA256") else {
        if directory.exists() {
            return Err(Error::engine(
                "Private OCR libraries require a build-time manifest pin",
            ));
        }
        return Ok(None);
    };
    if directory.canonicalize()? != directory {
        return Err(Error::engine(
            "Private OCR library directory must not redirect",
        ));
    }
    let manifest_path = root.join("libraries.json");
    if manifest_path.canonicalize()? != manifest_path {
        return Err(Error::engine("OCR library manifest must not redirect"));
    }
    let bytes = read_bounded(&manifest_path, 64 * 1024)?;
    if !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(pin) {
        return Err(Error::engine("OCR library manifest checksum mismatch"));
    }
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    let libraries = manifest["libraries"]
        .as_object()
        .filter(|entries| !entries.is_empty() && entries.len() <= 128)
        .ok_or_else(|| Error::engine("Invalid OCR library manifest"))?;
    if manifest["schema_version"].as_u64() != Some(1) {
        return Err(Error::engine("Unsupported OCR library manifest version"));
    }
    let mut count = 0;
    let mut total_bytes = 0u64;
    for entry in fs::read_dir(&directory)? {
        job.check()?;
        let entry = entry?;
        count += 1;
        let name = entry.file_name();
        let name = name
            .to_str()
            .filter(|name| {
                name.contains(".so")
                    && !name.starts_with('.')
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
            })
            .ok_or_else(|| Error::engine("Invalid private OCR library name"))?;
        let expected = libraries
            .get(name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::engine(format!("Unlisted OCR library: {name}")))?;
        let metadata = entry.metadata()?;
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .filter(|size| *size <= 256 * 1024 * 1024)
            .ok_or_else(|| Error::engine("OCR library pack exceeds the byte limit"))?;
        component(&directory, name, Some(expected), 64 * 1024 * 1024)?;
    }
    if count != libraries.len() {
        return Err(Error::engine("OCR library pack is incomplete"));
    }
    std::env::join_paths([directory])
        .map(Some)
        .map_err(|error| Error::engine(error.to_string()))
}

fn png(input: &Path) -> Result<Vec<u8>> {
    let bytes = read_bounded(input, 16 * 1024 * 1024)?;
    if bytes.get(..16) != Some(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR") {
        return Err(Error::input("OCR expects a PNG image"));
    }
    let dimensions = bytes
        .get(16..24)
        .ok_or_else(|| Error::input("Incomplete PNG dimensions"))?;
    let width = u32::from_be_bytes(dimensions[..4].try_into().unwrap());
    let height = u32::from_be_bytes(dimensions[4..].try_into().unwrap());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::new(
            ErrorKind::Limit,
            "PNG exceeds the OCR pixel limit",
        ));
    }
    Ok(bytes)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn wait(child: &mut Child, job: &Job) -> Result<ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        job.check()?;
        if Instant::now() >= deadline {
            return Err(Error::engine("OCR timed out"));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn recognize(input: &Path, job: &Job) -> Result<String> {
    let bytes = run(input, false, job)?;
    String::from_utf8(bytes).map_err(|_| Error::engine("OCR did not produce UTF-8 text"))
}

pub fn searchable_pdf(input: &Path, job: &Job) -> Result<Vec<u8>> {
    let bytes = run(input, true, job)?;
    let document = lopdf::Document::load_mem(&bytes)
        .map_err(|_| Error::engine("OCR did not produce a valid PDF"))?;
    if document.is_encrypted() || document.get_pages().len() != 1 {
        return Err(Error::engine("Expected a one-page unencrypted OCR PDF"));
    }
    job.check()?;
    Ok(bytes)
}

/// Render a bounded number of PDF pages and create a searchable, raster-page PDF.
/// The original PDF is never modified; page annotations, forms and bookmarks are not retained.
pub fn searchable_pdf_document(input: &Path, job: &Job) -> Result<Vec<u8>> {
    let source = read_bounded(input, crate::pdf_workspace::MAX_DOCUMENT_BYTES)?;
    let inspected = crate::load_pdf_bytes(&source)?;
    let page_count = inspected.get_pages().len();
    validate_pdf_page_count(page_count)?;

    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::hayro_syntax::Pdf::new(source)
    }))
    .map_err(|_| Error::input("PDF renderer stopped unexpectedly while parsing the document"))?
    .map_err(|_| Error::input("PDF renderer could not parse the document"))?;
    let pages = rendered.pages();
    if pages.len() != page_count {
        return Err(Error::input(
            "PDF parser and renderer disagree about the page count",
        ));
    }

    let scratch = tempfile::tempdir()?;
    let mut combined = None;
    let mut generated_bytes = 0usize;
    for (index, page) in pages.iter().enumerate() {
        job.check()?;
        let page_number = index + 1;
        let pixels = crate::render_page_bitmap(
            page,
            page_number,
            crate::pdf_workspace::PREVIEW_MAX_EDGE as u16,
            job,
        )?;
        let rgba = image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.pixels)
            .ok_or_else(|| Error::engine("PDF renderer returned an invalid pixel buffer"))?;
        let image_path = scratch.path().join(format!("page-{page_number}.png"));
        let mut file = fs::File::create(&image_path)?;
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut file, image::ImageFormat::Png)
            .map_err(|error| Error::engine(error.to_string()))?;
        job.check()?;

        let pdf_bytes = run(&image_path, true, job)?;
        generated_bytes = generated_bytes
            .checked_add(pdf_bytes.len())
            .filter(|total| *total <= MAX_SEARCHABLE_PDF_BYTES)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Limit,
                    "Searchable PDF exceeds the 128 MiB output limit",
                )
            })?;
        let document = crate::load_pdf_bytes(&pdf_bytes)?;
        if document.get_pages().len() != 1 {
            return Err(Error::engine(
                "OCR engine did not produce exactly one output page",
            ));
        }
        combined = Some(match combined {
            Some(previous) => crate::merge(previous, document, job)?,
            None => document,
        });
        job.progress(page_number as u64, page_count as u64);
    }

    let mut output = Vec::new();
    combined
        .ok_or_else(|| Error::engine("OCR produced no document pages"))?
        .save_to(&mut output)
        .map_err(|error| Error::engine(error.to_string()))?;
    if output.len() > MAX_SEARCHABLE_PDF_BYTES {
        return Err(Error::new(
            ErrorKind::Limit,
            "Searchable PDF exceeds the 128 MiB output limit",
        ));
    }
    job.check()?;
    Ok(output)
}

fn run(input: &Path, pdf: bool, job: &Job) -> Result<Vec<u8>> {
    const MAX_PDF: u64 = 64 * 1024 * 1024;
    let output_limit = if pdf { MAX_PDF } else { MAX_TEXT };
    job.check()?;
    let app = std::env::current_exe()?.canonicalize()?;
    let root = app
        .parent()
        .ok_or_else(|| Error::engine("Application directory unavailable"))?
        .join("engines/tesseract");
    let executable_name = if cfg!(windows) {
        "tesseract.exe"
    } else {
        "tesseract"
    };
    let executable_pin = option_env!("DOCUMENT_OCR_TESSERACT_SHA256");
    let executable = component(&root, executable_name, executable_pin, 64 * 1024 * 1024)?;
    let model = component(
        &root,
        "tessdata/eng.traineddata",
        option_env!("DOCUMENT_OCR_ENGLISH_SHA256"),
        64 * 1024 * 1024,
    )?;
    let scratch = tempfile::tempdir()?;
    let image = scratch.path().join("input.png");
    fs::write(&image, png(input)?)?;
    job.check()?;

    #[cfg(target_os = "linux")]
    let libraries = private_libraries(&root, job)?;

    #[cfg(target_os = "linux")]
    if libraries.is_some() {
        // Localwright reserves LD_LIBRARY_PATH. Keep this narrow compatibility route for a
        // hash-verified private library pack until the shared runtime supports loader paths.
        return run_direct(
            &executable,
            &model,
            &image,
            pdf,
            output_limit,
            libraries,
            job,
        );
    }
    run_with_engine_runtime(
        RuntimeRequest {
            executable,
            model,
            image,
            executable_name,
            executable_pin,
            cache_dir: scratch.path(),
            pdf,
            output_limit,
        },
        job,
    )
}

struct RuntimeRequest<'a> {
    executable: PathBuf,
    model: PathBuf,
    image: PathBuf,
    executable_name: &'a str,
    executable_pin: Option<&'a str>,
    cache_dir: &'a Path,
    pdf: bool,
    output_limit: u64,
}

fn run_with_engine_runtime(request: RuntimeRequest<'_>, job: &Job) -> Result<Vec<u8>> {
    let RuntimeRequest {
        executable,
        model,
        image,
        executable_name,
        executable_pin,
        cache_dir,
        pdf,
        output_limit,
    } = request;
    if image.to_str().is_none()
        || model.parent().and_then(Path::to_str).is_none()
        || cache_dir.to_str().is_none()
    {
        #[cfg(target_os = "linux")]
        return run_direct(&executable, &model, &image, pdf, output_limit, None, job);
        #[cfg(target_os = "macos")]
        return run_direct(&executable, &model, &image, pdf, output_limit, job);
        #[cfg(target_os = "windows")]
        return run_direct(&executable, &model, &image, pdf, output_limit, job);
    }
    let spec = tesseract_engine_spec(
        executable_name,
        executable_pin.ok_or_else(|| Error::engine("OCR build has no pinned engine pack"))?,
        usize::try_from(output_limit)
            .map_err(|_| Error::input("OCR output limit exceeds this platform's capacity"))?,
    );
    let engine = EngineRuntime::resolve_trusted_path(spec, &executable)?;
    let image_arg = image
        .to_str()
        .ok_or_else(|| Error::input("OCR temporary image path is not valid UTF-8"))?;
    let tessdata = model
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(|| Error::input("OCR model path is not valid UTF-8"))?;
    let cache_path = cache_dir
        .to_str()
        .ok_or_else(|| Error::input("OCR temporary directory is not valid UTF-8"))?;
    let mut args = vec![
        image_arg.to_owned(),
        "stdout".into(),
        "--tessdata-dir".into(),
        tessdata.to_owned(),
        "-l".into(),
        "eng".into(),
        "--psm".into(),
        "6".into(),
        "--dpi".into(),
        "300".into(),
    ];
    if pdf {
        args.extend(["-c".into(), "tessedit_create_pdf=1".into()]);
    }
    let output = engine.run_with_options(
        EngineInvocation {
            args,
            environment: [
                ("XDG_CACHE_HOME".into(), cache_path.to_owned()),
                ("OMP_THREAD_LIMIT".into(), "1".into()),
            ]
            .into_iter()
            .collect(),
            ..EngineInvocation::default()
        },
        EngineRunOptions {
            hide_console: cfg!(target_os = "windows"),
        },
        job,
    )?;
    job.check()?;
    Ok(output.stdout)
}

fn tesseract_engine_spec(executable_name: &str, sha256: &str, output_limit: usize) -> EngineSpec {
    EngineSpec {
        id: "document-tesseract".into(),
        version: "runtime-adapter-2".into(),
        executable: PathBuf::from(executable_name),
        sha256: sha256.to_owned(),
        license: "Apache-2.0".into(),
        capabilities: vec![Capability {
            format: "PNG".into(),
            direction: "input-to-output".into(),
            operation: "English OCR".into(),
            engine: "Tesseract".into(),
            limitations: "Experimental English-only offline OCR".into(),
            fixture: "Document Studio OCR fixtures".into(),
        }],
        timeout_ms: 45_000,
        max_stdin_bytes: 0,
        max_stdout_bytes: output_limit,
        max_stderr_bytes: 64 * 1024,
        environment_allowlist: vec!["XDG_CACHE_HOME".into(), "OMP_THREAD_LIMIT".into()],
        health_check_args: None,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_direct(
    executable: &Path,
    model: &Path,
    image: &Path,
    pdf: bool,
    output_limit: u64,
    #[cfg(target_os = "linux")] libraries: Option<std::ffi::OsString>,
    job: &Job,
) -> Result<Vec<u8>> {
    let mut command = Command::new(executable);
    command
        .current_dir(image.parent().expect("OCR image is staged in a directory"))
        .env_clear()
        .env(
            "XDG_CACHE_HOME",
            image.parent().expect("image parent exists"),
        )
        .env("TMPDIR", image.parent().expect("image parent exists"))
        .env("TEMP", image.parent().expect("image parent exists"))
        .env("TMP", image.parent().expect("image parent exists"))
        .env("OMP_THREAD_LIMIT", "1")
        .args([
            image.as_os_str(),
            "stdout".as_ref(),
            "--tessdata-dir".as_ref(),
        ])
        .arg(model.parent().expect("fixed tessdata path has a parent"))
        .args(["-l", "eng", "--psm", "6", "--dpi", "300"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    if pdf {
        command.args(["-c", "tessedit_create_pdf=1"]);
    }
    #[cfg(target_os = "linux")]
    if let Some(search_path) = libraries {
        command.env("LD_LIBRARY_PATH", search_path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
    }

    let mut child = command.spawn().map_err(|e| Error::engine(e.to_string()))?;
    let stdout = child.stdout.take().expect("OCR stdout is piped");
    let result = thread::scope(|scope| {
        let reader = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(output_limit + 1).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let status = wait(&mut child, job);
        if status.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let bytes = reader
            .join()
            .map_err(|_| Error::engine("OCR reader stopped"))??;
        let status = status?;
        if bytes.len() as u64 > output_limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "OCR output exceeds the size limit",
            ));
        }
        if !status.success() {
            return Err(Error::engine(format!("OCR engine failed ({status})")));
        }
        Ok::<_, Error>(bytes)
    })?;
    job.check()?;
    Ok(result)
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn runtime_descriptor_is_hash_pinned_and_detects_pack_changes() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("tesseract");
        std::fs::write(&executable, b"pinned OCR engine fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let digest = format!("{:x}", Sha256::digest(b"pinned OCR engine fixture"));
        let spec = tesseract_engine_spec("tesseract", &digest, MAX_TEXT as usize);
        assert_eq!(spec.timeout_ms, 45_000);
        assert_eq!(spec.max_stdout_bytes, MAX_TEXT as usize);
        assert_eq!(spec.max_stdin_bytes, 0);
        assert_eq!(spec.license, "Apache-2.0");
        let resolved = EngineRuntime::resolve_trusted_path(spec.clone(), &executable).unwrap();
        assert_eq!(resolved.executable(), executable.as_path());

        std::fs::write(&executable, b"modified engine fixture").unwrap();
        assert!(EngineRuntime::resolve_trusted_path(spec, &executable).is_err());
    }

    #[test]
    fn multipage_pdf_ocr_enforces_a_bounded_page_count() {
        assert!(validate_pdf_page_count(1).is_ok());
        assert!(validate_pdf_page_count(MAX_PDF_PAGES).is_ok());
        assert_eq!(
            validate_pdf_page_count(0).unwrap_err().kind,
            ErrorKind::Limit
        );
        assert_eq!(
            validate_pdf_page_count(MAX_PDF_PAGES + 1).unwrap_err().kind,
            ErrorKind::Limit
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_pack_uses_localwright_runtime_with_bounded_output() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("tesseract");
        let script = b"#!/bin/sh\nprintf '%s' 'runtime OCR fixture'\n";
        std::fs::write(&executable, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

        let tessdata = directory.path().join("tessdata");
        std::fs::create_dir(&tessdata).unwrap();
        let model = tessdata.join("eng.traineddata");
        let image = directory.path().join("input.png");
        std::fs::write(&image, b"fixture image").unwrap();
        let digest = format!("{:x}", Sha256::digest(script));

        let output = run_with_engine_runtime(
            RuntimeRequest {
                executable,
                model,
                image,
                executable_name: "tesseract",
                executable_pin: Some(&digest),
                cache_dir: directory.path(),
                pdf: false,
                output_limit: 64,
            },
            &Job::new(),
        )
        .unwrap();
        assert_eq!(output, b"runtime OCR fixture");
    }

    #[cfg(unix)]
    #[test]
    fn localwright_runtime_enforces_the_ocr_output_limit() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("tesseract");
        let script = b"#!/bin/sh\nprintf '0123456789'\n";
        std::fs::write(&executable, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

        let tessdata = directory.path().join("tessdata");
        std::fs::create_dir(&tessdata).unwrap();
        let model = tessdata.join("eng.traineddata");
        let image = directory.path().join("input.png");
        std::fs::write(&image, b"fixture image").unwrap();
        let digest = format!("{:x}", Sha256::digest(script));

        let error = run_with_engine_runtime(
            RuntimeRequest {
                executable,
                model,
                image,
                executable_name: "tesseract",
                executable_pin: Some(&digest),
                cache_dir: directory.path(),
                pdf: false,
                output_limit: 4,
            },
            &Job::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Limit);
    }
}
