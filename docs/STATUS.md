# Status | 0.1.0-alpha.1

M0 complete as source planning; partial M1 implementation, not verified. All five repos created locally as source trees only. No remote resources created.

| Operation | Scope | Status |
|---|---|---|
| markdown-docx | Basic ATX headings and literal paragraphs to DOCX | implemented-unverified |
| pdf-text | PDF text-layer extraction | implemented-unverified |
| pdf-split | Experimental page range | implemented-unverified |
| pdf-merge | Experimental two-document page-only merge | implemented-unverified |

## Known gaps

Basic DOCX only; inline Markdown and layout omitted. PDF parsing and decompression limits are incomplete, active content can survive split/merge, and these operations are not redaction or sanitization. Page-only merge drops page annotations, document-level metadata/bookmarks/forms, and may lose inherited or unusual structures; no independent reader validation yet. No OCR, PDF renderer, Office fidelity, legacy converters or comparison.

Cargo and rustc absent; registry and distribution DNS failed. No lockfile, compilation, rustfmt, clippy, Rust tests, GUI/device tests, package builds or security audit performed. All five platform builds blocked pending CI. See MASTER_SPECIFICATION.md for authoritative acceptance gates.
