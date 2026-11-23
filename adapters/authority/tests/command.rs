//! Native process acquisition and decoder-port boundaries; no external service.

#![cfg(unix)]

use gazette_adapter_authority::{CommandSource, Observation};
use gazette_adapter_process::Runner;
use gazette_core::{
    process::{CaptureRequest, Cleanup, Command, Failure, Options, Request},
    release::Binding,
    source::Sha256Digest,
    store::JournalHeader,
};
use gazette_ports::release::{AuthorityBindingDecoder, AuthorityError, AuthoritySource as _};
use std::{collections::BTreeMap, fs, time::Duration};

struct FixtureDecoder {
    maximum: usize,
}

const DECODER: FixtureDecoder = FixtureDecoder { maximum: 1 << 20 };

impl AuthorityBindingDecoder for FixtureDecoder {
    fn decode_binding(&self, bytes: &[u8]) -> Result<Binding, &'static str> {
        if bytes.len() > self.maximum {
            return Err("release_limit");
        }
        let value = gazette_core::json::parse(bytes).map_err(|_| "release_json")?;
        if !value.is_object() {
            return Err("release_shape");
        }
        serde_json::from_value(value).map_err(|_| "release_shape")
    }
}

fn binding(observed_at: u64) -> Binding {
    Binding {
        canonical_body_sha256: Sha256Digest::of(b"independent expected body"),
        coordinator_id: "fixture-coordinator".into(),
        journal: JournalHeader {
            schema_version: 1,
            journal_id: [3; 32],
            journal_epoch: 0,
            writer_fence: 7,
        },
        ordered_targets: vec![],
        publishers: vec![],
        authority_frontier_sha256: Sha256Digest::of(b"independent authority"),
        revocation_frontier_sha256: Sha256Digest::of(b"independent revocations"),
        supersession_frontier_sha256: Sha256Digest::of(b"independent supersessions"),
        observed_at,
        emergency_stop: false,
    }
}

fn command(directory: &std::path::Path, program: &str, arguments: &[&str]) -> Command {
    Command {
        program: program.into(),
        executable: None,
        arguments: arguments
            .iter()
            .map(|argument| (*argument).into())
            .collect(),
        directory: directory.to_str().expect("fixture path").into(),
        environment: BTreeMap::new(),
    }
}

fn request<'a>(command: &'a Command, input: &'a [u8], maximum: usize) -> CaptureRequest<'a> {
    CaptureRequest {
        request: Request {
            command,
            input,
            options: Options {
                allow_execute: true,
                max_input_bytes: 1 << 20,
                timeout: Duration::from_secs(3),
            },
        },
        max_output_bytes: maximum,
        max_stderr_bytes: None,
    }
}

#[test]
fn every_acquisition_invokes_again_and_failure_never_returns_a_cached_binding() {
    let root = tempfile::tempdir().expect("private provider fixture");
    let path = root.path().join("current.json");
    let first = binding(100);
    fs::write(&path, serde_json::to_vec(&first).expect("binding")).expect("fixture state");
    // The file is only a changing fake provider fixture, not a freshness contract.
    let command = command(root.path(), "/bin/cat", &["current.json"]);
    let mut source = CommandSource::new(&Runner, request(&command, &[], 8192), &DECODER);
    assert_eq!(source.last_observation(), None);
    assert_eq!(
        source
            .acquire_authority()
            .expect("first complete observation"),
        first
    );
    let mut next = binding(101);
    next.emergency_stop = true;
    fs::write(&path, serde_json::to_vec(&next).expect("next binding"))
        .expect("updated provider state");
    assert_eq!(
        source.acquire_authority().expect("fresh stop observation"),
        next
    );
    fs::write(&path, b"PRIVATE invalid output").expect("provider invalid state");
    assert_eq!(source.acquire_authority(), Err(AuthorityError::Invalid));
    assert!(matches!(
        source.last_observation(),
        Some(Observation::InvalidBinding { .. })
    ));
    assert!(!format!("{:?}", source.last_observation()).contains("PRIVATE"));
    fs::remove_file(&path).expect("fixture provider unavailable");
    assert_eq!(source.acquire_authority(), Err(AuthorityError::Unavailable));
    assert!(matches!(
        source.last_observation(),
        Some(Observation::NonzeroExit { .. })
    ));
}

#[test]
fn finite_stdin_roundtrips_and_decoder_refusals_are_redacted() {
    let root = tempfile::tempdir().expect("provider fixture");
    let command = command(root.path(), "/bin/cat", &[]);
    let expected = binding(100);
    let valid = serde_json::to_vec(&expected).expect("binding");
    let mut source = CommandSource::new(&Runner, request(&command, &valid, valid.len()), &DECODER);
    assert_eq!(
        source.acquire_authority().expect("exact finite stdin"),
        expected
    );
    assert_eq!(
        source.last_observation(),
        Some(&Observation::Acquired {
            stdout_bytes: valid.len()
        })
    );
    let duplicate = format!(
        "{{\"observed_at\":100,{}",
        std::str::from_utf8(&valid[1..]).expect("JSON")
    );
    for bytes in [
        b"[]".as_slice(),
        b"{}",
        b"null",
        b"\xff",
        duplicate.as_bytes(),
    ] {
        let mut source = CommandSource::new(&Runner, request(&command, bytes, 8192), &DECODER);
        assert_eq!(source.acquire_authority(), Err(AuthorityError::Invalid));
        assert!(matches!(
            source.last_observation(),
            Some(Observation::InvalidBinding { .. })
        ));
    }
}

#[test]
fn complete_nonzero_output_and_incomplete_or_excessive_capture_never_become_authority() {
    let root = tempfile::tempdir().expect("provider fixture");
    let valid = serde_json::to_vec(&binding(100)).expect("binding");
    let nonzero = command(root.path(), "/bin/sh", &["-c", "/bin/cat; exit 3"]);
    let mut source = CommandSource::new(&Runner, request(&nonzero, &valid, 8192), &DECODER);
    assert_eq!(source.acquire_authority(), Err(AuthorityError::Unavailable));
    assert!(
        matches!(source.last_observation(), Some(Observation::NonzeroExit { exit, .. }) if exit.code == Some(3))
    );
    let echo = command(root.path(), "/bin/cat", &[]);
    let mut source = CommandSource::new(&Runner, request(&echo, &valid, valid.len() - 1), &DECODER);
    assert_eq!(source.acquire_authority(), Err(AuthorityError::Unavailable));
    assert!(matches!(
        source.last_observation(),
        Some(Observation::ExecutionFailed {
            failure: Failure::OutputLimit,
            ..
        })
    ));
    let small_decoder = FixtureDecoder {
        maximum: valid.len() - 1,
    };
    let mut source =
        CommandSource::new(&Runner, request(&echo, &valid, valid.len()), &small_decoder);
    assert_eq!(source.acquire_authority(), Err(AuthorityError::Invalid));
    assert!(matches!(
        source.last_observation(),
        Some(Observation::InvalidBinding {
            code: "release_limit",
            ..
        })
    ));
    let stalled = command(root.path(), "/bin/sh", &["-c", "while :; do :; done"]);
    let mut bounded = request(&stalled, &[], 8192);
    bounded.request.options.timeout = Duration::from_millis(30);
    let mut source = CommandSource::new(&Runner, bounded, &DECODER);
    assert_eq!(source.acquire_authority(), Err(AuthorityError::Unavailable));
    assert!(matches!(
        source.last_observation(),
        Some(Observation::ExecutionFailed {
            failure: Failure::Timeout,
            ..
        })
    ));
}

#[test]
fn denied_or_invalid_request_does_not_execute_native_provider() {
    let root = tempfile::tempdir().expect("provider fixture");
    let command = command(root.path(), "/bin/sh", &["-c", "printf invoked > marker"]);
    for mode in 0..3 {
        let mut bounded = request(&command, b"x", 8192);
        let expected = match mode {
            0 => {
                bounded.request.options.allow_execute = false;
                Failure::Permission
            }
            1 => {
                bounded.request.options.max_input_bytes = 0;
                Failure::Limit
            }
            _ => {
                bounded.max_output_bytes = (64 << 20) + 1;
                Failure::OutputLimit
            }
        };
        let mut source = CommandSource::new(&Runner, bounded, &DECODER);
        assert_eq!(source.acquire_authority(), Err(AuthorityError::Unavailable));
        assert_eq!(
            source.last_observation(),
            Some(&Observation::ExecutionFailed {
                failure: expected,
                cleanup: Cleanup::NotStarted
            })
        );
        assert!(!root.path().join("marker").exists());
    }
}
