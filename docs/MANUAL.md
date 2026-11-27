# Offline manual | Document Studio

Development source only. Launch the GUI after compiling. Choose an operation, enter local input path and a new output path where needed, set the parameter as described under the operation selector, then Run. Status and errors are shown below. Cancel requests cooperative cancellation. The draft editor accepts transcript or plain text. Choose a theme to change workbench colors and control borders. UI scale adjusts the controls from 80% to 300% and remembers your appearance choices when you reopen the app. Paths are manually typed in this increment; native dialogs and full accessibility remain pending.

- `text-docx`: Writes literal paragraphs from UTF-8 text. Leading Markdown characters are preserved.
- `markdown-docx`: Basic ATX headings and literal paragraphs to DOCX.
- `ocr-png-pdf`: Create a new one-page PDF with the source PNG and recognized English text. The optional OCR build and matching installed pack are required.
- `ocr-png`: English PNG OCR in an [optional engine build](OCR_ENGINE.md).
- `docx-text`: Preview UTF-8 main-body text or write it to a new `.txt` file. Paragraphs, tabs and line breaks are retained. Layout, list markers, headers, footnotes and images are omitted. Hidden body text may appear. Macros, linked resources and external XML entities are never loaded by this route.
- `compare-text`: Provide the original UTF-8 file as input and the revised file path as the parameter. Preview changes or save a new `.txt` report. Escaped line endings make missing final newlines and CRLF changes visible. Each input is limited to 1 MiB and 50,000 lines. Highly dissimilar inputs can reach the diff work limit.
- `pdf-info` reports the PDF version and page count with optional JSON output.
- `pdf-text`: PDF text-layer extraction.
- `pdf-split`: Experimental page range.
- `pdf-reorder` writes a new PDF using the requested page order. Enter every page once, for example `--cli pdf-reorder input.pdf reordered.pdf 3,1,2`. The output omits annotations, bookmarks, forms and document metadata.
- `pdf-delete-pages` writes a new PDF without selected pages. Enter unique 1-based page numbers separated by commas. The result must retain at least one page. This page-tree rewrite can omit annotations, bookmarks, forms and document metadata; it is not redaction or sanitization.
- `pdf-merge`: Experimental two-document page-only merge.

Limitations: Basic DOCX only; inline Markdown and layout omitted. PDF parsing and decompression limits are incomplete, active content can survive split/merge, and these operations are not redaction or sanitization. Page-only merge drops page annotations, document-level metadata/bookmarks/forms, and may lose inherited or unusual structures; no independent reader validation yet. No OCR, PDF renderer, Office fidelity, legacy converters, or structural and visual comparison.

No output overwrite is allowed. No accounts, ads, telemetry or runtime downloads are intended. Do not use this unverified snapshot with valuable or hostile input. Citation and donations are voluntary.

- `pdf-rotate`: Enter a page range and a clockwise angle, such as `2-4,90`, and choose a new PDF path. Angles of 90, 180 and 270 degrees are supported. Rotation is relative to each selected page’s current orientation.
