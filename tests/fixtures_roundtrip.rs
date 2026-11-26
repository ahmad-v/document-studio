use localwright_core::{Job, Request};
#[test]
fn pdf_extract_split_merge() {
    let root = tempfile::tempdir().unwrap();
    let input = std::path::PathBuf::from("tests/fixtures/two-pages.pdf");
    let text = document_studio::handle(
        Request {
            operation: "pdf-text".into(),
            input: input.clone(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    assert!(text.preview.contains("Page one"));
    assert!(text.preview.contains("Page two"));
    let split = root.path().join("split.pdf");
    document_studio::handle(
        Request {
            operation: "pdf-split".into(),
            input: input.clone(),
            output: split.clone(),
            parameter: "2-2".into(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    let d = lopdf::Document::load(&split).unwrap();
    assert_eq!(d.get_pages().len(), 1);
    assert!(d.extract_text(&[1]).unwrap().contains("Page two"));
    let merged = root.path().join("merged.pdf");
    document_studio::handle(
        Request {
            operation: "pdf-merge".into(),
            input,
            output: merged.clone(),
            parameter: split.to_str().unwrap().into(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    let d = lopdf::Document::load(&merged).unwrap();
    assert_eq!(d.get_pages().len(), 3);
    assert!(d.extract_text(&[3]).unwrap().contains("Page two"));
}

#[test]
fn pdf_information_fixture() {
    let outcome = document_studio::handle(
        Request {
            operation: "pdf-info".into(),
            input: "tests/fixtures/two-pages.pdf".into(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    let report: serde_json::Value = serde_json::from_str(&outcome.preview).unwrap();
    assert_eq!(report["pages"], 2);
    assert_eq!(report["pdf_version"], "1.3");
}
