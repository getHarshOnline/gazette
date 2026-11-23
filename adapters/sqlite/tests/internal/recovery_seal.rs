//! Whole-journal terminal sealing through real restricted native custody.

use super::*;
use gazette_core::store::RecoveryProfile;
use gazette_ports::store::ReleaseRecoverySealer;

fn recover(fixture: &Fixture, header: JournalHeader) -> crate::Reconciler {
    LocalCoordinator::new(fixture.anchor.path().into(), fixture.binding.clone(), true)
        .recover_release(
            fixture.root.path(),
            header,
            RecoveryProfile::LocalTransactions,
            session(options()),
        )
        .expect("exact child-free restricted custody")
}

#[test]
fn terminal_local_missing_result_seals_without_rewriting_intent_then_acquires_new_generation() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    writer
        .start_release_attempt(&path, &local_start(&consumed, 0, 1), permission())
        .expect("local start");
    let header = writer.inspect().expect("exact fence").header;
    drop(writer);
    let mut recovered = recover(&fixture, header);
    let selected = selection(
        &mut recovered,
        &path,
        &consumed,
        [RefComparison::Intended; 2],
    );
    let receipt = recovered
        .record_release_reconciliation(&path, &selected, grant())
        .expect("terminal readback");
    assert_eq!(
        receipt.record.settlement.state,
        ReleaseSettlementState::Succeeded
    );
    let before = fs::read(&path).expect("retained SQL bytes");
    let seal = recovered
        .seal_recovery(true)
        .expect("explicit terminal seal");
    assert_eq!(seal.header, header);
    assert_eq!(fs::read(&path).expect("seal does not mutate SQL"), before);
    assert_eq!(
        LocalCoordinator::inspect(fixture.anchor.path(), &fixture.binding, session(options()))
            .expect("readback"),
        gazette_core::store::LocalCoordinatorState::Clean {
            seal: seal.clone(),
            adoption: None,
        }
    );
    let mut later = fixture.writer(seal.header, options());
    assert_eq!(
        later
            .inspect()
            .expect("fresh generation")
            .header
            .writer_fence,
        header.writer_fence + 1
    );
    let history = later
        .inspect_release_reconciliations(&path, "first")
        .expect("same history");
    assert_eq!(history.history.consumption, Some(consumed.clone()));
    assert!(history.history.attempts[0].result.is_none());
    assert_eq!(history.records, vec![receipt.record]);
    assert!(
        later
            .start_release_attempt(&path, &local_start(&consumed, 0, 2), permission())
            .is_err(),
        "new custody cannot redispatch old intent"
    );
}

#[test]
fn absent_partial_or_ambiguous_settlement_cannot_seal_even_without_starts() {
    for kinds in [
        None,
        Some([RefComparison::Intended, RefComparison::Prestate]),
        Some([RefComparison::Intended, RefComparison::Indeterminate]),
    ] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        if let Some(kinds) = kinds {
            let selected = selection(&mut writer, &path, &consumed, kinds);
            let receipt = writer
                .record_release_reconciliation(&path, &selected, grant())
                .expect("explicit unresolved record");
            assert_eq!(
                receipt.record.settlement.state,
                ReleaseSettlementState::ReconciliationRequired
            );
        }
        let header = writer.inspect().expect("fence").header;
        drop(writer);
        let anchor = fs::read(fixture.anchor.path().join("coordinator.json")).expect("dirty");
        let bytes = fs::read(&path).expect("original bytes");
        assert!(matches!(
            recover(&fixture, header).seal_recovery(true),
            Err(Error::ReleaseReconciliationRequired)
        ));
        assert_eq!(
            fs::read(fixture.anchor.path().join("coordinator.json")).expect("still dirty"),
            anchor
        );
        assert_eq!(fs::read(&path).expect("unchanged"), bytes);
    }
}

#[test]
fn terminal_batch_cannot_hide_another_consumed_zero_start_batch() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    let selected = selection(&mut writer, &path, &consumed, [RefComparison::Prestate; 2]);
    writer
        .record_release_reconciliation(&path, &selected, grant())
        .expect("first terminal");
    let registration = pending(&mut writer, &path, "second");
    let leases = writer
        .acquire_release_leases("second", lease_permission())
        .expect("successor leases");
    let rows = readbacks(&registration);
    let second = writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .expect("second consumed, no starts")
        .record;
    let header = writer.inspect().expect("fence").header;
    drop(writer);
    assert!(matches!(
        recover(&fixture, header).seal_recovery(true),
        Err(Error::ReleaseReconciliationRequired)
    ));
    let mut recovered = recover(&fixture, header);
    let selected = selection(&mut recovered, &path, &second, [RefComparison::Prestate; 2]);
    recovered
        .record_release_reconciliation(&path, &selected, grant())
        .expect("second terminal");
    recovered
        .seal_recovery(true)
        .expect("all consumed batches terminal");
}

#[test]
fn receive_pack_terminal_and_reaped_history_remains_unsupported() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    let selected = start(&consumed, 0, 1);
    writer
        .start_release_attempt(&path, &selected, permission())
        .expect("receive-pack start");
    writer
        .finish_release_attempt(&rejected(&selected), permission())
        .expect("reaped result");
    let selected = selection(&mut writer, &path, &consumed, [RefComparison::Prestate; 2]);
    writer
        .record_release_reconciliation(&path, &selected, grant())
        .expect("terminal");
    let header = writer.inspect().expect("fence").header;
    drop(writer);
    assert!(matches!(
        LocalCoordinator::new(fixture.anchor.path().into(), fixture.binding.clone(), true)
            .recover_release(
                fixture.root.path(),
                header,
                RecoveryProfile::LocalTransactions,
                session(options())
            ),
        Err(Error::ReleaseQuiescenceRequired)
    ));
}

#[test]
fn altered_terminal_record_is_not_a_sealing_shortcut() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    let selected = selection(&mut writer, &path, &consumed, [RefComparison::Prestate; 2]);
    writer
        .record_release_reconciliation(&path, &selected, grant())
        .expect("terminal");
    let header = writer.inspect().expect("fence").header;
    drop(writer);
    let recovered = recover(&fixture, header);
    let connection = rusqlite::Connection::open(&path).expect("fixture-only tamper");
    connection
        .execute(
            "UPDATE gazette_release_reconciliations SET payload_sha256=zeroblob(32)",
            [],
        )
        .expect("tamper digest");
    drop(connection);
    assert!(recovered.seal_recovery(true).is_err());
    assert!(matches!(
        LocalCoordinator::inspect(fixture.anchor.path(), &fixture.binding, session(options()))
            .expect("anchor preserved"),
        gazette_core::store::LocalCoordinatorState::InProgress { .. }
    ));
}
