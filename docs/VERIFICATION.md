# Verification evidence

The integration run used Rust 1.88.0 on Linux x86_64 with Localwright pinned to the revision in `Cargo.lock`. Formatting, locked all-target checking, default and headless tests, strict Clippy, and CLI help passed.

The DOCX and PDF tests passed. Python-docx independently read the generated paragraphs and heading markup. pypdf independently confirmed the page count, order, and content after CLI split and merge.

The earlier Check workflow defined five platform jobs. The current workflow architecture keeps a stable Linux x64 quick check, with five-target verification moved to the manual/`platform-validation` native workflow. Device tests and installer verification remain open.

On 2026-11-27, the locked headless suite passed 10 tests and strict Clippy passed on Linux x86_64 with Rust 1.88.0. The page-delete fixture reloaded the output PDF, confirmed the remaining page text and checked refusal to delete every page, duplicate selections and invalid page numbers. The capability registry JSON and whitespace checks passed.

The new Hayro 0.9.0 route was tested under Rust 1.95.0 on a generated two-page PDF. It rendered page 1 as 200×300 RGBA with a black center pixel (FNV-1a64 `f02a254f4e23a225`) and page 2 as 200×300 RGBA with a white center pixel. Page selection and rejection of page 0 and page 3 passed. The separate Iced preview window, native multi-platform behavior, hostile input limits and package integration remain unverified.
