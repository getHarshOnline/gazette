//! Native reconciliation chains over real anchored consumption and attempt fixtures.

use super::*;
#[path = "observation_recovery.rs"]
mod observation_recovery;
#[path = "recovery_seal.rs"]
mod recovery_seal;
use gazette_core::store::{
    ReleaseAttemptReference, ReleaseReconciliation, ReleaseReconciliationFrontier,
    ReleaseReconciliationOptions, ReleaseReconciliationRecord, ReleaseSettlement,
    ReleaseSettlementState, ReleaseTargetOutcome, ReleaseTargetSettlement,
};
use gazette_ports::store::{ReleaseReconciliationReader, ReleaseReconciliationWriter};

const MAXIMUM: usize = 1 << 20;

#[test]
fn recovered_local_missing_result_stays_unresolved_and_retains_original_custody() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    writer
        .start_release_attempt(&path, &local_start(&consumed, 0, 1), permission())
        .expect("actual local start without result");
    let header = writer.inspect().expect("original generation").header;
    drop(writer);
    let coordinator = LocalCoordinator::new(
        fixture.anchor.path().to_owned(),
        fixture.binding.clone(),
        true,
    );
    let mut recovered = coordinator
        .recover_release(
            fixture.root.path(),
            header,
            gazette_core::store::RecoveryProfile::LocalTransactions,
            session(options()),
        )
        .expect("restricted local recovery");
    let input = selection(
        &mut recovered,
        &path,
        &consumed,
        [RefComparison::Prestate; 2],
    );
    let receipt = recovered
        .record_release_reconciliation(&path, &input, grant())
        .expect("retained actual reconciliation");
    assert_eq!(
        receipt.record.settlement.state,
        ReleaseSettlementState::ReconciliationRequired
    );
    assert!(!receipt.record.reconciliation.frontier.attempts[0].has_result);
    let replay = recovered
        .record_release_reconciliation(&path, &input, grant())
        .expect("exact replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, receipt.record);
    let anchor = fs::read(fixture.anchor.path().join("coordinator.json"))
        .expect("updated expected transaction");
    recovered.close().expect("release custody only");
    assert_eq!(
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("not sealed"),
        anchor
    );
    let history = Sqlite
        .inspect_release_reconciliations(&path, "first", options())
        .expect("history remains available");
    assert_eq!(history.history.consumption, Some(consumed));
    assert!(history.history.attempts[0].result.is_none());
    assert_eq!(history.records, vec![receipt.record]);
}

fn grant() -> ReleaseReconciliationOptions {
    ReleaseReconciliationOptions {
        allow_record: true,
        evidence_bytes: MAXIMUM,
    }
}

fn observations(
    consumed: &ReleaseConsumption,
    kinds: [RefComparison; 2],
) -> Vec<TargetReadbackRow> {
    consumed
        .targets
        .iter()
        .zip(kinds)
        .map(|(intent, kind)| {
            let target = &intent.target;
            let advertisement = match kind {
                RefComparison::Intended => RefAdvertisement::Commit {
                    oid: target.local_commit_oid.clone(),
                },
                RefComparison::Prestate => {
                    let RemoteState::Commit { oid } = &target.expected_remote_oid_or_unborn else {
                        panic!("fixture commit prestate")
                    };
                    RefAdvertisement::Commit { oid: oid.clone() }
                }
                RefComparison::Conflict => RefAdvertisement::Commit {
                    oid: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                        .parse()
                        .expect("other commit"),
                },
                RefComparison::Indeterminate => RefAdvertisement::NotAdvertised,
            };
            TargetReadbackRow {
                ordinal: target.ordinal,
                readback: TargetReadback::Observed {
                    comparison: RefComparison::compare(target, &advertisement),
                    advertisement,
                },
            }
        })
        .collect()
}

fn selection(
    writer: &mut impl ReleaseReconciliationWriter,
    path: &Path,
    consumed: &ReleaseConsumption,
    kinds: [RefComparison; 2],
) -> ReleaseReconciliation {
    let history = writer
        .inspect_release_reconciliations(path, &consumed.authorization_id)
        .expect("exact current native history");
    ReleaseReconciliation {
        mode: writer.reconciliation_mode(),
        authorization_id: consumed.authorization_id.clone(),
        number: history
            .records
            .last()
            .map_or(1, |record| record.reconciliation.number + 1),
        frontier: ReleaseReconciliationFrontier::current(
            &history.history.attempts,
            history.records.last(),
            MAXIMUM,
        )
        .expect("complete selected frontier"),
        coordinator: writer.inspect_release_coordinator().expect("actual holder"),
        readbacks: observations(consumed, kinds),
    }
}

#[test]
fn restricted_recovery_separates_read_custody_from_evidence_and_anchor_write_permissions() {
    let (fixture, mut writer, consumed) = held();
    let database = fixture.database();
    let input = selection(
        &mut writer,
        &database,
        &consumed,
        [RefComparison::Prestate; 2],
    );
    let header = writer.inspect().expect("exact consumed generation").header;
    drop(writer);
    let before = fs::read(&database).expect("original consumed bytes");
    let anchor = fs::read(fixture.anchor.path().join("coordinator.json"))
        .expect("original continuity expectation");
    let mut recovered = LocalCoordinator::new(
        fixture.anchor.path().to_owned(),
        fixture.binding.clone(),
        false,
    )
    .recover_release(
        fixture.root.path(),
        header,
        gazette_core::store::RecoveryProfile::Unstarted,
        session(options()),
    )
    .expect("observation-only recovery admission");
    let denied = recovered
        .record_release_reconciliation(&database, &input, grant())
        .expect_err("evidence permission does not imply anchor-write permission");
    assert!(matches!(denied.cause, Error::CoordinatorPermission));
    assert_eq!(denied.effect, ReleaseWriteEffect::NotAttempted);
    let denied = recovered
        .record_release_reconciliation(&database, &input, ReleaseReconciliationOptions::default())
        .expect_err("no implicit evidence permission");
    assert!(matches!(denied.cause, Error::ReleaseSettlementPermission));
    assert_eq!(denied.effect, ReleaseWriteEffect::NotAttempted);
    assert!(
        recovered
            .inspect_release_reconciliations(&database, &consumed.authorization_id)
            .expect("denied effect does not prohibit held observation")
            .records
            .is_empty()
    );
    recovered.close().expect("release without a write");
    assert_eq!(fs::read(&database).expect("no SQL effect"), before);
    assert_eq!(
        fs::read(fixture.anchor.path().join("coordinator.json")).expect("no anchor effect"),
        anchor
    );
}

pub(super) fn retained_fixture(
    writer: &mut crate::Writer,
    path: &Path,
    consumed: &ReleaseConsumption,
) {
    let input = selection(writer, path, consumed, [RefComparison::Prestate; 2]);
    writer
        .record_release_reconciliation(path, &input, grant())
        .expect("durable nonterminal reconciliation");
}

#[test]
fn historical_missing_result_remains_missing_after_later_result_and_terminal_reconciliation() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    let first = start(&consumed, 0, 1);
    writer
        .start_release_attempt(&path, &first, permission())
        .expect("incomplete start");
    let input = selection(&mut writer, &path, &consumed, [RefComparison::Prestate; 2]);
    assert!(!input.frontier.attempts[0].has_result);
    let first_record = writer
        .record_release_reconciliation(&path, &input, grant())
        .expect("uncertainty retained")
        .record;
    assert_eq!(
        first_record.settlement.state,
        ReleaseSettlementState::ReconciliationRequired
    );
    writer
        .finish_release_attempt(&rejected(&first), permission())
        .expect("later exact result");
    let history = Sqlite
        .inspect_release_reconciliations(&path, "first", options())
        .expect("historical absent result preserved");
    assert_eq!(history.records, vec![first_record.clone()]);
    assert!(history.history.attempts[0].result.is_some());
    assert!(!history.records[0].reconciliation.frontier.attempts[0].has_result);
    let next = selection(&mut writer, &path, &consumed, [RefComparison::Prestate; 2]);
    assert!(next.frontier.attempts[0].has_result);
    let terminal = writer
        .record_release_reconciliation(&path, &next, grant())
        .expect("complete conclusive no-effect settlement")
        .record;
    assert_eq!(
        terminal.settlement.state,
        ReleaseSettlementState::FailedNoEffect
    );
    let before = fs::read(&path).expect("before old exact replay");
    let replay = writer
        .record_release_reconciliation(&path, &input, grant())
        .expect("old immutable replay after terminal");
    assert!(!replay.inserted);
    assert_eq!(replay.record, first_record);
    assert_eq!(fs::read(&path).expect("no replay mutation"), before);
    writer.close().expect("seal history");
    let before = fs::read(&path).expect("sealed database");
    let history = Sqlite
        .inspect_release_reconciliations(&path, "first", options())
        .expect("clock-free reopened chain");
    assert_eq!(history.records, vec![first_record, terminal]);
    assert_eq!(history.history.consumption, Some(consumed));
    assert_eq!(fs::read(&path).expect("read-only history"), before);
}

#[test]
fn all_target_states_are_derived_from_complete_native_evidence() {
    for (kinds, expected) in [
        (
            [RefComparison::Intended; 2],
            ReleaseSettlementState::Succeeded,
        ),
        (
            [RefComparison::Prestate; 2],
            ReleaseSettlementState::FailedNoEffect,
        ),
        (
            [RefComparison::Intended, RefComparison::Conflict],
            ReleaseSettlementState::PartialTerminal,
        ),
        (
            [RefComparison::Intended, RefComparison::Indeterminate],
            ReleaseSettlementState::ReconciliationRequired,
        ),
    ] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let input = selection(&mut writer, &path, &consumed, kinds);
        let receipt = writer
            .record_release_reconciliation(&path, &input, grant())
            .expect("complete-set derivation");
        assert!(receipt.inserted);
        assert_eq!(receipt.record.settlement.state, expected);
        assert_eq!(
            receipt.record.settlement.targets.len(),
            consumed.targets.len()
        );
        writer.close().expect("seal complete evidence");
    }
}

#[test]
fn terminal_settlement_blocks_new_start_and_result_but_preserves_exact_replay() {
    for effect in ["start", "result", "reconciliation"] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        let unfinished = start(&consumed, 1, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("first");
        let completed = writer
            .finish_release_attempt(&rejected(&first), permission())
            .expect("known result");
        writer
            .start_release_attempt(&path, &unfinished, permission())
            .expect("unfinished target");
        let selected = selection(&mut writer, &path, &consumed, [RefComparison::Intended; 2]);
        writer
            .record_release_reconciliation(&path, &selected, grant())
            .expect("all destinations observed intended");
        assert!(
            !writer
                .start_release_attempt(&path, &unfinished, permission())
                .expect("replay incomplete original")
                .created
        );
        assert_eq!(
            writer
                .finish_release_attempt(&rejected(&first), permission())
                .expect("replay completed original"),
            completed
        );
        let before = fs::read(&path).expect("terminal original bytes");
        let error = match effect {
            "start" => {
                writer
                    .start_release_attempt(&path, &start(&consumed, 0, 2), permission())
                    .expect_err("closed target")
                    .cause
            }
            "result" => {
                writer
                    .finish_release_attempt(&rejected(&unfinished), permission())
                    .expect_err("closed incomplete target")
                    .cause
            }
            _ => {
                let next = selection(&mut writer, &path, &consumed, [RefComparison::Prestate; 2]);
                writer
                    .record_release_reconciliation(&path, &next, grant())
                    .expect_err("terminal chain cannot reopen")
                    .cause
            }
        };
        assert!(
            matches!(error, Error::ReleaseTerminal),
            "{effect}: {error:?}"
        );
        drop(writer);
        assert_eq!(
            Sqlite
                .inspect_release_reconciliations(&path, "first", options())
                .expect("terminal preserved")
                .records
                .len(),
            1
        );
        assert_eq!(fs::read(&path).expect("terminal unchanged"), before);
    }
}

#[test]
fn current_full_frontier_previous_cas_and_database_identity_refuse_stale_inputs() {
    for mode in [
        "start", "result", "previous", "partial", "holder", "copy", "bound",
    ] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        if mode == "result" {
            writer
                .start_release_attempt(&path, &first, permission())
                .expect("original incomplete start");
        }
        let mut input = selection(
            &mut writer,
            &path,
            &consumed,
            [RefComparison::Indeterminate; 2],
        );
        match mode {
            "start" => {
                writer
                    .start_release_attempt(&path, &first, permission())
                    .expect("frontier grows");
            }
            "result" => {
                writer
                    .finish_release_attempt(&rejected(&first), permission())
                    .expect("frontier result grows");
            }
            "previous" => {
                writer
                    .record_release_reconciliation(&path, &input, grant())
                    .expect("first wins CAS");
                input.number = 2;
            }
            "partial" => {
                input.readbacks.pop();
            }
            "holder" => {
                input.coordinator.journal.writer_fence += 1;
            }
            _ => {}
        }
        let copy = private();
        let selected = if mode == "copy" {
            let selected = Sqlite::journal_database(copy.path());
            fs::copy(&path, &selected).expect("same-coordinate copy");
            assert!(
                writer
                    .inspect_release_reconciliations(&selected, "first")
                    .is_err(),
                "selection refused before any provider work"
            );
            selected
        } else {
            path.clone()
        };
        let before = fs::read(&path).expect("before stale append");
        let mut options = grant();
        if mode == "bound" {
            options.evidence_bytes = 8;
        }
        let error = writer
            .record_release_reconciliation(&selected, &input, options)
            .expect_err("exact native CAS or boundary refusal");
        assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted, "{mode}");
        drop(writer);
        assert_eq!(
            fs::read(&path).expect("refusal unchanged"),
            before,
            "{mode}"
        );
    }
}

#[test]
fn clean_newer_holder_records_reconciliation_and_terminal_history_allows_only_new_authorized_leases()
 {
    for terminal in [false, true] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("original incomplete attempt");
        let first_input = selection(
            &mut writer,
            &path,
            &consumed,
            [RefComparison::Indeterminate; 2],
        );
        let first_record = writer
            .record_release_reconciliation(&path, &first_input, grant())
            .expect("first original evidence")
            .record;
        writer.close().expect("clean original holder close");
        let mut later = fixture.writer(consumed.created, options());
        let replay = later
            .record_release_reconciliation(&path, &first_input, grant())
            .expect("new holder replays original evidence only");
        assert!(!replay.inserted);
        assert_eq!(replay.record, first_record);
        let mut expected = vec![first_record];
        if terminal {
            let selected = selection(&mut later, &path, &consumed, [RefComparison::Intended; 2]);
            assert!(selected.coordinator.journal.writer_fence > consumed.created.writer_fence);
            expected.push(
                later
                    .record_release_reconciliation(&path, &selected, grant())
                    .expect("new holder observes final destinations without dispatch adoption")
                    .record,
            );
        }
        pending(&mut later, &path, "second");
        let leases = later.acquire_release_leases("second", lease_permission());
        if terminal {
            assert!(
                leases
                    .expect("verified terminal predecessor permits new complete lease set")
                    .targets
                    .iter()
                    .all(|lease| lease.generation == 2)
            );
            later
                .close()
                .expect("seal new independently authorized lease custody");
        } else {
            assert!(matches!(
                leases
                    .expect_err("unresolved predecessor cannot be stolen")
                    .cause,
                Error::ReleaseLeaseConflict
            ));
            drop(later);
        }
        let history = Sqlite
            .inspect_release_reconciliations(&path, "first", options())
            .expect("old chain remains inspectable");
        assert_eq!(history.records, expected);
        assert_eq!(history.history.consumption, Some(consumed));
        assert_eq!(history.history.attempts[0].start, first);
        assert!(history.history.attempts[0].result.is_none());
    }
}

#[test]
fn same_holder_terminal_lease_succession_preserves_exact_old_attempt_replay() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    let first = start(&consumed, 0, 1);
    writer
        .start_release_attempt(&path, &first, permission())
        .expect("original start");
    let complete = writer
        .finish_release_attempt(&rejected(&first), permission())
        .expect("original result");
    let selected = selection(&mut writer, &path, &consumed, [RefComparison::Intended; 2]);
    writer
        .record_release_reconciliation(&path, &selected, grant())
        .expect("terminal complete original set");
    pending(&mut writer, &path, "second");
    let later = writer
        .acquire_release_leases("second", lease_permission())
        .expect("new authorization gets new destination generations in the same held lifetime");
    assert!(later.targets.iter().all(|row| row.generation == 2));
    let before = fs::read(&path).expect("before retired-lease exact replay");
    let replay = writer
        .start_release_attempt(&path, &first, permission())
        .expect("old start is history, not active lease use");
    assert!(!replay.created);
    assert_eq!(replay.record, complete);
    assert_eq!(
        writer
            .finish_release_attempt(&rejected(&first), permission())
            .expect("old result replay"),
        complete
    );
    assert_eq!(fs::read(&path).expect("replay unchanged"), before);
    writer.close().expect("seal original and successor history");
}

#[test]
fn corrupt_terminal_chain_or_later_attempt_is_rejected_without_repair_or_lease_release() {
    for mode in ["digest", "derived", "next", "late-start", "late-result"] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        if mode == "late-result" {
            writer
                .start_release_attempt(&path, &first, permission())
                .expect("unfinished start");
        }
        let input = selection(&mut writer, &path, &consumed, [RefComparison::Intended; 2]);
        let record = writer
            .record_release_reconciliation(&path, &input, grant())
            .expect("terminal original")
            .record;
        writer.close().expect("close before intentional corruption");
        let connection = Connection::open(&path).expect("native corruptor fixture");
        connection
            .execute_batch("PRAGMA foreign_keys=ON")
            .expect("native constraints");
        match mode {
            "digest" => {
                connection
                    .execute(
                        "UPDATE gazette_release_reconciliations SET payload_sha256=zeroblob(32)",
                        [],
                    )
                    .expect("wrong record domain digest");
            }
            "derived" => {
                let mut wrong = record.clone();
                wrong.settlement.state = ReleaseSettlementState::FailedNoEffect;
                let bytes =
                    crate::settlement::payload(&wrong, MAXIMUM).expect("tampered canonical bytes");
                connection
                    .execute(
                        "UPDATE gazette_release_reconciliations SET payload=?1,payload_sha256=?2",
                        params![
                            bytes,
                            wrong
                                .identity(MAXIMUM)
                                .expect("rehash deliberate tamper")
                                .as_bytes()
                        ],
                    )
                    .expect("forged conclusion");
            }
            "next" => {
                let mut next = record.clone();
                next.reconciliation.number = 2;
                next.reconciliation.frontier.previous =
                    Some(record.identity(MAXIMUM).expect("original link"));
                crate::settlement::insert(&connection, &next, MAXIMUM)
                    .expect("SQL allows linked row; semantic reader rejects reopening");
            }
            "late-start" => crate::attempt::insert_start(&connection, &first, options())
                .expect("SQL fixture late start"),
            _ => crate::attempt::insert_result(&connection, &rejected(&first), options())
                .expect("SQL fixture late result"),
        }
        connection.close().expect("corruptor closed");
        let before = fs::read(&path).expect("corrupt bytes");
        assert!(
            Sqlite
                .inspect_release_reconciliations(&path, "first", options())
                .is_err(),
            "{mode}"
        );
        assert_eq!(fs::read(&path).expect("no repair"), before);
        with_source(&path, options(), |connection| {
            assert!(
                crate::settlement::terminal(connection, "first", options()).is_err(),
                "corrupt existence never grants lease succession"
            );
            Ok(())
        })
        .expect("native snapshot opened without repairing corrupt semantic rows");
    }
}

#[test]
fn forged_shrinking_or_impossible_lost_result_frontier_is_rejected() {
    for shrinking in [true, false] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("first");
        if !shrinking {
            writer
                .finish_release_attempt(&rejected(&first), permission())
                .expect("first rejected before the legitimate retry");
            writer
                .start_release_attempt(&path, &start(&consumed, 0, 2), permission())
                .expect("second valid start");
        }
        let kinds = if shrinking {
            [RefComparison::Prestate; 2]
        } else {
            [RefComparison::Indeterminate; 2]
        };
        let input = selection(&mut writer, &path, &consumed, kinds);
        let history = writer
            .inspect_release_attempts("first")
            .expect("original attempts");
        let original = if shrinking {
            assert!(history.attempts[0].result.is_none());
            Some(
                writer
                    .record_release_reconciliation(&path, &input, grant())
                    .expect("unresolved start remains nonterminal despite prestate observations")
                    .record,
            )
        } else {
            None
        };
        writer.close().expect("close before forged append");
        let mut forged = ReleaseReconciliationRecord {
            settlement: input
                .settle(&consumed, &history.attempts, None, MAXIMUM)
                .expect("valid native history before deliberate frontier corruption"),
            reconciliation: input,
        };
        if let Some(original) = &original {
            assert_eq!(
                original.settlement.state,
                ReleaseSettlementState::ReconciliationRequired
            );
            forged.reconciliation.number = 2;
            forged.reconciliation.frontier =
                ReleaseReconciliationFrontier::current(&[], Some(original), MAXIMUM)
                    .expect("construct deliberate omission");
            forged.settlement = ReleaseSettlement {
                state: ReleaseSettlementState::FailedNoEffect,
                targets: consumed
                    .targets
                    .iter()
                    .map(|intent| ReleaseTargetSettlement {
                        ordinal: intent.target.ordinal,
                        outcome: ReleaseTargetOutcome::NoEffect,
                        ever_intended: false,
                    })
                    .collect(),
            };
        } else {
            let mut selected = history.attempts.clone();
            selected[0].result = None;
            let mut bytes = b"GAZETTE_RELEASE_ATTEMPT_FRONTIER_V1\0".to_vec();
            bytes.extend(
                serde_json::to_vec(&selected)
                    .expect("deliberately forged selected-history payload"),
            );
            forged.reconciliation.frontier = ReleaseReconciliationFrontier {
                attempts: vec![
                    ReleaseAttemptReference {
                        ordinal: 0,
                        number: 1,
                        has_result: false,
                    },
                    ReleaseAttemptReference {
                        ordinal: 0,
                        number: 2,
                        has_result: false,
                    },
                ],
                attempts_sha256: Sha256Digest::of(&bytes),
                previous: None,
            };
            assert_eq!(forged.reconciliation.number, 1);
        }
        // Exercise the production derivation directly: the native terminal-frontier
        // check must not mask a missing monotonicity or safe-retry-prefix guard.
        assert!(
            forged
                .reconciliation
                .settle(&consumed, &history.attempts, original.as_ref(), MAXIMUM)
                .is_err()
        );
        let connection = Connection::open(&path).expect("deliberate semantic corruptor");
        connection
            .execute_batch("PRAGMA foreign_keys=ON")
            .expect("real FK");
        crate::settlement::insert(&connection, &forged, MAXIMUM)
            .expect("rehashed linked representation is not accepted semantics");
        connection.close().expect("close forged history");
        assert!(
            Sqlite
                .inspect_release_reconciliations(&path, "first", options())
                .is_err()
        );
    }
}

#[test]
fn reconciliation_foreign_key_and_uncertain_postcommit_delivery_preserve_exact_history() {
    let fixture = Fixture::new();
    let path = fixture.database();
    let ceiling = fs::metadata(&path).expect("native allocation").len() + 64 * 1024;
    let bounds = ReadOptions {
        storage_bytes: ceiling,
        ..options()
    };
    let mut writer = fixture.writer(fixture.initial, bounds);
    let registration = pending(&mut writer, &path, "first");
    let leases = writer
        .acquire_release_leases("first", lease_permission())
        .expect("leases");
    let rows = readbacks(&registration);
    let consumed = writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .expect("consumed")
        .record;
    let mut retained = Vec::new();
    let mut uncertain = None;
    for _ in 0..128 {
        let input = selection(
            &mut writer,
            &path,
            &consumed,
            [RefComparison::Indeterminate; 2],
        );
        match writer.record_release_reconciliation(&path, &input, grant()) {
            Ok(receipt) => retained.push(receipt.record),
            Err(error) => {
                assert!(matches!(error.cause, Error::StorageLimit), "{error:?}");
                assert_eq!(error.effect, ReleaseWriteEffect::Unverified);
                uncertain = Some(input);
                break;
            }
        }
    }
    let uncertain = uncertain.expect("native growth eventually exceeds selected ceiling");
    assert!(
        writer.close().is_err(),
        "uncertain journal cannot silently become clean"
    );
    let inspected = Sqlite
        .inspect_release_reconciliations(&path, "first", options())
        .expect("wider-bound independent inspection");
    assert_eq!(&inspected.records[..retained.len()], retained.as_slice());
    assert_eq!(
        inspected
            .records
            .last()
            .expect("committed final record")
            .reconciliation,
        uncertain
    );
    assert_eq!(inspected.records.len(), retained.len() + 1);
    let mut forged = inspected.records[0].clone();
    forged.reconciliation.authorization_id = "missing-consumption".into();
    let connection = Connection::open(&path).expect("isolated native FK fixture");
    connection
        .execute_batch("PRAGMA foreign_keys=ON")
        .expect("FK enabled");
    assert!(
        crate::settlement::insert(&connection, &forged, MAXIMUM).is_err(),
        "cannot attach settlement to an absent consumed intent"
    );
}
