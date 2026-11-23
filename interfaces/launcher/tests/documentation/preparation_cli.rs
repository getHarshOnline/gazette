//! The existing executable assembly command exposes durable technical preparation.

use super::*;
use gazette_sdk::store;
use std::os::unix::fs::PermissionsExt;

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn journal() -> (TempDir, store::InitializationReceipt) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let receipt = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
            ..Default::default()
        },
    )
    .unwrap();
    (root, receipt)
}

fn selected(root: &Path, header: store::JournalHeader, parent: &Path) -> Value {
    json!({
        "journal": root, "expected": header, "parent": parent, "name": "docs-inputs",
        "id": source::Sha256Digest::of(b"CLI documentation preparation"),
        "create_stage": true, "record_preparation": true,
        "writer": {"allow_generation_advance": true,
            "session": {"allow_local_reservation": true,
                "read": {"allow_coordination_files": true}}},
        "evidence_bytes": 1048576
    })
}

fn invoke(config: &str, selection: &str, extra: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "assemble-docs",
            "--local",
            "--site",
            "site:one",
            "--preparation-json",
            selection,
        ])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Invalid selections may exit before stdin acquisition, so BrokenPipe is a valid rejection.
    let _ = child.stdin.take().unwrap().write_all(config.as_bytes());
    child.wait_with_output().unwrap()
}

#[test]
fn executable_prepares_exact_provider_inputs_and_reports_replay_without_certification() {
    let fixture = Fixture::new(&artifact());
    let (root, initialized) = journal();
    let parent = tempdir().unwrap();
    let mut selection = selected(root.path(), initialized.header, parent.path());
    let output = invoke(&fixture.configuration(), &selection.to_string(), &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["status"],
        "documentation-prepared-local-not-certified"
    );
    assert!(report["preparation"]["record"]["check"].is_null());
    assert!(report["preparation"]["record"]["worktree"].is_null());
    assert_eq!(
        report["preparation"]["record"]["intent"]["destination"],
        json!({
            "kind": "git",
            "remote": "https://example.invalid/configured-domain-doc.git",
            "branch": "published"
        })
    );
    assert!(report["jobs"].as_array().unwrap().is_empty());
    assert!(!fixture.intended().exists());
    let header = serde_json::from_value(report["preparation"]["journal"].clone()).unwrap();
    let id = source::Sha256Digest::of(b"CLI documentation preparation");
    let record = store::inspect_preparation(&initialized.database, id, Some(&header), read())
        .unwrap()
        .record
        .unwrap();
    assert_eq!(
        source::inspect(&record.intent.address.path(), None, Default::default())
            .unwrap()
            .identity(),
        record.intent.output
    );
    let evidence: Value = serde_json::from_str(&record.intent.evidence).unwrap();
    assert_eq!(evidence["artifact_class"], "documentation-build-input");
    selection["expected"] = json!(header);
    let replay = invoke(&fixture.configuration(), &selection.to_string(), &[]);
    assert!(!replay.status.success());
    let custody: Value = serde_json::from_slice(&replay.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&replay.stdout),
            String::from_utf8_lossy(&replay.stderr)
        )
    });
    assert_eq!(
        custody["failure"]["code"],
        "documentation_preparation_replay"
    );
    assert_eq!(custody["preparation"]["receipt"]["id"], json!(id));
    assert_eq!(
        custody["preparation"]["receipt"]["evidence_sha256"],
        json!(record.intent.evidence_sha256)
    );
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
}

#[test]
fn malformed_duplicate_or_conflicting_preparation_is_rejected_before_native_effects() {
    let fixture = Fixture::new(&artifact());
    let (root, initialized) = journal();
    let parent = tempdir().unwrap();
    let selection = selected(root.path(), initialized.header, parent.path());
    let encoded = selection.to_string();
    let duplicate = format!("{{\"create_stage\":false,{}", &encoded[1..]);
    let mut denied = selection.clone();
    denied["record_preparation"] = json!(false);
    for value in [duplicate, "[]".into(), denied.to_string()] {
        let output = invoke(&fixture.configuration(), &value, &[]);
        assert!(!output.status.success());
    }
    let conflict = invoke(
        &fixture.configuration(),
        &encoded,
        &["--output-parent", parent.path().to_str().unwrap()],
    );
    assert!(!conflict.status.success());
    let bounded = invoke(
        &fixture.configuration(),
        &encoded,
        &["--max-output-bytes", "1"],
    );
    assert!(!bounded.status.success());
    assert!(fs::read_dir(parent.path()).unwrap().next().is_none());
    assert!(!fixture.intended().exists());
    assert!(
        store::inspect_preparation(
            &initialized.database,
            source::Sha256Digest::of(b"CLI documentation preparation"),
            Some(&initialized.header),
            read()
        )
        .unwrap()
        .record
        .is_none()
    );
}
