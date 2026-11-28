# Private OCR development pack

The manual `Private OCR portable preview` workflow builds an unsigned Linux
x86_64 development bundle. It does not publish a release and does not claim
the OCR pack is available on the other four native targets.

The workflow downloads source and model assets from immutable upstream commits
and checks their SHA-256 values before use:

| Asset | Version or commit | License |
| --- | --- | --- |
| Tesseract | 5.5.0, `64eab6c457b2337dd690746a5fde5c222b40d5f8` | Apache-2.0 |
| English `tessdata_fast` | `923915d4ced2a7235221788285785a29c4a42d4a` | Apache-2.0 |
| Model license text | `27cfc71a8874cce2483679eea010e391bb38c2ae` | Apache-2.0 |

The resulting pack contains a build provenance record, the engine and model
hashes, a private-library manifest, and notices for bundled native libraries.
The application embeds the generated asset hashes at build time and validates
the files before invoking Tesseract. It does not search `PATH` for OCR engines
or download assets during use. glibc and the ELF loader remain host-provided.

The workflow builds Tesseract on Ubuntu 24.04 x86_64, then builds an
OCR-enabled Document Studio binary, stages the pack next to it, extracts the
portable archive in a fresh temporary directory, and recognizes the checked-in
English reference fixture. It runs the recognition process under `strace` and
fails if socket, connect, or send network syscalls occur. This is a smoke check
for the packaged route, not an operating-system sandbox or a guarantee about
every future OCR input.

The hosted workflow and artifact do not verify installer installation,
upgrade, uninstallation, user-data preservation, signing, other Linux
distributions, ARM64, Windows, or macOS. The artifact is an internal unsigned
development preview retained for 14 days.
