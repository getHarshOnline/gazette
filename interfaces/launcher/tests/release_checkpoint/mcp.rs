//! CLI-produced checkpoints and MCP writes share exact native history and effect semantics.

#[path = "../support/mcp.rs"]
mod carrier;

use super::*;
use serde_json::json;

fn arguments(f: &Fixture, id: Sha256Digest) -> Value {
    json!({"root":f.journal,"repository":f.repository,"commit":f.prepared.commit,
        "checkpoint_id":id,"expected_header":f.header(),"expected_tree":f.expected,
        "metadata_json":serde_json::to_string(&f.metadata).expect("exact native metadata"),
        "protected_roots":[f.source,f.output],"allow_read":true,"read":read(),
        "record_checkpoint":true,"advance_writer_generation":true,"reserve_local_coordination":true})
}

fn call(arguments: Value, failed: bool) -> Value {
    let responses = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"checkpoint_release_commit","arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|t| t["name"] == "checkpoint_release_commit")
        .expect("checkpoint tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("native request");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let output = response["result"]["structuredContent"].clone();
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&output)
        .expect("native effect schema");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("same result");
    assert_eq!(text, output);
    output
}

#[test]
fn checkpoint_executables_share_creation_replay_and_independent_native_readback() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"CLI MCP same original checkpoint");
    let untouched = fixture.snapshot();
    let cli = success(fixture.create(id, fixture.header()));
    let replay = call(arguments(&fixture, id), false);
    assert_eq!(cli["checkpoint"], replay["checkpoint"]);
    assert_eq!(
        replay["journal"]["writer_fence"]
            .as_u64()
            .expect("generation"),
        cli["journal"]["writer_fence"].as_u64().expect("generation") + 1
    );
    assert_eq!(fixture.inspect(id)["checkpoint"], cli["checkpoint"]);
    let other = Sha256Digest::of(b"MCP creates distinct checkpoint coordinate");
    let created = call(arguments(&fixture, other), false);
    let sdk = release_commit::checkpoint(
        &fixture.journal,
        fixture.header(),
        CheckpointRequest {
            id: other,
            repository: &fixture.repository,
            commit: &fixture.prepared.commit,
            output: fixture.expected,
            metadata: &fixture.metadata,
            protected_roots: &[fixture.source.clone(), fixture.output.clone()],
        },
        options(),
    )
    .expect("SDK replays actual MCP history");
    assert_eq!(created["checkpoint"], json!(sdk.checkpoint));
    assert_eq!(
        fixture.snapshot(),
        untouched,
        "no source/output/Git changes from checkpoint"
    );
}

#[test]
fn executable_checkpoint_stale_and_metadata_refusals_do_not_repeat_writer_effects() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"MCP exact refusal coordinate");
    let original = arguments(&fixture, id);
    let mut malformed = original.clone();
    let metadata = malformed["metadata_json"]
        .as_str()
        .expect("exact JSON")
        .replacen("\"parent\":null", "\"parent\":null,\"parent\":null", 1);
    malformed["metadata_json"] = json!(metadata);
    let header = fixture.header();
    assert_eq!(call(malformed, true)["code"], "commit_metadata_input");
    assert_eq!(fixture.header(), header);
    let first = call(original.clone(), false);
    let current = fixture.header();
    assert_eq!(
        call(original, true)["failure"]["code"],
        "store_journal_mismatch"
    );
    assert_eq!(fixture.header(), current);
    assert_eq!(fixture.inspect(id)["checkpoint"], first["checkpoint"]);
}
