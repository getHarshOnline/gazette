//! Real CLI/MCP coordinator enrollment, delivery and managed-history boundaries.

#[path = "support/mcp.rs"]
mod mcp_carrier;

use gazette_test_support as native;

use gazette_sdk::{
    release,
    source::Sha256Digest,
    store::{self, coordinator},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
};

fn protected(root: &Path) -> store::LifecycleProtectedRoots {
    let roots = vec![root.to_path_buf()];
    store::LifecycleProtectedRoots {
        authored: roots.clone(),
        staged: roots.clone(),
        published: roots.clone(),
        live_journals: roots,
    }
}

const SESSION: &[&str] = &["--reserve-local-coordination", "--allow-coordination-files"];
const ENROLL: &[&str] = &[
    "--write-coordinator-anchor",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];
const ADOPT: &[&str] = &[
    "--adopt-restore",
    "--write-coordinator-anchor",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private fixture")
}

fn session() -> store::SessionOptions {
    store::SessionOptions {
        allow_local_reservation: true,
        read: store::ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
    }
}

struct Fixture {
    anchor: tempfile::TempDir,
    journal: tempfile::TempDir,
    initialized: store::InitializationReceipt,
}

impl Fixture {
    fn new() -> Self {
        let anchor = private();
        let journal = private();
        let initialized = store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session: session(),
                ..Default::default()
            },
        )
        .expect("real fresh journal");
        Self {
            anchor,
            journal,
            initialized,
        }
    }

    fn input(&self) -> Value {
        json!({"anchor_root":self.anchor.path(),"journal_root":self.journal.path(),
            "coordinator_id":"coordinator local-Éxact", "principal_scope":Sha256Digest::of(b"explicit local scope"),"protected_roots":[]})
    }

    fn enroll(&self) -> coordinator::LocalCoordinatorSeal {
        let output = run(
            "enroll-local-coordinator",
            &self.input().to_string(),
            ENROLL,
        );
        serde_json::from_value(report(&output)).expect("native seal")
    }

    fn inspect(&self, seal: &coordinator::LocalCoordinatorSeal) -> Value {
        json!({"anchor_root":self.anchor.path(),"binding":seal.binding})
    }
}

fn run(command: &str, input: &str, flags: &[&str]) -> Output {
    let cwd = private();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg(command)
        .args(flags)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real Gazette executable");
    child
        .stdin
        .take()
        .expect("private input")
        .write_all(input.as_bytes())
        .expect("complete input");
    let output = child.wait_with_output().expect("native process completion");
    assert_eq!(
        fs::read_dir(cwd.path()).expect("no cwd fallback").count(),
        0
    );
    output
}

fn report(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("complete native JSON")
}

#[test]
fn cli_adopts_exact_restore_into_fresh_successor_and_replays_same_receipt() {
    let fixture = Fixture::new();
    let seal = fixture.enroll();
    let backups = private();
    let backup = store::backup(store::BackupRequest {
        source: &fixture.initialized.database,
        output_parent: backups.path(),
        protected: protected(fixture.journal.path()),
        options: store::BackupOptions {
            allow_create: true,
            read: session().read,
            ..Default::default()
        },
    })
    .expect("real backup");
    let restores = private();
    let restore = store::prepare_restore(store::PrepareRestoreRequest {
        backup: &backup.directory,
        manifest_sha256: backup.manifest_sha256,
        output_parent: restores.path(),
        protected: protected(fixture.journal.path()),
        options: store::RestoreOptions {
            allow_prepare: true,
            verification: store::BackupVerifyOptions {
                read: session().read,
                ..Default::default()
            },
            ..Default::default()
        },
    })
    .expect("detached restore");
    let successor = private();
    let input = json!({
        "anchor_root": fixture.anchor.path(),
        "binding": seal.binding,
        "previous": seal,
        "restore_root": restore.directory,
        "receipt_sha256": restore.receipt_sha256,
        "successor_root": successor.path(),
        "protected_roots": []
    });
    let first = report(&run("adopt-store-restore", &input.to_string(), ADOPT));
    assert_eq!(
        first["successor"]["header"]["writer_fence"],
        json!(fixture.initialized.header.writer_fence + 1)
    );
    let replay = report(&run("adopt-store-restore", &input.to_string(), ADOPT));
    assert_eq!(replay, first);
}

#[test]
fn cli_and_mcp_enrollment_inspection_match_sdk_with_no_adoption_or_repair() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.initialized.database).expect("initial journal");
    let seal = fixture.enroll();
    assert_eq!(seal.header, fixture.initialized.header);
    assert_eq!(seal.binding.coordinator_id, "coordinator local-Éxact");
    let inspection = fixture.inspect(&seal);
    let cli = report(&run(
        "inspect-local-coordinator",
        &inspection.to_string(),
        SESSION,
    ));
    let sdk = coordinator::inspect(fixture.anchor.path(), &seal.binding, session())
        .expect("SDK inspection");
    assert_eq!(cli, json!(sdk));
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("no generation advance"),
        before
    );
    let anchor_before =
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("native anchor");
    assert!(
        !run(
            "enroll-local-coordinator",
            &fixture.input().to_string(),
            ENROLL
        )
        .status
        .success()
    );
    assert_eq!(
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("no adoption"),
        anchor_before
    );

    let fresh = Fixture::new();
    let mut input = fresh.input();
    input["allow_read"] = true.into();
    input["write_coordinator_anchor"] = true.into();
    input["reserve_local_coordination"] = true.into();
    input["read"] = json!(session().read);
    let responses = mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"enroll_local_coordinator","arguments":input}}),
    ]);
    let tools = responses[0]["result"]["tools"]
        .as_array()
        .expect("native catalog");
    let tool = tools
        .iter()
        .find(|tool| tool["name"] == "enroll_local_coordinator")
        .expect("enrollment discovery");
    jsonschema::validator_for(&tool["inputSchema"])
        .expect("input schema")
        .validate(&input)
        .expect("actual valid request");
    let result = &responses[1]["result"];
    assert_eq!(result["isError"], false);
    let value = &result["structuredContent"];
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(value)
        .expect("actual native result");
    assert_eq!(
        *value,
        serde_json::from_str::<Value>(result["content"][0]["text"].as_str().expect("text JSON"))
            .expect("same JSON")
    );
    let seal: coordinator::LocalCoordinatorSeal =
        serde_json::from_value(value.clone()).expect("MCP native seal");
    let inspection = fresh.inspect(&seal);
    let mut args = inspection.clone();
    args["allow_read"] = true.into();
    args["reserve_local_coordination"] = true.into();
    args["read"] = json!(session().read);
    let responses = mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"inspect_local_coordinator","arguments":args}}),
    ]);
    assert_eq!(
        responses[0]["result"]["structuredContent"],
        report(&run(
            "inspect-local-coordinator",
            &inspection.to_string(),
            SESSION
        ))
    );
    fs::write(fresh.anchor.path().join("coordinator.json"), b"broken")
        .expect("disposable corruption");
    assert!(
        !run(
            "inspect-local-coordinator",
            &inspection.to_string(),
            SESSION
        )
        .status
        .success()
    );
    assert_eq!(
        fs::read(fresh.anchor.path().join("coordinator.json")).expect("no repair"),
        b"broken"
    );
}

#[test]
fn exact_permissions_strict_objects_and_bounds_refuse_before_effects() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.initialized.database).expect("initial journal");
    for omitted in ENROLL {
        let flags: Vec<_> = ENROLL
            .iter()
            .copied()
            .filter(|flag| flag != omitted)
            .collect();
        let output = run(
            "enroll-local-coordinator",
            &fixture.input().to_string(),
            &flags,
        );
        assert!(!output.status.success(), "{omitted}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            fs::read_dir(fixture.anchor.path())
                .expect("no anchor")
                .count(),
            0
        );
        assert_eq!(
            fs::read(&fixture.initialized.database).expect("no SQL mutation"),
            before
        );
    }
    let mut unknown = fixture.input();
    unknown["unknown"] = true.into();
    let mut relative = fixture.input();
    relative["journal_root"] = "relative".into();
    let duplicate =
        fixture
            .input()
            .to_string()
            .replacen('{', "{\"coordinator_id\":\"duplicate\",", 1);
    for input in [
        "[]".to_owned(),
        "null".to_owned(),
        unknown.to_string(),
        relative.to_string(),
        duplicate,
    ] {
        let output = run("enroll-local-coordinator", &input, ENROLL);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("coordinator_request"));
    }
    for (flag, value) in [
        ("--max-input-bytes", "1"),
        ("--max-output-bytes", "1"),
        ("--max-value-bytes", "1"),
    ] {
        let mut flags = ENROLL.to_vec();
        flags.extend([flag, value]);
        assert!(
            !run(
                "enroll-local-coordinator",
                &fixture.input().to_string(),
                &flags
            )
            .status
            .success()
        );
        assert_eq!(
            fs::read_dir(fixture.anchor.path())
                .expect("no partial anchor")
                .count(),
            0
        );
    }
    let seal = fixture.enroll();
    let mut tuple = fixture.inspect(&seal);
    tuple["binding"] = json!([
        seal.binding.coordinator_id,
        seal.binding.principal_scope,
        seal.binding.journal_root,
        seal.binding.journal_id,
        seal.binding.journal_epoch
    ]);
    let mut extra = fixture.inspect(&seal);
    extra["binding"]["unknown"] = true.into();
    let duplicate = fixture
        .inspect(&seal)
        .to_string()
        .replace("\"binding\":{", "\"binding\":{\"journal_epoch\":1,");
    for input in [tuple.to_string(), extra.to_string(), duplicate] {
        let output = run("inspect-local-coordinator", &input, SESSION);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("coordinator_request"));
    }
    let help = run("enroll-local-coordinator", "", &["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).expect("help UTF-8");
    for flag in ENROLL {
        assert!(text.contains(flag));
    }
    assert!(!text.contains("--advance-writer-generation"));
    assert_eq!(
        run(
            "inspect-local-coordinator",
            "",
            &["--write-coordinator-anchor"]
        )
        .status
        .code(),
        Some(2)
    );
}

#[test]
fn lost_stdout_preserves_effect_coordinates_and_refuses_false_success() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    let fixture = Fixture::new();
    let (reader, writer) = UnixStream::pair().expect("closed response carrier");
    drop(reader);
    let stdout: OwnedFd = writer.into();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("enroll-local-coordinator")
        .args(ENROLL)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::piped())
        .spawn()
        .expect("real command");
    child
        .stdin
        .take()
        .expect("input")
        .write_all(fixture.input().to_string().as_bytes())
        .expect("request");
    let output = child.wait_with_output().expect("closed-carrier result");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("redacted error and private coordinates");
    assert!(stderr.contains("coordinator_response_unavailable"));
    assert!(stderr.contains(fixture.anchor.path().to_str().expect("anchor path")));
    assert!(stderr.contains(fixture.journal.path().to_str().expect("journal path")));
    assert!(stderr.contains("coordinator local-Éxact"));
    let state: coordinator::LocalCoordinatorState = serde_json::from_slice(
        &fs::read(fixture.anchor.path().join("coordinator.json"))
            .expect("native effect survived response loss"),
    )
    .expect("state");
    let coordinator::LocalCoordinatorState::Clean { seal, .. } = state else {
        panic!("fresh seal");
    };
    assert_eq!(seal.header, fixture.initialized.header);
    assert_eq!(
        report(&run(
            "inspect-local-coordinator",
            &fixture.inspect(&seal).to_string(),
            SESSION
        )),
        json!(coordinator::LocalCoordinatorState::Clean {
            seal,
            adoption: None,
        })
    );
}

#[test]
fn enrolled_cli_selection_wraps_real_managed_observation_and_clean_readback() {
    let fixture = Fixture::new();
    let seal = fixture.enroll();
    let remote = private();
    let isolation = private();
    let repository =
        git2::Repository::init_bare(remote.path()).expect("disposable native Git destination");
    let oid = repository
        .treebuilder(None)
        .expect("tree builder")
        .write()
        .expect("empty tree");
    let tree = repository.find_tree(oid).expect("native tree");
    let signature =
        git2::Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(1, 0))
            .expect("explicit identity");
    let commit = repository
        .commit(
            Some("refs/heads/main"),
            &signature,
            &signature,
            "fixture",
            &tree,
            &[],
        )
        .expect("local fixture commit");
    let authorization: Value = serde_json::from_str(include_str!(
        "../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("native target fixture");
    let mut target: release::Target =
        serde_json::from_value(authorization["ordered_targets"][0].clone()).expect("typed target");
    target.ordinal = 0;
    target.remote = remote.path().to_str().expect("remote path").into();
    target.branch = "main".into();
    target.local_commit_oid = commit.to_string().parse().expect("complete OID");
    target.refspec = format!("{}:refs/heads/main", target.local_commit_oid);
    let id = Sha256Digest::of(b"coordinated executable observation");
    let request = json!({"root":seal.binding.journal_root,"expected_header":seal.header,"id":id,
        "targets":[target],"git":{"program":native::git(),"isolation":isolation.path()},
        "protected_roots":[remote.path()],"coordinator":fixture.inspect(&seal)});
    let flags = [
        "--allow-read",
        "--record-observation",
        "--advance-writer-generation",
        "--reserve-local-coordination",
        "--allow-coordination-files",
        "--write-coordinator-anchor",
    ];
    let before = fs::read(&fixture.initialized.database).expect("before denied effect");
    let denied = run(
        "record-release-observation",
        &request.to_string(),
        &flags[..5],
    );
    assert!(!denied.status.success());
    let denied: Value = serde_json::from_slice(&denied.stdout).expect("native refusal evidence");
    assert_eq!(denied["status"], "not-started");
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("permission before generation"),
        before
    );
    let ref_before =
        fs::read(remote.path().join("refs/heads/main")).expect("before local advertisement");
    let recorded = report(&run(
        "record-release-observation",
        &request.to_string(),
        &flags,
    ));
    assert_eq!(recorded["status"], "observation-recorded-not-settlement");
    assert_eq!(
        recorded["receipt"]["record"]["readbacks"][0]["readback"]["comparison"],
        "intended"
    );
    let inspected = report(&run(
        "inspect-local-coordinator",
        &fixture.inspect(&seal).to_string(),
        SESSION,
    ));
    assert_eq!(inspected["state"], "clean");
    assert_eq!(inspected["seal"]["header"], recorded["receipt"]["journal"]);
    assert_eq!(
        inspected["seal"]["database_sha256"],
        json!(Sha256Digest::of(
            &fs::read(&fixture.initialized.database).expect("sealed database")
        ))
    );
    let history = report(&run("inspect-release-observation", &json!({"database":fixture.initialized.database,"id":id,"expected_header":recorded["receipt"]["journal"]}).to_string(), &["--allow-coordination-files"]));
    assert_eq!(history["record"], recorded["receipt"]["record"]);
    assert_eq!(
        fs::read(remote.path().join("refs/heads/main")).expect("no push"),
        ref_before
    );
}
