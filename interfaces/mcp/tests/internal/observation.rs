//! Real native managed-history results and strict transport/discovery boundaries.

use super::*;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

struct Fixture {
    journal: tempfile::TempDir,
    isolation: tempfile::TempDir,
    remote: tempfile::TempDir,
    initialized: store::InitializationReceipt,
    targets: Vec<release::Target>,
}

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

impl Fixture {
    fn new() -> Self {
        let private = || {
            tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()
                .expect("private disposable native directory")
        };
        let journal = private();
        let isolation = private();
        let remote = private();
        let initialized = store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session: store::SessionOptions {
                    allow_local_reservation: true,
                    read: read(),
                },
                ..Default::default()
            },
        )
        .expect("real supported journal");
        let repository =
            git2::Repository::init_bare(remote.path()).expect("local bare destination");
        let tree_id = repository
            .treebuilder(None)
            .expect("empty tree builder")
            .write()
            .expect("native tree");
        let tree = repository.find_tree(tree_id).expect("retained Git tree");
        let signature =
            git2::Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(1, 0))
                .expect("explicit fixture identity");
        let oid = repository
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "fixture",
                &tree,
                &[],
            )
            .expect("fixture commit, not a release push");
        let authorization: Value = serde_json::from_str(include_str!(
            "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
        ))
        .expect("native target fixture shape");
        let mut target: release::Target =
            serde_json::from_value(authorization["ordered_targets"][0].clone())
                .expect("exact typed target");
        target.ordinal = 0;
        target.remote = remote
            .path()
            .to_str()
            .expect("UTF-8 fixture path")
            .to_owned();
        target.branch = "main".into();
        target.local_commit_oid = oid.to_string().parse().expect("full native OID");
        target.refspec = format!("{}:refs/heads/main", target.local_commit_oid);
        drop(tree);
        drop(repository);
        Self {
            journal,
            isolation,
            remote,
            initialized,
            targets: vec![target],
        }
    }

    fn record(&self) -> Value {
        json!({"root":self.journal.path(),"expected_header":self.initialized.header,
            "id":Sha256Digest::of(b"new diagnostic observation"),
            "targets_json":serde_json::to_string(&self.targets).expect("exact target JSON"),
            "git":{"program":crate::native::git(),"isolation":self.isolation.path()},
            "protected_roots":[self.remote.path()],"allow_read":true,"record_observation":true,
            "advance_writer_generation":true,"reserve_local_coordination":true,"read":read()})
    }

    fn inspect(&self) -> Value {
        json!({"path":self.initialized.database,"id":Sha256Digest::of(b"new diagnostic observation"),
            "allow_read":true,"read":read()})
    }

    fn unchanged_header(&self) {
        assert_eq!(
            store::inspect_journal(
                &self.initialized.database,
                Some(&self.initialized.header),
                read()
            )
            .expect("no generation advance")
            .header,
            self.initialized.header
        );
    }
}

fn execute(route: Route, value: Value, maximum: usize) -> CallToolResult {
    route
        .decode(value.as_object().expect("request object").clone())
        .expect("valid transport")
        .execute(maximum)
        .expect("complete tool result")
}

fn payload(result: &CallToolResult) -> &Value {
    let payload = result
        .structured_content
        .as_ref()
        .expect("structured native result");
    let wire = json!(result);
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().expect("readable JSON"))
            .expect("same JSON projection"),
        *payload
    );
    payload
}

#[test]
fn native_record_and_independent_inspection_share_exact_history_and_schema() {
    let fixture = Fixture::new();
    let original_ref =
        fs::read(fixture.remote.path().join("refs/heads/main")).expect("native destination ref");
    let recorded = execute(Route::Record, fixture.record(), 1 << 20);
    assert_eq!(recorded.is_error, Some(false));
    let recorded = payload(&recorded);
    assert_eq!(recorded["status"], "observation-recorded-not-settlement");
    assert_eq!(
        recorded["receipt"]["record"]["readbacks"][0]["readback"]["comparison"],
        "intended"
    );
    assert_eq!(
        recorded["receipt"]["record"]["targets"],
        json!(fixture.targets)
    );
    let inspected = execute(Route::Inspect, fixture.inspect(), 1 << 20);
    assert_eq!(inspected.is_error, Some(false));
    let native = release::inspect_target_observation(
        &fixture.initialized.database,
        Sha256Digest::of(b"new diagnostic observation"),
        None,
        read(),
        release::Limits::default(),
    )
    .expect("independent SDK history");
    assert_eq!(payload(&inspected), &json!(native));
    assert_eq!(payload(&inspected)["record"], recorded["receipt"]["record"]);
    for (route, value) in [
        (Route::Record, recorded),
        (Route::Inspect, payload(&inspected)),
    ] {
        let descriptor = json!(route.descriptor());
        assert!(
            jsonschema::validator_for(&descriptor["outputSchema"])
                .expect("native output schema")
                .is_valid(value)
        );
    }
    let mut wrong = fixture.inspect();
    wrong["expected_header"] = json!(fixture.initialized.header);
    assert_eq!(
        payload(&execute(Route::Inspect, wrong, 1 << 20))["code"],
        "store_journal_mismatch"
    );
    assert_eq!(
        fs::read(fixture.remote.path().join("refs/heads/main")).expect("unchanged destination"),
        original_ref
    );
    assert!(
        fs::read_dir(fixture.isolation.path())
            .expect("isolation remains empty")
            .next()
            .is_none()
    );
}

#[test]
fn independent_permissions_and_impossible_delivery_budget_precede_acquisition() {
    let fixture = Fixture::new();
    for field in [
        "allow_read",
        "record_observation",
        "advance_writer_generation",
        "reserve_local_coordination",
    ] {
        let mut denied = fixture.record();
        denied[field] = false.into();
        denied["git"]["program"] = "/nonexistent/native-provider".into();
        let response = execute(Route::Record, denied, 1 << 20);
        assert_eq!(response.is_error, Some(true), "{field}");
        assert_eq!(payload(&response)["status"], "not-started", "{field}");
        fixture.unchanged_header();
    }
    let mut denied = fixture.record();
    denied["read"]["allow_coordination_files"] = false.into();
    assert_eq!(
        payload(&execute(Route::Record, denied, 1 << 20))["status"],
        "not-started"
    );
    let limited = execute(Route::Record, fixture.record(), 180);
    assert_eq!(limited.is_error, Some(true));
    assert_eq!(payload(&limited)["code"], "observation_response_budget");
    fixture.unchanged_header();
    let mut denied = fixture.inspect();
    denied["allow_read"] = false.into();
    denied["path"] = "/nonexistent/history.sqlite".into();
    assert_eq!(
        payload(&execute(Route::Inspect, denied, 1 << 20))["code"],
        "storage_permission"
    );
}

#[test]
fn post_effect_overflow_retains_coordinates_and_history_remains_inspectable() {
    let fixture = Fixture::new();
    // This ceiling fits the precomputed coordinate fallback but not the complete native receipt.
    let fallback = Delivery::ReconciliationRequired {
        root: fixture.journal.path(),
        input_expected_header: fixture.initialized.header,
        id: Sha256Digest::of(b"new diagnostic observation"),
        coordinator: None,
        message: "Complete observation result unavailable; selected anchor writes, generation advancement, destination reads and record insertion may have occurred. Preserve the selected anchor/binding and inspect the original root and observation ID with independently retained journal coordinates. Input expected_header is not a current observation. No automatic retry or rollback inference.",
    };
    let maximum = serde_json::to_vec(&fallback)
        .expect("complete fallback bytes")
        .len();
    let response = execute(Route::Record, fixture.record(), maximum);
    assert_eq!(response.is_error, Some(true));
    assert_eq!(payload(&response), &json!(fallback));
    let inspected = execute(Route::Inspect, fixture.inspect(), 1 << 20);
    assert!(
        payload(&inspected)["record"].is_object(),
        "actual append survived lost full response"
    );
    assert!(
        serde_json::to_vec(payload(&inspected))
            .expect("complete native history")
            .len()
            > maximum
    );
    let mut repeated = fixture.record();
    repeated["expected_header"] = payload(&inspected)["journal"]["header"].clone();
    repeated["git"]["program"] = "/missing/provider-must-not-run-for-existing-id".into();
    let refused = execute(Route::Record, repeated, 1 << 20);
    assert_eq!(payload(&refused)["status"], "recording-failed");
    assert_eq!(payload(&refused)["failure"]["phase"], "before-read");
    assert_eq!(
        payload(&execute(Route::Inspect, fixture.inspect(), 1 << 20))["record"],
        payload(&inspected)["record"]
    );
}

#[test]
fn discovery_matches_strict_objects_native_target_bytes_and_effects() {
    let fixture = Fixture::new();
    for (route, valid) in [
        (Route::Record, fixture.record()),
        (Route::Inspect, fixture.inspect()),
    ] {
        let descriptor = json!(route.descriptor());
        let schema =
            jsonschema::validator_for(&descriptor["inputSchema"]).expect("native input schema");
        assert!(schema.is_valid(&valid));
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(
            descriptor["annotations"]["idempotentHint"],
            matches!(route, Route::Inspect)
        );
        for (field, value) in [
            ("read", json!([])),
            ("read", Value::Null),
            ("limits", json!([])),
            ("expected_header", json!([1, [], 0, 0])),
            ("unknown", json!(true)),
            ("read", json!({"busy_timeout_ms":18446744073709551616_f64})),
        ] {
            let mut wrong = valid.clone();
            wrong[field] = value;
            assert!(!schema.is_valid(&wrong), "{field}");
            assert!(
                route
                    .decode(wrong.as_object().expect("object").clone())
                    .is_err(),
                "{field}"
            );
        }
    }
    let descriptor = json!(Route::Record.descriptor());
    let target_schema = jsonschema::validator_for(
        &descriptor["inputSchema"]["properties"]["targets_json"]["contentSchema"],
    )
    .expect("actual native target content schema");
    assert!(target_schema.is_valid(&json!(fixture.targets)));
    let mut wrong = json!(fixture.targets);
    wrong[0]["ordinal"] = json!(4294967296_u64);
    assert!(!target_schema.is_valid(&wrong));
    let mut duplicate = fixture.record();
    duplicate["targets_json"] = duplicate["targets_json"]
        .as_str()
        .expect("exact target bytes")
        .replacen("\"ordinal\":0", "\"ordinal\":0,\"ordinal\":0", 1)
        .into();
    assert_eq!(
        payload(&execute(Route::Record, duplicate, 1 << 20))["code"],
        "release_json"
    );
    for (field, value) in [
        ("root", json!("relative")),
        ("protected_roots", json!(["relative"])),
        ("expected_header", Value::Null),
        ("allow_network", Value::Null),
    ] {
        let mut wrong = fixture.record();
        wrong[field] = value;
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("record schema");
        assert!(!schema.is_valid(&wrong), "{field}");
        assert!(
            Route::Record
                .decode(wrong.as_object().expect("object").clone())
                .is_err()
        );
    }
    fixture.unchanged_header();
}

#[test]
fn explicit_coordinator_uses_native_guard_and_preserves_its_clean_history() {
    let mut fixture = Fixture::new();
    // Two configured destinations exercise complete-set history and make the
    // native receipt larger than the independently retained custody fallback.
    let mut documentation = fixture.targets[0].clone();
    documentation.ordinal = 1;
    documentation.domain = "docs.fixture.invalid".into();
    documentation.branch = "documentation".into();
    documentation.refspec = format!(
        "{}:refs/heads/documentation",
        documentation.local_commit_oid
    );
    fixture.targets.push(documentation);
    let anchor = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private native anchor");
    let session = store::SessionOptions {
        allow_local_reservation: true,
        read: read(),
    };
    let seal = store::coordinator::enroll(
        store::coordinator::EnrollmentRequest {
            anchor_root: anchor.path(),
            journal_root: fixture.journal.path(),
            coordinator_id: "mcp fixture coordinator",
            principal_scope: Sha256Digest::of(b"local fixture principal"),
            protected_roots: &[
                fixture.remote.path().to_owned(),
                fixture.isolation.path().to_owned(),
            ],
        },
        session,
        true,
    )
    .expect("native fresh enrollment");
    let selected = store::coordinator::LocalCoordinatorSelection {
        anchor_root: anchor.path().to_owned(),
        binding: seal.binding.clone(),
    };
    let before = fs::read(&fixture.initialized.database).expect("original native journal");
    let mut request = fixture.record();
    request["root"] = json!(seal.binding.journal_root);
    request["coordinator"] = json!(selected);
    let denied = execute(Route::Record, request.clone(), 1 << 20);
    assert_eq!(payload(&denied)["status"], "not-started");
    assert_eq!(
        payload(&denied)["failure"]["code"],
        "store_coordinator_permission"
    );
    request["write_coordinator_anchor"] = true.into();
    let tiny = execute(Route::Record, request.clone(), 180);
    assert_eq!(payload(&tiny)["code"], "observation_response_budget");
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("no refused writes"),
        before
    );
    assert_eq!(
        store::coordinator::inspect(anchor.path(), &seal.binding, session)
            .expect("unchanged anchor"),
        store::coordinator::LocalCoordinatorState::Clean {
            seal: seal.clone(),
            adoption: None,
        }
    );
    let recorded = execute(Route::Record, request.clone(), 1 << 20);
    assert_eq!(recorded.is_error, Some(false));
    assert_eq!(
        payload(&recorded)["receipt"]["record"]["readbacks"]
            .as_array()
            .expect("complete actual destination set")
            .len(),
        2
    );
    let clean = store::coordinator::inspect(anchor.path(), &seal.binding, session)
        .expect("native clean close evidence");
    let store::coordinator::LocalCoordinatorState::Clean {
        seal: ref closed, ..
    } = clean
    else {
        panic!("MCP success must include a successful native close")
    };
    assert_eq!(
        json!(closed.header),
        payload(&recorded)["receipt"]["journal"]
    );
    assert_eq!(
        closed.database_sha256,
        Sha256Digest::of(&fs::read(&fixture.initialized.database).expect("complete journal bytes"))
    );
    let history = execute(Route::Inspect, fixture.inspect(), 1 << 20);
    assert_eq!(
        payload(&history)["record"],
        payload(&recorded)["receipt"]["record"]
    );
    // A second same-shaped result is one byte over this ceiling. The complete
    // smaller custody fallback must survive after real append and clean close.
    let maximum = serde_json::to_vec(payload(&recorded))
        .expect("complete successful result size")
        .len()
        - 1;
    let mut next = request.clone();
    let overflow_id = Sha256Digest::of(b"coordinated delivery overflow");
    next["expected_header"] = json!(closed.header);
    next["id"] = json!(overflow_id);
    let overflow = execute(Route::Record, next, maximum);
    assert_eq!(overflow.is_error, Some(true));
    assert_eq!(payload(&overflow)["status"], "reconciliation-required");
    assert_eq!(payload(&overflow)["coordinator"], json!(selected));
    assert_eq!(
        payload(&overflow)["input_expected_header"],
        json!(closed.header)
    );
    let mut lookup = fixture.inspect();
    lookup["id"] = json!(overflow_id);
    let persisted = execute(Route::Inspect, lookup, 1 << 20);
    assert!(
        payload(&persisted)["record"].is_object(),
        "overflow cannot erase native effects"
    );
    let clean = store::coordinator::inspect(anchor.path(), &seal.binding, session)
        .expect("post-overflow native close remains inspectable");
    assert!(matches!(
        clean,
        store::coordinator::LocalCoordinatorState::Clean { .. }
    ));
    fs::write(&fixture.initialized.database, &before).expect("rollback disposable journal only");
    request["id"] = json!(Sha256Digest::of(b"cannot reacquire rolled-back history"));
    request["git"]["program"] = "/nonexistent/never-fall-back".into();
    let refused = execute(Route::Record, request, 1 << 20);
    assert_eq!(payload(&refused)["status"], "acquisition-failed");
    assert_eq!(
        payload(&refused)["generation"],
        json!(store::GenerationEffect::NotAttempted)
    );
    assert_eq!(payload(&refused)["code"], "store_coordinator_mismatch");
    assert_eq!(
        store::coordinator::inspect(anchor.path(), &seal.binding, session)
            .expect("control record retained"),
        clean
    );
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("refusal preserved rollback evidence"),
        before
    );
}

#[test]
fn coordinator_discovery_and_decode_reject_nonobjects_and_unbound_paths() {
    let fixture = Fixture::new();
    let descriptor = json!(Route::Record.descriptor());
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("discovery schema");
    let selected = store::coordinator::LocalCoordinatorSelection {
        anchor_root: "/private/coordinator".into(),
        binding: store::coordinator::LocalCoordinatorBinding {
            coordinator_id: "exact owner id".into(),
            principal_scope: Sha256Digest::of(b"scope"),
            journal_root: fixture.journal.path().to_owned(),
            journal_id: fixture.initialized.header.journal_id,
            journal_epoch: fixture.initialized.header.journal_epoch,
        },
    };
    let mut valid = fixture.record();
    valid["coordinator"] = json!(selected);
    valid["write_coordinator_anchor"] = true.into();
    assert!(schema.is_valid(&valid));
    for pointer in ["/coordinator", "/coordinator/binding"] {
        let mut wrong = valid.clone();
        *wrong.pointer_mut(pointer).expect("selected object") = json!([]);
        assert!(!schema.is_valid(&wrong));
        assert!(
            Route::Record
                .decode(wrong.as_object().expect("outer object").clone())
                .is_err()
        );
    }
    for pointer in [
        "/coordinator/anchor_root",
        "/coordinator/binding/journal_root",
    ] {
        let mut wrong = valid.clone();
        *wrong.pointer_mut(pointer).expect("selected path") = "relative".into();
        assert!(!schema.is_valid(&wrong));
        assert!(
            Route::Record
                .decode(wrong.as_object().expect("outer object").clone())
                .is_err()
        );
    }
    let mut wrong = valid;
    wrong["coordinator"]["binding"]["invented"] = true.into();
    assert!(!schema.is_valid(&wrong));
    assert!(
        Route::Record
            .decode(wrong.as_object().expect("outer object").clone())
            .is_err()
    );
    assert!(
        descriptor["description"]
            .as_str()
            .expect("discoverable meaning")
            .contains("write_coordinator_anchor")
    );
    fixture.unchanged_header();
}
