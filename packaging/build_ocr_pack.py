#!/usr/bin/env python3
"""Build a pinned, app-private Linux x86_64 Tesseract pack on a native runner.

This script prepares an unsigned development asset bundle. It does not modify
the host, install the engine globally, or run during application execution.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path


TESSERACT_COMMIT = "64eab6c457b2337dd690746a5fde5c222b40d5f8"
TESSERACT_ARCHIVE_SHA256 = "a2845cf9a8cd6d673c0893a37ebb8cac1b883f84d8a1bcd801e8bbf88a4b8004"
TESSDATA_COMMIT = "923915d4ced2a7235221788285785a29c4a42d4a"
TESSDATA_SHA256 = "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
TESSDATA_LICENSE_COMMIT = "27cfc71a8874cce2483679eea010e391bb38c2ae"
TESSDATA_LICENSE_SHA256 = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"
MAX_DOWNLOAD = 32 * 1024 * 1024
CORE_LIBRARIES = (
    "libc.so", "libm.so", "libpthread.so", "libdl.so", "librt.so",
    "libresolv.so", "ld-linux", "linux-vdso",
)


def download(url: str, destination: Path, expected_sha256: str, limit: int) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "Document-Studio-OCR-build/1"})
    digest = hashlib.sha256()
    size = 0
    with urllib.request.urlopen(request, timeout=45) as response, destination.open("xb") as output:
        while chunk := response.read(1024 * 1024):
            size += len(chunk)
            if size > limit:
                raise RuntimeError(f"Download exceeded its bound: {url}")
            digest.update(chunk)
            output.write(chunk)
    if digest.hexdigest() != expected_sha256:
        raise RuntimeError(f"SHA-256 mismatch for {url}")


def run(args: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(args, cwd=cwd, env=env, check=True, text=True, capture_output=True)
    return completed.stdout.strip()


def owner_package(path: Path) -> str:
    result = subprocess.run(
        ["dpkg-query", "-S", str(path)], check=False, capture_output=True, text=True
    )
    if result.returncode:
        raise RuntimeError(f"No installed Debian package owns runtime library {path}")
    return result.stdout.splitlines()[0].split(": ", 1)[0].split(":", 1)[0]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("output directory must not already exist")

    with tempfile.TemporaryDirectory(prefix="document-ocr-build-") as temp_name:
        temp = Path(temp_name)
        archive = temp / "tesseract-source.tar.gz"
        model = temp / "eng.traineddata"
        model_license = temp / "tessdata-LICENSE"
        download(
            f"https://github.com/tesseract-ocr/tesseract/archive/{TESSERACT_COMMIT}.tar.gz",
            archive,
            TESSERACT_ARCHIVE_SHA256,
            8 * 1024 * 1024,
        )
        download(
            f"https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{TESSDATA_COMMIT}/eng.traineddata",
            model,
            TESSDATA_SHA256,
            8 * 1024 * 1024,
        )
        download(
            f"https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{TESSDATA_LICENSE_COMMIT}/LICENSE",
            model_license,
            TESSDATA_LICENSE_SHA256,
            64 * 1024,
        )

        source_root = temp / "source"
        source_root.mkdir()
        with tarfile.open(archive, "r:gz") as bundle:
            members = bundle.getmembers()
            expected_root = f"tesseract-{TESSERACT_COMMIT}"
            if not members or any(
                not member.name.startswith(expected_root + "/")
                or member.issym() or member.islnk() or member.isdev()
                for member in members
            ):
                raise RuntimeError("Unexpected or unsafe Tesseract source archive layout")
            bundle.extractall(source_root, filter="data")
        source = source_root / f"tesseract-{TESSERACT_COMMIT}"
        if (source / "VERSION").read_text(encoding="utf-8").strip() != "5.5.0":
            raise RuntimeError("Pinned Tesseract source version did not match 5.5.0")

        prefix = temp / "install"
        build = temp / "build"
        run([
            "cmake", "-S", str(source), "-B", str(build),
            "-DCMAKE_BUILD_TYPE=Release",
            f"-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DBUILD_TRAINING_TOOLS=OFF", "-DOPENMP_BUILD=OFF",
            "-DSW_BUILD=OFF", "-DHAVE_LIBARCHIVE=OFF", "-DHAVE_LIBCURL=OFF",
            "-DDISABLED_LEGACY_ENGINE=ON", "-DGRAPHICS_DISABLED=ON",
            "-DMARCH_NATIVE_OPT=OFF", "-DBUILD_TESTING=OFF",
        ])
        run(["cmake", "--build", str(build), "--parallel", "2"])
        run(["cmake", "--install", str(build)])

        executable_candidates = list(prefix.rglob("tesseract"))
        if len(executable_candidates) != 1 or not executable_candidates[0].is_file():
            raise RuntimeError("Expected one installed Tesseract executable")
        executable = executable_candidates[0]
        pack = args.output
        lib_dir = pack / "lib"
        notice_dir = pack / "notices"
        tessdata_dir = pack / "tessdata"
        lib_dir.mkdir(parents=True)
        notice_dir.mkdir()
        tessdata_dir.mkdir()
        shutil.copy2(executable, pack / "tesseract")
        shutil.copy2(model, tessdata_dir / "eng.traineddata")
        shutil.copy2(source / "LICENSE", notice_dir / "Tesseract-LICENSE")
        shutil.copy2(model_license, notice_dir / "Tessdata-fast-LICENSE")

        runtime_env = os.environ.copy()
        installed_lib = prefix / "lib"
        runtime_env["LD_LIBRARY_PATH"] = str(installed_lib)
        dependency_text = run(["ldd", str(executable)], env=runtime_env)
        runtime_paths: dict[str, Path] = {}
        for line in dependency_text.splitlines():
            before, separator, after = line.strip().partition(" => ")
            if not separator:
                continue
            resolved = after.split(" (", 1)[0]
            if not resolved.startswith("/"):
                raise RuntimeError(f"Unresolved Tesseract runtime dependency: {line}")
            if any(Path(resolved).name.startswith(prefix_name) for prefix_name in CORE_LIBRARIES):
                continue
            runtime_paths[before] = Path(resolved)
        if not runtime_paths:
            raise RuntimeError("No private runtime libraries were discovered")

        package_versions: dict[str, str] = {}
        for soname, path in sorted(runtime_paths.items()):
            resolved_path = path.resolve(strict=True)
            if resolved_path.is_relative_to(prefix):
                shutil.copy2(resolved_path, lib_dir / soname)
                continue
            package = owner_package(resolved_path)
            version = run(["dpkg-query", "-W", "-f=${Version}", package])
            package_versions[package] = version
            shutil.copy2(resolved_path, lib_dir / soname)
            copyright_file = Path("/usr/share/doc") / package / "copyright"
            if not copyright_file.is_file():
                raise RuntimeError(f"Missing redistribution notice for dependency {package}")
            shutil.copy2(copyright_file, notice_dir / f"{package}-copyright")

        (notice_dir / "build-dependencies.txt").write_text(
            "\n".join(f"{name}\t{version}" for name, version in sorted(package_versions.items())) + "\n",
            encoding="utf-8",
        )
        (pack / "tesseract-source-license.txt").write_text(
            (source / "LICENSE").read_text(encoding="utf-8"), encoding="utf-8"
        )
        # Embed every private-library digest and the executable/model digests
        # into the application at compile time via the artifact pins file.
        executable_sha = hashlib.sha256((pack / "tesseract").read_bytes()).hexdigest()
        model_sha = hashlib.sha256((tessdata_dir / "eng.traineddata").read_bytes()).hexdigest()
        env_lines = [
            f"DOCUMENT_OCR_TESSERACT_SHA256={executable_sha}",
            f"DOCUMENT_OCR_ENGLISH_SHA256={model_sha}",
        ]
        subprocess.run([
            "python3", str(Path(__file__).with_name("ocr_library_manifest.py")), str(pack)
        ], check=True, capture_output=True, text=True)
        libraries_manifest = pack / "libraries.json"
        libraries_sha = hashlib.sha256(libraries_manifest.read_bytes()).hexdigest()
        env_lines.append("DOCUMENT_OCR_LIBRARIES_SHA256=" + libraries_sha)
        pack.with_name("ocr-build-pins.env").write_text(
            "\n".join(env_lines) + "\n", encoding="ascii"
        )
        provenance = {
            "schema_version": 1,
            "engine": {
                "name": "Tesseract OCR", "version": "5.5.0", "commit": TESSERACT_COMMIT,
                "archive_sha256": TESSERACT_ARCHIVE_SHA256, "binary_sha256": executable_sha,
                "license": "Apache-2.0",
                "source": f"https://github.com/tesseract-ocr/tesseract/tree/{TESSERACT_COMMIT}",
            },
            "model": {
                "name": "English tessdata_fast", "commit": TESSDATA_COMMIT,
                "sha256": model_sha, "license": "Apache-2.0",
                "source": f"https://github.com/tesseract-ocr/tessdata_fast/blob/{TESSDATA_COMMIT}/eng.traineddata",
                "license_source_commit": TESSDATA_LICENSE_COMMIT,
                "license_sha256": TESSDATA_LICENSE_SHA256,
            },
            "private_library_manifest_sha256": libraries_sha,
            "build_dependencies": package_versions,
            "build_tools": {
                "cmake": run(["cmake", "--version"]).splitlines()[0],
                "compiler": run(["c++", "--version"]).splitlines()[0],
            },
            "runtime_limitations": ["glibc and the ELF loader remain host-provided"],
        }
        (pack / "model-provenance.json").write_text(
            json.dumps(provenance, indent=2) + "\n", encoding="utf-8"
        )


if __name__ == "__main__":
    main()
