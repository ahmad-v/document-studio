# Format and operation compatibility

Nothing is verified on a native target. The machine-readable registry is a development inventory, not an advertised capability list.

| Operation | Route | Evidence status |
|---|---|---|
| pdf-rotate | Rotate a selected page range clockwise relative to existing rotation | Linux fixture and CLI-tested |
| pdf-preview | Render one bounded PDF page to PNG; native viewer with page navigation, thumbnails, zoom, structural tools and literal case-insensitive text-layer search | Synthetic render and text-layer search fixtures pass locally; GUI/platform verification pending; scanned-page OCR is not integrated |
| markdown-docx | Basic ATX headings and literal paragraphs to DOCX | implemented-unverified |
| ocr-png-pdf | One-page searchable PDF from a PNG with optional private OCR | Linux x86_64 CLI and independent reader smoke-tested |
| docx-text | UTF-8 WordprocessingML main-body plain text | Linux fixture and independent reader-tested |
| epub-inspect | Bounded EPUB container, metadata, manifest and reading-order spine inspection | Fixtures added; hosted verification pending; not a complete EPUB conformance validator |
| epub-text | UTF-8 XHTML body text in EPUB spine order | Fixtures added; hosted verification pending; no layout/CSS/image rendering |
| epub-markdown | Bounded package metadata, spine headings and escaped plain-text XHTML body export | Operation-route fixture added; hosted verification pending; no layout, style, navigation, image or media conversion |
| markdown-epub | Bounded UTF-8 CommonMark to self-contained EPUB 3 XHTML/OPF package; supports headings, lists, emphasis, tables and code; removes links/images and escapes raw HTML | Operation-route tests added; hosted verification pending; structural conversion only, not page-layout conversion |
| epub-reader | Native offline chapter reader with UTF-8-safe bounded text pages and adjustable text size | Source unit/route tests added; hosted GUI and native target verification pending; plain-text content only |
| compare-text | UTF-8 line changes with optional plain-text report | implemented-unverified |
| citation-validate | Bounded CSL JSON object/array validation | Linux unit and operation-route fixtures tested |
| citation-convert | Supported-field CSL JSON/RIS interchange and CSL JSON/RIS to BibTeX export | Linux unit and route fixtures tested; exported records require human review |
| pdf-text | PDF text-layer extraction | implemented-unverified |
| pdf-compare | Structural metadata/text/page-geometry and low-resolution raster comparison of two bounded PDFs | Synthetic renderer fixture added; hosted verification pending; not complete structure or signature validation |
| pdf-page-compare | Selected-page side-by-side PNG from two bounded Hayro renders, with exact pixel counts and mean RGB difference when raster dimensions match | Synthetic identical/modified page fixtures added; hosted verification pending; raster pixels only, not text/metadata/structure comparison |
| pdf-split | Experimental page range | implemented-unverified |
| pdf-reorder | Experimental page-only PDF reordering | implemented-unverified |
| pdf-delete-pages | Delete unique 1-based pages, retaining remaining pages in order | Linux selected-page, all-page refusal and invalid-selection fixtures tested |
| pdf-merge | Experimental two-document page-only merge | implemented-unverified |

See section 04-07 and 10 of the master specification for complete targets.
