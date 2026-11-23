//! Real restricted recovery custody; no generation acquisition or implicit clean seal.

use gazette_sdk::{
    release::recovery_session::{self, OpenRequest, Operation},
    source::Sha256Digest,
    store::{self, SessionOptions, WriterOptions, coordinator},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
};

struct Fixture {
    _root: tempfile::TempDir,
    database: PathBuf,
    open: OpenRequest,
    dirty: coordinator::LocalCoordinatorState,
    bytes: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("disposable custody");
        let base = fs::canonicalize(root.path()).expect("canonical fixture");
        let journal = base.join("journal");
        let anchor = base.join("anchor");
        for path in [&journal, &anchor] {
            fs::create_dir(path).expect("native directory");
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("private directory");
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
        .expect("fresh native journal");
        let seal = coordinator::enroll(
            coordinator::EnrollmentRequest {
                anchor_root: &anchor,
                journal_root: &journal,
                coordinator_id: "recovery-cli-fixture",
                principal_scope: Sha256Digest::of(b"synthetic local scope"),
                protected_roots: &[],
            },
            options,
            true,
        )
        .expect("fresh independent anchor");
        let mut writer = coordinator::acquire_writer(
            &anchor,
            &seal.binding,
            initialized.header,
            WriterOptions {
                allow_generation_advance: true,
                session: options,
            },
            true,
        )
        .expect("real predecessor lifetime");
        let expected = writer.inspect().expect("actual advanced generation").header;
        drop(writer);
        let dirty =
            coordinator::inspect(&anchor, &seal.binding, options).expect("retained interruption");
        assert!(matches!(
            dirty,
            coordinator::LocalCoordinatorState::InProgress { .. }
        ));
        let bytes = fs::read(&initialized.database).expect("original database bytes");
        Self {
            _root: root,
            database: initialized.database,
            open: OpenRequest {
                coordinator: coordinator::LocalCoordinatorSelection {
                    anchor_root: anchor,
                    binding: seal.binding,
                },
                expected,
                profile: coordinator::RecoveryProfile::Unstarted,
                protected_roots: vec![],
                session: options,
                allow_anchor_write: false,
                reviewed: None,
                git: None,
            },
            dirty,
            bytes,
        }
    }

    fn spawn(args: &[&str]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("recovery-session")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("real recovery CLI")
    }

    fn invoke(&self, open: &Value, suffix: &[u8], args: &[&str]) -> Output {
        let mut child = Self::spawn(args);
        let mut input = serde_json::to_vec(open).expect("open transport");
        input.push(b'\n');
        input.extend_from_slice(suffix);
        let _ = child.stdin.take().expect("stdin").write_all(&input);
        child.wait_with_output().expect("CLI exit")
    }

    fn unchanged(&self) {
        assert_eq!(
            store::inspect_journal(&self.database, None, self.open.session.read)
                .expect("native header")
                .header,
            self.open.expected
        );
        assert_eq!(
            coordinator::inspect(
                &self.open.coordinator.anchor_root,
                &self.open.coordinator.binding,
                self.open.session
            )
            .expect("native dirty state"),
            self.dirty
        );
        assert_eq!(
            fs::read(&self.database).expect("database readback"),
            self.bytes
        );
    }
}

fn lines(output: &Output) -> Vec<Value> {
    output
        .stdout
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("complete JSON line"))
        .collect()
}

#[test]
fn explicit_seal_projects_sdk_result_and_allows_separate_writer_acquisition() {
    let mut fixture = Fixture::new();
    fixture.open.allow_anchor_write = true;
    let output = fixture.invoke(
        &json!(fixture.open),
        b"{\"operation\":\"seal\",\"allow_seal\":true}\n",
        &[],
    );
    assert!(output.status.success(), "{:?}", lines(&output));
    let records = lines(&output);
    let sealed = records.last().expect("seal response");
    let seal: coordinator::LocalCoordinatorSeal =
        serde_json::from_value(sealed["seal"].clone()).expect("existing native seal");
    assert_eq!(seal.header, fixture.open.expected);
    let mut writer = coordinator::acquire_writer(
        &fixture.open.coordinator.anchor_root,
        &seal.binding,
        seal.header,
        WriterOptions {
            allow_generation_advance: true,
            session: fixture.open.session,
        },
        true,
    )
    .expect("separate fresh writer");
    assert_eq!(
        writer.inspect().expect("fence").header.writer_fence,
        seal.header.writer_fence + 1
    );
    writer.close().expect("ordinary close");
}

#[test]
fn readiness_and_repeated_inspection_match_sdk_without_advancing_or_sealing() {
    let fixture = Fixture::new();
    let (mut native, inspection) =
        recovery_session::Session::open(fixture.open.clone()).expect("native restricted custody");
    let expected = json!(native.execute(&Operation::Inspect));
    assert_eq!(expected["inspection"], json!(inspection));
    assert_eq!(
        json!(native.execute(&Operation::Close)),
        json!({"result":"closed"})
    );
    fixture.unchanged();
    let output = fixture.invoke(
        &json!(fixture.open),
        b"{\"operation\":\"inspect\"}\n{\"operation\":\"inspect\"}\n{\"operation\":\"close\"}\n",
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values = lines(&output);
    assert_eq!(values.len(), 4);
    assert_eq!(values[0], json!({"event":"ready","inspection":inspection}));
    assert_eq!(values[1], expected);
    assert_eq!(values[2], expected);
    assert_eq!(values[3], json!({"result":"closed"}));
    fixture.unchanged();
}

#[test]
fn ready_holds_both_reservations_and_explicit_close_releases_only_custody() {
    let fixture = Fixture::new();
    let mut child = Fixture::spawn(&[]);
    let mut input = child.stdin.take().expect("stdin");
    writeln!(input, "{}", json!(fixture.open)).expect("open");
    input.flush().expect("flush");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("actual ready");
    assert_eq!(
        serde_json::from_str::<Value>(&line).expect("ready")["event"],
        "ready"
    );
    assert!(
        recovery_session::Session::open(fixture.open.clone()).is_err(),
        "second native recovery cannot overlap held custody"
    );
    writeln!(input, "{{\"operation\":\"close\"}}").expect("explicit close");
    drop(input);
    line.clear();
    reader.read_line(&mut line).expect("close receipt");
    assert_eq!(
        serde_json::from_str::<Value>(&line).expect("closed"),
        json!({"result":"closed"})
    );
    drop(reader);
    assert!(child.wait_with_output().expect("exit").status.success());
    let (mut recovered, _) = recovery_session::Session::open(fixture.open.clone())
        .expect("explicit later restricted reacquisition");
    assert!(!recovered.execute(&Operation::Close).is_error());
    fixture.unchanged();
}

#[test]
fn refused_permissions_header_roots_and_response_reservation_preserve_exact_state() {
    for case in [
        "permission",
        "header",
        "root",
        "budget",
        "authority",
        "writer",
        "tuple",
    ] {
        let fixture = Fixture::new();
        let mut selected = fixture.open.clone();
        if case == "permission" {
            selected.session.allow_local_reservation = false;
        }
        if case == "header" {
            selected.expected.writer_fence += 1;
        }
        if case == "root" {
            selected
                .protected_roots
                .push(selected.coordinator.binding.journal_root.clone());
        }
        let mut open = json!(selected);
        if case == "authority" {
            open["authority"] = json!({"PRIVATE":"never execute"});
        }
        if case == "writer" {
            open["writer"] = json!({"allow_generation_advance":true});
        }
        if case == "tuple" {
            open["session"] = json!([true, selected.session.read]);
        }
        let args = if case == "budget" {
            vec!["--max-output-bytes", "1"]
        } else {
            vec![]
        };
        let output = fixture.invoke(&open, b"", &args);
        assert!(!output.status.success(), "{case}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE"));
        let values = lines(&output);
        assert!(
            values.iter().all(|v| v["event"] != "ready"),
            "{case}: {values:?}"
        );
        if ["permission", "header", "root"].contains(&case) {
            assert_eq!(values[0]["event"], "open_failed");
        } else {
            assert!(values.is_empty(), "{case}");
        }
        fixture.unchanged();
    }
}

#[test]
fn eof_malformed_forbidden_and_unterminated_operations_do_not_infer_close() {
    for (suffix, code) in [
        (b"".as_slice(), "recovery_session_eof_without_close"),
        (
            b"{\"operation\":\"close\"}".as_slice(),
            "release_session_incomplete_line",
        ),
        (
            b"{\"operation\":\"inspect\",\"operation\":\"close\"}\n".as_slice(),
            "release_session_request",
        ),
        (
            b"{\"operation\":\"dispatch\",\"PRIVATE\":true}\n".as_slice(),
            "release_session_request",
        ),
        (
            b"{\"operation\":\"register\"}\n".as_slice(),
            "release_session_request",
        ),
        (
            b"{\"operation\":\"consume\"}\n".as_slice(),
            "release_session_request",
        ),
        (b"[\"inspect\"]\n".as_slice(), "release_session_request"),
    ] {
        let fixture = Fixture::new();
        let output = fixture.invoke(&json!(fixture.open), suffix, &[]);
        assert!(!output.status.success());
        let values = lines(&output);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["event"], "ready");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(code), "{error}");
        assert!(!error.contains("PRIVATE"));
        fixture.unchanged();
    }
}

#[test]
fn valid_reconcile_failure_is_reported_then_close_remains_independent() {
    let fixture = Fixture::new();
    let operation = Operation::Reconcile {
        expected: fixture.open.expected,
        authorization_id: "not-an-approved-record".into(),
        number: 1,
        options: Default::default(),
    };
    let (mut native, _) =
        recovery_session::Session::open(fixture.open.clone()).expect("native recovery");
    let expected = json!(native.execute(&operation));
    assert_eq!(
        expected["failure"]["cause"]["code"],
        "release_session_git_unconfigured"
    );
    assert!(!native.execute(&Operation::Close).is_error());
    let mut suffix = serde_json::to_vec(&operation).expect("operation");
    suffix.extend_from_slice(b"\n{\"operation\":\"close\"}\n");
    let output = fixture.invoke(&json!(fixture.open), &suffix, &[]);
    assert!(
        !output.status.success(),
        "native failure cannot be erased by later close"
    );
    let values = lines(&output);
    assert_eq!(values.len(), 3);
    assert_eq!(values[1], expected);
    assert_eq!(values[2], json!({"result":"closed"}));
    fixture.unchanged();
}

#[test]
fn lost_ready_pipe_preserves_exact_recovery_coordinates_and_dirty_generation() {
    let fixture = Fixture::new();
    let mut child = Fixture::spawn(&[]);
    drop(child.stdout.take());
    let mut input = serde_json::to_vec(&fixture.open).expect("open");
    input.push(b'\n');
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&input)
        .expect("complete open request");
    let output = child.wait_with_output().expect("lost pipe exit");
    assert!(!output.status.success());
    let first = output
        .stderr
        .split(|&b| b == b'\n')
        .next()
        .expect("fallback line");
    let fallback: Value = serde_json::from_slice(first).expect("bounded original custody fallback");
    assert_eq!(fallback["code"], "release_session_response_unavailable");
    assert_eq!(fallback["request_sha256"], json!(Sha256Digest::of(&input)));
    assert_eq!(fallback["coordinator"], json!(fixture.open.coordinator));
    fixture.unchanged();
}

#[test]
fn complete_open_line_counts_its_lf_before_native_recovery() {
    let fixture = Fixture::new();
    let open = json!(fixture.open);
    let exact = serde_json::to_vec(&open)
        .expect("original transport bytes")
        .len()
        + 1;
    let output = fixture.invoke(
        &open,
        b"{\"operation\":\"close\"}\n",
        &["--max-input-bytes", &exact.to_string()],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(lines(&output).len(), 2);
    fixture.unchanged();
    let output = fixture.invoke(&open, b"", &["--max-input-bytes", &(exact - 1).to_string()]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("release_session_input_limit"));
    fixture.unchanged();
}
