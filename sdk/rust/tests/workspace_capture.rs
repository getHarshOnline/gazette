//! Invalid selected configuration stays available for editor repair, never build admission.

use std::fs;

use gazette_sdk::{authoring::session, configuration};

const ROOT: &str = "includes=['site.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
const SITE: &str = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn capture(root: &std::path::Path) -> configuration::CapturedWorkspace {
    configuration::capture_workspace(root, &"gazette.toml".parse().unwrap(), Default::default())
        .unwrap()
}

#[test]
fn malformed_root_is_retained_without_guessing_an_include_set() {
    let root = tempfile::tempdir().unwrap();
    let text = "includes=['site.toml']\n[[principals\n";
    fs::write(root.path().join("gazette.toml"), text).unwrap();
    fs::write(root.path().join("site.toml"), SITE).unwrap();
    let captured = capture(root.path());
    assert_eq!(captured.native_root(), root.path());
    assert_eq!(captured.workspace_path().as_str(), "gazette.toml");
    assert_eq!(
        captured.documents().collect::<Vec<_>>(),
        vec![(captured.workspace_path(), text.as_bytes())]
    );
    assert_eq!(captured.inventory().entries().len(), 1);
    assert!(captured.selection_error().is_some());
    assert!(captured.resolve().is_err());
}

#[test]
fn invalid_included_text_is_retained_with_exact_selection_and_no_sibling_scan() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("gazette.toml"), ROOT).unwrap();
    fs::write(root.path().join("site.toml"), "[[sites\n").unwrap();
    fs::write(root.path().join("unselected.toml"), [0xff]).unwrap();
    let captured = capture(root.path());
    assert!(captured.selection_error().is_none());
    assert_eq!(
        captured
            .documents()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        ["gazette.toml", "site.toml"]
    );
    assert_eq!(
        captured.source(&"site.toml".parse().unwrap()),
        Some(&b"[[sites\n"[..])
    );
    assert!(
        captured
            .source(&"unselected.toml".parse().unwrap())
            .is_none()
    );
    fs::write(root.path().join("site.toml"), SITE).unwrap();
    assert!(
        captured.resolve().is_err(),
        "resolution must use retained invalid bytes"
    );
    assert!(capture(root.path()).resolve().is_ok());
}

#[test]
fn retained_semantic_error_can_be_repaired_in_a_session_without_changing_disk() {
    let root = tempfile::tempdir().unwrap();
    let invalid = SITE.replace("owner:one", "owner:missing");
    fs::write(root.path().join("gazette.toml"), ROOT).unwrap();
    fs::write(root.path().join("site.toml"), &invalid).unwrap();
    let captured = capture(root.path());
    let mut sources = captured.documents().map(|(path, bytes)| session::Source {
        id: path.as_str().to_owned(),
        path: path.clone(),
        text: std::str::from_utf8(bytes).unwrap().to_owned(),
    });
    let mut editor = session::Session::new_with_workspace_documents(
        sources.next().unwrap(),
        sources,
        [],
        [],
        Default::default(),
    )
    .unwrap();
    assert!(!editor.analysis().as_ref().unwrap().structurally_valid());
    assert!(editor.open("site.toml", 1, invalid.clone()).unwrap());
    assert!(editor.change("site.toml", 2, SITE.into()).unwrap());
    assert!(editor.analysis().as_ref().unwrap().structurally_valid());
    assert_eq!(
        fs::read_to_string(root.path().join("site.toml")).unwrap(),
        invalid
    );
    assert!(
        captured.resolve().is_err(),
        "editor repair does not admit the invalid captured build"
    );
}

#[test]
fn binding_bytes_are_retained_separately_and_valid_capture_resolves_without_rereads() {
    let root = tempfile::tempdir().unwrap();
    let config = format!(
        "machine_binding='machine.toml'\n{ROOT}[[sources]]\nid='source:one'\nlocation={{kind='filesystem',root='machine:content'}}\n"
    );
    let binding = "[roots]\ncontent='writing'\n";
    fs::write(root.path().join("gazette.toml"), config).unwrap();
    fs::write(root.path().join("site.toml"), SITE).unwrap();
    fs::write(root.path().join("machine.toml"), binding).unwrap();
    let captured = capture(root.path());
    assert_eq!(captured.documents().count(), 2);
    assert_eq!(
        captured.source(&"machine.toml".parse().unwrap()),
        Some(binding.as_bytes())
    );
    fs::write(
        root.path().join("machine.toml"),
        "[roots]\ncontent='changed'\n",
    )
    .unwrap();
    let loaded = captured.resolve().unwrap();
    assert_eq!(
        loaded.source_roots()[&"source:one".parse().unwrap()],
        root.path().join("writing")
    );
    fs::write(root.path().join("machine.toml"), "[roots]\ncontent=42\n").unwrap();
    let invalid = capture(root.path());
    assert_eq!(invalid.documents().count(), 2);
    assert_eq!(
        invalid.resolve().unwrap_err().code(),
        "workspace_machine_binding"
    );
}

#[test]
fn missing_selected_files_and_capture_limits_still_refuse_incomplete_inputs() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("gazette.toml"), ROOT).unwrap();
    assert!(
        configuration::capture_workspace(
            root.path(),
            &"gazette.toml".parse().unwrap(),
            Default::default()
        )
        .is_err()
    );
    fs::write(root.path().join("site.toml"), SITE).unwrap();
    let mut limits = configuration::WorkspaceLoadLimits::default();
    limits.source.file_bytes = 1;
    assert!(
        configuration::capture_workspace(root.path(), &"gazette.toml".parse().unwrap(), limits)
            .is_err()
    );
}
