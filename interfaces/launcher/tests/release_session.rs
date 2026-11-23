//! Real held CLI custody: native readiness, explicit close and interrupted-input evidence.

use gazette_sdk::{
    release::session::OpenRequest,
    source::Sha256Digest,
    store::{self, JournalHeader, SessionOptions, WriterOptions, coordinator},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Fixture {
    _root: tempfile::TempDir,
    database: PathBuf,
    open: OpenRequest,
    options: SessionOptions,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("disposable roots");
        let base = fs::canonicalize(root.path()).expect("canonical fixture");
        let journal = base.join("journal");
        let anchor = base.join("anchor");
        for path in [&journal, &anchor] {
            fs::create_dir(path).expect("native root");
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("private roots");
        }
        let options = SessionOptions {
            allow_local_reservation: true,
            read: store::ReadOptions {
                allow_coordination_files: true,
                ..Default::default()
            },
        };
        let initialized = store::initialize(
            &journal,
            store::InitializeOptions {
                allow_create: true,
                session: options,
                ..Default::default()
            },
        )
        .expect("fresh journal");
        let seal = coordinator::enroll(
            coordinator::EnrollmentRequest {
                anchor_root: &anchor,
                journal_root: &journal,
                coordinator_id: "session-test",
                principal_scope: Sha256Digest::of(b"fixture principal"),
                protected_roots: &[],
            },
            options,
            true,
        )
        .expect("fresh custody");
        let open = OpenRequest {
            coordinator: coordinator::LocalCoordinatorSelection {
                anchor_root: anchor,
                binding: seal.binding,
            },
            expected: initialized.header,
            protected_roots: vec![],
            writer: WriterOptions {
                allow_generation_advance: true,
                session: options,
            },
            allow_anchor_write: true,
            git: None,
            reviewed: None,
            authority: None,
        };
        Self {
            _root: root,
            database: initialized.database,
            open,
            options,
        }
    }

    fn invoke(&self, open: &OpenRequest, suffix: &[u8], args: &[&str]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("release-session")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("CLI process");
        let mut input = serde_json::to_vec(open).expect("open JSON");
        input.push(b'\n');
        input.extend_from_slice(suffix);
        // Invalid/denied requests may terminate before consuming the remaining input.
        let _ = child.stdin.take().expect("stdin").write_all(&input);
        child.wait_with_output().expect("CLI exit")
    }

    fn state(&self) -> coordinator::LocalCoordinatorState {
        coordinator::inspect(
            &self.open.coordinator.anchor_root,
            &self.open.coordinator.binding,
            self.options,
        )
        .expect("native custody inspection")
    }

    fn header(&self) -> JournalHeader {
        store::inspect_journal(&self.database, None, self.options.read)
            .expect("native journal inspection")
            .header
    }
}

fn lines(output: &Output) -> Vec<Value> {
    output
        .stdout
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("bounded complete JSON response"))
        .collect()
}

#[test]
fn ready_and_inspect_share_one_actual_generation_and_explicit_close_seals_it() {
    let fixture = Fixture::new();
    let output = fixture.invoke(
        &fixture.open,
        b"{\"operation\":\"inspect\"}\n{\"operation\":\"close\"}\n",
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = lines(&output);
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["event"], "ready");
    assert_eq!(responses[1]["result"], "inspected");
    assert_eq!(responses[0]["inspection"], responses[1]["inspection"]);
    let ready: JournalHeader = serde_json::from_value(responses[0]["inspection"]["header"].clone())
        .expect("native ready header");
    assert_eq!(ready.writer_fence, fixture.open.expected.writer_fence + 1);
    assert_eq!(fixture.header(), ready);
    assert_eq!(responses[2], json!({"result":"closed"}));
    match fixture.state() {
        coordinator::LocalCoordinatorState::Clean { seal, .. } => assert_eq!(seal.header, ready),
        coordinator::LocalCoordinatorState::InProgress { .. } => panic!("explicit close must seal"),
        coordinator::LocalCoordinatorState::Adopting { .. } => panic!("unrelated adoption state"),
    }
}

#[test]
fn denied_open_and_unfit_recovery_response_do_not_advance_or_dirty_custody() {
    for response_limit in [None, Some("1")] {
        let fixture = Fixture::new();
        let before = fixture.state();
        let mut open = fixture.open.clone();
        let args = match response_limit {
            None => {
                open.allow_anchor_write = false;
                vec![]
            }
            Some(limit) => vec!["--max-output-bytes", limit],
        };
        let output = fixture.invoke(&open, b"", &args);
        assert!(!output.status.success());
        if response_limit.is_none() {
            let responses = lines(&output);
            assert_eq!(responses.len(), 1);
            assert_eq!(responses[0]["event"], "open_failed");
            assert_eq!(
                responses[0]["error"]["code"],
                "store_coordinator_permission"
            );
        } else {
            assert!(output.stdout.is_empty());
        }
        assert_eq!(fixture.header(), fixture.open.expected);
        assert_eq!(fixture.state(), before);
    }
}

#[test]
fn eof_malformed_or_oversized_operation_never_synthesizes_an_explicit_close() {
    let oversized = vec![b'x'; 5000];
    for (suffix, args, code) in [
        (b"".as_slice(), vec![], "release_session_eof_without_close"),
        (
            b"{\"operation\":\"close\"}".as_slice(),
            vec![],
            "release_session_incomplete_line",
        ),
        (
            b"{\"operation\":\"inspect\",\"operation\":\"close\"}\n".as_slice(),
            vec![],
            "release_session_request",
        ),
        (
            oversized.as_slice(),
            vec!["--max-input-bytes", "4096"],
            "release_session_input_limit",
        ),
    ] {
        let fixture = Fixture::new();
        let output = fixture.invoke(&fixture.open, suffix, &args);
        assert!(!output.status.success());
        let responses = lines(&output);
        assert_eq!(responses.len(), 1, "{responses:?}");
        assert_eq!(responses[0]["event"], "ready");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
        match fixture.state() {
            coordinator::LocalCoordinatorState::InProgress {
                previous,
                reserved_generation,
                transaction,
            } => {
                assert_eq!(previous.header, fixture.open.expected);
                assert_eq!(reserved_generation, fixture.header().writer_fence);
                assert_eq!(
                    transaction
                        .expect("ready was delivered after the native generation transaction")
                        .header,
                    fixture.header()
                );
            }
            coordinator::LocalCoordinatorState::Clean { .. } => {
                panic!("interrupted input must not seal")
            }
            coordinator::LocalCoordinatorState::Adopting { .. } => {
                panic!("unrelated adoption state")
            }
        }
    }
}

#[test]
fn lost_ready_delivery_drops_interrupted_custody_without_automatic_close() {
    let fixture = Fixture::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("release-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI process");
    drop(child.stdout.take());
    let mut input = serde_json::to_vec(&fixture.open).expect("open");
    input.push(b'\n');
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&input)
        .expect("complete open");
    let output = child.wait_with_output().expect("delivery failure exit");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("release_session_response_unavailable")
    );
    assert!(matches!(
        fixture.state(),
        coordinator::LocalCoordinatorState::InProgress { .. }
    ));
    assert_eq!(
        fixture.header().writer_fence,
        fixture.open.expected.writer_fence + 1
    );
}
