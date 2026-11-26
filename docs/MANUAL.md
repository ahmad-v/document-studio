# Offline manual | Document Studio

Development source only. Launch the GUI after compiling. Choose an operation, enter local input path and a new output path where needed, set the parameter as described under the operation selector, then Run. Status and errors are shown below. Cancel requests cooperative cancellation. The draft editor accepts transcript or plain text. Choose a theme to change workbench colors and control borders. UI scale adjusts the controls from 80% to 300% for the current session. Paths are manually typed in this increment; native dialogs and full accessibility remain pending.

- `text-docx`: Writes literal paragraphs from UTF-8 text. Leading Markdown characters are preserved.
- `markdown-docx`: Basic ATX headings and literal paragraphs to DOCX.
- `ocr-png`: English PNG OCR in an [optional engine build](OCR_ENGINE.md).
- `pdf-info` reports the PDF version and page count with optional JSON output.
- `pdf-text`: PDF text-layer extraction.
- `pdf-split`: Experimental page range.
- `pdf-merge`: Experimental two-document page-only merge.

Limitations: Basic DOCX only; inline Markdown and layout omitted. PDF parsing and decompression limits are incomplete, active content can survive split/merge, and these operations are not redaction or sanitization. Page-only merge drops page annotations, document-level metadata/bookmarks/forms, and may lose inherited or unusual structures; no independent reader validation yet. No OCR, PDF renderer, Office fidelity, legacy converters or comparison.

No output overwrite is allowed. No accounts, ads, telemetry or runtime downloads are intended. Do not use this unverified snapshot with valuable or hostile input. Citation and donations are voluntary.
