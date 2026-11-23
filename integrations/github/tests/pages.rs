//! Provider translation consumes the inward HTTP port, never a curl or process adapter.

use gazette_core::{http, process, source::Sha256Digest};
use gazette_integration_github::{
    BuildState, Error, HttpOutcome, Options, PagesBuild, observe_pages_build,
};
use gazette_ports::http::Reader;
use std::cell::RefCell;

const ENDPOINT: &str = "https://api.github.com/repos/owner/publication/pages/builds/42";
const COMMIT: &str = "0123456789012345678901234567890123456789";

struct Transport {
    result: RefCell<Option<Result<http::Response, http::Error>>>,
    requests: RefCell<Vec<String>>,
}

impl Transport {
    fn response(body: &[u8], status: u16) -> Self {
        Self {
            result: RefCell::new(Some(Ok(http::Response {
                status,
                body: body.to_vec(),
            }))),
            requests: RefCell::default(),
        }
    }
}

impl Reader for Transport {
    fn get(&self, request: http::Request<'_>) -> Result<http::Response, http::Error> {
        request.validate().expect("valid bounded HTTP request");
        assert_eq!(
            request.headers,
            [
                ("Accept", "application/vnd.github+json"),
                ("X-GitHub-Api-Version", "2026-03-10"),
                ("Cache-Control", "no-cache"),
            ]
        );
        assert_eq!(request.options.response_bytes, options().response_bytes);
        assert_eq!(request.options.timeout_ms, 30000);
        self.requests.borrow_mut().push(request.url.into());
        self.result
            .borrow_mut()
            .take()
            .expect("one acquisition, no retry")
    }
}

fn selection() -> PagesBuild {
    PagesBuild {
        remote: "https://github.com/owner/publication.git".into(),
        build_id: 42,
        expected_commit: COMMIT.parse().expect("full commit"),
    }
}

fn options() -> Options {
    Options {
        allow_read: true,
        allow_execute: true,
        allow_network: true,
        ..Default::default()
    }
}

fn body(url: &str, commit: &str, status: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "url": url, "commit": commit, "status": status,
        "pusher": {"email": "private@example.invalid"},
        "error": {"message": "private provider detail"}
    }))
    .expect("response fixture")
}

#[test]
fn exact_build_read_preserves_body_identity_without_promoting_provider_success() {
    let bytes = body(ENDPOINT, COMMIT, "built");
    let transport = Transport::response(&bytes, 200);
    let observed =
        observe_pages_build(&transport, &selection(), options()).expect("bound observation");
    assert_eq!(observed.response_sha256, Sha256Digest::of(&bytes));
    assert_eq!(observed.response_bytes, bytes.len());
    assert_eq!(observed.endpoint, ENDPOINT);
    assert_eq!(observed.selection.remote, selection().remote);
    assert_eq!(observed.http_status, 200);
    let HttpOutcome::Observed { build } = &observed.outcome else {
        panic!("native build expected")
    };
    assert!(build.matches_expected_commit);
    assert_eq!(build.state, BuildState::Built);
    let serialized = serde_json::to_string(&observed).expect("public evidence");
    assert!(!serialized.contains("private@"));
    assert!(!serialized.contains("private provider detail"));
    assert_eq!(*transport.requests.borrow(), [ENDPOINT]);
}

#[test]
fn provider_states_and_wrong_commits_remain_explicit_observations() {
    let other = "abcdefabcdefabcdefabcdefabcdefabcdefabcd";
    for (native, state) in [
        ("queued", BuildState::Pending),
        ("building", BuildState::Pending),
        ("errored", BuildState::Errored),
        ("new-provider-state", BuildState::Unknown),
        ("built", BuildState::Built),
    ] {
        let transport = Transport::response(&body(ENDPOINT, other, native), 200);
        let observed = observe_pages_build(&transport, &selection(), options())
            .expect("bound but nonmatching");
        let HttpOutcome::Observed { build } = observed.outcome else {
            panic!("build retained")
        };
        assert_eq!(build.native_status, native);
        assert_eq!(build.state, state);
        assert_eq!(build.commit.as_str(), other);
        assert!(!build.matches_expected_commit);
    }
}

#[test]
fn http_failures_retain_evidence_without_inventing_absence_or_retrying() {
    for (status, label) in [
        (401, "access_denied"),
        (403, "forbidden_or_rate_limited"),
        (404, "not_found_or_hidden"),
        (429, "rate_limited"),
        (301, "unavailable"),
        (302, "unavailable"),
        (307, "unavailable"),
        (500, "unavailable"),
    ] {
        let bytes = b"private non-JSON response";
        let transport = Transport::response(bytes, status);
        let observed =
            observe_pages_build(&transport, &selection(), options()).expect("HTTP outcome");
        assert_eq!(observed.http_status, status);
        assert_eq!(observed.response_sha256, Sha256Digest::of(bytes));
        assert_eq!(
            serde_json::to_value(observed.outcome).expect("typed outcome")["status"],
            label
        );
        assert_eq!(transport.requests.borrow().len(), 1);
    }
}

#[test]
fn successful_http_needs_exact_repository_build_and_valid_unambiguous_fields() {
    for url in [
        "https://api.github.com/repos/other/publication/pages/builds/42",
        "https://api.github.com/repos/owner/other/pages/builds/42",
        "https://api.github.com/repos/owner/publication/pages/builds/43",
        "https://api.github.com/repos/owner/publication/PAGES/BUILDS/42",
        "https://api.github.com/repos/owner/publication/pages/builds/42?extra=1",
        "https://api.github.com/repos/owner/publication/pages/builds/%34%32",
        "https://evil.invalid/repos/owner/publication/pages/builds/42",
    ] {
        let transport = Transport::response(&body(url, COMMIT, "built"), 200);
        assert_eq!(
            observe_pages_build(&transport, &selection(), options()).unwrap_err(),
            Error::Binding,
            "{url}"
        );
    }
    for bytes in [
        b"{".to_vec(),
        b"[]".to_vec(),
        b"null".to_vec(),
        body(ENDPOINT, "short", "built"),
        body(ENDPOINT, COMMIT, ""),
        body(ENDPOINT, COMMIT, "bad\nstatus"),
        body(ENDPOINT, COMMIT, &"s".repeat(65)),
        format!(
            r#"{{"url":"{ENDPOINT}","commit":"{COMMIT}","status":"built","status":"errored"}}"#
        )
        .into_bytes(),
    ] {
        let transport = Transport::response(&bytes, 200);
        assert_eq!(
            observe_pages_build(&transport, &selection(), options()).unwrap_err(),
            Error::Binding
        );
    }
    let transport = Transport::response(
        &body(
            "https://api.github.com/repos/OWNER/Publication/pages/builds/42",
            COMMIT,
            "built",
        ),
        200,
    );
    assert!(observe_pages_build(&transport, &selection(), options()).is_ok());
}

#[test]
fn permissions_selection_and_bounds_refuse_before_transport() {
    let transport = Transport::response(b"unused", 200);
    for options in [
        Options::default(),
        Options {
            allow_read: false,
            ..options()
        },
        Options {
            allow_execute: false,
            ..options()
        },
        Options {
            allow_network: false,
            ..options()
        },
    ] {
        assert_eq!(
            observe_pages_build(&transport, &selection(), options).unwrap_err(),
            Error::Permission
        );
    }
    for remote in [
        "git@github.com:owner/publication",
        "http://github.com/owner/publication",
        "https://github.com/owner/publication/",
        "https://github.com/owner/publication?token=secret",
        "https://github.com/../publication",
        "https://github.com/owner/%70ublication",
    ] {
        let mut selected = selection();
        selected.remote = remote.into();
        assert_eq!(
            observe_pages_build(&transport, &selected, options()).unwrap_err(),
            Error::Selection
        );
    }
    let mut selected = selection();
    selected.build_id = 0;
    assert_eq!(
        observe_pages_build(&transport, &selected, options()).unwrap_err(),
        Error::Selection
    );
    for (response_bytes, timeout_ms) in [
        (0, 30000),
        ((16 << 20) + 1, 30000),
        (1024, 0),
        (1024, 300001),
    ] {
        assert_eq!(
            observe_pages_build(
                &transport,
                &selection(),
                Options {
                    response_bytes,
                    timeout_ms,
                    ..options()
                }
            )
            .unwrap_err(),
            Error::Limit
        );
    }
    assert!(transport.requests.borrow().is_empty());
}

#[test]
fn invalid_response_bounds_and_transport_errors_do_not_become_provider_evidence() {
    for status in [0, 600] {
        let transport = Transport::response(b"{}", status);
        assert_eq!(
            observe_pages_build(&transport, &selection(), options()).unwrap_err(),
            Error::Response
        );
    }
    let transport = Transport::response(&vec![b'x'; options().response_bytes + 1], 200);
    assert_eq!(
        observe_pages_build(&transport, &selection(), options()).unwrap_err(),
        Error::Limit
    );
    for error in [
        http::Error::Permission,
        http::Error::Selection,
        http::Error::Limit,
        http::Error::Response,
        http::Error::Transport {
            failure: process::Failure::Timeout,
            cleanup: process::Cleanup::Reaped,
        },
        http::Error::Transport {
            failure: process::Failure::Output,
            cleanup: process::Cleanup::Uncertain,
        },
        http::Error::Transport {
            failure: process::Failure::OutputLimit,
            cleanup: process::Cleanup::Reaped,
        },
    ] {
        let transport = Transport {
            result: RefCell::new(Some(Err(error))),
            requests: RefCell::default(),
        };
        assert_eq!(
            observe_pages_build(&transport, &selection(), options()).unwrap_err(),
            Error::from(error)
        );
    }
}
