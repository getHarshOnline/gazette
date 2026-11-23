use super::*;
use gazette_sdk::authoring::session::{Limits, native::Workspace};
use gazette_sdk::source::Sha256Digest;

fn identifier(path: &std::path::Path) -> Result<String, &'static str> {
    // These fixtures use tempfile's ASCII names on the supported macOS/Linux hosts.
    Ok(format!(
        "file://{}",
        path.to_str().ok_or("non-UTF-8 fixture path")?
    ))
}

fn context() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let source = "[[principals]]\nid='owner:fixture'\nkind='person'\n";
    let path = root_path.join("gazette.toml");
    std::fs::write(&path, source).unwrap();
    let mut editor = Workspace::open_workspace(
        root_path.clone(),
        "gazette.toml".parse().unwrap(),
        vec![],
        identifier,
        Limits::default(),
    )
    .unwrap();
    let id = identifier(&path).unwrap();
    let text = format!("{source}# Unsaved café 🦀\n");
    editor.open(&id, &path, 7, text.clone()).unwrap();
    let value = json!({
        "format":"gazette-editor-context/2", "workspace":identifier(&root_path).unwrap(),
        "generation":editor.session().generation(),
        "active":{"document":id,"offset":0,"version":7,"source_sha256":Sha256Digest::of(text.as_bytes())},
        "manifest":editor.session().snapshot(), "native_selection":editor.handoff_selection(),
        "overlays":[{"id":id,"version":7,"text":text}],
    });
    (root, value)
}

fn inspect(context: &Value, allow: bool) -> std::process::Output {
    let mut command = Command::new(executable());
    command.arg("import-editor-context");
    if allow {
        command.arg("--allow-read");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), context).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn compact_import_requires_receiving_permission_and_matches_cli_mcp_state() {
    let (_root, context) = context();
    assert!(!inspect(&context, false).status.success());
    let output = inspect(&context, true);
    assert!(output.status.success(), "{output:?}");
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["active"], context["active"]);
    assert_eq!(
        receipt["manifest"]["sources"],
        context["manifest"]["sources"]
    );
    mcp::initialized_interact("2026-07-28", |_, send| {
        let denied = send(
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"authoring_import_editor_context","arguments":{"context":context}}}),
        );
        assert_eq!(denied["result"]["isError"], true, "{denied}");
        let accepted = send(
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"authoring_import_editor_context","arguments":{"context":context,"allow_native_reads":true}}}),
        );
        assert_eq!(accepted["result"]["isError"], false, "{accepted}");
        let state = &accepted["result"]["structuredContent"];
        let status = send(
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
            "name":"authoring_status","arguments":{"session":state["session"]}}}),
        );
        assert_eq!(
            status["result"]["structuredContent"]["files"][0]["version"],
            7
        );
        let closed = send(
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
            "name":"authoring_close_session","arguments":{"session":state["session"],"generation":state["generation"]}}}),
        );
        assert_eq!(closed["result"]["isError"], false, "{closed}");
    });
}

#[test]
fn compact_import_refuses_stale_saved_input_invalid_cursor_and_forged_permission() {
    let (root, mut context) = context();
    context["allow_native_reads"] = json!(true);
    assert!(
        !inspect(&context, false).status.success(),
        "export contents cannot grant permission"
    );
    context
        .as_object_mut()
        .unwrap()
        .remove("allow_native_reads");
    context["active"]["offset"] = json!(usize::MAX);
    assert!(!inspect(&context, true).status.success());
    context["active"]["offset"] = json!(0);
    std::fs::write(root.path().join("gazette.toml"), "# Later saved input\n").unwrap();
    assert!(!inspect(&context, true).status.success());
}
