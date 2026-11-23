//! Kill the real carrier after a local ref transaction, before durable result append.

use super::*;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

/// Only the fixture's selected read-only Git executable is delayed. The real
/// dispatcher still performs its native transaction without a product test hook.
struct ReadbackBarrier {
    armed: PathBuf,
    ready: PathBuf,
    release: PathBuf,
    finished: PathBuf,
}

impl ReadbackBarrier {
    fn install(selected: &Selected) -> Self {
        let root = selected.local.control.path();
        let barrier = Self {
            armed: root.join("readback-armed"),
            ready: root.join("readback-ready"),
            release: root.join("readback-release"),
            finished: root.join("readback-finished"),
        };
        // TempDir paths are fixture-owned; assert the shell quoting precondition.
        for path in [
            &barrier.armed,
            &barrier.ready,
            &barrier.release,
            &barrier.finished,
            &selected.local.calls,
            &selected.local.paths[0],
        ] {
            assert!(!path.to_str().expect("fixture path").contains('\''));
        }
        fs::write(
            &selected.local.program,
            format!(
                "#!/bin/sh\nprintf x >> '{calls}'\nread_only=no\nfor arg do\n  [ \"$arg\" = ls-remote ] && read_only=yes\ndone\n[ \"$read_only\" = yes ] || exit 91\nif [ -f '{armed}' ] && [ ! -f '{release}' ]; then\n  expected=$(/bin/cat '{armed}') || exit 92\n  actual=$({git} --git-dir='{remote}' rev-parse --verify refs/heads/publication) || exit 93\n  if [ \"$actual\" = \"$expected\" ]; then\n    trap 'printf done > \"{finished}\"' EXIT\n    printf '%s\\n' \"$$\" > '{ready}'\n    n=0\n    while [ ! -f '{release}' ] && [ \"$n\" -lt 300 ]; do\n      /bin/sleep 0.1\n      n=$((n+1))\n    done\n    exit 73\n  fi\nfi\nexec {git} \"$@\"\n",
                calls = selected.local.calls.display(),
                armed = barrier.armed.display(),
                release = barrier.release.display(),
                remote = selected.local.paths[0].display(),
                ready = barrier.ready.display(),
                finished = barrier.finished.display(),
                git = crate::native::shell(crate::native::git()),
            ),
        )
        .expect("fixture-owned read-back barrier");
        fs::set_permissions(&selected.local.program, fs::Permissions::from_mode(0o700))
            .expect("private executable");
        barrier
    }

    fn wait(&self, carrier: &Carrier) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.ready.exists() {
            if let Ok(response) = carrier.responses.try_recv() {
                panic!("dispatch returned before the missing-result barrier: {response:?}");
            }
            assert!(
                Instant::now() < deadline,
                "bounded native ref-update barrier"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn release_and_wait(&self) {
        fs::write(&self.release, b"release").expect("release only the fixture helper");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.finished.exists() {
            assert!(Instant::now() < deadline, "bounded read-only helper exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = fs::read_to_string(&self.ready).expect("exact helper PID");
        assert!(pid.trim().parse::<u32>().expect("native PID") > 1);
        while Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("read-only helper liveness")
            .success()
        {
            assert!(
                Instant::now() < deadline,
                "read-only helper must not survive fixture"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ReadbackBarrier {
    fn drop(&mut self) {
        // Panic cleanup also releases the independently bounded helper.
        let _ = fs::write(&self.release, b"release");
    }
}

pub(super) fn send_without_reply(carrier: &mut Carrier, operation: Operation) {
    let frame = if carrier.mcp {
        carrier.sequence += 1;
        json!({
            "jsonrpc":"2.0", "id":carrier.sequence, "method":"tools/call",
            "params":{
                "name":"operate_release_session",
                "arguments":{"session":carrier.handle,"operation":operation},
                "_meta":{
                    "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities":{}
                }
            }
        })
    } else {
        json!(operation)
    };
    let input = carrier.input.as_mut().expect("live carrier input");
    serde_json::to_writer(&mut *input, &frame).expect("actual operation frame");
    input.write_all(b"\n").expect("frame boundary");
    input.flush().expect("dispatch without awaiting result");
}

fn refuse_unstarted(mcp: bool, selected: &Selected, request: &recovery_session::OpenRequest) {
    let mut carrier = Carrier::start(mcp, selected.local.control.path(), "recovery");
    let mut value = json!(request);
    value["profile"] = json!(store::coordinator::RecoveryProfile::Unstarted);
    if mcp {
        let reply = carrier.rpc(
            "tools/call",
            json!({"name":"open_recovery_session","arguments":{"request":value}}),
        );
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert_eq!(
            reply["result"]["structuredContent"]["status"],
            "open_failed"
        );
        carrier.finish();
    } else {
        let reply = carrier.line(&value);
        assert_eq!(reply["event"], "open_failed", "{reply}");
        drop(carrier.input.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = carrier
                .process
                .0
                .try_wait()
                .expect("refused carrier status")
            {
                assert!(!status.success());
                break;
            }
            assert!(Instant::now() < deadline, "refused carrier exits");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn interrupted_local_transaction(mcp: bool) {
    let mut selected = Selected::new(true);
    let barrier = ReadbackBarrier::install(&selected);
    let mut carrier = Carrier::new(mcp, selected.local.control.path());
    let header = carrier.open(&selected.open);
    let (input, _) = selected.authorize(header);
    let registered = carrier.operate(Operation::Register {
        expected: header,
        input: input.clone(),
        options: release::RegisterAuthorizationOptions {
            verification: verification(),
            record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
        },
    });
    let response = carrier.operate(Operation::Consume {
        expected: header,
        input: input.clone(),
        options: consume_options(),
    });
    let consumed: store::ReleaseConsumption =
        serde_json::from_value(response["receipt"]["record"].clone()).expect("actual consumption");
    let read_ref = |ordinal: usize| {
        git2::Repository::open_bare(&selected.local.paths[ordinal])
            .expect("exact disposable destination")
            .refname_to_id("refs/heads/publication")
            .expect("native destination ref")
            .to_string()
    };
    let untouched = read_ref(1);
    fs::write(
        &barrier.armed,
        consumed.targets[0].target.local_commit_oid.as_str(),
    )
    .expect("arm only for the original intended ref");
    send_without_reply(
        &mut carrier,
        Operation::Dispatch {
            expected: header,
            input,
            ordinal: 0,
            number: 1,
            options: release::DispatchAttemptOptions {
                verification: verification(),
                read: release::RemoteReadOptions {
                    allow_read: true,
                    timeout: Duration::from_secs(60),
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    profile: release::DispatchProfile::LocalTransaction,
                    allow_dispatch: true,
                    ..Default::default()
                },
            },
        },
    );
    barrier.wait(&carrier);
    assert_eq!(
        read_ref(0),
        consumed.targets[0].target.local_commit_oid.as_str()
    );
    assert_eq!(read_ref(1), untouched);
    let custody = &selected.open.coordinator;
    let database = &selected.local.prepared.database;
    let read_history = || {
        store::inspect_release_reconciliations(
            database,
            &consumed.authorization_id,
            Some(&header),
            read(),
        )
        .expect("independent durable history")
    };
    let original = read_history();
    assert_eq!(original.history.consumption.as_ref(), Some(&consumed));
    assert!(original.records.is_empty());
    assert_eq!(original.history.attempts.len(), 1);
    let started = &original.history.attempts[0];
    assert_eq!(
        started.start.profile,
        release::DispatchProfile::LocalTransaction
    );
    assert_eq!(started.start.key.ordinal, 0);
    assert_eq!(started.start.key.number, 1);
    assert!(
        started.result.is_none(),
        "barrier precedes durable result append"
    );
    carrier.kill_and_reap();
    barrier.release_and_wait();
    let provider_calls = selected.local.reads();
    let authorities = fs::read(&selected.calls).expect("original authority calls");
    let selection = recovery_session::OpenRequest {
        coordinator: custody.clone(),
        expected: header,
        profile: store::coordinator::RecoveryProfile::LocalTransactions,
        protected_roots: selected.open.protected_roots.clone(),
        session: selected.open.writer.session,
        allow_anchor_write: true,
        reviewed: None,
        git: selected.open.git.clone(),
    };
    refuse_unstarted(mcp, &selected, &selection);
    assert_eq!(selected.local.reads(), provider_calls);
    assert_eq!(read_history().history.attempts, original.history.attempts);
    let mut recovered = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(recovered.recover(&selection), header);
    let operation = || recovery_session::Operation::Reconcile {
        expected: header,
        authorization_id: consumed.authorization_id.clone(),
        number: 1,
        options: release::ReconcileReleaseOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                timeout: Duration::from_secs(10),
                ..Default::default()
            },
            record: store::ReleaseReconciliationOptions {
                allow_record: true,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let response = recovered.recovered_operation(operation());
    assert_eq!(response["receipt"]["inserted"], true);
    let record: store::ReleaseReconciliationRecord =
        serde_json::from_value(response["receipt"]["record"].clone()).expect("complete settlement");
    assert_eq!(
        record.settlement.state,
        store::ReleaseSettlementState::ReconciliationRequired
    );
    assert_eq!(record.settlement.targets.len(), 2);
    assert_eq!(
        record.settlement.targets[0].outcome,
        store::ReleaseTargetOutcome::Intended
    );
    assert_eq!(
        record.settlement.targets[1].outcome,
        store::ReleaseTargetOutcome::NoEffect
    );
    assert_eq!(record.reconciliation.frontier.attempts.len(), 1);
    assert!(!record.reconciliation.frontier.attempts[0].has_result);
    assert_eq!(
        selected.local.reads(),
        provider_calls + 2,
        "complete-set reads, no redispatch"
    );
    assert_eq!(
        fs::read(&selected.calls).expect("authority calls"),
        authorities
    );
    assert_eq!(
        recovered.recovered_operation(recovery_session::Operation::Close)["result"],
        "closed"
    );
    recovered.finish();
    let after = read_history();
    assert_eq!(after.history.consumption, original.history.consumption);
    assert_eq!(
        after.history.attempts, original.history.attempts,
        "missing result is not synthesized"
    );
    assert_eq!(after.history.journal.header, header);
    assert_eq!(
        after.history.journal.schema_sha256,
        original.history.journal.schema_sha256
    );
    assert_eq!(after.records, vec![record.clone()]);
    assert_eq!(
        read_ref(0),
        consumed.targets[0].target.local_commit_oid.as_str()
    );
    assert_eq!(read_ref(1), untouched);
    let mut replay = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(replay.recover(&selection), header);
    let repeated = replay.recovered_operation(operation());
    assert_eq!(repeated["receipt"]["inserted"], false);
    assert_eq!(repeated["receipt"]["record"], json!(record));
    assert_eq!(
        selected.local.reads(),
        provider_calls + 2,
        "receipt replay performs no new reads"
    );
    assert_eq!(
        replay.recovered_operation(recovery_session::Operation::Close)["result"],
        "closed"
    );
    replay.finish();
    let registration = store::inspect_release_authorization(
        database,
        &consumed.authorization_id,
        Some(&header),
        read(),
    )
    .expect("unchanged original authorization bytes");
    assert_eq!(json!(registration.record), registered["receipt"]["record"]);
    let state = store::coordinator::inspect(
        &custody.anchor_root,
        &custody.binding,
        selected.open.writer.session,
    )
    .expect("original interrupted custody");
    let store::coordinator::LocalCoordinatorState::InProgress {
        reserved_generation,
        transaction: Some(transaction),
        ..
    } = state
    else {
        panic!("reconciliation must not seal clean")
    };
    assert_eq!(reserved_generation, header.writer_fence);
    assert_eq!(transaction.header, header);
}

#[test]
fn killed_cli_local_transaction_recovers_started_without_result() {
    interrupted_local_transaction(false);
}

#[test]
fn killed_mcp_local_transaction_recovers_started_without_result() {
    interrupted_local_transaction(true);
}
