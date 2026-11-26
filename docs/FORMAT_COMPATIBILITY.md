# Format and operation compatibility

Nothing is verified on a native target. The machine-readable registry is a development inventory, not an advertised capability list.

| Operation | Route | Evidence status |
|---|---|---|
| markdown-docx | Basic ATX headings and literal paragraphs to DOCX | implemented-unverified |
| ocr-png-pdf | One-page searchable PDF from a PNG with optional private OCR | Linux x86_64 CLI and independent reader smoke-tested |
| compare-text | UTF-8 line changes with optional plain-text report | implemented-unverified |
| pdf-text | PDF text-layer extraction | implemented-unverified |
| pdf-split | Experimental page range | implemented-unverified |
| pdf-reorder | Experimental page-only PDF reordering | implemented-unverified |
| pdf-merge | Experimental two-document page-only merge | implemented-unverified |

See section 04-07 and 10 of the master specification for complete targets.
