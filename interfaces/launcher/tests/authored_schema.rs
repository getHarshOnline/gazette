//! Authored schema discovery reaches the real CLI and an initialized MCP process.

#[allow(dead_code)]
#[path = "support/mcp.rs"]
mod carrier;

use gazette_sdk::schema::{self, Contract};
use serde_json::{Value, json};
use std::process::Command;

#[test]
fn cli_returns_the_native_authored_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["schema", "authored-content"])
        .output()
        .expect("run Gazette schema");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).expect("schema JSON");
    assert_eq!(actual, json!(schema::contract(Contract::AuthoredContent)));
    assert!(
        actual["$defs"]["ContentNode"]["properties"]
            .get("keywords")
            .is_some()
    );
    assert!(
        actual["$defs"]["ContentNode"]["properties"]
            .get("schema_type")
            .is_some()
    );
}

#[test]
fn initialized_mcp_discovers_and_returns_the_same_authored_contract() {
    carrier::interact(|send| {
        let initialized = send(
            json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
                "protocolVersion":"2026-07-28", "capabilities":{},
                "clientInfo":{"name":"authored-contract-proof", "version":"1"}
            }}),
        );
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let catalog = send(json!({"jsonrpc":"2.0", "id":2, "method":"tools/list", "params":{}}));
        let descriptor = catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "contract_schema")
            .expect("discoverable schema tool");
        assert!(
            descriptor["inputSchema"]
                .to_string()
                .contains("authored-content")
        );
        let response = send(
            json!({"jsonrpc":"2.0", "id":3, "method":"tools/call", "params":{
                "name":"contract_schema", "arguments":{"contract":"authored-content"}
            }}),
        );
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(
            response["result"]["structuredContent"],
            json!(schema::contract(Contract::AuthoredContent))
        );
    });
}

#[test]
fn legal_source_discovery_is_identical_over_cli_sdk_and_initialized_mcp() {
    let expected = json!(schema::contract(Contract::LegalSource));
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["schema", "legal-source"])
        .output()
        .expect("run real legal-source discovery");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        expected
    );
    let fields = expected["$defs"]["LegalSourceMetadata"]["properties"]
        .as_object()
        .expect("native reduced source fields");
    assert!(fields.contains_key("id"));
    for absent in ["site", "route", "structure", "publisher", "attributions"] {
        assert!(
            !fields.contains_key(absent),
            "{absent} is not source-owned metadata"
        );
    }
    carrier::interact(|send| {
        let initialized = send(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2026-07-28","capabilities":{},
                "clientInfo":{"name":"legal-contract-proof","version":"1"}
            }}),
        );
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let catalog = send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
        let tool = catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "contract_schema")
            .expect("schema discovery tool");
        assert!(tool["inputSchema"].to_string().contains("legal-source"));
        let response = send(
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                "name":"contract_schema","arguments":{"contract":"legal-source"}
            }}),
        );
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(response["result"]["structuredContent"], expected);
    });
}
