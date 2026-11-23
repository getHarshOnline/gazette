//! Executable MCP store observations retain the actual SDK/CLI boundaries.

use super::*;
use serde_json::json;

fn call(arguments: Value) -> Value {
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect_store","arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == "inspect_store")
        .expect("storage tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("valid request");
    let result = responses[1]["result"].clone();
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&result["structuredContent"])
        .expect("actual native result");
    let text: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().expect("JSON text"))
            .expect("complete readable projection");
    assert_eq!(text, result["structuredContent"]);
    result
}

#[test]
fn raw_store_metadata_matches_cli_sdk_without_claiming_journal_validity_or_exposing_rows() {
    let root = tempdir().expect("native database fixture");
    let path = root.path().join("store.db");
    let database = Connection::open(&path).expect("native fixture database");
    database.execute_batch("CREATE TABLE private(value); INSERT INTO private VALUES ('PRIVATE_ROW_CANARY'); PRAGMA application_id=987; PRAGMA user_version=123;")
        .expect("deliberately non-Gazette database");
    drop(database);
    let before = fs::read(&path).expect("baseline");
    let read = ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    };
    let native = store::observe(&path, read).expect("raw SDK observation");
    let cli = run(&path, &["--allow-coordination-files"]);
    assert!(cli.status.success(), "{cli:?}");
    let result = call(json!({"path":path,"allow_read":true,"read":read}));
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"], json!(native));
    assert_eq!(
        serde_json::from_slice::<Value>(&cli.stdout).expect("CLI JSON"),
        json!(native)
    );
    assert_eq!(result["structuredContent"]["application_id"], 987);
    assert!(!result.to_string().contains("PRIVATE_ROW_CANARY"));
    assert!(!result.to_string().contains(path.to_str().expect("path")));
    assert!(
        store::inspect_journal(&path, None, read).is_err(),
        "raw metadata must not imply accepted Gazette schema"
    );
    assert_eq!(fs::read(&path).expect("unchanged database"), before);
    assert_eq!(
        fs::read_dir(root.path()).expect("closed fixture").count(),
        1
    );
}

#[test]
fn denied_missing_corrupt_and_native_limits_return_exact_failures_without_creation() {
    let root = tempdir().expect("native fixture");
    let path = root.path().join("missing.db");
    for (allow_read, read, code) in [
        (false, ReadOptions::default(), "storage_permission"),
        (
            true,
            ReadOptions::default(),
            "store_coordination_permission",
        ),
        (
            true,
            ReadOptions {
                allow_coordination_files: true,
                vm_steps: 0,
                ..Default::default()
            },
            "store_invalid_limits",
        ),
        (
            true,
            ReadOptions {
                allow_coordination_files: true,
                ..Default::default()
            },
            "store_io",
        ),
    ] {
        let result = call(json!({"path":path,"allow_read":allow_read,"read":read}));
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["code"], code);
    }
    assert!(
        fs::read_dir(root.path())
            .expect("no database or sidecars")
            .next()
            .is_none()
    );
    fs::write(&path, b"not SQLite").expect("malformed native fixture");
    for read in [
        ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
        ReadOptions {
            allow_coordination_files: true,
            storage_bytes: 1,
            ..Default::default()
        },
    ] {
        let native = store::observe(&path, read).expect_err("native refusal");
        let result = call(json!({"path":path,"allow_read":true,"read":read}));
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["code"], native.code());
        assert_eq!(
            result["structuredContent"]
                .as_object()
                .expect("failure")
                .len(),
            2
        );
    }
    assert_eq!(
        fs::read(&path).expect("unchanged malformed bytes"),
        b"not SQLite"
    );
}
