# Format and operation compatibility

Nothing is verified on a native target. The machine-readable registry is a development inventory, not an advertised capability list.

| Operation | Route | Evidence status |
|---|---|---|
| pdf-rotate | Rotate a selected page range clockwise relative to existing rotation | Linux fixture and CLI-tested |
| markdown-docx | Basic ATX headings and literal paragraphs to DOCX | implemented-unverified |
| ocr-png-pdf | One-page searchable PDF from a PNG with optional private OCR | Linux x86_64 CLI and independent reader smoke-tested |
| docx-text | UTF-8 WordprocessingML main-body plain text | Linux fixture and independent reader-tested |
| compare-text | UTF-8 line changes with optional plain-text report | implemented-unverified |
| citation-validate | Bounded CSL JSON object/array validation | Linux unit and operation-route fixtures tested |
| citation-convert | Supported-field CSL JSON/RIS interchange and CSL JSON/RIS to BibTeX export | Linux unit and route fixtures tested; exported records require human review |
| pdf-text | PDF text-layer extraction | implemented-unverified |
| pdf-split | Experimental page range | implemented-unverified |
| pdf-reorder | Experimental page-only PDF reordering | implemented-unverified |
| pdf-delete-pages | Delete unique 1-based pages, retaining remaining pages in order | Linux selected-page, all-page refusal and invalid-selection fixtures tested |
| pdf-merge | Experimental two-document page-only merge | implemented-unverified |

See section 04-07 and 10 of the master specification for complete targets.
