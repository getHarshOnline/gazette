//! The existing assembly tool retains generator and journal custody together.

use super::*;
use gazette_cli::sdk::store;
use std::os::unix::fs::PermissionsExt;

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

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn selection(root: &Path, header: store::JournalHeader, parent: &Path) -> Value {
    json!({
        "journal": root, "expected": header, "parent": parent, "name": "retained-docs",
        "id": source::Sha256Digest::of(b"MCP documentation preparation"),
        "create_stage": true, "record_preparation": true,
        "writer": {"allow_generation_advance": true,
            "session": {"allow_local_reservation": true,
                "read": {"allow_coordination_files": true}}},
        "evidence_bytes": 1048576
    })
}

#[test]
fn generated_inputs_prepare_and_changed_retry_preserves_generator_and_journal_custody() {
    let fixture = Fixture::new(COPY, 1);
    let (root, initialized) = journal();
    let mut arguments = fixture.arguments(1, false);
    arguments["preparation"] = selection(root.path(), initialized.header, fixture.target.path());
    let tool = json!(descriptor());
    assert!(
        jsonschema::validator_for(&tool["inputSchema"])
            .unwrap()
            .is_valid(&arguments)
    );
    let prepared = report(call(arguments.clone(), MAXIMUM).unwrap(), false);
    assert_eq!(
        prepared["status"],
        "documentation-prepared-local-not-certified"
    );
    let receipt = &prepared["preparation"];
    assert!(receipt["record"]["check"].is_null());
    assert!(receipt["record"]["worktree"].is_null());
    assert!(receipt["record"]["intent"]["destination"].is_null());
    let header = serde_json::from_value(receipt["journal"].clone()).unwrap();
    let id = source::Sha256Digest::of(b"MCP documentation preparation");
    let reopened =
        store::inspect_preparation(&initialized.database, id, Some(&header), read()).unwrap();
    let record = reopened.record.unwrap();
    assert_eq!(
        record.intent.output,
        source::inspect(&record.intent.address.path(), None, Default::default())
            .unwrap()
            .identity()
    );
    assert_eq!(prepared["jobs"].as_array().unwrap().len(), 1);
    let evidence: Value = serde_json::from_str(&record.intent.evidence).unwrap();
    assert_eq!(evidence["artifact_class"], "documentation-build-input");
    assert!(!fixture.target.path().join("brand-destination").exists());

    // Fresh generator stage paths change invocation evidence; the same ID cannot adopt that intent.
    arguments["preparation"]["expected"] = json!(header);
    let retried = report(call(arguments, MAXIMUM).unwrap(), true);
    assert_eq!(retried["failure"]["code"], "store_preparation_conflict");
    assert_eq!(retried["completed"].as_array().unwrap().len(), 1);
    assert_eq!(retried["preparation"]["id"], json!(id));
    assert!(retried["preparation"]["receipt"].is_null());
    assert_eq!(retried["preparation"]["writer_acquired"], true);
    let advanced = serde_json::from_value(retried["preparation"]["journal"].clone()).unwrap();
    let unchanged =
        store::inspect_preparation(&initialized.database, id, Some(&advanced), read()).unwrap();
    assert_eq!(unchanged.record.as_ref(), Some(&record));
    assert_eq!(fixture.stages().len(), 4);
}

#[test]
fn preparation_conflicts_permissions_and_response_reservation_precede_all_effects() {
    let fixture = Fixture::new(COPY, 1);
    let (root, initialized) = journal();
    let selected = selection(root.path(), initialized.header, fixture.target.path());
    let mut valid = fixture.arguments(1, false);
    valid["preparation"] = selected;
    let mut conflict = valid.clone();
    conflict["output_parent"] = json!(fixture.target.path());
    assert!(call(conflict, MAXIMUM).is_err());
    for pointer in [
        "/preparation/create_stage",
        "/preparation/record_preparation",
        "/preparation/writer/allow_generation_advance",
        "/preparation/writer/session/allow_local_reservation",
    ] {
        let mut denied = valid.clone();
        *denied.pointer_mut(pointer).unwrap() = json!(false);
        let refused = call(denied, MAXIMUM).unwrap();
        assert_eq!(refused.is_error, Some(true));
        assert_eq!(
            json!(refused)["structuredContent"]["code"],
            "documentation_preparation_permission"
        );
    }
    assert!(call(valid, 1).is_err());
    assert!(fixture.stages().is_empty());
    assert!(!fixture.target.path().join("retained-docs").exists());
    assert!(
        store::inspect_preparation(
            &initialized.database,
            source::Sha256Digest::of(b"MCP documentation preparation"),
            Some(&initialized.header),
            read()
        )
        .unwrap()
        .record
        .is_none()
    );
}

#[test]
fn oversized_full_report_keeps_exact_native_preparation_and_completed_jobs() {
    let mut fixture = Fixture::new(COPY, 1);
    let version = vec![b'0'; 200_000];
    replace_first_generated_version(&mut fixture, "printf '%0200000d' 0", &version);
    let (root, initialized) = journal();
    let mut arguments = fixture.arguments(1, false);
    arguments["preparation"] = selection(root.path(), initialized.header, fixture.target.path());
    arguments["preparation"]["evidence_bytes"] = json!(8 << 20);
    let value = report(call(arguments, MAXIMUM).unwrap(), true);
    assert_eq!(value["code"], "documentation_response_unavailable");
    assert!(
        value["failure"]["code"].is_null(),
        "native operation succeeded; only full delivery overflowed"
    );
    assert_eq!(value["completed"].as_array().unwrap().len(), 1);
    let pin = &value["preparation"]["receipt"];
    let header = serde_json::from_value(pin["journal"].clone()).unwrap();
    let id = source::Sha256Digest::of(b"MCP documentation preparation");
    let record = store::inspect_preparation(&initialized.database, id, Some(&header), read())
        .unwrap()
        .record
        .unwrap();
    assert_eq!(pin["output"], json!(record.intent.output));
    assert_eq!(pin["evidence_sha256"], json!(record.intent.evidence_sha256));
    assert_eq!(pin["retention"], json!(record.retention));
    assert!(pin["check"].is_null() && pin["worktree"].is_null());
    assert_eq!(
        source::inspect(&record.intent.address.path(), None, Default::default())
            .unwrap()
            .identity(),
        record.intent.output
    );
    assert_eq!(fixture.stages().len(), 2);
}
