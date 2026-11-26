use localwright_core::{Job, Request};
use std::fs;

#[test]
fn compare_exports_line_changes_without_modifying_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before.txt");
    let after = dir.path().join("after.txt");
    let output = dir.path().join("diff.txt");
    let old = "Café\nold\nkeep\ntail\n";
    let new = "Café\nnew\nextra\nkeep\ntail";
    fs::write(&before, old).unwrap();
    fs::write(&after, new).unwrap();
    let request = Request {
        operation: "compare-text".into(),
        input: before.clone(),
        output: output.clone(),
        parameter: after.to_str().unwrap().into(),
        ..Request::default()
    };
    let result = document_studio::handle(request.clone(), Job::new()).unwrap();
    let expected = concat!(
        "Unchanged: 2; removed: 2; added: 3\n",
        "- 2: \"old\\n\"\n",
        "+ 2: \"new\\n\"\n",
        "+ 3: \"extra\\n\"\n",
        "- 4: \"tail\\n\"\n",
        "+ 5: \"tail\"\n",
    );
    assert_eq!(result.preview, expected);
    assert_eq!(fs::read_to_string(output).unwrap(), expected);
    assert!(document_studio::handle(request, Job::new()).is_err());
    assert_eq!(fs::read_to_string(before).unwrap(), old);
    assert_eq!(fs::read_to_string(after).unwrap(), new);
}
