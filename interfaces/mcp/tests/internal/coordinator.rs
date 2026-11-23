//! Native coordinator enrollment, exact discovery and bounded delivery evidence.

use super::*;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn protected(root: &Path) -> store::LifecycleProtectedRoots {
    let roots = vec![root.to_path_buf()];
    store::LifecycleProtectedRoots {
        authored: roots.clone(),
        staged: roots.clone(),
        published: roots.clone(),
        live_journals: roots,
    }
}

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable fixture")
}

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn session() -> store::SessionOptions {
    store::SessionOptions {
        allow_local_reservation: true,
        read: read(),
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
        .expect("real fresh SQLite journal");
        Self {
            anchor,
            journal,
            initialized,
        }
    }

    fn enrollment(&self) -> Value {
        json!({ "anchor_root":self.anchor.path(), "journal_root":self.journal.path(),
            "coordinator_id":"coordinator local-Éxact", "principal_scope":Sha256Digest::of(b"selected scope"),
            "protected_roots":[], "allow_read":true,"write_coordinator_anchor":true,
            "reserve_local_coordination":true,"read":read() })
    }

    fn untouched(&self, before: &[u8]) {
        assert_eq!(
            fs::read(&self.initialized.database).expect("journal bytes"),
            before
        );
        assert_eq!(
            fs::read_dir(self.anchor.path())
                .expect("anchor inventory")
                .count(),
            0
        );
    }
}

fn execute(route: Route, input: Value, maximum: usize) -> CallToolResult {
    route
        .decode(input.as_object().expect("named input").clone())
        .expect("valid input")
        .execute(maximum)
        .expect("bounded tool response")
}

fn payload(report: &CallToolResult) -> &Value {
    let value = report
        .structured_content
        .as_ref()
        .expect("structured native output");
    let wire = json!(report);
    let text: Value = serde_json::from_str(wire["content"][0]["text"].as_str().expect("JSON text"))
        .expect("same complete text projection");
    assert_eq!(*value, text);
    value
}

fn schema(route: Route, input: &Value, output: &Value) {
    let descriptor = json!(route.descriptor());
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("native input schema")
        .validate(input)
        .expect("input agrees with discovery");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native output schema")
        .validate(output)
        .expect("output agrees with actual serialized owner");
}

#[test]
fn adoption_is_discoverable_and_executes_the_same_sdk_transition() {
    let fixture = Fixture::new();
    let enrolled = execute(Route::Enroll, fixture.enrollment(), 1 << 20);
    let seal: coordinator::LocalCoordinatorSeal =
        serde_json::from_value(payload(&enrolled).clone()).expect("native seal");
    let backups = private();
    let backup = store::backup(store::BackupRequest {
        source: &fixture.initialized.database,
        output_parent: backups.path(),
        protected: protected(fixture.journal.path()),
        options: store::BackupOptions {
            allow_create: true,
            read: read(),
            ..Default::default()
        },
    })
    .expect("backup");
    let restores = private();
    let restore = store::prepare_restore(store::PrepareRestoreRequest {
        backup: &backup.directory,
        manifest_sha256: backup.manifest_sha256,
        output_parent: restores.path(),
        protected: protected(fixture.journal.path()),
        options: store::RestoreOptions {
            allow_prepare: true,
            verification: store::BackupVerifyOptions {
                read: read(),
                ..Default::default()
            },
            ..Default::default()
        },
    })
    .expect("restore");
    let successor = private();
    let input = json!({
        "anchor_root": fixture.anchor.path(),
        "binding": seal.binding,
        "previous": seal,
        "restore_root": restore.directory,
        "receipt_sha256": restore.receipt_sha256,
        "successor_root": successor.path(),
        "protected_roots": [],
        "allow_read": true,
        "options": {
            "allow_adopt": true,
            "allow_anchor_write": true,
            "read": read(),
            "receipt_bytes": 16384
        }
    });
    let result = execute(Route::Adopt, input.clone(), 1 << 20);
    assert_eq!(result.is_error, Some(false));
    schema(Route::Adopt, &input, payload(&result));
    assert_eq!(
        payload(&result)["successor"]["header"]["writer_fence"],
        json!(fixture.initialized.header.writer_fence + 1)
    );
    assert_eq!(Route::Adopt.name(), "adopt_store_restore");
}

#[test]
fn native_enrollment_and_sdk_inspection_share_exact_binding_without_journal_changes() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.initialized.database).expect("initial journal bytes");
    let input = fixture.enrollment();
    let result = execute(Route::Enroll, input.clone(), 1 << 20);
    assert_eq!(result.is_error, Some(false));
    schema(Route::Enroll, &input, payload(&result));
    let seal: coordinator::LocalCoordinatorSeal =
        serde_json::from_value(payload(&result).clone()).expect("native seal");
    assert_eq!(seal.binding.coordinator_id, "coordinator local-Éxact");
    assert_eq!(seal.header, fixture.initialized.header);
    let inspected = json!({"anchor_root":fixture.anchor.path(), "binding":seal.binding,
        "allow_read":true,"reserve_local_coordination":true,"read":read()});
    let result = execute(Route::Inspect, inspected.clone(), 1 << 20);
    assert_eq!(result.is_error, Some(false));
    schema(Route::Inspect, &inspected, payload(&result));
    assert_eq!(
        *payload(&result),
        json!(
            coordinator::inspect(fixture.anchor.path(), &seal.binding, session())
                .expect("SDK parity")
        )
    );
    let original_anchor =
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("retained anchor");
    let refused = execute(Route::Enroll, input, 1 << 20);
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(payload(&refused)["code"], "store_coordinator_state");
    assert_eq!(
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("no adoption"),
        original_anchor
    );
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("no generation"),
        before
    );
    for field in ["allow_read", "reserve_local_coordination"] {
        let mut denied = inspected.clone();
        denied[field] = false.into();
        assert_eq!(
            execute(Route::Inspect, denied, 1 << 20).is_error,
            Some(true)
        );
    }
    let mut denied = inspected.clone();
    denied["read"]["allow_coordination_files"] = false.into();
    assert_eq!(
        execute(Route::Inspect, denied, 1 << 20).is_error,
        Some(true)
    );
    assert_eq!(
        fs::read(fixture.anchor.path().join("coordinator.json"))
            .expect("denied reads do not change anchor"),
        original_anchor
    );
    let writer = coordinator::acquire_writer(
        fixture.anchor.path(),
        &seal.binding,
        seal.header,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
        true,
    )
    .expect("real guarded writer for in-progress projection");
    drop(writer);
    let dirty = execute(Route::Inspect, inspected.clone(), 1 << 20);
    assert_eq!(
        dirty.is_error,
        Some(false),
        "in-progress is inspectable evidence, not repair"
    );
    assert_eq!(payload(&dirty)["state"], "in_progress");
    schema(Route::Inspect, &inspected, payload(&dirty));
    assert_eq!(
        *payload(&dirty),
        json!(
            coordinator::inspect(fixture.anchor.path(), &seal.binding, session())
                .expect("exact retained in-progress evidence")
        )
    );
    fs::write(fixture.anchor.path().join("coordinator.json"), b"broken")
        .expect("disposable corruption");
    assert_eq!(
        execute(Route::Inspect, inspected, 1 << 20).is_error,
        Some(true)
    );
    assert_eq!(
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("no repair"),
        b"broken"
    );
}

#[test]
fn permissions_and_response_reservation_precede_native_effects() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.initialized.database).expect("initial bytes");
    for field in [
        "allow_read",
        "write_coordinator_anchor",
        "reserve_local_coordination",
    ] {
        let mut input = fixture.enrollment();
        input[field] = false.into();
        let output = execute(Route::Enroll, input, 1 << 20);
        assert_eq!(output.is_error, Some(true), "{field}");
        fixture.untouched(&before);
    }
    let mut input = fixture.enrollment();
    input["read"]["allow_coordination_files"] = false.into();
    assert_eq!(execute(Route::Enroll, input, 1 << 20).is_error, Some(true));
    fixture.untouched(&before);
    let mut input = fixture.enrollment();
    input["read"]["value_bytes"] = 1.into();
    assert_eq!(execute(Route::Enroll, input, 1 << 20).is_error, Some(true));
    fixture.untouched(&before);
    let output = execute(Route::Enroll, fixture.enrollment(), 200);
    assert_eq!(output.is_error, Some(true));
    assert_eq!(payload(&output)["code"], "coordinator_response_budget");
    fixture.untouched(&before);
}

#[test]
fn post_effect_overflow_preserves_coordinates_and_independent_native_seal() {
    let fixture = Fixture::new();
    let input = fixture.enrollment();
    let Input::Enroll(decoded) = Route::Enroll
        .decode(input.as_object().expect("input").clone())
        .expect("decoded input")
    else {
        panic!("enrollment route");
    };
    let fallback = decoded
        .fallback(1 << 20)
        .unwrap_or_else(|_| panic!("fallback fits"));
    let maximum = serde_json::to_vec(payload(&fallback))
        .expect("fallback bytes")
        .len();
    let output = execute(Route::Enroll, input.clone(), maximum);
    assert_eq!(output.is_error, Some(true));
    assert_eq!(payload(&output)["code"], "coordinator_response_unavailable");
    assert_eq!(
        payload(&output)["input"]["coordinator_id"],
        input["coordinator_id"]
    );
    assert_eq!(
        payload(&output)["input"]["anchor_root"],
        input["anchor_root"]
    );
    assert_eq!(
        payload(&output)["input"]["journal_root"],
        input["journal_root"]
    );
    assert_eq!(
        payload(&output)["input"]["principal_scope"],
        input["principal_scope"]
    );
    schema(Route::Enroll, &input, payload(&output));
    let state: coordinator::LocalCoordinatorState = serde_json::from_slice(
        &fs::read(fixture.anchor.path().join("coordinator.json"))
            .expect("native persisted effect despite lost seal"),
    )
    .expect("native state");
    let coordinator::LocalCoordinatorState::Clean { seal, .. } = state else {
        panic!("enrollment state");
    };
    assert!(
        serde_json::to_vec(&seal).expect("complete seal").len() > maximum,
        "actual overflow, not preflight refusal"
    );
    assert_eq!(
        coordinator::inspect(fixture.anchor.path(), &seal.binding, session())
            .expect("independent exact inspection"),
        coordinator::LocalCoordinatorState::Clean {
            seal,
            adoption: None,
        }
    );
}

#[test]
fn discovery_rejects_unknown_tuple_null_and_relative_inputs() {
    let fixture = Fixture::new();
    let seal = coordinator::enroll(
        coordinator::EnrollmentRequest {
            anchor_root: fixture.anchor.path(),
            journal_root: fixture.journal.path(),
            coordinator_id: "coordinator local-Éxact",
            principal_scope: Sha256Digest::of(b"scope"),
            protected_roots: &[],
        },
        session(),
        true,
    )
    .expect("real binding fixture");
    let inspection = json!({"anchor_root":fixture.anchor.path(), "binding":seal.binding});
    for (route, valid) in [
        (Route::Enroll, fixture.enrollment()),
        (Route::Inspect, inspection),
    ] {
        let description = json!(route.descriptor());
        let schema =
            jsonschema::validator_for(&description["inputSchema"]).expect("discoverable schema");
        assert!(schema.is_valid(&valid));
        for (field, bad) in [
            ("unknown", json!(true)),
            ("read", json!([])),
            ("read", Value::Null),
            ("read", json!({"unknown":true})),
            ("anchor_root", json!("relative")),
        ] {
            let mut input = valid.clone();
            input[field] = bad;
            assert!(!schema.is_valid(&input), "{field}: {input}");
            assert!(
                route
                    .decode(input.as_object().expect("input object").clone())
                    .is_err()
            );
        }
        if matches!(route, Route::Inspect) {
            for binding in [
                json!([]),
                json!(["id", "digest", "/root", [], 1]),
                Value::Null,
            ] {
                let mut input = valid.clone();
                input["binding"] = binding;
                assert!(!schema.is_valid(&input));
                assert!(
                    route
                        .decode(input.as_object().expect("input object").clone())
                        .is_err()
                );
            }
        }
    }
}
