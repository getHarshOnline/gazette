//! Observation custody preserves interrupted native effects without conferring quiescence.

use super::*;
use gazette_core::store::{RecoveryProfile, ReleaseReconciliationMode};
use gazette_ports::store::ReleaseRecoverySealer;

fn recover(fixture: &Fixture, header: JournalHeader) -> crate::Reconciler {
    LocalCoordinator::new(fixture.anchor.path().into(), fixture.binding.clone(), true)
        .recover_release(
            fixture.root.path(),
            header,
            RecoveryProfile::ObserveInterrupted,
            session(options()),
        )
        .expect("explicit observation-only native recovery")
}

#[test]
fn interrupted_observations_append_replay_and_read_back_without_erasing_attempts_or_settling() {
    for completed in [false, true] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        assert_eq!(
            writer.reconciliation_mode(),
            ReleaseReconciliationMode::Settlement
        );
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("receive-pack start");
        let second = local_start(&consumed, 1, 1);
        writer
            .start_release_attempt(&path, &second, permission())
            .expect("separate local start");
        if completed {
            writer
                .finish_release_attempt(&rejected(&first), permission())
                .expect("original recorded result");
        }
        let original = Sqlite
            .inspect_release_attempts(&path, "first", options())
            .expect("complete original attempt evidence");
        let header = writer.inspect().expect("original generation").header;
        drop(writer);
        let coordinator =
            LocalCoordinator::new(fixture.anchor.path().into(), fixture.binding.clone(), true);
        let before = fs::read(&path).expect("original bytes");
        assert_eq!(RecoveryProfile::default(), RecoveryProfile::Unstarted);
        for profile in [
            RecoveryProfile::default(),
            RecoveryProfile::LocalTransactions,
        ] {
            assert!(
                matches!(
                    coordinator.recover_release(
                        fixture.root.path(),
                        header,
                        profile,
                        session(options())
                    ),
                    Err(Error::ReleaseQuiescenceRequired)
                ),
                "existing profile never adopts receive-pack: {profile:?}"
            );
        }
        assert_eq!(
            fs::read(&path).expect("no failed-admission effects"),
            before
        );
        let mut recovered = recover(&fixture, header);
        assert_eq!(
            recovered.reconciliation_mode(),
            ReleaseReconciliationMode::ObservationOnly
        );
        assert_eq!(recovered.inspect().expect("same generation").header, header);
        let mut records = Vec::new();
        let mut first_input = None;
        for kinds in [
            [RefComparison::Prestate; 2],
            [RefComparison::Intended; 2],
            [RefComparison::Prestate; 2],
        ] {
            let input = selection(&mut recovered, &path, &consumed, kinds);
            assert_eq!(input.mode, ReleaseReconciliationMode::ObservationOnly);
            if first_input.is_none() {
                first_input = Some(input.clone());
            }
            let receipt = recovered
                .record_release_reconciliation(&path, &input, grant())
                .expect("native immutable observation append");
            assert!(receipt.inserted);
            assert_eq!(
                receipt.record.settlement.state,
                ReleaseSettlementState::ReconciliationRequired
            );
            assert!(
                receipt
                    .record
                    .settlement
                    .targets
                    .iter()
                    .all(|target| target.outcome != ReleaseTargetOutcome::NoEffect)
            );
            if kinds == [RefComparison::Intended; 2] {
                assert!(
                    receipt
                        .record
                        .settlement
                        .targets
                        .iter()
                        .all(|target| target.outcome == ReleaseTargetOutcome::Intended
                            && target.ever_intended)
                );
            }
            if records.len() == 2 {
                assert!(
                    receipt
                        .record
                        .settlement
                        .targets
                        .iter()
                        .all(|target| target.ever_intended),
                    "later observations cannot erase earlier intended evidence"
                );
            }
            records.push(receipt.record);
        }
        let before_replay = fs::read(&path).expect("complete native chain");
        let replay = recovered
            .record_release_reconciliation(
                &path,
                first_input.as_ref().expect("first exact observation"),
                grant(),
            )
            .expect("historical exact replay");
        assert!(!replay.inserted);
        assert_eq!(replay.record, records[0]);
        assert_eq!(
            fs::read(&path).expect("no replay row mutation"),
            before_replay
        );
        let observed = recovered
            .inspect_release_reconciliations(&path, "first")
            .expect("held complete readback");
        assert_eq!(observed.records, records);
        assert_eq!(observed.history.consumption, Some(consumed.clone()));
        assert_eq!(observed.history.attempts, original.attempts);
        assert_eq!(observed.history.attempts[0].result.is_some(), completed);
        assert!(observed.history.attempts[1].result.is_none());
        recovered
            .close()
            .expect("ordinary close releases custody only");
        let anchor =
            fs::read(fixture.anchor.path().join("coordinator.json")).expect("still interrupted");
        let before_seal = fs::read(&path).expect("native evidence before denied seal");
        assert!(matches!(
            recover(&fixture, header).seal_recovery(true),
            Err(Error::ReleaseQuiescenceRequired)
        ));
        assert_eq!(fs::read(&path).expect("no seal SQL effect"), before_seal);
        assert_eq!(
            fs::read(fixture.anchor.path().join("coordinator.json")).expect("no clean anchor"),
            anchor
        );
        let reopened = Sqlite
            .inspect_release_reconciliations(&path, "first", options())
            .expect("independent complete chain read");
        assert_eq!(reopened.records, records);
        assert_eq!(reopened.history.attempts, original.attempts);
        assert_eq!(reopened.history.consumption, Some(consumed));
    }
}

#[test]
fn native_holder_mode_cannot_be_selected_by_the_caller() {
    fn refuse(
        writer: &mut impl ReleaseReconciliationWriter,
        path: &Path,
        consumed: &ReleaseConsumption,
    ) {
        let mut selected = selection(writer, path, consumed, [RefComparison::Intended; 2]);
        selected.mode = match writer.reconciliation_mode() {
            ReleaseReconciliationMode::Settlement => ReleaseReconciliationMode::ObservationOnly,
            ReleaseReconciliationMode::ObservationOnly => ReleaseReconciliationMode::Settlement,
        };
        let before = fs::read(path).expect("pre-refusal database");
        let error = writer
            .record_release_reconciliation(path, &selected, grant())
            .expect_err("mode is native custody, not caller preference");
        assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted);
        assert!(
            matches!(error.cause, Error::ReleaseSettlementVerification),
            "{error:?}"
        );
        assert_eq!(fs::read(path).expect("no wrong-mode append"), before);
        assert!(
            Sqlite
                .inspect_release_reconciliations(path, "first", options())
                .expect("no invented row")
                .records
                .is_empty()
        );
    }
    let (fixture, mut writer, consumed) = held();
    refuse(&mut writer, &fixture.database(), &consumed);
    drop(writer);
    let (fixture, mut writer, consumed) = held();
    let header = writer.inspect().expect("original fence").header;
    drop(writer);
    let mut recovered = recover(&fixture, header);
    refuse(&mut recovered, &fixture.database(), &consumed);
}

#[test]
fn observation_only_mode_cannot_be_downgraded_by_rehashing_a_later_native_record() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    writer
        .start_release_attempt(&path, &start(&consumed, 0, 1), permission())
        .expect("original interrupted receive-pack");
    let header = writer.inspect().expect("fence").header;
    drop(writer);
    let mut recovered = recover(&fixture, header);
    let first = selection(
        &mut recovered,
        &path,
        &consumed,
        [RefComparison::Prestate; 2],
    );
    let first = recovered
        .record_release_reconciliation(&path, &first, grant())
        .expect("first observation")
        .record;
    let next = selection(
        &mut recovered,
        &path,
        &consumed,
        [RefComparison::Intended; 2],
    );
    let mut next = recovered
        .record_release_reconciliation(&path, &next, grant())
        .expect("later observation, still nonterminal")
        .record;
    let attempts = recovered
        .inspect_release_reconciliations(&path, "first")
        .expect("verified native chain")
        .history
        .attempts;
    recovered
        .close()
        .expect("release custody, retain dirty state");
    next.reconciliation.mode = ReleaseReconciliationMode::Settlement;
    assert!(
        next.reconciliation
            .settle(&consumed, &attempts, Some(&first), MAXIMUM)
            .is_err(),
        "native core rejects downgrade independently of persisted digest"
    );
    let connection = Connection::open(&path).expect("disposable fixture corruptor");
    let bytes = crate::settlement::payload(&next, MAXIMUM).expect("exact tampered serialization");
    connection.execute("UPDATE gazette_release_reconciliations SET payload=?1,payload_sha256=?2 WHERE number=2",
        params![bytes,next.identity(MAXIMUM).expect("rehashed changed mode").as_bytes()]).expect("tamper only existing second row");
    connection.close().expect("close intentional corruption");
    let corrupt = fs::read(&path).expect("retained corrupt bytes");
    assert!(
        Sqlite
            .inspect_release_reconciliations(&path, "first", options())
            .is_err(),
        "native reader rejects a correctly hashed semantic downgrade"
    );
    assert_eq!(fs::read(&path).expect("no repair"), corrupt);
}
