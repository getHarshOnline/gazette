//! Actual MCP preparation reaches the same captured compiler and durable native phase owners.

use crate::mcp_carrier as carrier;

use gazette_sdk::{build, release_commit, source, store};
use serde_json::{Value, json};
use std::time::Duration;

use super::super::policy::configured;
use super::{
    direct::{id, input, permitted, recipe},
    fs, initialize, private_directory, published_fixture, read, unchanged,
};

pub(super) fn arguments(request: &Value) -> Value {
    json!({"recipe_json":request.to_string(),"allow_read":true,"create_stage":true,
        "record_preparation":true,"advance_writer_generation":true,"reserve_local_coordination":true,
        "initialize_worktree":true,"read":{"allow_coordination_files":true}})
}

pub(super) fn call(arguments: Value, failed: bool) -> Value {
    // Preparation exercises parsing, rendering, policy validation, Git, and
    // SQLite through the real MCP process. Keep its transport bound explicit,
    // while allowing the all-feature suite to run this fixture under parallel
    // compiler and provider load without turning scheduler contention into a
    // protocol failure.
    let responses = carrier::exchange_with_call_timeout(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"prepare_sites","arguments":arguments}}),
        ],
        Duration::from_secs(30),
    );
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("discovery")
        .iter()
        .find(|t| t["name"] == "prepare_sites")
        .expect("implemented tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("valid input schema")
        .validate(&arguments)
        .expect("actual call shape");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let result = &response["result"]["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("valid output schema")
        .validate(result)
        .expect("native wire result matches discovery");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("same readable JSON");
    assert_eq!(&text, result);
    result.clone()
}

#[test]
fn mcp_and_sdk_prepare_the_same_single_and_multi_site_outputs_and_commits() {
    let fixture = published_fixture(&[]);
    for single in [true, false] {
        let mut runs = Vec::new();
        for mcp in [true, false] {
            let root = private_directory();
            let journal = initialize(root.path());
            let parent = private_directory();
            let mut request = input(
                &fixture.config(),
                root.path(),
                journal.header,
                parent.path(),
            );
            if single {
                request["sites"]
                    .as_array_mut()
                    .expect("explicit site set")
                    .remove(0);
            }
            let value = if mcp {
                call(arguments(&request), false)
            } else {
                let result = recipe(&request).execute(permitted());
                assert!(result.is_ok(), "{result:?}");
                json!(build::LocalPreparationRunReport::new(&result))
            };
            assert_eq!(value["status"], "local-preparations-complete-not-certified");
            let sites = value["sites"].as_array().expect("complete requested set");
            assert_eq!(sites.len(), if single { 1 } else { 2 });
            let header = serde_json::from_value(
                sites.last().expect("last complete site receipt")["preparation"]["journal"].clone(),
            )
            .expect("observed native header");
            let mut artifacts = Vec::new();
            for site in sites {
                let key = site["site"]
                    .as_str()
                    .expect("site identity")
                    .strip_prefix("site:")
                    .expect("site prefix");
                let inspected =
                    store::inspect_preparation(&journal.database, id(key), Some(&header), read())
                        .expect("independent SQLite inspection");
                let record = inspected.record.expect("durable exact history");
                assert_eq!(json!(record), site["preparation"]["record"]);
                let commit = &record
                    .worktree
                    .as_ref()
                    .expect("recorded Git intent")
                    .completion
                    .as_ref()
                    .expect("recorded Git completion")
                    .report
                    .commit;
                release_commit::inspect_journal_worktree(
                    release_commit::JournalWorktreeInspectionRequest {
                        database: &journal.database,
                        preparation_id: id(key),
                        expected_header: Some(&header),
                        commit,
                        protected_roots: &[fixture.directory.path().to_owned()],
                    },
                    release_commit::JournalWorktreeInspectionOptions {
                        read: read(),
                        ..Default::default()
                    },
                )
                .expect("real journal, Git and complete output read-back");
                let model: Value = serde_json::from_slice(
                    &fs::read(record.intent.address.path().join("model.json"))
                        .expect("real staged model bytes"),
                )
                .expect("complete shared model");
                assert!(model["pages"][format!("content:{key}")].is_object());
                let native = git2::Repository::open(record.intent.address.path())
                    .expect("native independent worktree");
                assert!(native.head_detached().expect("native HEAD observation"));
                assert_eq!(native.remotes().expect("native remote list").len(), 0);
                artifacts.push((record.intent.output, commit.clone()));
            }
            if single {
                assert!(!parent.path().join("other").exists());
            }
            runs.push(artifacts);
        }
        assert_eq!(
            runs[0], runs[1],
            "interface choice does not reconstruct publication meaning"
        );
    }
}

#[test]
fn every_mcp_effect_permission_remains_independent_and_required() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("initial database bytes");
    let parent = private_directory();
    let valid = arguments(&input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    ));
    for field in [
        "allow_read",
        "create_stage",
        "record_preparation",
        "advance_writer_generation",
        "reserve_local_coordination",
        "initialize_worktree",
        "read",
    ] {
        let mut denied = valid.clone();
        denied
            .as_object_mut()
            .expect("named tool arguments")
            .remove(field);
        let value = call(denied, true);
        assert_ne!(value["status"], "local-preparations-complete-not-certified");
        unchanged(&journal, id("other"), &baseline);
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("stage parent inspection")
                .count(),
            0
        );
    }
    for field in ["max_input_bytes", "max_output_bytes"] {
        let mut bounded = valid.clone();
        bounded[field] = json!(512);
        call(bounded, true);
        unchanged(&journal, id("one"), &baseline);
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("stage parent inspection")
                .count(),
            0
        );
    }
}

#[test]
fn mcp_second_site_failure_preserves_prefix_and_newer_native_evidence() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let sentinel = parent.path().join("one");
    fs::write(&sentinel, b"unrelated existing child").expect("disposable conflicting child");
    let value = call(
        arguments(&input(
            &fixture.config(),
            root.path(),
            journal.header,
            parent.path(),
        )),
        true,
    );
    assert_eq!(value["status"], "incomplete");
    assert_eq!(value["requested"], json!(["site:other", "site:one"]));
    assert_eq!(
        value["completed"]
            .as_array()
            .expect("completed site prefix")
            .len(),
        1
    );
    assert_eq!(value["completed"][0]["site"], "site:other");
    assert_eq!(value["failure"]["site"], "site:one");
    let evidence = &value["failure"]["phase"]["evidence"];
    assert_eq!(value["failure"]["phase"]["kind"], "stage");
    let current: store::JournalHeader =
        serde_json::from_value(evidence["journal"].clone()).expect("latest native header");
    let prior: store::JournalHeader =
        serde_json::from_value(value["completed"][0]["preparation"]["journal"].clone())
            .expect("earlier completed header");
    assert!(current.writer_fence > prior.writer_fence);
    let observed = store::inspect_preparation(&journal.database, id("one"), Some(&current), read())
        .expect("independent native inspection")
        .record
        .expect("recorded intent");
    assert_eq!(json!(observed), evidence["receipt"]["record"]);
    assert!(observed.retention.is_none() && observed.worktree.is_none());
    assert_eq!(
        fs::read(&sentinel).expect("preserved unrelated child"),
        b"unrelated existing child"
    );
}

#[test]
fn mcp_policy_failure_retains_exact_native_invocation_and_stops_later_sites() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("initial database bytes");
    let parent = private_directory();
    let observation = private_directory();
    let check = observation.path().join("received.json");
    let (config, _) = configured(&fixture, &check, 7);
    let mut args = arguments(&input(&config, root.path(), journal.header, parent.path()));
    call(args.clone(), true);
    assert!(!check.exists());
    unchanged(&journal, id("other"), &baseline);
    args["allow_policy_execution"] = json!(true);
    let value = call(args, true);
    assert_eq!(value["failure"]["phase"]["kind"], "policy");
    let evidence = &value["failure"]["phase"]["evidence"];
    assert_eq!(evidence["exit"]["code"], 7);
    assert_eq!(
        evidence["invocation"]["request_sha256"],
        json!(source::Sha256Digest::of(
            &fs::read(check).expect("exact checker input bytes")
        ))
    );
    assert_eq!(value["completed"], json!([]));
    assert!(value["failure"]["prior_receipt"]["record"]["retention"].is_object());
    assert_eq!(
        value["failure"]["prior_receipt"]["record"]["worktree"],
        Value::Null
    );
    assert!(
        store::inspect_preparation(&journal.database, id("one"), None, read())
            .expect("independent native inspection")
            .record
            .is_none()
    );
}

#[test]
fn mcp_overflow_after_real_effects_returns_exact_reconciliation_not_retry_advice() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let mut args = arguments(&input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    ));
    args["max_output_bytes"] = json!(2048);
    let value = call(args, true);
    assert_eq!(value["status"], "reconciliation-required");
    assert_eq!(value["journal"], json!(root.path()));
    assert_eq!(value["input_expected_header"], json!(journal.header));
    assert!(value.to_string().len() <= 2048);
    assert!(
        value["message"]
            .as_str()
            .expect("reconciliation explanation")
            .contains("Do not retry")
    );
    for (i, site) in ["other", "one"].iter().enumerate() {
        assert_eq!(value["preparations"][i]["site"], format!("site:{site}"));
        assert_eq!(value["preparations"][i]["preparation_id"], json!(id(site)));
        let observed = store::inspect_preparation(&journal.database, id(site), None, read())
            .expect("independent native inspection");
        assert!(observed.journal.header.writer_fence > journal.header.writer_fence);
        assert!(
            observed
                .record
                .expect("durable preparation")
                .worktree
                .expect("recorded Git intent")
                .completion
                .is_some(),
            "response overflow did not roll back or recreate native effects"
        );
    }
}

#[test]
fn failed_mcp_carrier_does_not_erase_completed_native_preparation() {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let args = arguments(&input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    ));
    let mut child = carrier::Process(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(fs::File::open("/dev/null").expect("read-only output descriptor"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("native MCP process"),
    );
    let mut writer = child.0.stdin.take().expect("process input");
    writeln!(writer,"{}",json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"prepare_sites","arguments":args,"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}
    }})).expect("complete request frame");
    writer.flush().expect("flushed request");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.0.try_wait().expect("process status") {
            assert!(!status.success());
            break;
        }
        assert!(Instant::now() < deadline, "failed carrier must terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(writer);
    let mut diagnostic = String::new();
    child
        .0
        .stderr
        .take()
        .expect("private diagnostic pipe")
        .read_to_string(&mut diagnostic)
        .expect("complete terminal diagnostic");
    assert!(
        diagnostic.contains("admitted operations may have run"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("no publication operation performed"));
    for site in ["other", "one"] {
        let observed = store::inspect_preparation(&journal.database, id(site), None, read())
            .expect("independent native inspection");
        assert!(
            observed
                .record
                .expect("durable preparation")
                .worktree
                .expect("recorded Git intent")
                .completion
                .is_some(),
            "lost carrier is not absent native effect"
        );
    }
}
