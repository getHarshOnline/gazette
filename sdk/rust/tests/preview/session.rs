//! Shared runner and worker over real capture, compilation and held HTTP listeners.

use super::*;
use gazette_sdk::preview::{runner, session};
use serde_json::json;
use std::{cell::RefCell, io, thread};

fn recipe(fixture: &Fixture) -> build::LocalPublicationRecipe {
    let bytes = serde_json::to_vec(&json!({
        "format":"gazette-local-publication-set/1",
        "workspace":{"kind":"local","root":fixture.directory.path(),"workspace":"gazette.toml"},
        "sites":SITES.map(|site| json!({"site":site,"model_output":"model.json"}))
    }))
    .expect("exact native recipe");
    build::LocalPublicationRecipe::parse_json(&bytes, 1 << 20, 2).expect("shared request decoder")
}

fn options(mode: runner::Mode) -> runner::Options {
    runner::Options {
        mode,
        local_urls: true,
        listeners: permission(),
        ..Default::default()
    }
}

fn until(
    session: &session::Session,
    condition: impl Fn(&session::Snapshot) -> bool,
) -> session::Snapshot {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = session.inspect().expect("native worker state");
        if condition(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "bounded preview worker: {snapshot:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn http(site: &runner::Site, path: &str) -> String {
    let mut client = TcpStream::connect_timeout(&site.address, Duration::from_secs(2))
        .expect("actual SDK-held listener");
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("bounded response");
    write!(
        client,
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        site.address
    )
    .expect("native request");
    let mut bytes = String::new();
    client
        .read_to_string(&mut bytes)
        .expect("worker serves without interface poll");
    assert!(bytes.starts_with("HTTP/1.1 200"), "{bytes}");
    bytes
}

fn close(session: &mut session::Session) -> session::Snapshot {
    session.request_stop().expect("explicit stop request");
    let snapshot = until(session, |snapshot| snapshot.finished);
    assert!(matches!(
        session.try_finish(),
        Some(session::Terminal::Closed)
    ));
    let again = session
        .inspect()
        .expect("terminal remains inspectable after join");
    assert!(again.finished && again.stop_requested);
    snapshot
}

fn schema<T: serde::Serialize + schemars::JsonSchema>(value: &T) {
    let schema = serde_json::to_value(schemars::schema_for!(T)).expect("native schema");
    jsonschema::validator_for(&schema)
        .expect("schema validator")
        .validate(&serde_json::to_value(value).expect("native observation"))
        .expect("actual serialization satisfies discovery schema");
}

#[test]
fn worker_serves_without_poll_and_preserves_nonconsuming_history_and_terminal() {
    let fixture = Fixture::linked();
    let captured = fixture.acquire_local(&SITES);
    let mut worker = session::Session::start(
        recipe(&fixture),
        addresses(&SITES),
        options(runner::Mode::Pinned),
        Default::default(),
    )
    .expect("validated worker starts");
    let ready = until(&worker, |snapshot| snapshot.latest.is_some());
    assert!(!ready.finished && ready.terminal.is_none());
    assert!(worker.try_finish().is_none());
    let state = ready.latest.as_ref().expect("whole generation ready");
    assert!(matches!(state.input_mode, runner::InputMode::Pinned));
    assert_eq!(
        state.input_identity,
        preview::watch::InputIdentity::of(&captured)
    );
    for site in state.sites.values() {
        assert!(http(site, "/pr%C3%A9fix/").contains("Retained"));
    }
    let first = worker.poll(0, 128).expect("initial observation");
    let repeated = worker.poll(0, 128).expect("same cursor is replayable");
    assert_eq!(
        serde_json::to_value(&first.events).expect("first"),
        serde_json::to_value(&repeated.events).expect("replayed")
    );
    assert!(matches!(first.events[0].event, runner::Event::Ready { .. }));
    assert!(matches!(
        worker.poll(u64::MAX, 1),
        Err(session::Error::Cursor)
    ));
    assert!(worker.poll(0, 0).is_err());
    schema(&first);
    let final_state = close(&mut worker);
    assert!(matches!(
        final_state.terminal,
        Some(session::Terminal::Closed)
    ));
    let history = worker
        .poll(0, 128)
        .expect("read-only original history after close");
    assert!(matches!(
        history.events.last().expect("closed observation").event,
        runner::Event::Closed
    ));
    schema(&history);
    for site in state.sites.values() {
        TcpListener::bind(site.address).expect("whole worker finished after release");
    }
}

#[test]
fn bounded_history_reports_gaps_without_losing_stale_recovery_or_latest_state() {
    let fixture = Fixture::linked();
    let mut worker = session::Session::start(
        recipe(&fixture),
        addresses(&SITES),
        options(runner::Mode::Development {
            interval: Duration::from_millis(100),
        }),
        session::Limits {
            events: 1,
            ..Default::default()
        },
    )
    .expect("bounded watch worker");
    let initial = until(&worker, |snapshot| snapshot.latest.is_some());
    let initial = initial.latest.expect("ready state");
    let page = fixture.directory.path().join("one/page.md");
    let original = fs::read_to_string(&page).expect("authored bytes");
    fs::write(&page, original.replace("Retained one", "Worker rebuilt"))
        .expect("bounded dirty update");
    let changed = until(&worker, |snapshot| {
        snapshot
            .latest
            .as_ref()
            .is_some_and(|state| state.generation == 2)
    });
    let changed = changed.latest.expect("complete replacement");
    assert_ne!(initial.input_identity, changed.input_identity);
    for key in initial.sites.keys() {
        assert_eq!(initial.sites[key].address, changed.sites[key].address);
    }
    let template = fixture.directory.path().join("theme/templates/page.html");
    let original = fs::read(&template).expect("valid template");
    fs::write(&template, b"{% if %}").expect("invalid dirty template");
    let stale = until(&worker, |snapshot| {
        snapshot.latest.as_ref().is_some_and(|state| state.stale)
    });
    assert_eq!(stale.latest.as_ref().expect("retained").generation, 2);
    assert!(http(&changed.sites[&key("site:one")], "/pr%C3%A9fix/").contains("Worker rebuilt"));
    fs::write(&template, original).expect("restore exact bytes");
    let recovered = until(&worker, |snapshot| {
        snapshot.last_sequence > stale.last_sequence
            && snapshot.latest.as_ref().is_some_and(|state| !state.stale)
    });
    assert_eq!(
        recovered.latest.as_ref().expect("recovered").input_identity,
        changed.input_identity
    );
    assert_eq!(recovered.latest.as_ref().expect("recovered").generation, 2);
    let events = worker.poll(0, 1).expect("explicit lost-prefix response");
    assert!(events.gap.is_some());
    assert_eq!(events.events.len(), 1);
    assert!(matches!(
        events.events[0].event,
        runner::Event::Recovered { .. }
    ));
    assert_eq!(events.next, events.events[0].sequence);
    schema(&events);
    close(&mut worker);
    let terminal = worker.poll(0, 1).expect("closed ring still bounded");
    assert_eq!(terminal.events.len(), 1);
    assert!(terminal.snapshot.latest.is_some() && terminal.snapshot.terminal.is_some());
}

#[test]
fn observer_failure_and_session_drop_release_native_listeners_without_retry() {
    let fixture = Fixture::new();
    for mode in [
        runner::Mode::Pinned,
        runner::Mode::Development {
            interval: Duration::from_millis(100),
        },
    ] {
        let held = RefCell::new(Vec::new());
        let error = runner::run(
            &recipe(&fixture),
            addresses(&SITES),
            options(mode),
            |event| {
                if let runner::Event::Ready { state } = event {
                    *held.borrow_mut() = state.sites.values().map(|site| site.address).collect();
                    return Err(io::Error::other("readiness transport lost"));
                }
                panic!("failed readiness must not manufacture Closed");
            },
            || panic!("failed readiness ends before control checkpoint"),
        )
        .expect_err("observer failed");
        assert!(matches!(
            error,
            runner::Error::Observer(_) | runner::Error::Watch(preview::watch::Error::Observer(_))
        ));
        assert_eq!(held.borrow().len(), 2);
        for address in held.borrow().iter() {
            TcpListener::bind(address).expect("scope released after failed observer");
        }
    }
    let worker = session::Session::start(
        recipe(&fixture),
        addresses(&SITES),
        options(runner::Mode::Pinned),
        Default::default(),
    )
    .expect("worker");
    let ready = until(&worker, |snapshot| snapshot.latest.is_some())
        .latest
        .expect("native readiness");
    drop(worker);
    let deadline = Instant::now() + Duration::from_secs(5);
    for site in ready.sites.values() {
        loop {
            if TcpListener::bind(site.address).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "drop requests native listener stop"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn invalid_start_has_no_worker_and_oversized_observation_retains_terminal_failure() {
    let fixture = Fixture::new();
    let invalid = [
        session::Limits {
            events: 0,
            ..Default::default()
        },
        session::Limits {
            bytes: 1,
            event_bytes: 2,
            ..Default::default()
        },
    ];
    for limits in invalid {
        assert!(matches!(
            session::Session::start(
                recipe(&fixture),
                addresses(&SITES),
                options(runner::Mode::Pinned),
                limits
            ),
            Err(session::Error::Options(_))
        ));
    }
    let denied = runner::Options::default();
    assert!(matches!(
        session::Session::start(
            recipe(&fixture),
            addresses(&SITES),
            denied,
            Default::default()
        ),
        Err(session::Error::Runner(runner::Error::Transport(_)))
    ));
    assert!(
        session::Session::start(
            recipe(&fixture),
            addresses(&["site:one"]),
            options(runner::Mode::Pinned),
            Default::default()
        )
        .is_err()
    );
    let mut worker = session::Session::start(
        recipe(&fixture),
        addresses(&SITES),
        options(runner::Mode::Pinned),
        session::Limits {
            event_bytes: 1,
            ..Default::default()
        },
    )
    .expect("valid tiny history selected before effects");
    let ended = until(&worker, |snapshot| snapshot.finished);
    assert!(ended.latest.is_none() && ended.last_sequence == 0);
    assert!(matches!(
        ended.terminal,
        Some(session::Terminal::Failed {
            code: "preview_delivery"
        })
    ));
    assert!(matches!(
        worker.try_finish(),
        Some(session::Terminal::Failed { .. })
    ));
    assert!(
        worker
            .poll(0, 1)
            .expect("failure inspectable without events")
            .events
            .is_empty()
    );
    schema(&ended);
}
