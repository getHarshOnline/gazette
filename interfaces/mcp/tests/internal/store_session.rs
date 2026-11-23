//! Native ordinary lifetimes and bounded effect projection; no coordinator enrollment.

use super::*;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

const MAXIMUM: usize = 256 << 10;

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private native root")
}
fn options() -> store::WriterOptions {
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
    root: tempfile::TempDir,
    header: store::JournalHeader,
}
impl Fixture {
    fn new() -> Self {
        let root = private();
        let receipt = store::initialize(
            root.path(),
            store::InitializeOptions {
                allow_create: true,
                session: options().session,
                ..Default::default()
            },
        )
        .expect("real initialized journal");
        Self {
            root,
            header: receipt.header,
        }
    }
    fn writer(&self) -> Value {
        json!({"request":{"kind":"writer","root":self.root.path(),"expected_header":self.header,
            "allow_native_reads":true,"options":options()}})
    }
    fn connection(&self, coordination: &std::path::Path) -> Value {
        json!({"request":{"kind":"connection","path":self.root.path().join("store.sqlite"),
            "coordination_root":coordination,"allow_native_reads":true,"options":options().session}})
    }
    fn current(&self) -> store::JournalHeader {
        store::inspect_journal(
            &self.root.path().join("store.sqlite"),
            None,
            options().session.read,
        )
        .expect("independent read-back")
        .header
    }
}
fn input(route: Route, value: Value) -> Input {
    route
        .decode(value.as_object().expect("object").clone())
        .expect("valid input")
}
fn result(sessions: &Sessions, route: Route, value: Value) -> Value {
    let output = sessions
        .execute(input(route, value), MAXIMUM)
        .expect("bounded native result");
    let value = output.structured_content.expect("structured native result");
    let tool = serde_json::to_value(route.descriptor()).expect("descriptor");
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("native schema")
        .validate(&value)
        .expect("actual wire shape");
    value
}
fn handle(value: &Value) -> Value {
    json!({"session":value["session"]})
}

#[test]
fn ordinary_writer_advances_once_and_fresh_inspections_hold_the_same_generation() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let ready = result(&sessions, Route::Open, fixture.writer());
    assert_eq!(ready["status"], "acquired");
    assert_eq!(ready["active"], true);
    let actual: store::JournalHeader =
        serde_json::from_value(ready["inspection"]["header"].clone()).expect("native header");
    assert_eq!(actual.writer_fence, fixture.header.writer_fence + 1);
    for _ in 0..3 {
        let inspected = result(&sessions, Route::Inspect, handle(&ready));
        assert_eq!(inspected["status"], "inspected");
        assert_eq!(inspected["inspection"]["header"], json!(actual));
    }
    let competing = result(&sessions, Route::Open, fixture.writer());
    assert_eq!(competing["active"], false);
    assert_eq!(competing["failure"]["generation"], "NotAttempted");
    assert_eq!(competing["acquired"], false);
    assert_eq!(
        result(&sessions, Route::Close, handle(&ready))["status"],
        "closed"
    );
    assert_eq!(fixture.current(), actual);
    assert_eq!(
        result(&sessions, Route::Inspect, handle(&ready))["code"],
        "store_session_unknown"
    );
    let stale = result(&sessions, Route::Open, fixture.writer());
    assert_eq!(stale["failure"]["code"], "store_journal_mismatch");
    assert_eq!(fixture.current(), actual);
}

#[test]
fn connection_keeps_native_reservation_and_drop_never_resets_a_writer_generation() {
    let fixture = Fixture::new();
    let coordination = private();
    let sessions = Sessions::default();
    let ready = result(
        &sessions,
        Route::Open,
        fixture.connection(coordination.path()),
    );
    assert_eq!(ready["status"], "connected");
    assert_eq!(
        result(&sessions, Route::Inspect, handle(&ready))["observation"],
        ready["observation"]
    );
    assert!(
        store::connect(
            &fixture.root.path().join("store.sqlite"),
            coordination.path(),
            options().session
        )
        .is_err()
    );
    assert_eq!(
        result(&sessions, Route::Close, handle(&ready))["status"],
        "closed"
    );
    store::connect(
        &fixture.root.path().join("store.sqlite"),
        coordination.path(),
        options().session,
    )
    .expect("released namespace")
    .close()
    .expect("close");
    let writer = result(&sessions, Route::Open, fixture.writer());
    let committed = writer["inspection"]["header"].clone();
    drop(sessions);
    assert_eq!(json!(fixture.current()), committed);
    let next = store::acquire_writer(fixture.root.path(), fixture.current(), options())
        .expect("new explicitly selected generation");
    next.close().expect("close does not reset");
}

#[test]
fn capacity_and_response_reservation_refuse_before_generation_effects() {
    let fixtures: Vec<_> = (0..5).map(|_| Fixture::new()).collect();
    let sessions = Sessions::default();
    assert!(
        sessions
            .execute(input(Route::Open, fixtures[0].writer()), 1)
            .is_err()
    );
    assert_eq!(fixtures[0].current(), fixtures[0].header);
    for fixture in &fixtures[..4] {
        assert_eq!(
            result(&sessions, Route::Open, fixture.writer())["active"],
            true
        );
    }
    assert_eq!(
        result(&sessions, Route::Open, fixtures[4].writer())["code"],
        "store_session_capacity"
    );
    assert_eq!(fixtures[4].current(), fixtures[4].header);
    drop(sessions);
}

#[test]
fn native_path_failure_is_permanent_and_close_consumes_the_failed_handle() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let ready = result(&sessions, Route::Open, fixture.writer());
    let original = fixture.root.path().join("store.sqlite");
    let moved = fixture.root.path().join("moved.sqlite");
    fs::rename(&original, &moved).expect("native namespace disturbance");
    let failure = result(&sessions, Route::Inspect, handle(&ready));
    assert_eq!(failure["status"], "failed");
    assert_eq!(failure["active"], true);
    fs::rename(&moved, &original).expect("restore bytes, not authority");
    assert_eq!(
        result(&sessions, Route::Inspect, handle(&ready))["failure"]["code"],
        "store_changed"
    );
    let closed = result(&sessions, Route::Close, handle(&ready));
    assert_eq!(closed["active"], false);
    assert_eq!(closed["failure"]["phase"], "close");
    assert_eq!(
        result(&sessions, Route::Close, handle(&ready))["code"],
        "store_session_unknown"
    );
}

#[test]
fn closed_discovery_and_decoder_enforce_native_permissions_paths_and_nested_maps() {
    let fixture = Fixture::new();
    let valid = fixture.writer();
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(descriptor["annotations"]["idempotentHint"], false);
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
        assert_eq!(
            descriptor["annotations"]["destructiveHint"],
            !matches!(route, Route::Inspect)
        );
    }
    for (pointer, replacement) in [
        ("/request/root", json!("relative")),
        ("/request/allow_native_reads", json!(false)),
        ("/request/options", json!([])),
        ("/request/options/session", json!([])),
        ("/request/options/session/read", json!([])),
        ("/request/expected_header", json!([])),
        ("/request/options/allow_generation_advance", json!(false)),
        (
            "/request/options/session/allow_local_reservation",
            json!(false),
        ),
        (
            "/request/options/session/read/allow_coordination_files",
            json!(false),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).expect("field") = replacement;
        assert!(
            Route::Open
                .decode(invalid.as_object().expect("object").clone())
                .is_err(),
            "{pointer}"
        );
    }
    let mut unknown = valid.clone();
    unknown["request"]["options"]["unknown"] = json!(true);
    assert!(
        Route::Open
            .decode(unknown.as_object().expect("object").clone())
            .is_err()
    );
    assert_eq!(fixture.current(), fixture.header);
    let tool = serde_json::to_value(Route::Open.descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&tool["inputSchema"]).expect("input schema");
    assert!(schema.is_valid(&valid));
    let mut relative = valid;
    relative["request"]["root"] = json!("relative");
    assert!(!schema.is_valid(&relative));
}

#[test]
fn carrier_witness_bounds_native_error_codes_and_exact_generation_evidence() {
    // Structural guard for the finite static code owner, supplementing real native failure tests.
    let owner = include_str!("../../../../crates/gazette-core/src/store.rs");
    let codes: Vec<_> = owner
        .split('"')
        .filter(|text| text.starts_with("store_") && !text.contains(char::is_whitespace))
        .collect();
    assert!(!codes.is_empty());
    assert!(codes.iter().all(|code| code.len() <= 256));
    let fixture = Fixture::new();
    let Input::Open { request, digest } = input(Route::Open, fixture.writer()) else {
        panic!("open")
    };
    let handle = "0".repeat(64);
    let (mut low, mut high) = (1, MAXIMUM);
    while low < high {
        let middle = low + (high - low) / 2;
        if Reservation::new(&handle, &request, digest, middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(Reservation::new(&handle, &request, digest, low - 1).is_err());
    let reservation = Reservation::new(&handle, &request, digest, low).expect("exact carrier");
    for generation in [
        store::GenerationEffect::NotAttempted,
        store::GenerationEffect::Unverified,
    ] {
        let failure = Failure {
            phase: FailurePhase::Acquisition,
            code: "store_busy",
            generation: Some(generation),
            close_error: None,
        };
        let delivery = Delivery {
            handle: &handle,
            selection: &request,
            digest,
            phase: Phase::Open,
        };
        let response = reservation
            .finish(
                &Report {
                    session: &handle,
                    active: false,
                    outcome: Outcome::Failed {
                        failure: &failure,
                        acquired: false,
                        acquired_header: None,
                    },
                },
                &delivery.report(false, false, None, Some(&failure)),
                true,
            )
            .expect("exact effect carrier");
        let value = response.structured_content.expect("projection");
        assert_eq!(value["failure"]["generation"], json!(generation));
        assert_eq!(value["active"], false);
    }
}

#[test]
fn oversized_native_observation_and_broken_code_contract_keep_bounded_handle_evidence() {
    let fixture = Fixture::new();
    let Input::Open { request, digest } = input(Route::Open, fixture.writer()) else {
        panic!("open")
    };
    let handle = "0".repeat(64);
    let reservation =
        Reservation::new(&handle, &request, digest, 16 << 10).expect("pre-effect allowance");
    let mut writer = store::acquire_writer(fixture.root.path(), fixture.header, options())
        .expect("actual held writer");
    let mut inspection = writer.inspect().expect("actual readiness");
    // Carrier contract test only: a hypothetical larger provider observation is not SQLite status.
    inspection.verifier.provider_source_id = "x".repeat(MAXIMUM);
    let delivery = Delivery {
        handle: &handle,
        selection: &request,
        digest,
        phase: Phase::Open,
    };
    let response = reservation
        .finish(
            &Report {
                session: &handle,
                active: true,
                outcome: Outcome::Acquired {
                    inspection: &inspection,
                },
            },
            &delivery.report(true, true, Some(inspection.header), None),
            false,
        )
        .expect("bounded fallback");
    let value = response.structured_content.expect("complete fallback");
    assert_eq!(value["status"], "delivery");
    assert_eq!(value["active"], true);
    assert_eq!(value["acquired_header"], json!(inspection.header));
    assert_eq!(
        writer
            .inspect()
            .expect("same actual writer remains held")
            .header,
        inspection.header
    );
    let huge_code = "x".repeat(MAXIMUM);
    let failure = Failure {
        phase: FailurePhase::Inspection,
        code: &huge_code,
        generation: None,
        close_error: None,
    };
    let full = Report {
        session: &handle,
        active: true,
        outcome: Outcome::Failed {
            failure: &failure,
            acquired: true,
            acquired_header: Some(inspection.header),
        },
    };
    let response = reservation
        .finish(
            &full,
            &delivery.report(true, true, Some(inspection.header), Some(&failure)),
            true,
        )
        .expect("bounded contract uncertainty");
    let value = response
        .structured_content
        .expect("bounded original coordinates");
    assert_eq!(value["status"], "delivery_contract");
    assert!(
        value["generation"].is_null(),
        "inspection failure never invents failed acquisition evidence"
    );
    assert_eq!(value["session"], handle);
    assert!(serde_json::to_vec(&value).expect("wire").len() <= 16 << 10);
    writer.close().expect("explicit close");
}
