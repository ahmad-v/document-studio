# Status | 0.1.0-alpha.1

Alpha source is on GitHub. The initial Rust integration checks ran on Linux x86_64.

| Operation | Scope | Status |
|---|---|---|
| text-docx | UTF-8 plain text to DOCX | implemented-unverified |
| markdown-docx | Basic ATX headings and literal paragraphs to DOCX | Linux fixture-tested |
| ocr-png | Optional private Tesseract and English data | Linux x64 smoke-tested |
| pdf-info | PDF version and page count; optional JSON | Linux fixture-tested |
| pdf-text | PDF text-layer extraction | Linux fixture-tested |
| pdf-split | Experimental page range | Linux fixture-tested |
| pdf-merge | Experimental two-document page-only merge | Linux fixture-tested |

## Known gaps

Basic DOCX only; inline Markdown and layout omitted. PDF parsing and decompression limits are incomplete, active content can survive split/merge, and these operations are not redaction or sanitization. Page-only merge drops page annotations, document-level metadata/bookmarks/forms, and may lose inherited or unusual structures. No standard OCR pack, PDF renderer, Office fidelity, legacy converters or comparison.

Hosted matrix, device, and packaging verification remain open. See [verification evidence](VERIFICATION.md) and [dependency review](DEPENDENCIES.md).
