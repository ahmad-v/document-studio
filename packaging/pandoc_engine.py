"""Verify and stage the pinned Pandoc 3.14 Linux x64 private engine."""
import argparse
import hashlib
import json
import os
import shutil
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = "3.14"
ASSET = f"pandoc-{VERSION}-linux-amd64.tar.gz"
MEMBER = f"pandoc-{VERSION}/bin/pandoc"
ARCHIVE_SIZE = 36_635_549
ARCHIVE_SHA256 = "c5f4720cdefb3f7bdc5d07202ef579feef0d1370773c873804dd8fdaa8c4f1b8"
SOURCE_ASSET = f"pandoc-{VERSION}.tar.gz"
SOURCE_SIZE = 7_562_354
SOURCE_SHA256 = "94804ddd05c67e9ad91740e7c010ba4ed4da96fada0f0b66d668e90cdd8aea38"
COPYING_SHA256 = "3ebfbf9235b049d384cdba5a66a1a3434bb544391e720b76faa8e1b3ee61a4e5"
COPYRIGHT_SHA256 = "842e33ef01625e93f85bebb8bac83aa570186b7aa77a09971257cc29f8f60740"
MAX_BINARY_BYTES = 192 * 1024 * 1024
MAX_ARCHIVE_BYTES = 64 * 1024 * 1024


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def descriptor(binary_sha256):
    return {
        "schema_version": 1,
        "engines": [
            {
                "id": "document-pandoc",
                "version": VERSION,
                "executable": "engines/pandoc/bin/pandoc",
                "sha256": binary_sha256,
                "license": "GPL-2.0-or-later",
                "capabilities": [
                    {
                        "format": "TXT, Markdown, HTML, DOCX, ODT, EPUB, LaTeX",
                        "direction": "structural conversion",
                        "operation": "bounded offline conversion",
                        "engine": f"Pandoc {VERSION}",
                        "limitations": "Requires the app-relative private pack; not layout-preserving and does not render PDF.",
                        "fixture": "Document Studio structural conversion fixtures",
                    }
                ],
                "timeout_ms": 90_000,
                "max_stdin_bytes": 33_554_432,
                "max_stdout_bytes": 67_108_864,
                "max_stderr_bytes": 1_048_576,
                "environment_allowlist": [],
                "health_check_args": ["--version"],
            }
        ],
    }


def verify_archive(path, size=ARCHIVE_SIZE, sha256=ARCHIVE_SHA256):
    if not path.is_file() or path.stat().st_size != size:
        raise ValueError("Pandoc source archive size differs from the pinned release")
    if digest(path) != sha256:
        raise ValueError("Pandoc source archive SHA-256 differs from the pinned release")


def verify_source(path, size=SOURCE_SIZE, sha256=SOURCE_SHA256):
    if not path.is_file() or path.stat().st_size != size:
        raise ValueError("Pandoc corresponding source archive size differs from the pinned release")
    if digest(path) != sha256:
        raise ValueError("Pandoc corresponding source archive SHA-256 differs from the pinned release")


def extract_binary(archive_path, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    try:
        with tarfile.open(archive_path, "r:gz") as archive:
            matches = [member for member in archive.getmembers() if member.name == MEMBER]
            if len(matches) != 1:
                raise ValueError("Pandoc archive must contain exactly one pinned executable")
            member = matches[0]
            if not member.isfile() or member.size == 0 or member.size > MAX_BINARY_BYTES:
                raise ValueError("Pandoc archive executable is not a bounded regular file")
            source = archive.extractfile(member)
            if source is None:
                raise ValueError("Pandoc executable could not be read from its archive")
            written = 0
            with source, destination.open("xb") as output:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    written += len(chunk)
                    if written > member.size or written > MAX_BINARY_BYTES:
                        raise ValueError("Pandoc executable exceeded its declared size")
                    output.write(chunk)
            if written != member.size:
                raise ValueError("Pandoc executable was truncated in the source archive")
            os.chmod(destination, 0o755)
    except Exception:
        destination.unlink(missing_ok=True)
        raise


def verify_pack(pack, engine_sha256=None, source_sha256=SOURCE_SHA256):
    engines = pack / "engines"
    executable = engines / "pandoc" / "bin" / "pandoc"
    manifest_path = engines / "manifest.json"
    if engines.is_symlink() or not engines.is_dir():
        raise ValueError("Pandoc pack is missing its regular engines directory")
    actual_entries = {
        path.relative_to(pack).as_posix()
        for path in pack.rglob("*")
        if path.is_file() or path.is_symlink()
    }
    expected_entries = {
        "engines/manifest.json",
        "engines/pandoc/bin/pandoc",
        "third-party/Pandoc/COPYING.md",
        "third-party/Pandoc/COPYRIGHT",
        f"third-party/Pandoc/{SOURCE_ASSET}",
    }
    if actual_entries != expected_entries:
        raise ValueError("Pandoc pack contains missing or unexpected files")
    if executable.is_symlink() or not executable.is_file():
        raise ValueError("Pandoc pack executable must be a regular file")
    actual_hash = digest(executable)
    if engine_sha256 is not None and actual_hash != engine_sha256:
        raise ValueError("Pandoc executable hash does not match the pack descriptor")
    if manifest_path.is_symlink() or not manifest_path.is_file():
        raise ValueError("Pandoc engine manifest must be a regular file")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest != descriptor(actual_hash):
        raise ValueError("Pandoc engine descriptor is invalid")
    for name, expected in (("COPYING.md", COPYING_SHA256), ("COPYRIGHT", COPYRIGHT_SHA256)):
        notice = pack / "third-party" / "Pandoc" / name
        if notice.is_symlink() or not notice.is_file() or digest(notice) != expected:
            raise ValueError(f"Pandoc redistribution notice failed verification: {name}")
    source = pack / "third-party" / "Pandoc" / SOURCE_ASSET
    if source.is_symlink() or not source.is_file() or digest(source) != source_sha256:
        raise ValueError("Pandoc corresponding source archive failed verification")


def create_pack(
    archive,
    destination,
    source_archive,
    archive_size=ARCHIVE_SIZE,
    archive_sha256=ARCHIVE_SHA256,
    source_size=SOURCE_SIZE,
    source_sha256=SOURCE_SHA256,
):
    verify_archive(archive, archive_size, archive_sha256)
    verify_source(source_archive, source_size, source_sha256)
    destination.mkdir(parents=True, exist_ok=False)
    try:
        executable = destination / "engines" / "pandoc" / "bin" / "pandoc"
        extract_binary(archive, executable)
        executable_hash = digest(executable)
        (destination / "engines" / "manifest.json").write_text(
            json.dumps(descriptor(executable_hash), indent=2) + "\n", encoding="utf-8"
        )
        notice_dir = destination / "third-party" / "Pandoc"
        notice_dir.mkdir(parents=True)
        for name in ("COPYING.md", "COPYRIGHT"):
            source = ROOT / "packaging" / "pandoc" / name
            expected = COPYING_SHA256 if name == "COPYING.md" else COPYRIGHT_SHA256
            if not source.is_file() or digest(source) != expected:
                raise ValueError(f"Checked-in Pandoc redistribution notice failed verification: {name}")
            shutil.copyfile(source, notice_dir / name)
        shutil.copyfile(source_archive, notice_dir / SOURCE_ASSET)
        verify_pack(destination, executable_hash, source_sha256)
        return destination
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help=f"official {ASSET} release asset")
    parser.add_argument("destination", type=Path, help="new output directory")
    parser.add_argument("source_archive", type=Path, help=f"corresponding Hackage {SOURCE_ASSET}")
    args = parser.parse_args()
    try:
        create_pack(args.archive, args.destination, args.source_archive)
    except (OSError, ValueError, tarfile.TarError) as error:
        parser.error(str(error))
    print(args.destination)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
