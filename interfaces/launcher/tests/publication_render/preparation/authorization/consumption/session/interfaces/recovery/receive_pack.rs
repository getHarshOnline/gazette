//! Real receiver lifetime survives the carrier; observations never certify its completion.

use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

struct Receiver {
    before: PathBuf,
    after: PathBuf,
    advance: PathBuf,
    release: PathBuf,
    unavailable: PathBuf,
    pushes: PathBuf,
    finished: PathBuf,
}

fn executable(path: &Path, body: String) {
    fs::write(path, body).expect("disposable fixture executable");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("private fixture executable");
}

fn wait_for(mut ready: impl FnMut() -> bool, reason: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "{reason}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Receiver {
    fn install(selected: &Selected) -> Self {
        let root = selected.local.control.path();
        assert!(!root.to_str().expect("fixture path").contains('\''));
        let receiver = Self {
            before: root.join("receiver-before"),
            after: root.join("receiver-after"),
            advance: root.join("receiver-advance"),
            release: root.join("receiver-release"),
            unavailable: root.join("reads-unavailable"),
            pushes: root.join("actual-pushes"),
            finished: root.join("native-push-finished"),
        };
        for (name, heartbeat, release) in [
            ("pre-receive", &receiver.before, &receiver.advance),
            ("post-receive", &receiver.after, &receiver.release),
        ] {
            executable(
                &selected.local.paths[0].join("hooks").join(name),
                format!(
                    "#!/bin/sh\nwhile read old new ref; do :; done\nn=0\nwhile [ ! -f '{release}' ] && [ \"$n\" -lt 1200 ]; do\n printf x >> '{heartbeat}'\n /bin/sleep 0.1\n n=$((n+1))\ndone\ntest -f '{release}'\n",
                    release = release.display(),
                    heartbeat = heartbeat.display()
                ),
            );
        }
        // Buffer only the disposable push's stdout so its receiver can finish after the
        // Gazette reader dies. Native Git still performs the exact requested receive-pack.
        executable(
            &selected.local.program,
            format!(
                "#!/bin/sh\nprintf x >> '{calls}'\nfor arg do\n if [ \"$arg\" = ls-remote ] && [ -f '{unavailable}' ]; then exit 71; fi\n if [ \"$arg\" = push ]; then\n  printf p >> '{pushes}'\n  {git} \"$@\" > '{output}'\n  status=$?\n  printf '%s' \"$status\" > '{finished}'\n  /bin/cat '{output}'\n  exit \"$status\"\n fi\ndone\nexec {git} \"$@\"\n",
                calls = selected.local.calls.display(),
                unavailable = receiver.unavailable.display(),
                pushes = receiver.pushes.display(),
                output = root.join("push-output").display(),
                finished = receiver.finished.display(),
                git = crate::native::shell(crate::native::git()),
            ),
        );
        receiver
    }

    fn still_running(&self, path: &Path) {
        let before = fs::metadata(path).expect("actual receiver heartbeat").len();
        wait_for(
            || fs::metadata(path).is_ok_and(|value| value.len() > before),
            "receiver continues executing independently of Gazette carrier",
        );
        assert!(!self.finished.exists(), "native dispatch has not returned");
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        // Release only our bounded disposable hooks, including on assertion failure.
        let _ = fs::write(&self.advance, b"release fixture pre-receive");
        let _ = fs::write(&self.release, b"release fixture post-receive");
    }
}

fn interrupted_receive_pack(mcp: bool) {
    let mut selected = Selected::new(true);
    let receiver = Receiver::install(&selected);
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
        serde_json::from_value(response["receipt"]["record"].clone())
            .expect("actual consumed intent");
    let mut prepared_bytes = selected.local.prepared.snapshot();
    prepared_bytes.remove(&selected.local.prepared.database);
    started::send_without_reply(
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
                    ..Default::default()
                },
                record: store::ReleaseAttemptOptions { allow_record: true },
                dispatch: release::DispatchOptions {
                    profile: release::DispatchProfile::ReceivePack,
                    allow_dispatch: true,
                    timeout: Duration::from_secs(120),
                    ..Default::default()
                },
            },
        },
    );
    wait_for(|| receiver.before.exists(), "real pre-receive hook reached");
    let custody = &selected.open.coordinator;
    let database = &selected.local.prepared.database;
    let history = || {
        store::inspect_release_reconciliations(
            database,
            &consumed.authorization_id,
            Some(&header),
            read(),
        )
        .expect("independent original history")
    };
    let original = history();
    assert_eq!(original.history.consumption.as_ref(), Some(&consumed));
    assert_eq!(original.history.attempts.len(), 1);
    assert_eq!(
        original.history.attempts[0].start.profile,
        release::DispatchProfile::ReceivePack
    );
    assert!(original.history.attempts[0].result.is_none());
    carrier.kill_and_reap();
    receiver.still_running(&receiver.before);
    let authorities = fs::read(&selected.calls).expect("original authority observations");
    let selection = recovery_session::OpenRequest {
        coordinator: custody.clone(),
        expected: header,
        profile: store::coordinator::RecoveryProfile::ObserveInterrupted,
        protected_roots: selected.open.protected_roots.clone(),
        session: selected.open.writer.session,
        allow_anchor_write: true,
        reviewed: None,
        git: selected.open.git.clone(),
    };
    let mut recovered = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(recovered.recover(&selection), header);
    let operation = |number| recovery_session::Operation::Reconcile {
        expected: header,
        authorization_id: consumed.authorization_id.clone(),
        number,
        options: release::ReconcileReleaseOptions {
            read: release::RemoteReadOptions {
                allow_read: true,
                ..Default::default()
            },
            record: store::ReleaseReconciliationOptions {
                allow_record: true,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let mut records = Vec::new();
    for number in 1..=3 {
        if number == 1 {
            fs::write(&receiver.unavailable, b"explicit fixture read failure")
                .expect("unavailable reads");
        } else if number == 2 {
            fs::remove_file(&receiver.unavailable).expect("restore only fixture read availability");
            receiver.still_running(&receiver.before);
        } else {
            fs::write(&receiver.advance, b"original receiver may update")
                .expect("release pre-receive");
            wait_for(
                || receiver.after.exists(),
                "native ref updated and post-receive held",
            );
            receiver.still_running(&receiver.after);
            // Independent fixture-owner movement, NOT another Gazette dispatch. Matching
            // all intended refs must not imply causation or completed receiver lifetime.
            let target = &consumed.targets[1].target;
            let output = Command::new(crate::native::git())
                .arg("-C")
                .arg(selected.local.prepared.records[1].intent.address.path())
                .args(["push", "--no-verify", "--"])
                .arg(&selected.local.paths[1])
                .arg(&target.refspec)
                .output()
                .expect("fixture owner updates only disposable sibling");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let calls = selected.local.reads();
        let response = recovered.recovered_operation(operation(number));
        assert_eq!(response["receipt"]["inserted"], true);
        let record: store::ReleaseReconciliationRecord =
            serde_json::from_value(response["receipt"]["record"].clone())
                .expect("complete native observation record");
        assert_eq!(
            record.reconciliation.mode,
            store::ReleaseReconciliationMode::ObservationOnly
        );
        assert_eq!(
            record.settlement.state,
            store::ReleaseSettlementState::ReconciliationRequired
        );
        assert_eq!(record.settlement.targets.len(), 2);
        for row in &record.settlement.targets {
            assert_eq!(
                row.outcome,
                if number == 3 {
                    store::ReleaseTargetOutcome::Intended
                } else {
                    store::ReleaseTargetOutcome::Unresolved
                }
            );
        }
        assert_eq!(record.reconciliation.frontier.attempts.len(), 1);
        assert!(!record.reconciliation.frontier.attempts[0].has_result);
        assert_eq!(
            selected.local.reads(),
            calls + 2,
            "complete native set sampled exactly once"
        );
        let replay = recovered.recovered_operation(operation(number));
        assert_eq!(replay["receipt"]["inserted"], false);
        assert_eq!(replay["receipt"]["record"], json!(record));
        assert_eq!(
            selected.local.reads(),
            calls + 2,
            "exact replay has no provider calls"
        );
        assert_eq!(
            fs::read(&receiver.pushes).expect("actual Gazette pushes"),
            b"p"
        );
        records.push(record);
    }
    receiver.still_running(&receiver.after);
    assert_eq!(
        recovered.recovered_operation(recovery_session::Operation::Close)["result"],
        "closed"
    );
    recovered.finish();
    let observed = history();
    assert_eq!(observed.records, records);
    assert_eq!(observed.history.consumption, original.history.consumption);
    assert_eq!(observed.history.attempts, original.history.attempts);
    assert_eq!(observed.history.journal.header, header);
    let mut after_bytes = selected.local.prepared.snapshot();
    after_bytes.remove(database);
    assert_eq!(after_bytes, prepared_bytes, "no prepared bytes changed");
    assert_eq!(
        fs::read(&selected.calls).expect("no renewed authority"),
        authorities
    );
    let registration = store::inspect_release_authorization(
        database,
        &consumed.authorization_id,
        Some(&header),
        read(),
    )
    .expect("original authorization");
    assert_eq!(json!(registration.record), registered["receipt"]["record"]);
    let before_seal = store::coordinator::inspect(
        &custody.anchor_root,
        &custody.binding,
        selected.open.writer.session,
    )
    .expect("actual interrupted anchor");
    let store::coordinator::LocalCoordinatorState::InProgress {
        reserved_generation,
        transaction: Some(ref transaction),
        ..
    } = before_seal
    else {
        panic!("observation must not seal")
    };
    assert_eq!(reserved_generation, header.writer_fence);
    assert_eq!(transaction.header, header);
    let mut replay = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(replay.recover(&selection), header);
    let calls = selected.local.reads();
    let repeated = replay.recovered_operation(operation(3));
    assert_eq!(repeated["receipt"]["inserted"], false);
    assert_eq!(repeated["receipt"]["record"], json!(records[2]));
    assert_eq!(
        selected.local.reads(),
        calls,
        "read-back replay preserves unresolved mode"
    );
    assert_eq!(
        replay.recovered_operation(recovery_session::Operation::Close)["result"],
        "closed"
    );
    replay.finish();
    let sealer = store::coordinator::recover_release(
        &custody.anchor_root,
        &custody.binding,
        header,
        selection.profile,
        selected.open.writer.session,
        true,
    )
    .expect("restricted native custody");
    assert!(matches!(
        sealer.seal(true),
        Err(store::Error::ReleaseQuiescenceRequired)
    ));
    assert_eq!(
        store::coordinator::inspect(
            &custody.anchor_root,
            &custody.binding,
            selected.open.writer.session
        )
        .expect("no clean seal"),
        before_seal
    );
    assert!(
        store::coordinator::acquire_writer(
            &custody.anchor_root,
            &custody.binding,
            header,
            selected.open.writer,
            true
        )
        .is_err(),
        "no fresh generation or release capability"
    );
    fs::write(&receiver.release, b"release only disposable post-receive").expect("fixture cleanup");
    wait_for(
        // Shell redirection creates the file before printf publishes its status.
        || fs::read(&receiver.finished).is_ok_and(|status| !status.is_empty()),
        "original native Git publishes status after fixture release",
    );
    assert_eq!(
        fs::read(&receiver.finished).expect("native push status"),
        b"0"
    );
    assert_eq!(
        history().history.attempts,
        original.history.attempts,
        "receiver completion never fabricates the lost Gazette result"
    );
}

#[test]
fn killed_cli_receive_pack_observes_unresolved_lifetime_without_terminal_effects() {
    interrupted_receive_pack(false);
}

#[test]
fn killed_mcp_receive_pack_observes_unresolved_lifetime_without_terminal_effects() {
    interrupted_receive_pack(true);
}
