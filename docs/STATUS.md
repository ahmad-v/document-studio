# Status | 0.1.0-alpha.1

Alpha source is on GitHub. The initial Rust integration checks ran on Linux x86_64.

| Operation | Scope | Status |
|---|---|---|
| pdf-rotate | Rotate a selected page range clockwise relative to existing rotation | Linux fixture and CLI-tested |
| text-docx | UTF-8 plain text to DOCX | implemented-unverified |
| markdown-docx | Basic ATX headings and literal paragraphs to DOCX | Linux fixture-tested |
| ocr-png | Optional private Tesseract and English data | Linux x64 smoke-tested |
| ocr-png-pdf | One-page searchable PDF from a PNG with optional private OCR | Linux x86_64 CLI and independent reader smoke-tested |
| docx-text | UTF-8 WordprocessingML main-body plain text | Linux fixture and independent reader-tested |
| compare-text | UTF-8 line changes with optional plain-text report | Linux integration fixture-tested |
| citation-validate | Bounded CSL JSON bibliography validation | Linux route tests passed; hosted matrix pending |
| citation-convert | CSL JSON/RIS interchange and CSL JSON/RIS to BibTeX export | Linux round-trip and route tests passed; hosted matrix pending |
| pdf-info | PDF version and page count; optional JSON | Linux fixture-tested |
| pdf-preview | Bounded one-page raster PNG and native paginated viewer | Synthetic two-page Linux fixture passed; GUI/platform checks pending |
| pdf-text | PDF text-layer extraction | Linux fixture-tested |
| pdf-split | Experimental inclusive page-range extraction; native viewer saves the selected range to a new page-only PDF | Linux fixture-tested; native workflow source tests added, hosted GUI verification pending |
| pdf-reorder | Experimental page-only PDF reordering | Linux fixture-tested |
| pdf-delete-pages | Delete selected PDF pages while retaining remaining order | Linux fixture-tested |
| pdf-merge | Experimental page-only merge of the open document with a second PDF through the native viewer | Linux fixture-tested; native workflow source tests added, hosted GUI verification pending |

## Known gaps

Basic DOCX creation and body-text extraction; inline Markdown and layout omitted. Hayro 0.9.0 now renders selected PDF pages to bounded PNG previews and opens a separate native paginated viewer. Synthetic pixel fixtures pass, but GUI/platform checks remain pending and the renderer omits some PDF features. PDF parsing and decompression limits are incomplete, active content can survive structural edits, and these operations are not redaction or sanitization. Page-only merge/reorder/delete may drop page annotations, document-level metadata, bookmarks and forms, and may lose inherited or unusual structures. No standard OCR pack, Office fidelity, legacy converters, or structural and visual comparison.

`check.yml` now runs the stable Linux x64 development gate on ordinary PRs and `main` pushes, with Cargo steps skipped only for docs-only changes. `integration.yml` is label/manual; it covers GUI/headless routes and optional OCR compilation. `native.yml` retains five native target checks behind a label/manual trigger. `distribution.yml` is manual: unsigned previews are available, and all 15 configured target-format pairs are held until identity/maintainer approval. `security.yml` is weekly/manual and produces advisory, SBOM, and license metadata inventory outputs. These workflows are not themselves successful target or installed-package evidence.

Hosted native matrices, device, package lifecycle, offline engine, and packaging verification remain open. See [verification evidence](VERIFICATION.md) and [dependency review](DEPENDENCIES.md).
