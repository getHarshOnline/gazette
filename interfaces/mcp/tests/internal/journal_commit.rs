//! Native journal effects through the real carrier, not reconstructed success fixtures.

use super::{call, exchange, request, result, runtime, server};
use gazette_cli::sdk::{release_commit as commit, source, store};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

struct Fixture {
    _root: tempfile::TempDir,
    journal: PathBuf,
    repository: PathBuf,
    output: PathBuf,
    initialized: store::InitializationReceipt,
    metadata: commit::CommitMetadata,
    prepared: commit::CommitReport,
}

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn tree(path: &Path) -> source::TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("native complete inventory")
        .identity()
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private fixture");
        let journal = root.path().join("journal");
        let output = root.path().join("output");
        let repository = root.path().join("objects.git");
        for path in [&journal, &output] {
            fs::create_dir(path).expect("fixture directory");
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("private directory");
        }
        fs::write(output.join("index.html"), b"<h1>Exact</h1>\r\n").expect("retained output");
        git2::Repository::init_bare(&repository).expect("fixture bare repository");
        let signature = commit::CommitSignature {
            name: "PRIVATE fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 100,
            offset_minutes: 0,
        };
        let metadata = commit::CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "PRIVATE exact recipe\n".into(),
        };
        let prepared = commit::prepare(
            &repository,
            &output,
            tree(&output),
            &metadata,
            &[],
            commit::CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("native fixture commit");
        let initialized = store::initialize(
            &journal,
            store::InitializeOptions {
                allow_create: true,
                session: store::SessionOptions {
                    allow_local_reservation: true,
                    read: read(),
                },
                ..Default::default()
            },
        )
        .expect("native fixture journal");
        Self {
            _root: root,
            journal,
            repository,
            output,
            initialized,
            metadata,
            prepared,
        }
    }

    fn header(&self) -> store::JournalHeader {
        store::inspect_journal(&self.initialized.database, None, read())
            .expect("native current header")
            .header
    }

    fn arguments(&self, checkpoint: bool) -> Value {
        let mut value = json!({"allow_read":true,"expected_header":self.header(),"advance_writer_generation":true,
            "reserve_local_coordination":true,"read":read(),"protected_roots":[self.output],"commit":self.prepared.commit});
        if checkpoint {
            value["root"] = json!(self.journal);
            value["repository"] = json!(self.repository);
            value["checkpoint_id"] = json!(source::Sha256Digest::of(b"exact MCP checkpoint"));
            value["expected_tree"] = json!(self.prepared.output);
            value["metadata_json"] =
                json!(serde_json::to_string(&self.metadata).expect("exact JSON"));
            value["record_checkpoint"] = json!(true);
        } else {
            value["database"] = json!(self.initialized.database);
            value["preparation_id"] = json!(source::Sha256Digest::of(b"absent preparation"));
            value["record_preparation"] = json!(true);
            value["synchronize_worktree"] = json!(true);
        }
        value
    }
}

fn checked(name: &str, arguments: Value, failed: bool) -> Value {
    let list = call_list();
    let tool = list["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|t| t["name"] == name)
        .expect("named tool");
    jsonschema::validator_for(&tool["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("native-shaped request");
    let response = call(name, arguments);
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let output = response["result"]["structuredContent"].clone();
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(&output)
        .expect("native result schema");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("readable result"),
    )
    .expect("same JSON");
    assert_eq!(text, output);
    output
}

fn call_list() -> Value {
    runtime().block_on(exchange(
        request("tools/list", json!({})),
        server::Limits::default(),
    ))
}

#[test]
fn checkpoint_native_creation_replay_and_conflict_preserve_actual_journal_effects() {
    let f = Fixture::new();
    let original = (tree(&f.repository), tree(&f.output));
    let args = f.arguments(true);
    let id = serde_json::from_value(args["checkpoint_id"].clone()).expect("exact id");
    let before = f.header();
    let first = checked("checkpoint_release_commit", args.clone(), false);
    assert_eq!(f.header().writer_fence, before.writer_fence + 1);
    let history =
        commit::inspect_checkpoint(&f.initialized.database, id, Some(&f.header()), read())
            .expect("independent native inspection");
    assert_eq!(
        first["checkpoint"],
        json!(history.checkpoint.expect("actual checkpoint"))
    );
    let replay = checked("checkpoint_release_commit", f.arguments(true), false);
    assert_eq!(first["checkpoint"], replay["checkpoint"]);
    assert_eq!(f.header().writer_fence, before.writer_fence + 2);
    let stale = checked("checkpoint_release_commit", args, true);
    assert_eq!(stale["failure"]["code"], "store_journal_mismatch");
    assert_eq!(f.header().writer_fence, before.writer_fence + 2);
    assert_eq!((tree(&f.repository), tree(&f.output)), original);

    let mut other = f.metadata.clone();
    other.message.push_str("different valid recipe");
    let prepared = commit::prepare(
        &f.repository,
        &f.output,
        f.prepared.output,
        &other,
        &[],
        commit::CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("distinct native commit");
    let mut conflict = f.arguments(true);
    conflict["metadata_json"] = json!(serde_json::to_string(&other).expect("exact recipe"));
    conflict["commit"] = json!(prepared.commit);
    let failed = checked("checkpoint_release_commit", conflict, true);
    assert_eq!(failed["failure"]["code"], "store_checkpoint_conflict");
    assert_eq!(failed["failure"]["writer_acquired"], true);
    assert!(failed["failure"]["effect"].is_string());
    assert_eq!(
        f.header().writer_fence,
        before.writer_fence + 3,
        "conflict is not rollback of writer acquisition"
    );
    let history = commit::inspect_checkpoint(&f.initialized.database, id, None, read())
        .expect("original row survives");
    assert_eq!(
        first["checkpoint"],
        json!(history.checkpoint.expect("original record"))
    );
}

#[test]
fn permissions_exact_metadata_shapes_and_response_reservation_precede_native_writes() {
    let f = Fixture::new();
    let header = f.header();
    let bytes = fs::read(&f.initialized.database).expect("original journal");
    for (checkpoint, name) in [
        (true, "checkpoint_release_commit"),
        (false, "recover_journal_worktree"),
    ] {
        let args = f.arguments(checkpoint);
        let mut flags = vec![
            "allow_read",
            "advance_writer_generation",
            "reserve_local_coordination",
        ];
        flags.push(if checkpoint {
            "record_checkpoint"
        } else {
            "record_preparation"
        });
        if !checkpoint {
            flags.push("synchronize_worktree");
        }
        for flag in flags {
            let mut denied = args.clone();
            denied[flag] = json!(false);
            checked(name, denied, true);
            assert_eq!(f.header(), header);
        }
        let mut denied = args.clone();
        denied["read"]["allow_coordination_files"] = json!(false);
        checked(name, denied, true);
        for (field, value) in [
            ("expected_header", json!([])),
            ("expected_header", Value::Null),
            ("protected_roots", json!(["relative"])),
            ("read", json!([])),
            ("metadata", json!({})),
        ] {
            let mut bad = args.clone();
            bad[field] = value;
            assert!(call(name, bad).get("error").is_some());
        }
        let limits = server::Limits {
            output_bytes: 128,
            ..Default::default()
        };
        let response = runtime().block_on(exchange(
            request("tools/call", json!({"name":name,"arguments":args})),
            limits,
        ));
        assert_eq!(
            response["result"]["structuredContent"]["code"], "journal_response_contract",
            "reservation must precede mutation: {response}"
        );
        assert_eq!(f.header(), header);
    }
    let complete = serde_json::to_string(&f.metadata).expect("metadata");
    for malformed in [
        complete.replacen("\"parent\":null,", "", 1),
        complete.replacen(
            "\"parent\":null",
            "\"parent\":null,\"par\\u0065nt\":null",
            1,
        ),
    ] {
        let mut bad = f.arguments(true);
        bad["metadata_json"] = json!(malformed);
        let failed = checked("checkpoint_release_commit", bad, true);
        assert_eq!(failed["code"], "commit_metadata_input");
    }
    assert_eq!(f.header(), header);
    assert_eq!(
        fs::read(&f.initialized.database).expect("unchanged journal"),
        bytes
    );
}

#[test]
fn recorded_recovery_refuses_absent_history_without_inventing_recipe_or_generation() {
    let f = Fixture::new();
    let before = f.header();
    let args = f.arguments(false);
    let failed = checked("recover_journal_worktree", args.clone(), true);
    assert_eq!(failed["failure"]["code"], "preparation_missing");
    assert_eq!(failed["failure"]["progress"]["writer_acquired"], false);
    assert_eq!(f.header(), before);
    let id = serde_json::from_value(args["preparation_id"].clone()).expect("exact id");
    assert!(
        store::inspect_preparation(&f.initialized.database, id, Some(&before), read())
            .expect("independent inspection")
            .record
            .is_none()
    );
    let mut rebind = args;
    rebind["worktree"] = json!(f.output);
    assert!(
        call("recover_journal_worktree", rebind)
            .get("error")
            .is_some()
    );
    let response = call(
        "inspect_journal",
        json!({"path":f.initialized.database,"allow_read":true,"read":read(),"expected_header":before}),
    );
    let _ = result(&response);
}

#[test]
fn compact_checkpoint_delivery_keeps_original_native_coordinates_without_repeating_effects() {
    use std::os::unix::ffi::OsStrExt;
    let mut f = Fixture::new();
    f.metadata.message = "PRIVATE exact bounded recipe\n".repeat(1600);
    f.prepared = commit::prepare(
        &f.repository,
        &f.output,
        tree(&f.output),
        &f.metadata,
        &[],
        commit::CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("real commit with larger exact metadata");
    let before = f.header();
    let mut args = f.arguments(true);
    let refused = checked("checkpoint_release_commit", args.clone(), true);
    assert_eq!(refused["code"], "commit_metadata_input");
    assert_eq!(f.header(), before, "input refusal precedes journal effects");
    args["max_metadata_bytes"] = json!(
        args["metadata_json"]
            .as_str()
            .expect("original recipe bytes")
            .len()
    );
    let limits = server::Limits {
        output_bytes: 16 << 10,
        ..Default::default()
    };
    let response = runtime().block_on(exchange(
        request(
            "tools/call",
            json!({"name":"checkpoint_release_commit","arguments":args}),
        ),
        limits,
    ));
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], true);
    let output = &response["result"]["structuredContent"];
    assert_eq!(output["code"], "checkpoint_report_limit");
    assert_eq!(output["locator"], json!(f.journal.as_os_str().as_bytes()));
    assert_eq!(output["id"], args["checkpoint_id"]);
    assert!(
        output["failure"].is_null(),
        "native operation succeeded; delivery did not"
    );
    assert_eq!(output["receipt"]["commit"], json!(f.prepared));
    let current = f.header();
    assert_eq!(current.writer_fence, before.writer_fence + 1);
    assert_eq!(output["receipt"]["journal"], json!(current));
    let id = serde_json::from_value(args["checkpoint_id"].clone()).expect("exact id");
    let observed = commit::inspect_checkpoint(&f.initialized.database, id, Some(&current), read())
        .expect("read back original row")
        .checkpoint
        .expect("original recorded checkpoint");
    assert_eq!(observed.metadata, f.metadata);
    assert_eq!(f.header(), current, "readback is not an automatic replay");
    let list = call_list();
    let tool = list["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|t| t["name"] == "checkpoint_release_commit")
        .expect("tool");
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(output)
        .expect("compact native custody schema");
}
