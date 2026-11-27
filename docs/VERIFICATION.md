# Verification evidence

The integration run used Rust 1.88.0 on Linux x86_64 with Localwright pinned to the revision in `Cargo.lock`. Formatting, locked all-target checking, default and headless tests, strict Clippy, and CLI help passed.

The DOCX and PDF tests passed. Python-docx independently read the generated paragraphs and heading markup. pypdf independently confirmed the page count, order, and content after CLI split and merge.

The GitHub workflow defines five platform checks. Device tests and installer verification remain open.

On 2026-11-27, the locked headless suite passed 10 tests and strict Clippy passed on Linux x86_64 with Rust 1.88.0. The page-delete fixture reloaded the output PDF, confirmed the remaining page text and checked refusal to delete every page, duplicate selections and invalid page numbers. The capability registry JSON and whitespace checks passed. Native PDF rendering and target package verification remain open.
