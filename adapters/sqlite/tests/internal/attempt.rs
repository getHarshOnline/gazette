//! Shares the real anchored preparation/consumption fixture, not a second fake store.

#[path = "settlement.rs"]
mod settlements;

use super::*;
use gazette_core::{
    process::Cleanup,
    release::{
        DispatchEffect, DispatchError, DispatchFailure, DispatchObservation, DispatchStatus,
    },
    store::{ReleaseAttemptKey, ReleaseAttemptOptions, ReleaseAttemptResult, ReleaseAttemptStart},
};
use gazette_ports::store::{ReleaseAttemptReader, ReleaseAttemptWriter, ReleaseRecoveryConnector};

#[test]
fn receive_pack_history_refuses_both_recovery_profiles_even_after_completed_result() {
    for completed in [false, true] {
        let (fixture, mut writer, consumed) = held();
        let selected = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&fixture.database(), &selected, permission())
            .expect("actual persisted start");
        if completed {
            writer
                .finish_release_attempt(&rejected(&selected), permission())
                .expect("a completed result does not erase historical starts");
        }
        // Unrelated pending history cannot narrow recovery's global exclusion scope.
        pending(&mut writer, &fixture.database(), "different-batch");
        let header = writer.inspect().expect("current exact fence").header;
        drop(writer);
        let bytes = fs::read(fixture.database()).expect("exact native history");
        let coordinator = LocalCoordinator::new(
            fixture.anchor.path().to_owned(),
            fixture.binding.clone(),
            true,
        );
        for profile in [
            gazette_core::store::RecoveryProfile::Unstarted,
            gazette_core::store::RecoveryProfile::LocalTransactions,
        ] {
            assert!(matches!(
                coordinator.recover_release(
                    fixture.root.path(),
                    header,
                    profile,
                    session(options())
                ),
                Err(Error::ReleaseQuiescenceRequired)
            ));
        }
        assert_eq!(
            fs::read(fixture.database()).expect("no history reset"),
            bytes
        );
    }
}

fn local_start(consumed: &ReleaseConsumption, ordinal: u32, number: u64) -> ReleaseAttemptStart {
    ReleaseAttemptStart {
        profile: gazette_core::release::DispatchProfile::LocalTransaction,
        ..start(consumed, ordinal, number)
    }
}

fn local_rejection(start: &ReleaseAttemptStart) -> ReleaseAttemptResult {
    let mut result = rejected(start);
    let observed = result
        .transport
        .as_mut()
        .expect("native representation fixture");
    observed.cleanup = Cleanup::NotStarted;
    observed.exit_code = None;
    result
}

#[test]
fn local_transaction_recovery_retains_complete_and_incomplete_history_without_new_generation() {
    use gazette_core::store::RecoveryProfile;
    use gazette_ports::store::ReleaseReconciliationWriter;
    for completed in [false, true] {
        let (fixture, mut writer, consumed) = held();
        let selected = local_start(&consumed, 0, 1);
        writer
            .start_release_attempt(&fixture.database(), &selected, permission())
            .expect("real local start");
        if completed {
            writer
                .finish_release_attempt(&local_rejection(&selected), permission())
                .expect("real local result");
        }
        let original = Sqlite
            .inspect_release_attempts(&fixture.database(), "first", options())
            .expect("original history");
        let header = writer.inspect().expect("same original generation").header;
        let coordinator = LocalCoordinator::new(
            fixture.anchor.path().to_owned(),
            fixture.binding.clone(),
            true,
        );
        assert!(matches!(
            coordinator.recover_release(
                fixture.root.path(),
                header,
                RecoveryProfile::LocalTransactions,
                session(options())
            ),
            Err(Error::ReservationBusy)
        ));
        drop(writer);
        let bytes = fs::read(fixture.database()).expect("before");
        let anchor =
            fs::read(fixture.anchor.path().join("coordinator.json")).expect("dirty custody");
        assert!(matches!(
            coordinator.recover_release(
                fixture.root.path(),
                header,
                RecoveryProfile::Unstarted,
                session(options())
            ),
            Err(Error::ReleaseQuiescenceRequired)
        ));
        let mut recovered = coordinator
            .recover_release(
                fixture.root.path(),
                header,
                RecoveryProfile::LocalTransactions,
                session(options()),
            )
            .expect("verified local-only history");
        assert_eq!(
            recovered.inspect().expect("held exact generation").header,
            header
        );
        let history = recovered
            .inspect_release_reconciliations(&fixture.database(), "first")
            .expect("retained native history");
        assert_eq!(history.history.attempts, original.attempts);
        assert_eq!(history.history.consumption, Some(consumed));
        assert_eq!(history.history.attempts[0].result.is_some(), completed);
        recovered.close().expect("no clean seal or replay");
        assert_eq!(
            fs::read(fixture.database()).expect("unchanged history"),
            bytes
        );
        assert_eq!(
            fs::read(fixture.anchor.path().join("coordinator.json")).expect("still interrupted"),
            anchor
        );
    }
}

#[test]
fn mixed_profile_history_and_same_generation_rollback_refuse_local_recovery() {
    use gazette_core::store::RecoveryProfile;
    let (fixture, mut writer, consumed) = held();
    let local = local_start(&consumed, 0, 1);
    writer
        .start_release_attempt(&fixture.database(), &local, permission())
        .expect("local start");
    let older = fs::read(fixture.database()).expect("older same-generation local history");
    let receive_pack = start(&consumed, 1, 1);
    writer
        .start_release_attempt(&fixture.database(), &receive_pack, permission())
        .expect("other target receive-pack");
    writer
        .finish_release_attempt(&rejected(&receive_pack), permission())
        .expect("completed receive-pack");
    let header = writer.inspect().expect("actual header").header;
    drop(writer);
    let coordinator = LocalCoordinator::new(
        fixture.anchor.path().to_owned(),
        fixture.binding.clone(),
        true,
    );
    assert!(matches!(
        coordinator.recover_release(
            fixture.root.path(),
            header,
            RecoveryProfile::LocalTransactions,
            session(options())
        ),
        Err(Error::ReleaseQuiescenceRequired)
    ));
    fs::write(fixture.database(), older).expect("fixture rollback retains same file identity");
    assert!(matches!(
        coordinator.recover_release(
            fixture.root.path(),
            header,
            RecoveryProfile::LocalTransactions,
            session(options())
        ),
        Err(Error::CoordinatorMismatch)
    ));
}

#[test]
fn local_profile_result_and_retry_contracts_are_enforced_by_native_history() {
    for invalid_exit in [false, true] {
        let (fixture, mut writer, consumed) = held();
        let first = local_start(&consumed, 0, 1);
        writer
            .start_release_attempt(&fixture.database(), &first, permission())
            .expect("local start");
        let mut invalid = if invalid_exit {
            local_rejection(&first)
        } else {
            rejected(&first)
        };
        if invalid_exit {
            invalid.transport.as_mut().expect("observation").exit_code = Some(0);
        }
        assert!(
            writer
                .finish_release_attempt(&invalid, permission())
                .is_err(),
            "local result must describe no spawned process"
        );
        let history = Sqlite
            .inspect_release_attempts(&fixture.database(), "first", options())
            .expect("refusal retains incomplete history");
        assert!(history.attempts[0].result.is_none());
    }
    let (fixture, mut writer, consumed) = held();
    let first = local_start(&consumed, 0, 1);
    writer
        .start_release_attempt(&fixture.database(), &first, permission())
        .expect("local start");
    writer
        .finish_release_attempt(&local_rejection(&first), permission())
        .expect("validated local rejection");
    let history = Sqlite
        .inspect_release_attempts(&fixture.database(), "first", options())
        .expect("native completed history");
    assert!(
        start(&consumed, 0, 2)
            .follows(history.attempts.first())
            .is_err(),
        "retry cannot change profile"
    );
    writer
        .start_release_attempt(
            &fixture.database(),
            &local_start(&consumed, 0, 2),
            permission(),
        )
        .expect("same-holder exact-prestate local retry");
    let history = Sqlite
        .inspect_release_attempts(&fixture.database(), "first", options())
        .expect("validated native history");
    assert_eq!(history.attempts.len(), 2);
    assert!(history.attempts[1].result.is_none());
}

#[test]
fn local_recovery_gate_verifies_native_start_hash_instead_of_trusting_profile_column() {
    let (fixture, mut writer, consumed) = held();
    writer
        .start_release_attempt(
            &fixture.database(),
            &local_start(&consumed, 0, 1),
            permission(),
        )
        .expect("native start");
    drop(writer);
    let connection = Connection::open(fixture.database()).expect("fixture native reader");
    crate::attempt::require_recovery(
        &connection,
        gazette_core::store::RecoveryProfile::LocalTransactions,
        options(),
    )
    .expect("complete valid history");
    connection
        .execute(
            "UPDATE gazette_release_attempt_starts SET payload_sha256=zeroblob(32)",
            [],
        )
        .expect("deliberate native corruption");
    assert!(matches!(
        crate::attempt::require_recovery(
            &connection,
            gazette_core::store::RecoveryProfile::LocalTransactions,
            options()
        ),
        Err(Error::ReleaseAttemptVerification)
    ));
}

fn permission() -> ReleaseAttemptOptions {
    ReleaseAttemptOptions { allow_record: true }
}

fn held() -> (Fixture, crate::Writer, ReleaseConsumption) {
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
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
    (fixture, writer, consumed)
}

fn start(consumed: &ReleaseConsumption, ordinal: u32, number: u64) -> ReleaseAttemptStart {
    ReleaseAttemptStart {
        profile: gazette_core::release::DispatchProfile::ReceivePack,
        key: ReleaseAttemptKey {
            authorization_id: consumed.authorization_id.clone(),
            ordinal,
            number,
        },
        binding: consumed.binding.clone(),
        prestate: consumed.readbacks[ordinal as usize].readback.clone(),
        created: consumed.created,
    }
}

fn rejected(start: &ReleaseAttemptStart) -> ReleaseAttemptResult {
    ReleaseAttemptResult {
        key: start.key.clone(),
        transport: Ok(DispatchObservation {
            status: DispatchStatus::Rejected,
            exit_code: Some(1),
            response_sha256: Sha256Digest::of(b"representation-only native rejection"),
            cleanup: Cleanup::Reaped,
        }),
        readback: start.prestate.clone(),
        created: start.created,
    }
}

pub(super) fn retained_fixture(
    writer: &mut crate::Writer,
    path: &Path,
    consumed: &ReleaseConsumption,
) -> Vec<gazette_core::store::ReleaseAttempt> {
    let first = start(consumed, 0, 1);
    writer
        .start_release_attempt(path, &first, permission())
        .expect("durable complete target start");
    let complete = writer
        .finish_release_attempt(&rejected(&first), permission())
        .expect("durable result");
    let incomplete = writer
        .start_release_attempt(path, &start(consumed, 1, 1), permission())
        .expect("durable incomplete target start");
    settlements::retained_fixture(writer, path, consumed);
    vec![complete, incomplete.record]
}

#[test]
fn incomplete_start_exact_replay_result_and_safe_retry_reopen_as_original_history() {
    let (fixture, mut writer, consumed) = held();
    let path = fixture.database();
    let first = start(&consumed, 0, 1);
    let receipt = writer
        .start_release_attempt(&path, &first, permission())
        .expect("durable start");
    assert!(receipt.created);
    assert_eq!(receipt.record.start, first);
    assert_eq!(receipt.record.result, None);
    let before = fs::read(&path).expect("before replay");
    let replay = writer
        .start_release_attempt(&path, &first, permission())
        .expect("same start is history only");
    assert!(!replay.created);
    assert_eq!(replay.record, receipt.record);
    assert_eq!(fs::read(&path).expect("no replay mutation"), before);
    assert_eq!(
        Sqlite
            .inspect_release_attempts(&path, "first", options())
            .expect("incomplete inspection")
            .attempts,
        vec![receipt.record]
    );
    let result = rejected(&first);
    let complete = writer
        .finish_release_attempt(&result, permission())
        .expect("retain actual result representation");
    let before = fs::read(&path).expect("before result replay");
    assert_eq!(
        writer
            .finish_release_attempt(&result, permission())
            .expect("exact result replay"),
        complete
    );
    let replay = writer
        .start_release_attempt(&path, &first, permission())
        .expect("start replay retains completed result");
    assert!(!replay.created);
    assert_eq!(replay.record, complete);
    assert_eq!(fs::read(&path).expect("result replay unchanged"), before);
    let retry = writer
        .start_release_attempt(&path, &start(&consumed, 0, 2), permission())
        .expect("reaped rejection and exact prestate permit bounded retry");
    assert!(retry.created);
    let other = writer
        .start_release_attempt(&path, &start(&consumed, 1, 1), permission())
        .expect("other target independent sequence");
    writer.close().expect("seal exact history");
    let before = fs::read(&path).expect("closed history bytes");
    let history = Sqlite
        .inspect_release_attempts(&path, "first", options())
        .expect("read without authority lookup");
    assert_eq!(history.consumption, Some(consumed));
    assert_eq!(history.attempts, vec![complete, retry.record, other.record]);
    assert_eq!(fs::read(&path).expect("read-only bytes"), before);
}

#[test]
fn unresolved_or_successful_attempt_never_becomes_an_implicit_retry() {
    for mode in [
        "missing",
        "uncertain",
        "reported-success",
        "observed-success",
    ] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("start");
        if mode != "missing" {
            let mut result = rejected(&first);
            match mode {
                "uncertain" => {
                    result.transport = Err(DispatchError {
                        failure: DispatchFailure::Timeout,
                        effect: DispatchEffect::Unverified,
                        cleanup: Cleanup::Reaped,
                    })
                }
                "reported-success" => {
                    result.transport.as_mut().expect("observation").status = DispatchStatus::Updated
                }
                _ => {
                    let advertisement = RefAdvertisement::Commit {
                        oid: consumed.targets[0].target.local_commit_oid.clone(),
                    };
                    result.readback = TargetReadback::Observed {
                        comparison: RefComparison::compare(
                            &consumed.targets[0].target,
                            &advertisement,
                        ),
                        advertisement,
                    };
                }
            }
            writer
                .finish_release_attempt(&result, permission())
                .expect("retain result, not permission");
        }
        let before = fs::read(&path).expect("unresolved bytes");
        let error = writer
            .start_release_attempt(&path, &start(&consumed, 0, 2), permission())
            .expect_err("explicit reconciliation required");
        assert!(
            matches!(error.cause, Error::ReleaseReconciliationRequired),
            "{mode}: {error:?}"
        );
        assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted);
        drop(writer);
        assert_eq!(
            Sqlite
                .inspect_release_attempts(&path, "first", options())
                .expect("original history survives")
                .attempts
                .len(),
            1
        );
        assert_eq!(fs::read(&path).expect("no second start"), before);
    }
}

#[test]
fn changed_start_result_missing_start_and_sequence_gaps_preserve_original_bytes() {
    for mode in [
        "start",
        "result",
        "missing",
        "gap",
        "ordinal",
        "permission",
        "database",
    ] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("first");
        let result = rejected(&first);
        writer
            .finish_release_attempt(&result, permission())
            .expect("result");
        let before = fs::read(&path).expect("before refusal");
        let error = match mode {
            "result" | "missing" => {
                let mut changed = result.clone();
                if mode == "result" {
                    changed.transport = Err(DispatchError::refused(DispatchFailure::Permission));
                } else {
                    changed.key.ordinal = 1;
                }
                writer
                    .finish_release_attempt(&changed, permission())
                    .expect_err("wrong result")
                    .cause
            }
            _ => {
                let mut changed = first.clone();
                let mut selected = path.clone();
                let mut granted = permission();
                match mode {
                    "start" => changed.binding.as_signed_mut().unwrap().observed_at += 1,
                    "gap" => changed.key.number = 3,
                    "ordinal" => changed.key.ordinal = 99,
                    "permission" => granted.allow_record = false,
                    "database" => selected = fixture.anchor.path().join("store.sqlite"),
                    _ => unreachable!(),
                }
                writer
                    .start_release_attempt(&selected, &changed, granted)
                    .expect_err("wrong start")
                    .cause
            }
        };
        assert!(!matches!(error, Error::StorageLimit), "{mode}");
        drop(writer);
        assert_eq!(
            Sqlite
                .inspect_release_attempts(&path, "first", options())
                .expect("retained history")
                .attempts
                .len(),
            1
        );
        assert_eq!(
            fs::read(&path).expect("refusal unchanged"),
            before,
            "{mode}"
        );
    }
}

#[test]
fn ordinary_or_newer_anchored_writer_cannot_adopt_consumed_attempts() {
    for anchored in [false, true] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("original start");
        writer.close().expect("original clean close");
        let mut next = if anchored {
            fixture.writer(consumed.created, options())
        } else {
            Sqlite
                .acquire_writer(
                    fixture.root.path(),
                    consumed.created,
                    writer_options(options()),
                )
                .expect("ordinary native writer")
        };
        let error = next
            .start_release_attempt(&path, &first, permission())
            .expect_err("no adoption");
        assert!(matches!(error.cause, Error::CoordinatorMismatch));
        drop(next);
        let inspected = Sqlite
            .inspect_release_attempts(&path, "first", options())
            .expect("historical inspection remains possible");
        assert_eq!(inspected.attempts.len(), 1);
        assert_eq!(inspected.attempts[0].start, first);
    }
}

#[test]
fn result_foreign_key_requires_an_exact_existing_start() {
    let (fixture, writer, consumed) = held();
    let path = fixture.database();
    writer.close().expect("close fixture holder");
    let mut connection = Connection::open(&path).expect("fixture connection");
    connection
        .execute_batch("PRAGMA foreign_keys=ON")
        .expect("native FK gate");
    let transaction = connection.transaction().expect("atomic append fixture");
    let first = start(&consumed, 0, 1);
    crate::attempt::insert_start(&transaction, &first, options()).expect("valid target FK");
    let mut wrong = rejected(&first);
    wrong.key.ordinal = 1;
    assert!(crate::attempt::insert_result(&transaction, &wrong, options()).is_err());
    drop(transaction);
    connection.close().expect("close rolled back transaction");
    assert!(
        Sqlite
            .inspect_release_attempts(&path, "first", options())
            .expect("native rolled back history")
            .attempts
            .is_empty()
    );
}

#[test]
fn oversized_input_refuses_before_parsing_and_committed_undelivered_start_survives() {
    use gazette_core::release::{KeyState, PublisherBinding, PublisherRole};
    let fixture = Fixture::new();
    let path = fixture.database();
    let initial_size = fs::metadata(&path).expect("native fresh size").len();
    let ceiling = initial_size + 128 * 1024;
    let bounded = ReadOptions {
        storage_bytes: ceiling,
        ..options()
    };
    let mut writer = fixture.writer(fixture.initial, bounded);
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
        .expect("consumed within bound")
        .record;
    let mut selected = start(&consumed, 0, 1);
    let publisher = |length| PublisherBinding {
        publisher_id: "fixture".into(),
        publisher_key_id: "fixture".into(),
        publisher_key_epoch: 1,
        authority_reference: Sha256Digest::of(b"fixture"),
        role: PublisherRole::Publisher,
        authorized_target_ordinals: vec![0, 1],
        public_key: "x".repeat(length),
        state: KeyState::Active,
    };
    // Representation-only adapter stress: the application separately verifies keys/signatures.
    selected
        .binding
        .as_signed_mut()
        .unwrap()
        .publishers
        .push(publisher(
            usize::try_from(ceiling).expect("bounded fixture"),
        ));
    let before = fs::read(&path).expect("before oversize refusal");
    let error = writer
        .start_release_attempt(&path, &selected, permission())
        .expect_err("bound before cloning or parsing");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted);
    assert_eq!(fs::read(&path).expect("oversize unchanged"), before);
    let current_size = fs::metadata(&path).expect("current native size").len();
    selected.binding.as_signed_mut().unwrap().publishers = vec![publisher(
        usize::try_from(ceiling - current_size + 32 * 1024).expect("remaining native allocation"),
    )];
    crate::consumption::encode(&selected, bounded).expect("record itself fits selected bound");
    let error = writer
        .start_release_attempt(&path, &selected, permission())
        .expect_err("postcommit independent delivery cannot fit native store ceiling");
    assert!(matches!(error.cause, Error::StorageLimit), "{error:?}");
    assert_eq!(error.effect, ReleaseWriteEffect::Unverified);
    assert!(
        writer.close().is_err(),
        "uncertain write cannot seal custody"
    );
    let history = Sqlite
        .inspect_release_attempts(&path, "first", options())
        .expect("wider-bound historical inspection");
    assert_eq!(history.attempts.len(), 1);
    assert_eq!(history.attempts[0].start, selected);
    assert_eq!(history.attempts[0].result, None);
}

#[test]
fn corrupted_attempt_hashes_numbering_or_result_links_are_not_repaired() {
    for mutation in ["hash", "gap", "result", "unknown"] {
        let (fixture, mut writer, consumed) = held();
        let path = fixture.database();
        let first = start(&consumed, 0, 1);
        writer
            .start_release_attempt(&path, &first, permission())
            .expect("first");
        writer
            .finish_release_attempt(&rejected(&first), permission())
            .expect("result");
        writer.close().expect("seal original");
        let connection = Connection::open(&path).expect("deliberate corruptor");
        connection
            .execute_batch("PRAGMA foreign_keys=OFF")
            .expect("corruptor");
        match mutation {
            "hash" => {
                connection
                    .execute(
                        "UPDATE gazette_release_attempt_starts SET payload_sha256=zeroblob(32)",
                        [],
                    )
                    .expect("wrong domain/hash");
            }
            "gap" => {
                connection
                    .execute("UPDATE gazette_release_attempt_starts SET number=2", [])
                    .expect("wrong ordering");
            }
            "result" => {
                connection
                    .execute("UPDATE gazette_release_attempt_results SET ordinal=1", [])
                    .expect("orphan result");
            }
            _ => {
                connection
                    .execute(
                        "UPDATE gazette_release_attempt_starts SET payload=x'7b7d'",
                        [],
                    )
                    .expect("invalid complete schema");
            }
        }
        connection.close().expect("close corruptor");
        let before = fs::read(&path).expect("corrupt history");
        assert!(
            Sqlite
                .inspect_release_attempts(&path, "first", options())
                .is_err(),
            "{mutation}"
        );
        assert_eq!(fs::read(&path).expect("no repair"), before);
    }
}
