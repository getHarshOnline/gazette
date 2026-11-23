//! Workspace defaults reach native page checks without relaxing parser bounds.

use super::{PAGE, WORKSPACE, fixture};
use gazette_sdk::{build, configuration, corpus};
use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn configured_corpus_limit_reaches_cli_sdk_and_mcp_checks() {
    let root = fixture();
    let path = root.path().canonicalize().unwrap();
    let page = PAGE.replace("related=['content:missing']\n", "");
    fs::write(path.join("pages/one.md"), &page).unwrap();
    fs::write(path.join("pages/asset.bin"), vec![0; 17 << 20]).unwrap();
    let location = configuration::locate_workspace(configuration::WorkspaceLookup {
        request: Some(&path),
        client: None,
        start: &path,
        default: None,
    })
    .unwrap()
    .unwrap();
    let sdk = |limits| {
        build::check_workspace(
            location.clone(),
            Default::default(),
            build::PublicationSetAcquisitionLimits::default().sources,
            limits,
        )
    };
    let cli = || {
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(["check", "--workspace-root"])
            .arg(&path)
            .output()
            .unwrap()
    };
    let mcp = || {
        let responses = crate::mcp_carrier::exchange(vec![json!({
            "jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":"check_workspace","arguments":{"workspace":path}}
        })]);
        responses[0]["result"].clone()
    };

    let default = sdk(Default::default());
    assert!(!default.is_valid());
    assert!(
        default
            .diagnostics
            .iter()
            .any(|row| row.code == "corpus_limit")
    );
    let command = cli();
    assert!(!command.status.success());
    let expected = serde_json::to_value(&default).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&command.stdout).unwrap(),
        expected
    );
    let response = mcp();
    assert_eq!(response["isError"], true);
    assert_eq!(response["structuredContent"], expected);

    fs::write(
        path.join("gazette.toml"),
        format!(
            "[build]\nsites=['site:one']\nlocal_drafts='include'\ncorpus_bytes={}\n{WORKSPACE}",
            18 << 20
        ),
    )
    .unwrap();
    let configured = sdk(Default::default());
    assert!(configured.is_valid(), "{configured:?}");
    assert_eq!(configured.checked_sites.len(), 1);
    let expected = serde_json::to_value(configured).unwrap();
    let command = cli();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&command.stdout).unwrap(),
        expected
    );
    let response = mcp();
    assert_eq!(response["isError"], false);
    assert_eq!(response["structuredContent"], expected);

    let explicit = sdk(build::CorpusLimits {
        source_bytes: Some(16 << 20),
        ..Default::default()
    });
    assert!(!explicit.is_valid());
    assert!(
        explicit
            .diagnostics
            .iter()
            .any(|row| row.code == "corpus_limit")
    );
    let concrete = sdk(corpus::Limits::default().into());
    assert_eq!(
        serde_json::to_value(concrete).unwrap(),
        serde_json::to_value(explicit).unwrap(),
        "an explicitly supplied native default stays an override"
    );

    let mut limits = build::CorpusLimits::default();
    limits.document.source_bytes = page.len() - 1;
    let bounded = sdk(limits);
    assert!(!bounded.is_valid());
    assert!(
        bounded
            .diagnostics
            .iter()
            .any(|row| row.code == "document_limit")
    );
    assert!(
        bounded
            .diagnostics
            .iter()
            .all(|row| row.code != "corpus_limit")
    );
}
