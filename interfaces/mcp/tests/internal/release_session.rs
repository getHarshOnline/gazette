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
                coordinator_id: "mcp-release-fixture",
                principal_scope: Sha256Digest::of(b"fixture principal"),
                protected_roots: &[],
            },
            writer_options().session,
            true,
        )
        .expect("real independent anchor");
        let request = session::OpenRequest {
            coordinator: coordinator::LocalCoordinatorSelection {
                anchor_root: anchor.path().into(),
                binding: seal.binding,
            },
            expected: initialized.header,
            protected_roots: vec![],
            writer: writer_options(),
            allow_anchor_write: true,
            git: None,
            reviewed: None,
            authority: None,
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
fn native_ready_inspect_and_explicit_close_preserve_one_generation() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let ready = open(&sessions, &fixture);
    let handle = ready["session"].clone();
    assert_eq!(
        ready["inspection"]["header"]["writer_fence"],
        fixture.request.expected.writer_fence + 1
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
    let coordinator::LocalCoordinatorState::Clean { seal, .. } = fixture.state() else {
        panic!("explicit close sealed custody")
    };
    assert_eq!(json!(seal.header), ready["inspection"]["header"]);
    assert_eq!(
        execute(&sessions, Route::Close, json!({"session":handle}))["code"],
        "release_session_unknown"
    );
}

#[test]
fn unknown_and_capacity_start_no_extra_native_acquisition() {
    let sessions = Sessions::default();
    assert_eq!(
        execute(&sessions, Route::Close, json!({"session":"unknown"}))["code"],
        "release_session_unknown"
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
        "release_session_capacity"
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
    denied["request"]["allow_anchor_write"] = json!(false);
    let result = execute(&sessions, Route::Open, denied);
    assert_eq!(result["status"], "open_failed");
    assert_eq!(result["error"]["phase"], "before");
    assert_eq!(
        execute(
            &sessions,
            Route::Close,
            json!({"session":result["session"]})
        )["code"],
        "release_session_unknown"
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
        "release_session_unknown"
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
fn shared_decoders_reject_nested_positional_shapes_and_preserve_authorization_bytes() {
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
            "/request/writer",
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
    let original = "{ \"duplicate\": 1, \"duplicate\": 2 }\n";
    let input = decode(
        Route::Operate,
        json!({"session":"opaque","operation":{"operation":"register","expected":fixture.request.expected,"input":{"authorization":original,"selections":[]}}}),
    );
    let Input::Operate { operation, .. } = input else {
        panic!("operation")
    };
    let session::Operation::Register { input, .. } = *operation else {
        panic!("registration")
    };
    assert_eq!(
        input.authorization, original,
        "strict release verifier must receive original duplicate evidence"
    );
}
