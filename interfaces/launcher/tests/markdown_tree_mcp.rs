//! Real CLI and MCP transports select the same pinned capture and SDK projection.

#[path = "support/mcp.rs"]
mod carrier;

use gazette_sdk::{markdown, source};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Stdio},
};

#[test]
fn pinned_tree_cli_mcp_and_sdk_match_with_no_stdin_fallback() {
    let root = tempfile::tempdir().expect("native tree");
    let page = root.path().join("page.md");
    fs::write(&page, "+++\ntitle='Metadata'\n+++\n# Captured é\n").expect("document");
    fs::write(root.path().join(".asset"), "complete tree").expect("asset");
    let identity = source::inspect(root.path(), None, Default::default())
        .expect("independent pin")
        .identity();
    let arguments = json!({"tree":{"root":root.path(),"identity":identity,"document":"page.md"},
        "allow_native_reads":true,"front_matter":"toml","html":true});
    let responses = carrier::exchange(vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"inspect_markdown","arguments":arguments}})]);
    assert_eq!(responses[0]["result"]["isError"], false);
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(["inspect-markdown", "--tree"])
            .arg(root.path())
            .arg("--tree-identity")
            .arg(serde_json::to_string(&identity).expect("pin"))
            .args(["--document", "page.md", "--front-matter", "toml", "--html"])
            .stdin(Stdio::null())
            .output()
            .expect("direct CLI")
    };
    let cli = run();
    assert!(cli.status.success(), "{cli:?}");
    let cli: Value = serde_json::from_slice(&cli.stdout).expect("complete CLI report");
    let selected: markdown::input::Tree =
        serde_json::from_value(arguments["tree"].clone()).expect("SDK selector");
    let loaded = selected.load(true).expect("SDK capture");
    let native = markdown::input::Report {
        input: loaded.evidence(),
        projection: gazette_sdk::document::inspect(
            loaded.text().expect("UTF-8"),
            Default::default(),
            true,
        )
        .expect("native projection"),
    };
    assert_eq!(cli, json!(native));
    assert_eq!(cli, responses[0]["result"]["structuredContent"]);
    fs::write(root.path().join(".asset"), "changed asset only").expect("later asset edit");
    let failed = run();
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    let responses = carrier::exchange(vec![json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"inspect_markdown","arguments":arguments}})]);
    assert_eq!(responses[0]["result"]["isError"], true);
    assert!(
        responses[0]["result"]["structuredContent"]
            .get("input")
            .is_none()
    );
}
