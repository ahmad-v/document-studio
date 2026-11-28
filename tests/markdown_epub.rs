use localwright_core::{Job, Request};
use std::io::Read;
use zip::ZipArchive;

#[test]
fn markdown_to_epub_builds_a_bounded_self_contained_epub3_package() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("notes.md");
    let output = root.path().join("book.epub");
    std::fs::write(
        &input,
        "# A <safe> title\n\nIntro with **emphasis** and `code`.\n\n- first\n- second\n\n| Name | Count |\n| --- | ---: |\n| café | 3 |\n\n[remote link](https://invalid.example) and ![remote cover](https://invalid.example/cover.png).\n\n<script>alert(1)</script>\n",
    )
    .unwrap();
    let result = document_studio::handle(
        Request {
            operation: "markdown-epub".into(),
            input: input.clone(),
            output: output.clone(),
            parameter: r#"{"title":"Research & Notes","language":"en-GB"}"#.into(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    assert!(result.preview.contains("EPUB 3"));
    assert!(result.warnings[0].contains("Structural"));
    assert!(input.exists(), "source Markdown must remain untouched");

    let file = std::fs::File::open(&output).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mimetype = archive.by_index(0).unwrap();
    assert_eq!(mimetype.name(), "mimetype");
    assert_eq!(mimetype.compression(), zip::CompressionMethod::Stored);
    let mut package = String::new();
    archive
        .by_name("OEBPS/content.opf")
        .unwrap()
        .read_to_string(&mut package)
        .unwrap();
    assert!(package.contains("<dc:title>Research &amp; Notes</dc:title>"));
    assert!(package.contains("<dc:language>en-GB</dc:language>"));
    let mut chapter = String::new();
    archive
        .by_name("OEBPS/chapter-001.xhtml")
        .unwrap()
        .read_to_string(&mut chapter)
        .unwrap();
    assert!(chapter.contains("<strong>emphasis</strong>"));
    assert!(chapter.contains("<table>"));
    assert!(chapter.contains("café"));
    assert!(chapter.contains("remote link"));
    assert!(chapter.contains("remote cover"));
    assert!(!chapter.contains("https://"));
    assert!(chapter.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    let text = document_studio::handle(
        Request {
            operation: "epub-text".into(),
            input: output.clone(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    assert!(text.preview.contains("Research & Notes"));
    assert!(text.preview.contains("second"));

    let before = std::fs::read(&output).unwrap();
    assert!(document_studio::handle(
        Request {
            operation: "markdown-epub".into(),
            input,
            output: output.clone(),
            ..Request::default()
        },
        Job::new(),
    )
    .is_err());
    assert_eq!(std::fs::read(output).unwrap(), before);
}

#[test]
fn markdown_epub_rejects_invalid_metadata_and_non_markdown_inputs_before_output() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("source.md");
    std::fs::write(&input, "# title").unwrap();
    let text_input = root.path().join("source.txt");
    std::fs::write(&text_input, "plain text").unwrap();
    for (path, metadata) in [
        (text_input, "{}"),
        (input.clone(), r#"{"unexpected":"field"}"#),
        (input.clone(), r#"{"language":"en--US"}"#),
        (input.clone(), r#"{"title":42}"#),
    ] {
        let output = root.path().join(format!("bad-{}.epub", metadata.len()));
        let request = Request {
            operation: "markdown-epub".into(),
            input: path,
            output: output.clone(),
            parameter: metadata.into(),
            ..Request::default()
        };
        assert!(document_studio::handle(request, Job::new()).is_err());
        assert!(!output.exists());
    }
}
