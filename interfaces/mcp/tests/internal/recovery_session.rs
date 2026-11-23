//! Real held-journal MCP lifetimes; no simulated writer or publisher authority.

use super::*;
use gazette_cli::sdk::{
    source::Sha256Digest,
    store::{self, coordinator},
};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private native fixture")
}

fn writer_options() -> store::WriterOptions {
    store::WriterOptions {
        allow_generation_advance: true,
        session: store::SessionOptions {
            allow_local_reservation: true,
            read: store::ReadOptions {
                allow_coordination_files: true,
                ..Default::default()
            },
        },
    }
}

struct Fixture {
    _anchor: tempfile::TempDir,
    journal: tempfile::TempDir,
    request: session::OpenRequest,
}

impl Fixture {
    fn new() -> Self {
        let anchor = private();
        let journal = private();
        let initialized = store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session: writer_options().session,
                ..Default::default()
            },
        )
        .expect("real native initialization");
        let seal = coordinator::enroll(
            coordinator::EnrollmentRequest {
                anchor_root: anchor.path(),
                journal_root: journal.path(),
                coordinator_id: "mcp-recovery-fixture",
                principal_scope: Sha256Digest::of(b"fixture principal"),
                protected_roots: &[],
            },
            writer_options().session,
            true,
        )
        .expect("real independent anchor");
        let mut writer = coordinator::acquire_writer(
            anchor.path(),
            &seal.binding,
            initialized.header,
            writer_options(),
            true,
        )
        .expect("native anchored generation");
        let expected = writer.inspect().expect("actual held generation").header;
        drop(writer); // Native interruption, not a fabricated anchor.
        let request = session::OpenRequest {
            coordinator: coordinator::LocalCoordinatorSelection {
                anchor_root: anchor.path().into(),
                binding: seal.binding,
            },
            expected,
            profile: Default::default(),
            protected_roots: vec![],
            session: writer_options().session,
            allow_anchor_write: true,
            reviewed: None,
            git: None,
        };
        Self {
            _anchor: anchor,
            journal,
            request,
        }
    }

    fn state(&self) -> coordinator::LocalCoordinatorState {
        coordinator::inspect(
            &self.request.coordinator.anchor_root,
            &self.request.coordinator.binding,
            writer_options().session,
        )
        .expect("independent native custody inspection")
    }
}

fn decode(route: Route, value: Value) -> Input {
    route
        .decode(value.as_object().expect("object").clone())
        .expect("SDK-shaped input")
}

fn execute(sessions: &Sessions, route: Route, value: Value) -> Value {
    let result = sessions
        .execute(decode(route, value), 1 << 20)
        .expect("complete transport result");
    let value = result.structured_content.expect("structured result");
    let descriptor = json!(route.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("schema")
        .validate(&value)
        .expect("native result matches discovery");
    value
}

fn open(sessions: &Sessions, fixture: &Fixture) -> Value {
    let result = execute(sessions, Route::Open, json!({"request":fixture.request}));
    assert_eq!(result["status"], "ready", "{result}");
    result
}

#[test]
fn seal_discovery_and_dispatch_share_sdk_and_retire_handle() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let ready = open(&sessions, &fixture);
    let request =
        json!({"session":ready["session"],"operation":{"operation":"seal","allow_seal":true}});
    let descriptor = json!(Route::Operate.descriptor());
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&request)
        .expect("seal discovered");
    let result = execute(&sessions, Route::Operate, request.clone());
    assert_eq!(result["outcome"]["result"], "sealed", "{result}");
    let seal: coordinator::LocalCoordinatorSeal =
        serde_json::from_value(result["outcome"]["seal"].clone()).expect("native seal");
    assert_eq!(seal.header, fixture.request.expected);
    assert!(matches!(
        fixture.state(),
        coordinator::LocalCoordinatorState::Clean { .. }
    ));
    let mut writer = coordinator::acquire_writer(
        &fixture.request.coordinator.anchor_root,
        &seal.binding,
        seal.header,
        writer_options(),
        true,
    )
    .expect("separate acquisition");
    assert_eq!(
        writer.inspect().expect("fence").header.writer_fence,
        seal.header.writer_fence + 1
    );
    writer.close().expect("close");
    let repeated = execute(&sessions, Route::Operate, request);
    assert_ne!(
        repeated.get("outcome"),
        Some(&result["outcome"]),
        "closed handle never repeats seal"
    );
}

#[test]
fn native_ready_inspect_and_explicit_close_preserve_one_generation() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let before = fixture.state();
    let ready = open(&sessions, &fixture);
    let handle = ready["session"].clone();
    assert_eq!(
        ready["inspection"]["header"]["writer_fence"],
        fixture.request.expected.writer_fence
    );
    for _ in 0..2 {
        let result = execute(
            &sessions,
            Route::Operate,
            json!({"session":handle,"operation":{"operation":"inspect"}}),
        );
        assert_eq!(result["outcome"]["inspection"], ready["inspection"]);
    }
    let result = execute(&sessions, Route::Close, json!({"session":handle}));
    assert_eq!(result["outcome"]["result"], "closed");
    assert_eq!(
        fixture.state(),
        before,
        "close must not seal interrupted custody"
    );
    assert_eq!(
        execute(&sessions, Route::Close, json!({"session":handle}))["code"],
        "recovery_session_unknown"
    );
}

#[test]
fn unknown_and_capacity_start_no_extra_native_acquisition() {
    let sessions = Sessions::default();
    assert_eq!(
        execute(&sessions, Route::Close, json!({"session":"unknown"}))["code"],
        "recovery_session_unknown"
    );
    let fixtures: Vec<_> = (0..5).map(|_| Fixture::new()).collect();
    let handles: Vec<_> = fixtures[..4]
        .iter()
        .map(|fixture| open(&sessions, fixture)["session"].clone())
        .collect();
    let before = fixtures[4].state();
    assert_eq!(
        execute(
            &sessions,
            Route::Open,
            json!({"request":fixtures[4].request})
        )["code"],
        "recovery_session_capacity"
    );
    assert_eq!(fixtures[4].state(), before);
    for handle in handles {
        execute(&sessions, Route::Close, json!({"session":handle}));
    }
}

#[test]
fn denied_and_undeliverable_open_leave_exact_initial_custody() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let before = fixture.state();
    let mut denied = json!({"request":fixture.request});
    denied["request"]["session"]["allow_local_reservation"] = json!(false);
    let result = execute(&sessions, Route::Open, denied);
    assert_eq!(result["status"], "open_failed");
    assert_eq!(result["error"]["phase"], "before");
    assert_eq!(
        execute(
            &sessions,
            Route::Close,
            json!({"session":result["session"]})
        )["code"],
        "recovery_session_unknown"
    );
    assert!(
        sessions
            .execute(decode(Route::Open, json!({"request":fixture.request})), 1)
            .is_err()
    );
    assert_eq!(fixture.state(), before);
}

#[test]
fn undersized_close_does_not_drop_handle_and_failed_explicit_close_does() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let ready = open(&sessions, &fixture);
    let handle = ready["session"].clone();
    assert!(
        sessions
            .execute(decode(Route::Close, json!({"session":handle})), 1)
            .is_err()
    );
    assert_eq!(
        execute(
            &sessions,
            Route::Operate,
            json!({"session":handle,"operation":{"operation":"inspect"}})
        )["outcome"]["result"],
        "inspected"
    );
    fs::write(
        fixture.journal.path().join("unexpected"),
        b"native namespace interference",
    )
    .expect("disposable corruption");
    let result = execute(
        &sessions,
        Route::Operate,
        json!({"session":handle,"operation":{"operation":"close"}}),
    );
    assert_eq!(result["outcome"]["result"], "failed");
    assert_eq!(
        execute(&sessions, Route::Close, json!({"session":handle}))["code"],
        "recovery_session_unknown"
    );
    assert!(matches!(
        fixture.state(),
        coordinator::LocalCoordinatorState::InProgress { .. }
    ));
}

#[test]
fn service_drop_leaves_interrupted_custody_not_automatic_close() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    open(&sessions, &fixture);
    drop(sessions);
    assert!(matches!(
        fixture.state(),
        coordinator::LocalCoordinatorState::InProgress { .. }
    ));
}

#[test]
fn shared_decoders_reject_positional_shapes_and_unavailable_effects() {
    let fixture = Fixture::new();
    for (route, valid, pointer, invalid) in [
        (
            Route::Open,
            json!({"request":fixture.request}),
            "/request",
            json!([]),
        ),
        (
            Route::Open,
            json!({"request":fixture.request}),
            "/request/expected",
            json!([1, vec![0; 32], 0, 0]),
        ),
        (
            Route::Open,
            json!({"request":fixture.request}),
            "/request/session",
            json!([]),
        ),
        (
            Route::Open,
            json!({"request":fixture.request}),
            "/request/coordinator",
            json!([]),
        ),
        (
            Route::Operate,
            json!({"session":"opaque","operation":{"operation":"inspect"}}),
            "/operation",
            json!([]),
        ),
    ] {
        let schema = json!(route.descriptor());
        let validator = jsonschema::validator_for(&schema["inputSchema"]).expect("input schema");
        assert!(validator.is_valid(&valid));
        let mut bad = valid;
        *bad.pointer_mut(pointer).expect("selected field") = invalid;
        assert!(!validator.is_valid(&bad));
        assert!(
            route
                .decode(bad.as_object().expect("object").clone())
                .is_err()
        );
    }
    for route in Route::ALL {
        let mut bad = match route {
            Route::Open => json!({"request":fixture.request}),
            Route::Operate => json!({"session":"opaque","operation":{"operation":"inspect"}}),
            Route::Close => json!({"session":"opaque"}),
        };
        bad["extra"] = json!(true);
        assert!(
            route
                .decode(bad.as_object().expect("object").clone())
                .is_err()
        );
    }
    for operation in ["register", "consume", "dispatch"] {
        let value = json!({"session":"opaque","operation":{"operation":operation}});
        let schema = json!(Route::Operate.descriptor());
        assert!(
            !jsonschema::validator_for(&schema["inputSchema"])
                .expect("schema")
                .is_valid(&value)
        );
        assert!(
            Route::Operate
                .decode(value.as_object().expect("object").clone())
                .is_err()
        );
    }
    for field in ["authority", "writer"] {
        let mut value = json!({"request":fixture.request});
        value["request"][field] = json!({});
        assert!(
            Route::Open
                .decode(value.as_object().expect("object").clone())
                .is_err()
        );
    }
}

#[test]
fn read_only_recovery_and_optional_observer_do_not_restore_writer_authority() {
    let mut fixture = Fixture::new();
    fixture.request.allow_anchor_write = false;
    let before = fixture.state();
    let sessions = Sessions::default();
    let ready = open(&sessions, &fixture);
    let handle = ready["session"].clone();
    let result = execute(
        &sessions,
        Route::Operate,
        json!({
            "session":handle,
            "operation":{"operation":"reconcile","expected":fixture.request.expected,
                "authorization_id":"absent","number":1}
        }),
    );
    assert_eq!(result["outcome"]["result"], "failed");
    assert!(
        serde_json::to_string(&result)
            .expect("result")
            .contains("release_session_git_unconfigured")
    );
    assert_eq!(
        execute(
            &sessions,
            Route::Operate,
            json!({"session":handle,"operation":{"operation":"inspect"}})
        )["outcome"]["result"],
        "inspected"
    );
    // An identical opaque handle in another registry conveys no capability.
    let ordinary = crate::release_session::Sessions::default();
    let foreign = crate::release_session::Route::Close
        .decode(
            json!({"session":handle})
                .as_object()
                .expect("object")
                .clone(),
        )
        .expect("ordinary close shape");
    let refused = ordinary.execute(foreign, 1 << 20).expect("bounded refusal");
    assert_eq!(
        refused.structured_content.expect("refusal")["code"],
        "release_session_unknown"
    );
    execute(&sessions, Route::Close, json!({"session":handle}));
    assert_eq!(fixture.state(), before);
}
