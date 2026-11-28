#!/usr/bin/env python3
"""Build and smoke-test an unsigned portable development bundle."""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

from dependency_notices import collect
from package import CFG, R, TARGETS, check_binary, payload, validate_host


def stage_ocr(source, bundle, target):
    """Stage a reviewed Linux OCR pack without including unrelated source files."""

    def digest(path, expected, limit):
        if not isinstance(expected, str) or len(expected) != 64:
            raise ValueError(f"OCR provenance has no valid SHA-256 for {path.name}")
        if path.is_symlink() or not path.is_file() or path.stat().st_size > limit:
            raise ValueError(f"Invalid or oversized OCR provenance file: {path.name}")
        with path.open("rb") as stream:
            actual = hashlib.file_digest(stream, "sha256").hexdigest()
        if actual != expected:
            raise ValueError(f"OCR provenance SHA-256 does not match {path.name}")

    required_files = (
        "tesseract", "libraries.json", "model-provenance.json",
        "tessdata/eng.traineddata",
    )
    directories = {
        source: {
            "tesseract", "libraries.json", "model-provenance.json",
            "lib", "notices", "tessdata",
        },
        source / "tessdata": {"eng.traineddata"},
    }
    for directory, names in directories.items():
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"Expected real OCR pack directory: {directory.name}")
        if {entry.name for entry in directory.iterdir()} != names:
            raise ValueError(f"Unexpected or missing OCR pack files in {directory.name}")

    provenance_path = source / "model-provenance.json"
    if provenance_path.is_symlink() or provenance_path.stat().st_size > 64 * 1024:
        raise ValueError("Invalid or oversized OCR provenance record")
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    if provenance.get("schema_version") != 1:
        raise ValueError("Unsupported OCR provenance schema")
    checks = {
        "tesseract": (provenance.get("engine", {}).get("binary_sha256"), 64 * 1024 * 1024),
        "tessdata/eng.traineddata": (provenance.get("model", {}).get("sha256"), 64 * 1024 * 1024),
        "libraries.json": (provenance.get("private_library_manifest_sha256"), 64 * 1024),
        "notices/Tessdata-fast-LICENSE": (provenance.get("model", {}).get("license_sha256"), 64 * 1024),
    }
    for relative, (expected, limit) in checks.items():
        digest(source / relative, expected, limit)

    files = list(required_files)
    for name in ("lib", "notices"):
        directory = source / name
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"Expected real OCR pack directory: {name}")
        entries = sorted(directory.iterdir())
        if not 1 <= len(entries) <= 128:
            raise ValueError(f"Invalid OCR pack entry count in {name}")
        files.extend(f"{name}/{entry.name}" for entry in entries)

    destination = bundle / "engines/tesseract"
    total = 0
    for relative in files:
        original = source / relative
        if original.is_symlink() or not original.is_file():
            raise ValueError(f"Expected regular OCR pack file: {relative}")
        size = original.stat().st_size
        total += size
        if size > 64 * 1024 * 1024 or total > 384 * 1024 * 1024:
            raise ValueError("OCR pack exceeds file or total size limit")
        if relative == "tesseract" or relative.startswith("lib/"):
            check_binary(original, target)
        path = destination / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(original, path)


def smoke_ocr(command, directory):
    fixture = R / "tests/fixtures/ocr-english.png"
    network_trace = directory / "ocr-network.trace"
    arguments = [command, "--cli", "ocr-png", fixture]
    tracer = shutil.which("strace")
    if os.environ.get("DOCUMENT_OCR_REQUIRE_NETWORK_TRACE") == "1" and not tracer:
        raise SystemExit("strace is required for the offline OCR smoke check")
    if tracer:
        arguments = [
            tracer, "-f", "-qq", "-e", "trace=network", "-o", str(network_trace), *arguments
        ]
    result = subprocess.run(
        arguments,
        cwd=directory,
        env={"PATH": os.defpath, "LC_ALL": "C"},
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
        timeout=55,
    )
    if result.returncode:
        raise SystemExit(f"Packaged OCR failed its reference fixture: {result.stderr.strip()}")
    if json.loads(result.stdout)["preview"].strip() != "LOCAL DOCUMENT OCR":
        raise SystemExit("Packaged OCR did not recognize the reference text")
    if tracer:
        trace = network_trace.read_text(encoding="utf-8")
        if any(call in trace for call in ("socket(", "connect(", "sendto(", "sendmsg(")):
            raise SystemExit("Packaged OCR attempted a network syscall during its offline smoke test")
        network_trace.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ocr-pack", type=Path, help="reviewed matching Linux Tesseract pack directory")
    args = parser.parse_args()
    if args.ocr_pack and "linux" not in args.target:
        parser.error("OCR portable previews currently support Linux targets only")

    validate_host(args.target)
    name = CFG["name"]
    windows = "windows" in args.target
    executable = name + (".exe" if windows else "")
    if args.binary.name != executable or not args.binary.is_file():
        parser.error(f"--binary must point to the built {executable}")
    check_binary(args.binary, args.target)

    edition = "preview-ocr" if args.ocr_pack else "preview"
    stem = f"{name}-{CFG['package_version']}-{args.target}-{edition}"
    with tempfile.TemporaryDirectory(prefix="studio-portable-") as directory:
        temp = Path(directory)
        bundle = temp / name
        payload(bundle, args.binary)
        if args.ocr_pack:
            try:
                stage_ocr(args.ocr_pack, bundle, args.target)
            except (OSError, ValueError) as error:
                parser.error(str(error))
        collect(R, bundle, args.target, args.binary, features=("ocr",) if args.ocr_pack else ())
        (bundle / "PAYLOAD_STATUS.txt").write_text(
            "Unsigned portable development build. Launch the application in this directory.\n"
            + ("Includes the private Tesseract OCR pack and English data. Platform libraries may be required.\n"
               if args.ocr_pack else
               "This bundle contains no private OCR engine. Platform libraries may be required.\n"),
            encoding="utf-8",
        )
        extracted = temp / "extracted preview"
        if windows:
            archive = temp / f"{stem}.zip"
            with zipfile.ZipFile(archive, "x", zipfile.ZIP_DEFLATED, strict_timestamps=False) as output:
                for path in sorted(bundle.rglob("*")):
                    if path.is_file():
                        output.write(path, path.relative_to(temp).as_posix())
            with zipfile.ZipFile(archive) as output:
                output.extractall(extracted)
        else:
            archive = temp / f"{stem}.tar.gz"
            with tarfile.open(archive, "x:gz") as output:
                output.add(bundle, arcname=name)
            with tarfile.open(archive) as output:
                output.extractall(extracted, filter="data")

        command = (extracted / name / executable).resolve()
        check_binary(command, args.target)
        result = subprocess.run(
            [command, "--cli", "--help"],
            cwd=extracted,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=True,
            timeout=30,
        )
        if "Usage: --cli OP INPUT OUTPUT PARAMETER" not in result.stdout:
            raise SystemExit("Packaged CLI help did not return the expected usage text")

        has_ocr = any(line.startswith("ocr-png:") for line in result.stdout.splitlines())
        if has_ocr != bool(args.ocr_pack):
            raise SystemExit("OCR-enabled binary and private OCR pack must be supplied together")
        if args.ocr_pack:
            smoke_ocr(command, extracted)

        with archive.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        args.output.mkdir(parents=True, exist_ok=False)
        shutil.copy2(archive, args.output / archive.name)
        (args.output / "SHA256SUMS").write_text(
            f"{digest}  {archive.name}\n", encoding="ascii"
        )


if __name__ == "__main__":
    main()
