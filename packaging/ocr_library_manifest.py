#!/usr/bin/env python3
"""Pin the verified ELF library files already staged for private Tesseract."""

import argparse
import hashlib
import json
import re
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pack", type=Path, help="directory containing lib/")
    args = parser.parse_args()
    directory = args.pack / "lib"
    if directory.is_symlink() or not directory.is_dir():
        parser.error("pack must contain a real lib directory")
    paths = sorted(directory.iterdir())
    if not 1 <= len(paths) <= 128:
        parser.error("expected between 1 and 128 private libraries")

    total_bytes = 0
    libraries = {}
    for path in paths:
        if (path.is_symlink() or not path.is_file()
                or path.name.startswith(".") or ".so" not in path.name
                or not re.fullmatch(r"[A-Za-z0-9_.+-]+", path.name)):
            parser.error(f"invalid staged library: {path.name}")
        size = path.stat().st_size
        total_bytes += size
        if size > 64 * 1024 * 1024 or total_bytes > 256 * 1024 * 1024:
            parser.error("private libraries exceed the byte limits")
        with path.open("rb") as library:
            if library.read(4) != b"\x7fELF":
                parser.error(f"not an ELF library: {path.name}")
            library.seek(0)
            libraries[path.name] = hashlib.file_digest(library, "sha256").hexdigest()

    contents = (json.dumps({"schema_version": 1, "libraries": libraries}, indent=2) + "\n").encode()
    with (args.pack / "libraries.json").open("xb") as manifest:
        manifest.write(contents)
    print(hashlib.sha256(contents).hexdigest())


if __name__ == "__main__":
    main()
