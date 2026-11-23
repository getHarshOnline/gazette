//! Recorded-recipe recovery through the executable carrier and independent native owners.

use super::{
    direct::{id, input, permitted, recipe},
    fs, initialize, private_directory, published_fixture, read,
};
use crate::mcp_carrier as carrier;
use gazette_sdk::{release_commit, source, store};
use serde_json::{Value, json};
use std::process::Command;

fn call(arguments: Value, failed: bool) -> Value {
    let responses = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"recover_journal_worktree","arguments":arguments}}),
    ]);
    let tool = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|t| t["name"] == "recover_journal_worktree")
        .expect("recovery tool");
    jsonschema::validator_for(&tool["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("exact input");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let value = response["result"]["structuredContent"].clone();
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(&value)
        .expect("native result schema");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("same result");
    assert_eq!(value, text);
    value
}

#[test]
fn executable_recorded_recovery_uses_exact_existing_recipe_and_cli_readback() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let mut request = input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    );
    request["sites"]
        .as_array_mut()
        .expect("explicit site set")
        .remove(0);
    let native = recipe(&request).execute(permitted());
    assert!(
        native.is_ok(),
        "actual preparation prerequisite: {native:?}"
    );
    let before = store::inspect_journal(&journal.database, None, read())
        .expect("current header")
        .header;
    let history = store::inspect_preparation(&journal.database, id("one"), Some(&before), read())
        .expect("native recorded history")
        .record
        .expect("preparation");
    let completion = &history
        .worktree
        .as_ref()
        .expect("original recipe")
        .completion
        .as_ref()
        .expect("complete fixture")
        .report;
    let commit = completion.commit.clone();
    let worktree = history.intent.address.path();
    let output = fs::read(worktree.join("model.json")).expect("original model");
    let git_head = fs::read(worktree.join(".git/HEAD")).expect("original detached HEAD");
    let args = json!({"database":journal.database,"preparation_id":id("one"),"commit":commit,
        "expected_header":before,"protected_roots":[fixture.directory.path()],"allow_read":true,"read":read(),
        "advance_writer_generation":true,"reserve_local_coordination":true,"record_preparation":true,"synchronize_worktree":true});
    let recovered = call(args.clone(), false);
    assert_eq!(recovered["record"], json!(history));
    let current = store::inspect_journal(&journal.database, None, read())
        .expect("generation after recovery")
        .header;
    assert_eq!(current.writer_fence, before.writer_fence + 1);
    let inspected = release_commit::inspect_journal_worktree(
        release_commit::JournalWorktreeInspectionRequest {
            database: &journal.database,
            preparation_id: id("one"),
            expected_header: Some(&current),
            commit: &commit,
            protected_roots: &[fixture.directory.path().to_owned()],
        },
        release_commit::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("independent native history/output/Git readback");
    let mut cli = Command::new(env!("CARGO_BIN_EXE_gazette"));
    let cli = cli
        .arg("inspect-journal-worktree")
        .arg(&journal.database)
        .arg(id("one").to_string())
        .arg(commit.as_str())
        .arg("--expected-header")
        .arg(serde_json::to_string(&current).expect("header"))
        .arg("--protected-root")
        .arg(fixture.directory.path())
        .arg("--allow-coordination-files")
        .output()
        .expect("native CLI readback");
    assert!(cli.status.success(), "{cli:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&cli.stdout).expect("CLI result"),
        json!(inspected)
    );
    assert_eq!(
        fs::read(worktree.join("model.json")).expect("unchanged model"),
        output
    );
    assert_eq!(
        fs::read(worktree.join(".git/HEAD")).expect("unchanged detached HEAD"),
        git_head
    );
    assert_eq!(
        call(args.clone(), true)["failure"]["code"],
        "store_journal_mismatch"
    );
    assert_eq!(
        store::inspect_journal(&journal.database, None, read())
            .expect("no repeated generation")
            .header,
        current
    );

    // Output synchronization can finish before Git storage synchronization fails.
    let mut storage_limited = args;
    storage_limited["expected_header"] = json!(current);
    storage_limited["storage_bytes"] = json!(1);
    let failed = call(storage_limited, true);
    assert_eq!(failed["failure"]["code"], "preparation_synchronization");
    assert_eq!(failed["failure"]["progress"]["writer_acquired"], true);
    assert_eq!(failed["failure"]["progress"]["output_sync"], "Verified");
    assert_ne!(failed["failure"]["progress"]["git_sync"], "Verified");
    let later = store::inspect_journal(&journal.database, None, read())
        .expect("native generation survives synchronization failure")
        .header;
    assert_eq!(later.writer_fence, current.writer_fence + 1);
    assert_eq!(failed["failure"]["progress"]["journal"], json!(later));
    let current = later;

    // A native verification failure must neither synthesize completion nor repair drift.
    fs::write(worktree.join("model.json"), b"changed disposable output")
        .expect("explicit fixture corruption");
    let mut drift = json!({"database":journal.database,"preparation_id":id("one"),"commit":commit,"expected_header":current,
        "protected_roots":[fixture.directory.path()],"allow_read":true,"read":read(),"advance_writer_generation":true,
        "reserve_local_coordination":true,"record_preparation":true,"synchronize_worktree":true});
    let failed = call(drift.clone(), true);
    assert!(
        failed["failure"]["code"]
            .as_str()
            .expect("native refusal code")
            .contains("worktree")
    );
    assert_eq!(failed["failure"]["progress"]["writer_acquired"], false);
    assert_eq!(
        store::inspect_journal(&journal.database, None, read())
            .expect("preflight no generation")
            .header,
        current
    );
    assert_eq!(
        fs::read(worktree.join("model.json")).expect("not repaired"),
        b"changed disposable output"
    );
    drift["preparation_id"] = json!(source::Sha256Digest::of(b"missing"));
    assert_eq!(call(drift, true)["failure"]["code"], "preparation_missing");
}
