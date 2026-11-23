//! Shared recovery wire and lifetime proof over actual `SQLite` and retained anchor bytes.

use gazette_sdk::{release::recovery_session as recovery, source::Sha256Digest, store};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

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

#[test]
fn explicit_seal_requires_both_permissions_and_a_separate_new_writer() {
    for (anchor, seal_allowed) in [(false, true), (true, false), (true, true)] {
        let mut fixture = Fixture::new();
        fixture.request.allow_anchor_write = anchor;
        let before = fixture.state();
        let (mut held, _) =
            recovery::Session::open(fixture.request.clone()).expect("restricted lifetime");
        let result = held.execute(&recovery::Operation::Seal {
            allow_seal: seal_allowed,
        });
        assert!(
            held.is_closed(),
            "explicit seal consumes custody even on failure"
        );
        if !(anchor && seal_allowed) {
            assert!(result.is_error());
            assert_eq!(fixture.state(), before);
            continue;
        }
        let recovery::Outcome::Sealed { seal } = result else {
            panic!("native seal required")
        };
        assert_eq!(seal.header, fixture.request.expected);
        let mut writer = store::coordinator::acquire_writer(
            &fixture.request.coordinator.anchor_root,
            &seal.binding,
            seal.header,
            store::WriterOptions {
                allow_generation_advance: true,
                session: session(),
            },
            true,
        )
        .expect("separate ordinary acquisition");
        assert_eq!(
            writer.inspect().expect("new fence").header.writer_fence,
            seal.header.writer_fence + 1
        );
        writer.close().expect("ordinary close");
    }
    assert!(matches!(
        recovery::parse_operation(br#"{"operation":"seal"}"#).expect("denied default"),
        recovery::Operation::Seal { allow_seal: false }
    ));
}

struct Fixture {
    _journal: tempfile::TempDir,
    _anchor: tempfile::TempDir,
    request: recovery::OpenRequest,
}

impl Fixture {
    fn new() -> Self {
        let journal = private();
        let anchor = private();
        store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session: session(),
                ..Default::default()
            },
        )
        .expect("real database");
        let seal = store::coordinator::enroll(
            store::coordinator::EnrollmentRequest {
                anchor_root: anchor.path(),
                journal_root: journal.path(),
                coordinator_id: "recovery-session-fixture",
                principal_scope: Sha256Digest::of(b"fixture principal"),
                protected_roots: &[],
            },
            session(),
            true,
        )
        .expect("independently bound anchor");
        let mut writer = store::coordinator::acquire_writer(
            anchor.path(),
            &seal.binding,
            seal.header,
            store::WriterOptions {
                allow_generation_advance: true,
                session: session(),
            },
            true,
        )
        .expect("real anchored generation");
        let expected = writer.inspect().expect("actual ready header").header;
        drop(writer); // No synthetic dirty JSON and no close/seal.
        Self {
            request: recovery::OpenRequest {
                coordinator: store::coordinator::LocalCoordinatorSelection {
                    anchor_root: anchor.path().into(),
                    binding: seal.binding,
                },
                expected,
                profile: Default::default(),
                protected_roots: vec![],
                session: session(),
                allow_anchor_write: false,
                reviewed: None,
                git: None,
            },
            _journal: journal,
            _anchor: anchor,
        }
    }

    fn state(&self) -> store::coordinator::LocalCoordinatorState {
        store::coordinator::inspect(
            &self.request.coordinator.anchor_root,
            &self.request.coordinator.binding,
            session(),
        )
        .expect("independent custody read")
    }
}

#[test]
fn native_recovery_keeps_generation_and_dirty_state_across_inspection_drop_and_close() {
    let fixture = Fixture::new();
    let before = fixture.state();
    let (mut held, inspection) =
        recovery::Session::open(fixture.request.clone()).expect("exact interrupted custody");
    assert_eq!(inspection.header, fixture.request.expected);
    assert!(!held.is_closed());
    assert!(matches!(
        recovery::Session::open(fixture.request.clone()),
        Err(recovery::OpenError::Acquisition {
            code: "store_reservation_busy"
        })
    ));
    let recovery::Outcome::Inspected { inspection } = held.execute(&recovery::Operation::Inspect)
    else {
        panic!("native held inspection");
    };
    assert_eq!(inspection.header, fixture.request.expected);
    let unconfigured = held.execute(&recovery::Operation::Reconcile {
        expected: fixture.request.expected,
        authorization_id: "not-invented".into(),
        number: 1,
        options: Default::default(),
    });
    assert!(unconfigured.is_error());
    assert_eq!(
        json!(unconfigured)["failure"]["cause"]["code"],
        "release_session_git_unconfigured"
    );
    drop(held);
    assert_eq!(fixture.state(), before);
    let (mut again, _) = recovery::Session::open(fixture.request.clone())
        .expect("unchanged restricted reacquisition");
    assert!(matches!(
        again.execute(&recovery::Operation::Close),
        recovery::Outcome::Closed
    ));
    assert!(again.is_closed());
    assert!(again.execute(&recovery::Operation::Inspect).is_error());
    assert!(again.execute(&recovery::Operation::Close).is_error());
    assert_eq!(fixture.state(), before);
    assert!(
        store::coordinator::acquire_writer(
            &fixture.request.coordinator.anchor_root,
            &fixture.request.coordinator.binding,
            fixture.request.expected,
            store::WriterOptions {
                allow_generation_advance: true,
                session: session()
            },
            true
        )
        .is_err()
    );
}

#[test]
fn local_transaction_profile_also_admits_an_unstarted_journal_without_changing_custody() {
    let fixture = Fixture::new();
    let before = fixture.state();
    let mut request = fixture.request.clone();
    request.profile = store::coordinator::RecoveryProfile::LocalTransactions;
    let (mut held, inspection) =
        recovery::Session::open(request).expect("zero starts satisfy local-only history");
    assert_eq!(inspection.header, fixture.request.expected);
    assert!(matches!(
        held.execute(&recovery::Operation::Close),
        recovery::Outcome::Closed
    ));
    assert_eq!(fixture.state(), before);
}

#[test]
fn selection_permissions_overlap_and_changed_generation_refuse_without_custody_repair() {
    let fixture = Fixture::new();
    let before = fixture.state();
    let mut variants = vec![];
    let mut denied = fixture.request.clone();
    denied.session.allow_local_reservation = false;
    variants.push(denied);
    let mut stale = fixture.request.clone();
    stale.expected.writer_fence += 1;
    variants.push(stale);
    let mut overlap = fixture.request.clone();
    overlap
        .protected_roots
        .push(overlap.coordinator.binding.journal_root.clone());
    variants.push(overlap);
    let mut relative = fixture.request.clone();
    relative.protected_roots.push("relative".into());
    variants.push(relative);
    for request in variants {
        assert!(recovery::Session::open(request).is_err());
        assert_eq!(fixture.state(), before);
    }
    let (mut held, _) = recovery::Session::open(fixture.request.clone())
        .expect("refusals retained no hidden reservation");
    let mut wrong = fixture.request.expected;
    wrong.writer_fence += 1;
    assert_eq!(
        json!(held.execute(&recovery::Operation::Reconcile {
            expected: wrong,
            authorization_id: "not-invented".into(),
            number: 1,
            options: Default::default(),
        }))["failure"]["cause"]["code"],
        "release_session_generation"
    );
    assert!(matches!(
        held.execute(&recovery::Operation::Close),
        recovery::Outcome::Closed
    ));
}

#[test]
fn discovery_and_shared_parser_exclude_writer_authority_and_effect_operations() {
    let fixture = Fixture::new();
    let value = json!(fixture.request);
    let schema = schemars::schema_for!(recovery::OpenRequest);
    let validator = jsonschema::validator_for(schema.as_value()).expect("open schema");
    validator
        .validate(&value)
        .expect("native recovery selection");
    recovery::parse_open(&serde_json::to_vec(&value).expect("native request bytes"))
        .expect("strict native decoder");
    let mut omitted = value.clone();
    omitted
        .as_object_mut()
        .expect("request object")
        .remove("profile");
    let parsed = recovery::parse_open(&serde_json::to_vec(&omitted).expect("omitted profile"))
        .expect("default remains unstarted");
    assert_eq!(
        parsed.profile,
        store::coordinator::RecoveryProfile::Unstarted
    );
    for profile in ["unstarted", "local-transactions"] {
        let mut selected = omitted.clone();
        selected["profile"] = json!(profile);
        validator
            .validate(&selected)
            .expect("exact native enum schema");
        recovery::parse_open(&serde_json::to_vec(&selected).expect("explicit profile"))
            .expect("native profile decoder");
    }
    for profile in [
        json!("reaped"),
        json!({"quiescent":true}),
        json!(["local-transactions"]),
        json!(null),
    ] {
        let mut invalid = omitted.clone();
        invalid["profile"] = profile;
        assert!(validator.validate(&invalid).is_err());
        assert!(
            recovery::parse_open(&serde_json::to_vec(&invalid).expect("invalid profile")).is_err()
        );
    }
    for field in ["writer", "authority", "allow_generation_advance"] {
        let mut invalid = value.clone();
        invalid[field] = json!({});
        assert!(validator.validate(&invalid).is_err());
        assert!(
            recovery::parse_open(&serde_json::to_vec(&invalid).expect("invalid request bytes"))
                .is_err()
        );
    }
    let mut positional = value;
    positional["session"] = json!([true, {}]);
    assert!(
        recovery::parse_open(&serde_json::to_vec(&positional).expect("positional request bytes"))
            .is_err()
    );
    let schema = schemars::schema_for!(recovery::Operation);
    let validator = jsonschema::validator_for(schema.as_value()).expect("operation schema");
    for operation in ["register", "consume", "dispatch"] {
        let value = json!({"operation":operation});
        assert!(validator.validate(&value).is_err());
        assert!(
            recovery::parse_operation(
                &serde_json::to_vec(&value).expect("forbidden operation bytes")
            )
            .is_err()
        );
    }
    for bytes in [
        br#"{"operation":"close","extra":true}"#.as_slice(),
        br#"{"operation":"inspect","operation":"close"}"#,
        br#"["inspect"]"#,
    ] {
        assert!(recovery::parse_operation(bytes).is_err());
    }
    for operation in [
        recovery::Operation::Inspect,
        recovery::Operation::Close,
        recovery::Operation::Reconcile {
            expected: fixture.request.expected,
            authorization_id: "original".into(),
            number: 1,
            options: Default::default(),
        },
    ] {
        let value = json!(operation);
        validator.validate(&value).expect("narrow native operation");
        recovery::parse_operation(&serde_json::to_vec(&value).expect("native operation bytes"))
            .expect("strict operation decode");
    }
}
