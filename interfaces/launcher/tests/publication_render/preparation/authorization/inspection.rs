//! Real preparation evidence crosses SDK, CLI and discoverable MCP without changing custody.

use super::{Prepared, batch_options, cli, declaration, fs, json, read, release, sign, worktree};
use serde_json::Value;

pub(super) fn call(name: &str, arguments: Value, failed: bool) -> Value {
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == name)
        .expect("implemented tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("actual wire arguments match discovery");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let result = &response["result"]["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(result)
        .expect("actual native evidence matches discovery");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text evidence"),
    )
    .expect("JSON evidence");
    assert_eq!(&text, result);
    result.clone()
}

fn arguments(prepared: &Prepared) -> Value {
    json!({"database":prepared.database,"expected_header":prepared.header,
        "selections_json":serde_json::to_string(&prepared.selections).expect("exact selections"),
        "protected_roots":prepared.protected,"allow_read":true,
        "options":{"inspection":{"read":{"allow_coordination_files":true}}}})
}

#[test]
fn historical_checkpoint_is_the_same_native_evidence_through_all_interfaces() {
    let mut prepared = Prepared::new();
    let selected = &prepared.selections[0];
    let record = &prepared.records[0];
    let bare = super::private_directory();
    let repository = bare.path();
    let target = git2::Repository::init_bare(repository).expect("independent bare fixture");
    let source =
        git2::Repository::open(record.intent.address.path()).expect("prepared fixture Git");
    let source_objects = source.odb().expect("native prepared objects");
    let target_objects = target.odb().expect("independent fixture objects");
    source_objects
        .foreach(|oid| {
            let object = source_objects
                .read(*oid)
                .expect("read exact fixture object");
            assert_eq!(
                target_objects
                    .write(object.kind(), object.data())
                    .expect("copy fixture bytes"),
                *oid
            );
            true
        })
        .expect("copy complete disposable native object set");
    let id = super::Sha256Digest::of(b"disposable checkpoint inspection fixture");
    let receipt = worktree::checkpoint(
        prepared._journal_root.path(),
        prepared.header,
        worktree::CheckpointRequest {
            id,
            repository,
            commit: &selected.commit,
            output: record.intent.output,
            metadata: &record
                .worktree
                .as_ref()
                .expect("actual retained recipe")
                .metadata,
            protected_roots: &prepared.protected,
        },
        worktree::CheckpointOptions {
            writer: super::options().writer,
            record: worktree::CheckpointWriteOptions { allow_record: true },
            ..Default::default()
        },
    )
    .expect("record independently verified existing Git commit");
    prepared.header = receipt.journal;
    let before = prepared.snapshot();
    let native =
        worktree::inspect_checkpoint(&prepared.database, id, Some(&prepared.header), read())
            .expect("native checkpoint history");
    assert_eq!(json!(native.checkpoint), json!(Some(&receipt.checkpoint)));
    let args = json!({"path":prepared.database,"checkpoint_id":id,"expected_header":prepared.header,
        "allow_read":true,"read":{"allow_coordination_files":true}});
    assert_eq!(
        call("inspect_release_checkpoint", args, false),
        json!(native)
    );
    let output = cli(
        "",
        &[
            "inspect-release-checkpoint",
            prepared.database.to_str().expect("fixture UTF-8"),
            &id.to_string(),
            "--allow-coordination-files",
            "--expected-header",
            &serde_json::to_string(&prepared.header).expect("header"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI checkpoint"),
        json!(native)
    );
    prepared.unchanged(&before);
}

#[test]
fn complete_preparation_and_signature_join_have_sdk_cli_mcp_parity() {
    let prepared = Prepared::new();
    let batch = prepared.inspect();
    let before = prepared.snapshot();
    assert_eq!(
        call("inspect_release_preparations", arguments(&prepared), false),
        json!(batch)
    );
    let output = cli(
        &prepared.envelope().to_string(),
        &["inspect-release-preparations", "--allow-coordination-files"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI evidence"),
        json!(batch)
    );
    let (body, binding) = declaration(&batch);
    let (bytes, binding) = sign(body, binding);
    let report = super::verify(&prepared, &prepared.selections, &bytes, &binding)
        .expect("actual signed local join");
    let mut args = arguments(&prepared);
    args["authorization_json"] = json!(std::str::from_utf8(&bytes).expect("exact UTF-8"));
    args["binding_json"] = json!(serde_json::to_string(&binding).expect("independent binding"));
    assert_eq!(
        call("verify_prepared_authorization", args, false),
        json!(report)
    );
    #[derive(serde::Serialize)]
    struct Envelope<'a> {
        #[serde(flatten)]
        selection: Value,
        authorization: &'a serde_json::value::RawValue,
        binding: &'a release::Binding,
    }
    let envelope = serde_json::to_string(&Envelope {
        selection: prepared.envelope(),
        authorization: serde_json::from_slice(&bytes).expect("unmodified signed body"),
        binding: &binding,
    })
    .expect("preserve exact raw authorization transport");
    let output = cli(
        &envelope,
        &[
            "verify-prepared-authorization",
            "--allow-coordination-files",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI join"),
        json!(report)
    );
    prepared.unchanged(&before);
}

#[test]
fn worktree_inspection_and_changed_output_use_native_whole_set_failure() {
    let prepared = Prepared::new();
    let before = prepared.snapshot();
    for selected in &prepared.selections {
        let native = worktree::inspect_journal_worktree(
            worktree::JournalWorktreeInspectionRequest {
                database: &prepared.database,
                preparation_id: selected.preparation_id,
                expected_header: Some(&prepared.header),
                commit: &selected.commit,
                protected_roots: &prepared.protected,
            },
            worktree::JournalWorktreeInspectionOptions {
                read: read(),
                ..Default::default()
            },
        )
        .expect("native complete worktree");
        let args = json!({"path":prepared.database,"preparation_id":selected.preparation_id,
            "commit":selected.commit,"expected_header":prepared.header,
            "protected_roots":prepared.protected,"allow_read":true,"read":{"allow_coordination_files":true}});
        assert_eq!(call("inspect_journal_worktree", args, false), json!(native));
    }
    prepared.unchanged(&before);
    let changed = prepared.records[1].intent.address.path().join("model.json");
    fs::write(&changed, b"changed disposable prepared artifact")
        .expect("controlled native corruption");
    let native =
        release::inspect_preparations(prepared.request(&prepared.selections), batch_options())
            .expect_err("one changed target refuses whole set");
    let failed = call("inspect_release_preparations", arguments(&prepared), true);
    assert_eq!(failed["code"], native.code());
    assert!(
        failed.get("ordered_targets").is_none(),
        "no successful shrinking subset"
    );
    assert_eq!(
        fs::read(&changed).expect("not repaired"),
        b"changed disposable prepared artifact"
    );
}
