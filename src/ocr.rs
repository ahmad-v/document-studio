//! Experimental PNG OCR through an application-private Tesseract executable.

use localwright_core::{read_bounded, Error, ErrorKind, Job, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_TEXT: u64 = 4 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;

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
    let executable = component(
        &root,
        executable_name,
        option_env!("DOCUMENT_OCR_TESSERACT_SHA256"),
        64 * 1024 * 1024,
    )?;
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

    let mut command = Command::new(executable);
    command
        .current_dir(scratch.path())
        .env_clear()
        .env("XDG_CACHE_HOME", scratch.path())
        .env("TMPDIR", scratch.path())
        .env("TEMP", scratch.path())
        .env("TMP", scratch.path())
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
    #[cfg(target_os = "linux")]
    if let Some(search_path) = private_libraries(&root, job)? {
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
            stdout.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
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
        if bytes.len() as u64 > MAX_TEXT {
            return Err(Error::new(
                ErrorKind::Limit,
                "OCR output exceeds the text limit",
            ));
        }
        if !status.success() {
            return Err(Error::engine(format!("OCR engine failed ({status})")));
        }
        String::from_utf8(bytes).map_err(|_| Error::engine("OCR did not produce UTF-8 text"))
    })?;
    job.check()?;
    Ok(result)
}
