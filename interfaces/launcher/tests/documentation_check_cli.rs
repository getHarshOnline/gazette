//! The direct CLI projects the same effect-free documentation intent as MCP.

use serde_json::Value;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[path = "support/mcp.rs"]
mod carrier;

fn configuration() -> &'static str {
    "[[principals]]\nid='owner:one'\nkind='person'\n\
     [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
     [[projects]]\nid='project:gazette'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
     [[sources]]\nid='source:gazette'\nlocation={kind='repository',root='/not-opened/gazette',commit='1111111111111111111111111111111111111111',subdirectory='project'}\n\
     [[brands]]\nid='brand:gazette'\nname='Gazette'\n\
     [[brands.publications]]\nsite='site:one'\n\
     [brands.publications.documentation_repository]\nname='gazette-domain-doc'\npath='/not-opened/gazette-domain-doc'\n\
     [brands.publications.documentation_repository.build]\ntarget={kind='mintlify',theme='mint',primary_color='#123abc'}\n\
     [[brands.publications.documentation_repository.build.modules]]\nproject='project:gazette'\nslug='gazette'\ntitle='Gazette manual'\nmanual={source='source:gazette'}\n\
     navigation=[{kind='manual',title='Start',pages=[{title='Overview',route='gazette/overview',input={kind='file',path='docs/overview.md'}}]}]\n"
}

fn run(arguments: &[&str], source: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Gazette");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("workspace bytes");
    child.wait_with_output().expect("Gazette completion")
}

#[test]
fn check_prints_resolved_manual_intent_without_local_or_native_read_permission() {
    let output = run(
        &["assemble-docs", "--check", "--site", "site:one"],
        configuration(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete JSON report");
    assert_eq!(report["repository_name"], "gazette-domain-doc");
    assert_eq!(report["repository_path"], "/not-opened/gazette-domain-doc");
    assert_eq!(
        report["build"]["modules"][0]["navigation"][0]["kind"],
        "manual"
    );
}

#[test]
fn check_refuses_effect_selectors_and_target_incompatibility_before_native_work() {
    let conflict = run(
        &[
            "assemble-docs",
            "--check",
            "--site",
            "site:one",
            "--output-parent",
            "/not-opened/stage",
        ],
        configuration(),
    );
    assert!(!conflict.status.success());
    let invalid = configuration().replace("theme='mint'", "theme='unknown'");
    let output = run(
        &["assemble-docs", "--check", "--site", "site:one"],
        &invalid,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("documentation_target_input"));
}

#[test]
fn cli_and_initialized_mcp_preserve_nested_reference_intent_without_opening_sources() {
    let source = configuration().replace(
        "navigation=[{kind='manual',title='Start',pages=[{title='Overview',route='gazette/overview',input={kind='file',path='docs/overview.md'}}]}]",
        "navigation=[{kind='manual',title='Start',pages=[{title='Overview',route='gazette/overview',input={kind='file',path='docs/overview.md'}}]},{kind='reference',slug='library',title='Library',pages=[{kind='page',path='index'},{kind='group',title='Members',pages=[{kind='page',path='types/Value'}]}],input={kind='artifact',source='source:gazette',selection={kind='directory',path='reference'},format='markdown'}}]",
    );
    assert_ne!(source, configuration(), "fixture selects a reference");
    let output = run(&["assemble-docs", "--check", "--site", "site:one"], &source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        cli["build"]["modules"][0]["navigation"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        cli["build"]["modules"][0]["navigation"][0]["kind"],
        "manual"
    );
    assert_eq!(
        cli["build"]["modules"][0]["navigation"][1]["pages"],
        serde_json::json!([
            {"kind":"page","path":"index"},
            {"kind":"group","title":"Members","pages":[{"kind":"page","path":"types/Value"}]}
        ])
    );
    carrier::initialized_interact("2026-07-28", |_, send| {
        let response = send(serde_json::json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"check_documentation","arguments":{"source":source,"site":"site:one"}}
        }));
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(response["result"]["structuredContent"], cli);
        let invalid = source.replace("format='markdown'", "format='typedoc-json'");
        let failed = send(serde_json::json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"check_documentation","arguments":{"source":invalid,"site":"site:one"}}
        }));
        assert_eq!(failed["result"]["isError"], true, "{failed}");
        assert_eq!(
            failed["result"]["structuredContent"]["code"],
            "documentation_target_input"
        );
    });
}
