# Document Studio

Document Studio is an offline-first native document utility and an unverified development snapshot (`0.1.0-alpha.1`). It is one of four independent studios built on the reusable Localwright toolkit.

## Current capabilities

The application includes experimental PDF inspection, text extraction and page operations; basic Markdown/plain-text DOCX generation and DOCX body-text extraction; bounded text comparison; CSL JSON validation and CSL JSON/RIS/BibTeX interchange; and optional Linux-focused private Tesseract OCR routes. The OCR build does not include an engine or language pack by default. See [the manual](docs/MANUAL.md) and [capability registry](docs/capabilities.json) for the exact routes and limits.

Most operations have Linux x86_64 fixture or CLI evidence. Experimental PDF page rasterization and a native preview workspace are implemented; its synthetic two-page pixel fixture passed. The workspace includes native PDF opening, up to eight session-local recent documents, bounded thumbnail navigation with current-page highlighting, direct page selection, fit/zoom controls, selected-page extraction and rotation, visual page reordering, inclusive page-range extraction, and experimental page-only merge with a second selected PDF. New outputs preserve their sources and existing files are never overwritten. Focused workflow tests are present; native interactive clickthrough and platform verification remain pending. Merge accepts only a UTF-8 second-input path through the current request contract. Page-only structural operations may omit annotations, metadata, bookmarks, forms, or unusual structures and are not redaction or sanitization. Mature document conversion, structural/visual comparison, EPUB support, OCR workflows, and installed-device acceptance remain open.

## Development and verification

The package declares a Rust 1.92 minimum and pins toolchain 1.95.0 in `rust-toolchain.toml` for the Hayro PDF renderer. Run `cargo fmt --all -- --check`, `cargo check --locked --all-targets`, `cargo test --locked`, and `cargo clippy --locked --all-targets -- -D warnings` for the default GUI build. The [build notes](docs/BUILD.md) describe CI workflow levels.

Ordinary `Check` runs use Linux x64 and skip Rust builds for documentation-only changes while preserving the required `Check / Rust (ubuntu-24.04)` result. Label a pull request `integration` for Linux GUI/headless and OCR-adapter checks, or `platform-validation` for the five native target matrix. Distribution preparation is manual. The package matrix remains gated because the provisional application identity and maintainer are unapproved; an unsigned portable preview can be manually prepared without that identity. The weekly/manual security workflow audits Rust advisories and emits a CycloneDX SBOM plus a review-only license metadata inventory.

See [status](docs/STATUS.md), [verification evidence](docs/VERIFICATION.md), [dependency notes](docs/DEPENDENCIES.md), [architecture](docs/ARCHITECTURE.md), and the [master specification](MASTER_SPECIFICATION.md). This is not a verified release, and the source snapshot does not provide installers or a complete offline OCR engine pack.

## Project principles

The project is intended to remain free, local and private, without tracking, advertising, subscriptions or premium features. Original code is a candidate for MIT OR Apache-2.0, pending maintainer approval and dependency review. No DOI or donation account has been configured. See the [manifesto](MANIFESTO.md) and [contribution guide](CONTRIBUTING.md).
