//! Kill actual CLI/MCP consumption custody, then reconcile without dispatch or a new fence.

use super::*;
use release::recovery_session;
use std::os::unix::process::ExitStatusExt;

impl Carrier {
    fn recover(&mut self, selected: &recovery_session::OpenRequest) -> store::JournalHeader {
        let report = if self.mcp {
            let report = self.tool("open_recovery_session", json!({"request":selected}));
            assert_eq!(report["status"], "ready", "{report}");
            self.handle = report["session"].as_str().expect("recovery handle").into();
            report
        } else {
            let report = self.line(&json!(selected));
            assert_eq!(report["event"], "ready", "{report}");
            report
        };
        serde_json::from_value(report["inspection"]["header"].clone())
            .expect("actual unchanged generation")
    }

    fn recovered_operation(&mut self, operation: recovery_session::Operation) -> Value {
        let report = if self.mcp {
            let report = if matches!(operation, recovery_session::Operation::Close) {
                self.tool("close_recovery_session", json!({"session":self.handle}))
            } else {
                self.tool(
                    "operate_recovery_session",
                    json!({"session":self.handle,"operation":operation}),
                )
            };
            assert_eq!(report["status"], "outcome", "{report}");
            assert_eq!(report["session"], self.handle);
            report["outcome"].clone()
        } else {
            self.line(&json!(operation))
        };
        assert_ne!(report["result"], "failed", "{report}");
        report
    }

    fn kill_and_reap(&mut self) {
        self.process
            .0
            .kill()
            .expect("hard-kill only this disposable Gazette carrier");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self
                .process
                .0
                .try_wait()
                .expect("reap exact killed carrier")
            {
                assert_eq!(status.signal(), Some(9));
                break;
            }
            assert!(Instant::now() < deadline, "bounded hard-kill read-back");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn interrupted(mcp: bool) {
    let mut selected = Selected::new(true);
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
    assert_eq!(registered["result"], "registered");
    let response = carrier.operate(Operation::Consume {
        expected: header,
        input,
        options: consume_options(),
    });
    assert_eq!(response["result"], "consumed");
    let consumed: store::ReleaseConsumption =
        serde_json::from_value(response["receipt"]["record"].clone())
            .expect("actual complete consumed receipt, not timing-based readiness");
    let custody = &selected.open.coordinator;
    let database = &selected.local.prepared.database;
    let read_history = || {
        store::inspect_release_reconciliations(
            database,
            &consumed.authorization_id,
            Some(&header),
            read(),
        )
        .expect("independent complete native history")
    };
    let original = read_history();
    assert_eq!(original.history.consumption.as_ref(), Some(&consumed));
    assert!(original.history.attempts.is_empty());
    assert!(matches!(
        store::coordinator::recover_release(
            &custody.anchor_root,
            &custody.binding,
            header,
            store::coordinator::RecoveryProfile::Unstarted,
            selected.open.writer.session,
            true,
        ),
        Err(store::Error::ReservationBusy)
    ));

    carrier.kill_and_reap();
    let before = store::coordinator::inspect(
        &custody.anchor_root,
        &custody.binding,
        selected.open.writer.session,
    )
    .expect("kernel reservation released, anchor still interrupted");
    assert!(matches!(
        before,
        store::coordinator::LocalCoordinatorState::InProgress { .. }
    ));
    let authorities = fs::read(&selected.calls).expect("exact prior authority calls");
    let provider_calls = selected.local.reads();
    let selection = recovery_session::OpenRequest {
        coordinator: custody.clone(),
        expected: header,
        profile: Default::default(),
        protected_roots: selected.open.protected_roots.clone(),
        session: selected.open.writer.session,
        allow_anchor_write: true,
        reviewed: None,
        git: selected.open.git.clone(),
    };
    let mut recovered = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(recovered.recover(&selection), header);
    assert_eq!(
        recovered.recovered_operation(recovery_session::Operation::Inspect)["inspection"]["header"],
        json!(header)
    );
    let options = release::ReconcileReleaseOptions {
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
    };
    let operation = || recovery_session::Operation::Reconcile {
        expected: header,
        authorization_id: consumed.authorization_id.clone(),
        number: 1,
        options,
    };
    let response = recovered.recovered_operation(operation());
    assert_eq!(response["result"], "reconciled");
    assert_eq!(response["receipt"]["inserted"], true);
    let record: store::ReleaseReconciliationRecord =
        serde_json::from_value(response["receipt"]["record"].clone())
            .expect("complete actual two-target native record");
    assert_eq!(
        record.settlement.state,
        store::ReleaseSettlementState::FailedNoEffect
    );
    assert_eq!(
        selected.local.reads(),
        provider_calls + 2,
        "one read per original target, no push"
    );
    assert_eq!(
        fs::read(&selected.calls).expect("authority not reacquired"),
        authorities
    );
    recovered.kill_and_reap(); // Completed evidence survives actual carrier death without close.
    let after = store::coordinator::inspect(
        &custody.anchor_root,
        &custody.binding,
        selected.open.writer.session,
    )
    .expect("updated expected transaction remains interrupted");
    assert_ne!(after, before, "new evidence updates transaction identity");
    let store::coordinator::LocalCoordinatorState::InProgress {
        reserved_generation,
        transaction: Some(transaction),
        ..
    } = &after
    else {
        panic!("reconciliation must not seal clean");
    };
    assert_eq!(*reserved_generation, header.writer_fence);
    assert_eq!(transaction.header, header);
    let history = read_history();
    assert_eq!(
        history.history.journal.header,
        original.history.journal.header
    );
    assert_eq!(
        history.history.journal.schema_sha256,
        original.history.journal.schema_sha256
    );
    assert_eq!(
        history.history.consumption, original.history.consumption,
        "exact consumption, leases and intent remain unchanged"
    );
    assert_eq!(
        history.history.attempts, original.history.attempts,
        "reconciliation cannot create a dispatch start"
    );
    assert_eq!(history.records, vec![record.clone()]);
    let mut replay = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(replay.recover(&selection), header);
    let calls = selected.local.reads();
    let repeated = replay.recovered_operation(operation());
    assert_eq!(repeated["result"], "reconciled");
    assert_eq!(repeated["receipt"]["inserted"], false);
    assert_eq!(repeated["receipt"]["record"], json!(record));
    assert_eq!(selected.local.reads(), calls);
    assert_eq!(
        replay.recovered_operation(recovery_session::Operation::Close)["result"],
        "closed"
    );
    replay.finish();
    assert_eq!(
        store::coordinator::inspect(
            &custody.anchor_root,
            &custody.binding,
            selected.open.writer.session,
        )
        .expect("no clean seal"),
        after
    );
    assert!(
        store::coordinator::acquire_writer(
            &custody.anchor_root,
            &custody.binding,
            header,
            selected.open.writer,
            true,
        )
        .is_err(),
        "terminal reconciliation is not fresh release authority"
    );
    let original_registration = store::inspect_release_authorization(
        database,
        &consumed.authorization_id,
        Some(&header),
        read(),
    )
    .expect("original registration survives");
    assert_eq!(
        json!(original_registration.record),
        registered["receipt"]["record"]
    );
    let mut sealer = Carrier::start(mcp, selected.local.control.path(), "recovery");
    assert_eq!(sealer.recover(&selection), header);
    let response =
        sealer.recovered_operation(recovery_session::Operation::Seal { allow_seal: true });
    assert_eq!(response["result"], "sealed");
    let seal: store::coordinator::LocalCoordinatorSeal =
        serde_json::from_value(response["seal"].clone()).expect("existing clean seal");
    assert_eq!(seal.header, header);
    sealer.finish();
    assert_eq!(
        selected.local.reads(),
        calls,
        "sealing is not another provider operation"
    );
    assert_eq!(
        fs::read(&selected.calls).expect("authority calls"),
        authorities
    );
    let mut later = store::coordinator::acquire_writer(
        &custody.anchor_root,
        &custody.binding,
        seal.header,
        selected.open.writer,
        true,
    )
    .expect("separate fresh writer after explicit terminal seal");
    assert_eq!(
        later
            .inspect()
            .expect("successor generation")
            .header
            .writer_fence,
        header.writer_fence + 1
    );
    later.close().expect("ordinary writer close");
    let resumed =
        store::inspect_release_reconciliations(database, &consumed.authorization_id, None, read())
            .expect("native retained exact history");
    assert_eq!(resumed.history.consumption, original.history.consumption);
    assert_eq!(resumed.history.attempts, original.history.attempts);
    assert_eq!(resumed.records, history.records);
}

#[test]
fn killed_cli_consumption_reconciles_without_new_generation_or_dispatch() {
    interrupted(false);
}

#[test]
fn killed_mcp_consumption_reconciles_without_new_generation_or_dispatch() {
    interrupted(true);
}

#[path = "recovery/started.rs"]
mod started;

#[path = "recovery/receive_pack.rs"]
mod receive_pack;
