//! Real complete-set reconciliation, history replay and clean custody succession.

#[path = "settlement/qualification.rs"]
mod qualification;
#[path = "settlement/retry.rs"]
mod retry;
#[path = "settlement/unborn.rs"]
mod unborn;

use super::{Local, Repository, consume_options, fs, options, read, release, store, verification};
use std::{os::unix::fs::PermissionsExt, time::Duration};

struct Fixture {
    local: Local,
    writer: Option<store::Writer>,
    bytes: Vec<u8>,
    binding: release::Binding,
    consumed: store::ReleaseConsumption,
}

impl Fixture {
    fn new() -> Self {
        Self::from_local(Local::new(true))
    }

    fn from_local(mut local: Local) -> Self {
        let (mut writer, bytes, binding) = local.register(false);
        let mut authority = local.authority(&binding, &binding);
        let consumed = writer
            .consume_release_authorization(
                &mut authority,
                &local.git(),
                local.request(&bytes),
                consume_options(),
            )
            .expect("real two-target registration, pinned preparation and consumption")
            .record;
        Self {
            local,
            writer: Some(writer),
            bytes,
            binding,
            consumed,
        }
    }

    fn dispatch(
        &mut self,
        ordinal: u32,
        number: u64,
    ) -> Result<store::ReleaseAttempt, release::DispatchAttemptError> {
        self.dispatch_with_profile(ordinal, number, Default::default())
    }

    fn dispatch_with_profile(
        &mut self,
        ordinal: u32,
        number: u64,
        profile: release::DispatchProfile,
    ) -> Result<store::ReleaseAttempt, release::DispatchAttemptError> {
        let mut authority = self.local.authority(&self.binding, &self.binding);
        self.writer
            .as_mut()
            .expect("held native writer")
            .dispatch_release_attempt(
                &mut authority,
                &self.local.git(),
                release::DispatchAttemptRequest {
                    authorization: &self.bytes,
                    preparations: self.local.prepared.request(&self.local.prepared.selections),
                    ordinal,
                    number,
                },
                release::DispatchAttemptOptions {
                    verification: verification(),
                    read: reads(),
                    record: store::ReleaseAttemptOptions { allow_record: true },
                    dispatch: release::DispatchOptions {
                        profile,
                        allow_dispatch: true,
                        timeout: Duration::from_secs(10),
                        ..Default::default()
                    },
                },
            )
    }

    fn reconcile(
        &mut self,
        number: u64,
    ) -> Result<store::ReleaseReconciliationReceipt, release::ReconcileReleaseError> {
        self.writer
            .as_mut()
            .expect("held reconciliation writer")
            .reconcile_release(
                &self.local.git(),
                release::ReconcileReleaseRequest {
                    database: &self.local.prepared.database,
                    expected: self.local.prepared.header,
                    authorization_id: &self.consumed.authorization_id,
                    number,
                },
                release::ReconcileReleaseOptions {
                    read: reads(),
                    record: store::ReleaseReconciliationOptions {
                        allow_record: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
    }

    fn history(&self) -> store::ReleaseReconciliationInspection {
        store::inspect_release_reconciliations(
            &self.local.prepared.database,
            &self.consumed.authorization_id,
            Some(&self.local.prepared.header),
            read(),
        )
        .expect("independent complete immutable reconciliation history")
    }

    fn close(&mut self) {
        self.writer
            .take()
            .expect("actual open writer")
            .close()
            .expect("clean independently anchored close");
    }

    fn reopen(&mut self) {
        self.close();
        let (_, selected) = self
            .local
            .prepared
            .coordinator
            .as_ref()
            .expect("original fixed custody binding retained");
        let mut writer = store::coordinator::acquire_writer(
            &selected.anchor_root,
            &selected.binding,
            self.local.prepared.header,
            options().writer,
            true,
        )
        .expect("cleanly acquire real later coordinator generation");
        let header = writer
            .inspect()
            .expect("independently inspect actual newer fence")
            .header;
        assert!(header.writer_fence > self.local.prepared.header.writer_fence);
        self.local.prepared.header = header;
        self.writer = Some(writer);
    }

    fn provider_failure(&self, applied: bool) {
        let git = crate::native::shell(crate::native::git());
        let native = if applied {
            format!("{git} \"$@\" >/dev/null\n")
        } else {
            String::new()
        };
        fs::write(&self.local.program, format!(
            "#!/bin/sh\nprintf x >> '{}'\nfor arg do\n if [ \"$arg\" = push ]; then\n {native}exit 7\n fi\ndone\nexec {git} \"$@\"\n", self.local.calls.display()
        )).expect("fixture loses only push response; reads still use real Git");
        fs::set_permissions(&self.local.program, fs::Permissions::from_mode(0o700))
            .expect("private native fixture wrapper");
    }
}

fn reads() -> release::RemoteReadOptions {
    release::RemoteReadOptions {
        allow_read: true,
        timeout: Duration::from_secs(10),
        ..Default::default()
    }
}

#[test]
fn complete_history_has_cli_mcp_sdk_parity_after_clean_custody_succession() {
    use serde_json::{Value, json};
    let mut fixture = Fixture::new();
    fixture
        .dispatch(0, 1)
        .expect("first actual fixture dispatch");
    fixture
        .dispatch(1, 1)
        .expect("second actual fixture dispatch");
    fixture.reconcile(1).expect("complete native settlement");
    let consumed_at = fixture.consumed.created;
    fixture.reopen();
    fixture.close();
    let database = &fixture.local.prepared.database;
    let header = fixture.local.prepared.header;
    assert!(header.writer_fence > consumed_at.writer_fence);
    let record = &fixture.consumed.authorization_id;
    let before = fs::read(database).expect("closed journal bytes");
    let calls = fixture.local.reads();
    for path in &fixture.local.paths {
        fs::rename(path, path.with_extension("unavailable"))
            .expect("remove disposable provider access");
    }
    let expected = [
        (
            "inspect_release_authorization",
            json!(
                store::inspect_release_authorization(database, record, Some(&header), read())
                    .expect("original authorization")
            ),
        ),
        (
            "inspect_release_leases",
            json!(
                store::inspect_release_leases(database, record, Some(&header), read())
                    .expect("original leases")
            ),
        ),
        (
            "inspect_release_consumption",
            json!(
                store::inspect_release_consumption(database, record, Some(&header), read())
                    .expect("exact consumed intent")
            ),
        ),
        (
            "inspect_release_attempts",
            json!(
                store::inspect_release_attempts(database, record, Some(&header), read())
                    .expect("complete attempts")
            ),
        ),
        (
            "inspect_release_reconciliations",
            json!(
                store::inspect_release_reconciliations(database, record, Some(&header), read())
                    .expect("complete settlement")
            ),
        ),
    ];
    assert_eq!(expected[2].1["record"]["created"], json!(consumed_at));
    assert_eq!(
        expected[3].1["attempts"]
            .as_array()
            .expect("attempts")
            .len(),
        2
    );
    assert_eq!(
        expected[4].1["records"][0]["settlement"]["state"],
        "succeeded"
    );
    for (name, native) in expected {
        let args = json!({"path":database,"record_id":record,"expected_header":header,
            "allow_read":true,"read":{"allow_coordination_files":true}});
        assert_eq!(super::super::inspection::call(name, args, false), native);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg(name.replace('_', "-"))
            .arg(database)
            .arg(record)
            .args([
                "--allow-read",
                "--allow-coordination-files",
                "--expected-header",
            ])
            .arg(serde_json::to_string(&header).expect("exact current header"))
            .output()
            .expect("direct CLI inspection");
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).expect("complete CLI history"),
            native
        );
    }
    assert_eq!(
        fixture.local.reads(),
        calls,
        "history never acquires provider observations"
    );
    assert_eq!(fs::read(database).expect("unchanged history"), before);
}

#[test]
fn two_real_targets_progress_from_partial_pending_to_complete_terminal_success() {
    let mut fixture = Fixture::new();
    fixture
        .dispatch(0, 1)
        .expect("first actual native target dispatch");
    let partial = fixture
        .reconcile(1)
        .expect("complete two-target native reconciliation");
    assert!(partial.inserted);
    assert_eq!(
        partial.record.settlement.state,
        store::ReleaseSettlementState::ReconciliationRequired
    );
    assert_eq!(
        partial
            .record
            .settlement
            .targets
            .iter()
            .map(|row| row.outcome)
            .collect::<Vec<_>>(),
        vec![
            store::ReleaseTargetOutcome::Intended,
            store::ReleaseTargetOutcome::NoEffect
        ]
    );
    let calls = fixture.local.reads();
    let replay = fixture
        .reconcile(1)
        .expect("exact original reconciliation replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, partial.record);
    assert_eq!(
        fixture.local.reads(),
        calls,
        "replay never resamples native state"
    );
    fixture
        .dispatch(1, 1)
        .expect("still-authorized second exact target, not a new batch");
    let complete = fixture
        .reconcile(2)
        .expect("complete original target set reached intended state");
    assert_eq!(
        complete.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    assert_eq!(
        complete.record.reconciliation.frontier.previous,
        Some(
            partial
                .record
                .identity(1 << 20)
                .expect("bounded original record identity")
        )
    );
    let history = fixture.history();
    assert_eq!(history.history.consumption, Some(fixture.consumed.clone()));
    assert_eq!(history.history.attempts.len(), 2);
    assert_eq!(
        history.records,
        vec![partial.record, complete.record.clone()]
    );
    fixture
        .dispatch(0, 2)
        .expect_err("terminal history cannot start another effect");
    assert_eq!(
        fixture.history().history.attempts.len(),
        2,
        "terminal refusal never creates another durable start"
    );
    let calls = fixture.local.reads();
    fixture
        .reconcile(3)
        .expect_err("terminal state cannot be reopened");
    assert_eq!(fixture.local.reads(), calls);
    fixture.close();
    assert_eq!(fixture.history().records.last(), Some(&complete.record));
}

#[test]
fn actual_intended_refs_resolve_lost_transport_responses_without_claiming_causation() {
    let mut fixture = Fixture::new();
    fixture.provider_failure(true);
    for ordinal in 0..2 {
        let attempt = fixture
            .dispatch(ordinal, 1)
            .expect("real effect with retained transport uncertainty");
        let result = attempt
            .result
            .expect("native result plus independent read-back");
        assert_eq!(
            result
                .transport
                .expect_err("provider lost usable response")
                .effect,
            release::DispatchEffect::Unverified
        );
        assert!(matches!(
            result.readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Intended,
                ..
            }
        ));
    }
    let reconciled = fixture
        .reconcile(1)
        .expect("independent current intended observations");
    assert_eq!(
        reconciled.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    assert!(
        fixture.history().history.attempts.iter().all(|row| row
            .result
            .as_ref()
            .expect("original result")
            .transport
            .is_err()),
        "reconciliation never rewrites transport history into success"
    );
    fixture.close();
}

#[test]
fn uncertain_dispatch_and_current_prestate_cannot_be_promoted_to_no_effect() {
    let mut fixture = Fixture::new();
    fixture.provider_failure(false);
    let attempt = fixture
        .dispatch(0, 1)
        .expect("failed native provider retained as uncertain effect");
    let result = attempt.result.expect("uncertain dispatch result retained");
    assert_eq!(
        result
            .transport
            .expect_err("no usable native result")
            .effect,
        release::DispatchEffect::Unverified
    );
    assert!(matches!(
        result.readback,
        release::TargetReadback::Observed {
            comparison: release::RefComparison::Prestate,
            ..
        }
    ));
    let reconciled = fixture
        .reconcile(1)
        .expect("read-only current prestate observations");
    assert_eq!(
        reconciled.record.settlement.state,
        store::ReleaseSettlementState::ReconciliationRequired
    );
    assert_eq!(
        reconciled.record.settlement.targets[0].outcome,
        store::ReleaseTargetOutcome::Unresolved
    );
    assert_eq!(
        reconciled.record.settlement.targets[1].outcome,
        store::ReleaseTargetOutcome::NoEffect
    );
    fixture.close();
}

#[test]
fn prior_intended_observation_survives_a_real_native_rewind() {
    let mut fixture = Fixture::new();
    fixture
        .dispatch(0, 1)
        .expect("real first-target publication");
    fixture
        .reconcile(1)
        .expect("retain historical intended state in incomplete set");
    let release::RemoteState::Commit { oid } =
        &fixture.binding.ordered_targets[0].expected_remote_oid_or_unborn
    else {
        panic!("actual pinned parent");
    };
    Repository::open_bare(&fixture.local.paths[0])
        .expect("disposable destination")
        .reference(
            "refs/heads/publication",
            git2::Oid::from_str(oid.as_str()).expect("original parent OID"),
            true,
            "independent test-only rewind",
        )
        .expect("external fixture owner rewinds, not Gazette dispatch");
    let observed = fixture
        .reconcile(2)
        .expect("native rewind is retained as conflicting history");
    assert_eq!(
        observed.record.settlement.targets[0].outcome,
        store::ReleaseTargetOutcome::Conflict
    );
    assert!(observed.record.settlement.targets[0].ever_intended);
    assert_ne!(
        observed.record.settlement.state,
        store::ReleaseSettlementState::FailedNoEffect
    );
    assert_eq!(fixture.history().records.len(), 2);
    fixture.close();
}

#[test]
fn clean_new_holder_reconciles_after_expiry_without_rewriting_original_authority() {
    let mut fixture = Fixture::new();
    fixture
        .dispatch(0, 1)
        .expect("one real applied target before custody succession");
    let original = fixture.consumed.clone();
    let body: release::Authorization =
        serde_json::from_slice(&fixture.bytes).expect("original immutable expiry");
    let mut expired = fixture.binding.clone();
    expired.observed_at = body.expires_at;
    assert!(matches!(
        release::verify(&fixture.bytes, &expired, Default::default()),
        Err(release::Error::Time)
    ));
    fixture.reopen();
    // Reconciliation has no authority-source input. It reads the immutable intent
    // under actual later custody, never rebuilds a signature using the new fence.
    let record = fixture
        .reconcile(1)
        .expect("clean later holder may reconcile expired original intent")
        .record;
    assert_eq!(
        record.reconciliation.coordinator.journal,
        fixture.local.prepared.header
    );
    assert_eq!(
        record.reconciliation.coordinator.binding,
        original.leases.coordinator.binding
    );
    assert!(record.reconciliation.coordinator.journal.writer_fence > original.created.writer_fence);
    assert_eq!(fixture.history().history.consumption, Some(original));
    assert_eq!(
        record.settlement.state,
        store::ReleaseSettlementState::ReconciliationRequired
    );
    let calls = fixture.local.reads();
    fixture
        .dispatch(1, 1)
        .expect_err("later holder cannot rewrite old signed fence to dispatch");
    assert_eq!(fixture.local.reads(), calls);
    fixture.close();
}
