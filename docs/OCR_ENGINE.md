# Experimental private OCR engine

The optional `ocr` feature adds an English PNG-to-text route. It uses Tesseract from a fixed directory next to the executable. It stages input privately and starts an absolute executable with fixed arguments and a cleared environment. It limits PNG input to 16 MiB and 16 million pixels, caps captured text at 4 MiB, and terminates the child on cancellation or after 45 seconds. Output uses the document studio's no-overwrite writer.

## Building an engine-enabled candidate

Use audited, target-matching engine and language assets. Pin the expected SHA-256 of the executable in `DOCUMENT_OCR_TESSERACT_SHA256` and the English data file in `DOCUMENT_OCR_ENGLISH_SHA256` when building with `cargo build --locked --features ocr`. Use a review-controlled source for these digests. Stage the assets beside the application:

```text
document-studio
engines/tesseract/tesseract
engines/tesseract/tessdata/eng.traineddata
```

On Windows the executable is `tesseract.exe`. The application checks both assets before running OCR. The installed pack directory must be protected from modification by other users. Checksums establish integrity against the build-time pins. They do not sandbox the image parser or establish trust in its platform libraries.

Choose **Experimental PNG OCR** in the workbench. For CLI use `document-studio --cli ocr-png sample.png output.txt`. Omit the output for a preview. Review recognized text against the image.

## Linux smoke evidence

The Linux x86_64 smoke staged Debian's Tesseract 5.5.0 executable and English data next to the app. Both assets matched the build-time digests. A synthetic PNG returned the expected text with socket creation blocked. The test also refused an existing output and a modified English data file.

The smoke used host-provided Tesseract and Leptonica shared libraries as build/test fixtures. A distributable private pack still requires the complete native library set, license notices, a manifest and clean-device tests. The standard builds omit the optional OCR feature.
