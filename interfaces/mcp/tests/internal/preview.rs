//! Real native preview custody, bounded responses and strict protocol selection.

use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    time::{Duration, Instant},
};

#[path = "../../../launcher/tests/preview_mcp/support.rs"]
mod support;
use support::*;

fn call(sessions: &Sessions, route: Route, arguments: Value, maximum: usize) -> Value {
    let result = sessions
        .execute(
            route
                .decode(arguments.as_object().expect("arguments").clone())
                .expect("named input"),
            maximum,
        )
        .expect("complete protocol response");
    let value = result.structured_content.expect("structured evidence");
    let descriptor = json!(route.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native result schema")
        .validate(&value)
        .expect("actual result conforms");
    value
}

fn wait(sessions: &Sessions, handle: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let report = call(
            sessions,
            Route::Inspect,
            json!({"session":handle}),
            256 << 10,
        );
        if predicate(&report["snapshot"]) {
            return report;
        }
        assert!(
            Instant::now() < deadline,
            "preview transition timed out: {report}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stop(sessions: &Sessions, handle: &str) {
    let requested = call(sessions, Route::Stop, json!({"session":handle}), 256 << 10);
    assert_eq!(requested["retained"], true);
    assert_eq!(requested["snapshot"]["stop_requested"], true);
    let finished = wait(sessions, handle, |state| state["finished"] == true);
    assert_eq!(finished["snapshot"]["terminal"]["status"], "closed");
    let released = call(
        sessions,
        Route::Stop,
        json!({"session":handle,"release_handle":true}),
        256 << 10,
    );
    assert_eq!(released["status"], "released");
    assert_eq!(released["retained"], false);
    assert_eq!(released["terminal"]["status"], "closed");
}

#[test]
fn plan_route_uses_the_native_preview_preflight_without_starting_effects() {
    let fixture = Fixture::new();
    let mut request = fixture.request(true);
    let former_root = fixture.root.path().to_str().unwrap().to_owned();
    drop(fixture);
    let held = [
        TcpListener::bind("127.0.0.1:0").expect("hold first exact endpoint"),
        TcpListener::bind("127.0.0.1:0").expect("hold second exact endpoint"),
    ];
    for (index, listener) in held.iter().enumerate() {
        request["listen"][index]["address"] = json!(listener.local_addr().unwrap());
    }
    let plan = call(&Sessions::default(), Route::Plan, request, 8192);
    assert_eq!(plan["status"], "local-cross-site-noncertified");
    assert_eq!(plan["input_mode"], "pinned");
    assert_eq!(plan["sites"], json!(SITES));
    assert_eq!(plan["effects"]["source_reads"], "once");
    assert_eq!(plan["effects"]["derived_state_writes"], false);
    assert_eq!(plan["effects"]["output_writes"], false);
    assert_eq!(plan["effects"]["listeners"], true);
    assert_eq!(
        plan["effects"]["network"],
        json!({"loopback":true,"outbound":false})
    );
    assert_eq!(
        plan["effects"]["git"],
        json!({"reads":true,"mutation":false})
    );
    assert_eq!(plan["effects"]["release_authority_consumption"], false);
    assert_eq!(plan["effects"]["unconfined_process_execution"], false);
    assert!(!plan.to_string().contains(&former_root));
}

#[test]
fn strict_shapes_permissions_and_response_reservation_precede_native_start() {
    let fixture = Fixture::new();
    let original = fixture.request(false);
    let schema_value = json!(Route::Start.descriptor());
    let schema = jsonschema::validator_for(&schema_value["inputSchema"]).expect("discovery schema");
    assert!(schema.is_valid(&original));
    for pointer in ["/publication", "/mode", "/history", "/listen/0"] {
        for value in [json!([]), Value::Null] {
            let mut request = original.clone();
            if pointer == "/history" {
                request["history"] = value;
            } else {
                *request.pointer_mut(pointer).expect("known boundary") = value;
            }
            assert!(
                Route::Start
                    .decode(request.as_object().expect("object").clone())
                    .is_err(),
                "{pointer}"
            );
        }
    }
    for pointer in ["/unexpected", "/mode/unexpected", "/listen/0/unexpected"] {
        let mut request = original.clone();
        let (parent, field) = pointer.rsplit_once('/').expect("field");
        request.pointer_mut(parent).expect("parent")[field] = json!(true);
        assert!(
            Route::Start
                .decode(request.as_object().expect("object").clone())
                .is_err(),
            "{pointer}"
        );
    }
    let sessions = Sessions::default();
    for pointer in [
        "/allow_listeners",
        "/publication/local",
        "/publication/allow_read",
    ] {
        let mut request = original.clone();
        *request.pointer_mut(pointer).expect("permission") = json!(false);
        assert!(call(&sessions, Route::Start, request, 8192)["code"].is_string());
    }
    let input = Route::Start
        .decode(original.as_object().expect("arguments").clone())
        .expect("valid start");
    assert!(
        sessions.execute(input, 1).is_err(),
        "cannot lose custody after starting"
    );
    let mut missing = original.clone();
    missing["listen"].as_array_mut().expect("bindings").pop();
    let failed = call(&sessions, Route::Start, missing, 8192);
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["retained"], false);
    assert_eq!(
        call(
            &sessions,
            Route::Inspect,
            json!({"session":failed["session"]}),
            8192
        )["code"],
        "preview_session_unknown"
    );
    let mut duplicate = original;
    duplicate["listen"][1] = duplicate["listen"][0].clone();
    let input = Route::Start
        .decode(duplicate.as_object().expect("arguments").clone())
        .expect("named bindings");
    assert!(sessions.execute(input, 8192).is_err());
}

#[test]
fn readiness_overflow_preserves_active_handle_and_exact_native_state() {
    let fixture = Fixture::new();
    let recipe = fixture
        .recipe()
        .with_default_render_year(
            gazette_cli::calendar::resolve(None).expect("MCP invocation year"),
        )
        .resolve(Default::default())
        .expect("resolved native preview recipe");
    let inputs = gazette_cli::sdk::build::acquire_publications(
        &fixture.config,
        &SITES.map(key),
        Default::default(),
        Default::default(),
    )
    .expect("independent capture");
    let expected = gazette_cli::sdk::build::render_publications(
        &inputs,
        recipe.options(Default::default()),
        Default::default(),
    )
    .expect("native expected output");
    assert!(
        expected.is_complete(),
        "{}",
        serde_json::to_string(&expected.inspect()).expect("inspection")
    );
    let sessions = Sessions::default();
    let started = call(&sessions, Route::Start, fixture.request(false), 8192);
    let handle = started["session"].as_str().expect("custody");
    let ready = wait(&sessions, handle, |state| state["latest"].is_object());
    assert_eq!(
        ready["snapshot"]["latest"]["input_identity"],
        json!(gazette_cli::sdk::preview::watch::InputIdentity::of(&inputs))
    );
    let sockets = SITES.map(|site| address(&ready["snapshot"]["latest"], site));
    let compact = call(&sessions, Route::Inspect, json!({"session":handle}), 300);
    assert_eq!(compact["status"], "delivery");
    assert_eq!(compact["retained"], true);
    assert_eq!(compact["session"], handle);
    assert_eq!(compact["request_sha256"], started["request_sha256"]);
    let recovered = call(&sessions, Route::Inspect, json!({"session":handle}), 8192);
    assert_eq!(
        recovered, ready,
        "overflow must not consume state or reopen listeners"
    );
    for (site, socket) in SITES.into_iter().zip(sockets) {
        let output = expected.sites()[&key(site)]
            .as_ref()
            .expect("site")
            .site()
            .output();
        assert_eq!(
            ready["snapshot"]["latest"]["sites"][site]["output_identity"],
            json!(output.inventory().identity())
        );
        let path = if site == "site:one" {
            "/mount/model.json"
        } else {
            "/model.json"
        };
        assert_eq!(
            get(socket, path),
            output
                .file(&"model.json".parse().expect("entry"))
                .expect("retained model")
        );
    }
    stop(&sessions, handle);
    released(&sockets);
}

#[test]
fn replay_gap_stale_recovery_and_foreign_handles_use_the_same_native_lifetime() {
    let fixture = Fixture::new();
    let sessions = Sessions::default();
    let mut request = fixture.request(true);
    request["mode"] = json!({"kind":"development","interval_ms":100});
    request["history"] = json!({"events":1,"bytes":65536,"event_bytes":32768});
    let started = call(&sessions, Route::Start, request, 8192);
    let handle = started["session"].as_str().expect("handle");
    let ready = wait(&sessions, handle, |state| state["latest"].is_object());
    let sockets = SITES.map(|site| address(&ready["snapshot"]["latest"], site));
    let first = call(
        &sessions,
        Route::Poll,
        json!({"session":handle,"after":0,"maximum":1}),
        8192,
    );
    assert_eq!(
        first,
        call(
            &sessions,
            Route::Poll,
            json!({"session":handle,"after":0,"maximum":1}),
            8192
        )
    );
    assert_eq!(
        first["observations"]["events"][0]["event"]["event"],
        "ready"
    );
    assert_eq!(
        call(
            &sessions,
            Route::Poll,
            json!({"session":handle,"after":999,"maximum":1}),
            8192
        )["code"],
        "preview_session_cursor"
    );
    let foreign = Sessions::default();
    assert_eq!(
        call(&foreign, Route::Stop, json!({"session":handle}), 8192)["code"],
        "preview_session_unknown"
    );
    let before = get(sockets[0], "/mount/guide/index.html");
    fixture.replace("theme/templates/page.html", "{{ missing_fixture_value }}");
    let stale = wait(&sessions, handle, |state| state["latest"]["stale"] == true);
    assert_eq!(stale["snapshot"]["latest"]["generation"], 1);
    assert_eq!(get(sockets[0], "/mount/guide/index.html"), before);
    fixture.replace("theme/templates/page.html", TEMPLATE);
    let recovered = wait(&sessions, handle, |state| {
        state["last_sequence"].as_u64().is_some_and(|n| n > 2) && state["latest"]["stale"] == false
    });
    assert_eq!(recovered["snapshot"]["latest"], ready["snapshot"]["latest"]);
    let gap = call(
        &sessions,
        Route::Poll,
        json!({"session":handle,"after":0,"maximum":1}),
        8192,
    );
    assert_eq!(gap["observations"]["gap"]["after"], 0);
    assert!(
        gap["observations"]["gap"]["first_available"]
            .as_u64()
            .expect("gap")
            > 1
    );
    assert_eq!(
        gap["observations"]["events"][0]["event"]["event"],
        "recovered"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("theme/templates/page.html")).expect("source"),
        TEMPLATE
    );
    stop(&sessions, handle);
    released(&sockets);
    assert_eq!(
        call(&sessions, Route::Inspect, json!({"session":handle}), 8192)["code"],
        "preview_session_unknown"
    );
}

#[test]
fn failed_complete_listener_set_never_becomes_ready() {
    let fixture = Fixture::new();
    let held = TcpListener::bind("127.0.0.1:0").expect("occupied socket");
    let sessions = Sessions::default();
    let mut request = fixture.request(true);
    request["listen"][1]["address"] = json!(held.local_addr().expect("actual socket"));
    let started = call(&sessions, Route::Start, request, 8192);
    let handle = started["session"].as_str().expect("starting custody");
    let failed = wait(&sessions, handle, |state| state["finished"] == true);
    assert_eq!(failed["snapshot"]["terminal"]["status"], "failed");
    assert_eq!(failed["snapshot"]["terminal"]["code"], "preview_io");
    assert_eq!(failed["snapshot"]["latest"], Value::Null);
    assert_eq!(failed["snapshot"]["last_sequence"], 0);
    let removed = call(
        &sessions,
        Route::Stop,
        json!({"session":handle,"release_handle":true}),
        8192,
    );
    assert_eq!(removed["retained"], false);
    assert_eq!(removed["terminal"]["status"], "failed");
}
