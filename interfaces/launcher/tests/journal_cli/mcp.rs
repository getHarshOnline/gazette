//! Executable journal tools compare independent coordinates without acquiring a writer.

use super::*;
use serde_json::json;

fn call(path: &Path, expected: Option<JournalHeader>) -> Value {
    let arguments =
        json!({"path":path,"allow_read":true,"read":read_options(),"expected_header":expected});
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect_journal","arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == "inspect_journal")
        .expect("journal tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("request shape");
    let result = responses[1]["result"].clone();
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&result["structuredContent"])
        .expect("native output");
    assert_eq!(
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().expect("text"))
            .expect("complete JSON"),
        result["structuredContent"]
    );
    result
}

#[test]
fn independent_expected_header_and_observation_match_sdk_cli_and_never_rebind() {
    let (root, initial) = fixture();
    let before = fs::read(&initial.database).expect("baseline");
    for expected in [None, Some(initial.header)] {
        let native = store::inspect_journal(&initial.database, expected.as_ref(), read_options())
            .expect("SDK");
        let cli = command(&initial.database, expected.as_ref())
            .output()
            .expect("CLI");
        assert!(cli.status.success(), "{cli:?}");
        let result = call(&initial.database, expected);
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"], json!(native));
        assert_eq!(
            serde_json::from_slice::<Value>(&cli.stdout).expect("CLI JSON"),
            json!(native)
        );
    }
    for field in 0..4 {
        let mut expected = initial.header;
        match field {
            0 => expected.schema_version += 1,
            1 => expected.journal_id[0] ^= 1,
            2 => expected.journal_epoch += 1,
            _ => expected.writer_fence += 1,
        }
        let native = store::inspect_journal(&initial.database, Some(&expected), read_options())
            .expect_err("independent mismatch");
        let result = call(&initial.database, Some(expected));
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["code"], native.code());
        assert!(result["structuredContent"].get("header").is_none());
        reject(
            command(&initial.database, Some(&expected))
                .output()
                .expect("CLI mismatch"),
            native.code(),
        );
    }
    assert_eq!(fs::read(&initial.database).expect("unchanged"), before);
    assert_eq!(fs::read_dir(root.path()).expect("no new files").count(), 1);
}

#[test]
fn wal_inspection_observes_committed_state_not_an_uncommitted_writer_and_rejects_schema_corruption()
{
    let (_root, initial) = fixture();
    let writer = Connection::open(&initial.database).expect("independent fixture writer");
    writer.execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE; UPDATE gazette_journal SET writer_fence=writer_fence+1;")
        .expect("uncommitted competing writer");
    let native = store::inspect_journal(&initial.database, Some(&initial.header), read_options())
        .expect("committed snapshot");
    let result = call(&initial.database, Some(initial.header));
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"], json!(native));
    writer
        .execute_batch("ROLLBACK; PRAGMA journal_mode=DELETE;")
        .expect("close fixture transaction");
    drop(writer);
    contaminate(&initial.database, "CREATE TABLE unexpected(value);");
    let before = fs::read(&initial.database).expect("corrupt baseline");
    let native = store::inspect_journal(&initial.database, None, read_options())
        .expect_err("unexpected schema");
    let result = call(&initial.database, None);
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["code"], native.code());
    assert_eq!(fs::read(&initial.database).expect("no repair"), before);
}
