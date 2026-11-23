//! Native registry inspection through MCP verifies stored rows rather than trusting their head.

use super::*;
use serde_json::json;

fn reconcile_call(arguments: Value) -> Value {
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"reconcile_registry","arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == "reconcile_registry")
        .expect("mutation discovered");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("native request shape");
    let result = responses[1]["result"].clone();
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&result["structuredContent"])
        .expect("complete outcome schema");
    assert_eq!(
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().expect("text"))
            .expect("JSON"),
        result["structuredContent"]
    );
    result
}

fn reconcile_arguments(config: &str, root: &Path, expected: JournalHeader) -> Value {
    json!({"source":config,"site":"site:one","allow_read":true,"root":root,
        "expected_header":expected,"options":options()})
}

#[test]
fn mcp_reconciliation_replaces_the_complete_model_with_sdk_cli_readback_parity() {
    let fixture = Fixture::new(&[("one/page.md", PAGE.as_bytes())]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let root = private_directory();
    let initial = initialize(root.path());
    let response = reconcile_call(reconcile_arguments(&config, root.path(), initial.header));
    assert_eq!(response["isError"], false);
    let returned = response["structuredContent"].clone();
    let header: JournalHeader =
        serde_json::from_value(returned["journal"].clone()).expect("native header");
    assert_eq!(header.writer_fence, initial.header.writer_fence + 1);
    let inspection =
        store::inspect_registry(&initial.database, Some(&header), read()).expect("native rows");
    assert_eq!(json!(inspection.registry), returned["registry"]);
    let cli_inspection = inspect_cli(&initial.database, &header);
    assert!(cli_inspection.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&cli_inspection.stdout).expect("CLI inspection"),
        json!(inspection)
    );
    let native_rows = rows(&initial.database);
    let second = reconcile(&inputs, root.path(), header, options())
        .expect("same retained inputs through SDK");
    assert_eq!(json!(second.registry), returned["registry"]);
    assert_eq!(rows(&initial.database), native_rows);
    let third = reconcile_cli(&config, root.path(), &second.journal, None, &[]);
    assert!(third.status.success(), "{third:?}");
    let third: Value = serde_json::from_slice(&third.stdout).expect("same shared CLI result");
    assert_eq!(third["registry"], returned["registry"]);
    assert_eq!(rows(&initial.database), native_rows);

    let empty = Fixture::new(&[
        ("one/assets/input.bin", b"one"),
        ("other/assets/input.bin", b"other"),
    ]);
    let empty_config = empty
        .config()
        .replace("subdirectory='one'", "subdirectory='one/assets'")
        .replace("subdirectory='other'", "subdirectory='other/assets'");
    let next = serde_json::from_value(third["journal"].clone()).expect("retained header");
    let response = reconcile_call(reconcile_arguments(&empty_config, root.path(), next));
    assert_eq!(response["isError"], false);
    let connection =
        Connection::open_with_flags(&initial.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("independent content count");
    let count: i64 = connection
        .query_row("SELECT count(*) FROM gazette_registry_content", [], |row| {
            row.get(0)
        })
        .expect("complete empty replacement");
    assert_eq!(count, 0);
    assert_ne!(
        response["structuredContent"]["registry"]["rows_sha256"],
        returned["registry"]["rows_sha256"]
    );
}

#[test]
fn mcp_registry_refusals_preserve_store_and_report_native_acquisition_phase() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let root = private_directory();
    let initial = initialize(root.path());
    let before = fs::read(&initial.database).expect("before refusal");
    for pointer in [
        "/allow_read",
        "/options/registry/allow_replace",
        "/options/writer/allow_generation_advance",
        "/options/writer/session/allow_local_reservation",
        "/options/writer/session/read/allow_coordination_files",
    ] {
        let mut args = reconcile_arguments(&config, root.path(), initial.header);
        *args.pointer_mut(pointer).expect("independent permission") = json!(false);
        let result = reconcile_call(args);
        assert_eq!(result["isError"], true, "{pointer}");
        unchanged(&initial.database, &initial.header, &before);
    }
    let mut limited = reconcile_arguments(&config, root.path(), initial.header);
    limited["options"]["projection"]["bytes"] = json!(0);
    let result = reconcile_call(limited);
    assert_eq!(result["structuredContent"]["phase"], "projection");
    assert_eq!(result["structuredContent"]["code"], "registry_encoding");
    unchanged(&initial.database, &initial.header, &before);
    let mut stale = initial.header;
    stale.writer_fence += 1;
    let result = reconcile_call(reconcile_arguments(&config, root.path(), stale));
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["phase"], "acquire");
    assert_eq!(result["structuredContent"]["generation"], "NotAttempted");
    assert!(result["structuredContent"].get("journal").is_none());
    unchanged(&initial.database, &initial.header, &before);
    let output = reconcile_cli(&config, root.path(), &stale, None, &[]);
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("shared CLI error"),
        result["structuredContent"]
    );
    unchanged(&initial.database, &initial.header, &before);
    let bounded = reconcile_cli(
        &config,
        root.path(),
        &initial.header,
        None,
        &["--max-output-bytes", "1024"],
    );
    assert!(!bounded.status.success());
    assert!(bounded.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bounded.stderr).contains("registry_response_limit"));
    unchanged(&initial.database, &initial.header, &before);
}

fn call(path: &Path, expected: JournalHeader, read: ReadOptions) -> Value {
    let arguments = json!({"path":path,"allow_read":true,"read":read,"expected_header":expected});
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect_registry","arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == "inspect_registry")
        .expect("registry tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("request shape");
    let result = responses[1]["result"].clone();
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&result["structuredContent"])
        .expect("native output schema");
    assert_eq!(
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().expect("text"))
            .expect("complete JSON"),
        result["structuredContent"]
    );
    result
}

#[test]
fn empty_and_populated_registry_inspection_matches_sdk_cli_and_never_reconciles() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.config());
    let root = private_directory();
    let initial = initialize(root.path());
    let empty_before = fs::read(&initial.database).expect("empty baseline");
    let empty = call(&initial.database, initial.header, read());
    assert_eq!(empty["isError"], false);
    assert!(empty["structuredContent"]["registry"].is_null());
    assert_eq!(
        empty["structuredContent"],
        json!(
            store::inspect_registry(&initial.database, Some(&initial.header), read())
                .expect("empty SDK inspection")
        )
    );
    assert_eq!(
        fs::read(&initial.database).expect("no rows created"),
        empty_before
    );

    let receipt = reconcile(&inputs, root.path(), initial.header, options())
        .expect("actual retained publication registry");
    let before = fs::read(&initial.database).expect("populated baseline");
    let native_rows = rows(&initial.database);
    assert!(!native_rows.is_empty());
    let native = store::inspect_registry(&initial.database, Some(&receipt.journal), read())
        .expect("independent SDK rows");
    let cli = inspect_cli(&initial.database, &receipt.journal);
    assert!(cli.status.success(), "{cli:?}");
    let result = call(&initial.database, receipt.journal, read());
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"], json!(native));
    assert_eq!(
        serde_json::from_slice::<Value>(&cli.stdout).expect("CLI JSON"),
        json!(native)
    );
    assert_eq!(
        result["structuredContent"]["registry"],
        json!(receipt.registry)
    );
    assert!(!result.to_string().contains("content:one"), "no row dump");
    for (header, limits) in [
        (initial.header, read()),
        (
            receipt.journal,
            ReadOptions {
                storage_bytes: 1,
                ..read()
            },
        ),
    ] {
        let native = store::inspect_registry(&initial.database, Some(&header), limits)
            .expect_err("exact stale/bounds refusal");
        let result = call(&initial.database, header, limits);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["code"], native.code());
        assert!(result["structuredContent"].get("registry").is_none());
    }
    assert_eq!(rows(&initial.database), native_rows);
    unchanged(&initial.database, &receipt.journal, &before);
}

#[test]
fn native_registry_corruption_is_not_a_successful_stale_head_or_repair() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.config());
    let root = private_directory();
    let initial = initialize(root.path());
    let receipt =
        reconcile(&inputs, root.path(), initial.header, options()).expect("native registry");
    let connection = Connection::open(&initial.database).expect("test-only contamination");
    assert_eq!(connection.execute("UPDATE gazette_registry_content SET title='PRIVATE_CORRUPT_ROW' WHERE id='content:one'", []).expect("one changed row"), 1);
    connection.close().expect("contaminating fixture closed");
    let before = fs::read(&initial.database).expect("corrupt baseline");
    let native = store::inspect_registry(&initial.database, Some(&receipt.journal), read())
        .expect_err("row verification");
    let result = call(&initial.database, receipt.journal, read());
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["code"], native.code());
    assert_eq!(native.code(), "store_registry_verification");
    assert!(!result.to_string().contains("PRIVATE_CORRUPT_ROW"));
    assert!(result["structuredContent"].get("registry").is_none());
    let cli = inspect_cli(&initial.database, &receipt.journal);
    assert!(!cli.status.success());
    assert!(cli.stdout.is_empty());
    assert_eq!(fs::read(&initial.database).expect("no repair"), before);
}
