use localwright_core::{Job, Request};

#[test]
fn markdown_to_html_preserves_commonmark_structure_without_active_resources() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("draft.md");
    let output = directory.path().join("draft.html");
    std::fs::write(
        &input,
        "# Café & research\n\nA **bold** and _italic_ passage with `code`.\n\n| Term | Meaning |\n| --- | --- |\n| naïve | preserved |\n\n- first item\n- second item\n\n[Local label](https://outside.invalid) and ![diagram alt](https://outside.invalid/image.png).\n\n<script>alert(1)</script>\n",
    )
    .unwrap();

    let outcome = document_studio::handle(
        Request {
            operation: "markdown-html".into(),
            input: input.clone(),
            output: output.clone(),
            parameter: r#"{"title":"Café & Notes","language":"en-GB"}"#.into(),
            ..Request::default()
        },
        Job::new(),
    )
    .unwrap();
    let html = std::fs::read_to_string(&output).unwrap();
    assert!(html.contains("<title>Café &amp; Notes</title>"));
    assert!(html.contains("<html lang=\"en-GB\">"));
    assert!(html.contains("<h1>Café &amp; research</h1>"));
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>italic</em>"));
    assert!(html.contains("<code>code</code>"));
    assert!(html.contains("<table>"));
    assert!(html.contains("naïve"));
    assert!(html.contains("<li>first item</li>"));
    assert!(html.contains("Local label"));
    assert!(html.contains("diagram alt"));
    assert!(!html.contains("https://"));
    assert!(!html.contains("<img"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("default-src 'none'"));
    assert_eq!(outcome.written.as_deref(), Some(output.as_path()));
    assert!(input.exists(), "source Markdown must remain unchanged");

    let original_output = std::fs::read(&output).unwrap();
    assert!(document_studio::handle(
        Request {
            operation: "markdown-html".into(),
            input,
            output: output.clone(),
            ..Request::default()
        },
        Job::new(),
    )
    .is_err());
    assert_eq!(std::fs::read(output).unwrap(), original_output);
}

#[test]
fn markdown_html_rejects_invalid_metadata_and_wrong_extensions() {
    let directory = tempfile::tempdir().unwrap();
    let markdown = directory.path().join("input.md");
    let text = directory.path().join("input.txt");
    std::fs::write(&markdown, "# Valid").unwrap();
    std::fs::write(&text, "plain text").unwrap();
    for (input, parameter) in [(markdown, r#"{"unexpected":"field"}"#), (text, "{}")] {
        let output = directory
            .path()
            .join(format!("invalid-{}.html", parameter.len()));
        assert!(document_studio::handle(
            Request {
                operation: "markdown-html".into(),
                input,
                output: output.clone(),
                parameter: parameter.into(),
                ..Request::default()
            },
            Job::new(),
        )
        .is_err());
        assert!(!output.exists());
    }
}
