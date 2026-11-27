# Experimental private OCR engine

The optional `ocr` feature adds an English PNG-to-text route. It uses Tesseract from a fixed directory next to the executable and verifies the pinned executable and language model before execution. Document Studio pins Localwright revision `33b9af051960aca693ab92cc3a6dbd63c32baacb`. macOS and Linux packs without private shared libraries run through its `EngineRuntime`, which rechecks the executable digest and applies bounded output, a controlled environment, a timeout, and process-group cancellation. The OCR adapter retains a narrow direct-supervisor compatibility path for Linux packs that require the verified private `LD_LIBRARY_PATH` and for Windows, where `CREATE_NO_WINDOW` preserves the existing no-console launch behavior. A non-UTF-8 path also takes the compatibility path because the current Localwright API accepts UTF-8 argument strings. These fallback paths retain the OCR adapter's timeout and output limits but do not provide the runtime's process-group cancellation. All routes stage input privately and use fixed arguments. PNG input is limited to 16 MiB and 16 million pixels; captured text is capped at 4 MiB. Output uses the Document Studio no-overwrite writer. These controls are process hygiene, not an operating-system sandbox.

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

## Linux private libraries

For a reviewed Linux pack, place the private shared libraries under `engines/tesseract/lib/`. Every library must be a regular file, including each name required by the loader. Run `python3 packaging/ocr_library_manifest.py engines/tesseract` to create a new manifest and obtain its SHA-256. Add that reviewed digest to the two existing build pins as `DOCUMENT_OCR_LIBRARIES_SHA256`.

The app checks the pinned manifest and its exact library listing and limits before setting a child-only search path. Keep the installed files in a trusted directory. The source-built candidate below includes a matching library set and Debian package notices. Clean-target and in-app validation of this exact pack remain pending.

### Source-built Linux x86_64 candidate

Tesseract 5.5.0 was built from upstream commit `64eab6c457b2337dd690746a5fde5c222b40d5f8` against Debian Leptonica 1.84.1-4. The build disabled curl, archive support, training tools, the legacy recognizer, graphics, and OpenMP. The pack includes the shared libraries other than the host glibc and ELF loader. It records the Debian package versions and copyright notices alongside the upstream Tesseract license.

On Debian 13.7 with glibc 2.41, the staged engine recognized `LOCAL DOCUMENT OCR` from a synthetic PNG. The process ran with a cleared environment, an explicit private library path, and socket creation blocked. System call tracing observed no socket or connect calls. Dependency resolution confirmed that the remaining shared libraries loaded from the pack.

| Build pin | Candidate SHA-256 |
| --- | --- |
| `DOCUMENT_OCR_TESSERACT_SHA256` | `daee7207c0e56228102d5db02287d0f95f5a2bdd034d199a2b4ccf1f9a055ccd` |
| `DOCUMENT_OCR_ENGLISH_SHA256` | `7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2` |
| `DOCUMENT_OCR_LIBRARIES_SHA256` | `74aec2b6c155dc6de784f7bcb761fc6d6badd76eea036eff2d1fd8e952fc367e` |

The Linux x86_64 Document Studio CLI was built with `ocr` and all three candidate pins. From a fresh application staging directory it recognized the synthetic PNG through the source-built pack and wrote the expected text to a new output. The traced app and child made no socket or connect calls while socket creation was blocked. This validates the app-relative route on the build host. Clean-device and installer evidence remain open.

The same pack supports `document-studio --cli ocr-png-pdf sample.png searchable.pdf`. The PDF route uses the built-in PDF renderer and checks that the generated output is a one-page unencrypted PDF before publishing it. Poppler extracted the expected text, reported one page, and extracted image pixels matching the source PNG in the Linux fixture. A second write refused an existing output. The run was traced with socket creation blocked and made no socket or connect calls.

### Building a portable OCR preview

After building a default GUI-enabled binary with `--features ocr`, pass the reviewed Linux engine directory to the preview packager:

```sh
python3 packaging/portable.py --target x86_64-unknown-linux-gnu \
  --binary target/debug/document-studio --ocr-pack /path/to/engines/tesseract \
  --output dist/preview-ocr
```

The source pack must contain `tesseract`, `libraries.json`, `lib/`, `tessdata/eng.traineddata`, and a flat `notices/` directory. The packager checks architecture and bounds, collects the OCR-enabled dependency notices, and runs the included English reference fixture against the unpacked application. The application verifies the staged executable, data, and libraries against its build pins before engine execution. This route creates an unsigned development archive. Standard installers and cross-platform OCR packs still require separate verification.

The Linux x64 preview passed archive checksum verification and text recognition from its extracted payload. Searchable PDF generation also passed from that payload, with Poppler confirming the expected English text. The native Tesseract license was present. Staging rejected a symlinked pack and an ELF architecture mismatch.

The runtime API accepts structured argument strings. If an app/model or temporary path is not UTF-8, the adapter falls back to its direct supervisor. Its descriptor environment policy reserves `LD_LIBRARY_PATH`, preventing the verified Linux private library path from being supplied. It also does not set Windows `CREATE_NO_WINDOW`. Those cases keep the prior checked direct supervisor until Localwright exposes equivalent platform controls. The runtime's private working directory is separate from the OCR input staging directory; Tesseract receives an absolute image path. Neither execution route provides OS sandboxing.

App-relative pack lookup remains in the OCR adapter because the existing OCR pack does not include Localwright's `engines/manifest.json`. The adapter passes the resulting absolute executable to `EngineRuntime::resolve_trusted_path`, after checking the app-relative path and pack hashes. The runtime delegation has not yet been built or smoke-tested; earlier OCR evidence describes the direct supervisor implementation. Validate the runtime-backed Linux host-library and macOS paths, plus the Linux private-library and Windows fallback paths, before treating the delegation as verified.
