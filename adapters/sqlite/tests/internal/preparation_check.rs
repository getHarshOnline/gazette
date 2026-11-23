//! Real append, replay and independent recovery of exact private checker bytes.

use super::*;
use gazette_core::store::PreparationCheckInput;

const EVIDENCE: &str = "{\n  \"output_check\": \"PRIVATE native evidence\", \"exit\": 0\n}\n";

fn input<'a>(record: &'a PreparationRecord, evidence: &'a str) -> PreparationCheckInput<'a> {
    PreparationCheckInput {
        intent: &record.intent,
        retention: record.retention.as_ref().expect("retained input"),
        evidence,
        evidence_sha256: Sha256Digest::of(evidence.as_bytes()),
    }
}

#[test]
fn exact_check_survives_reopen_and_replay_after_worktree_completion() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let retained = retained(&mut writer, &data);
    let checked = writer
        .record_preparation_check(input(&retained, EVIDENCE), permission())
        .expect("exact check appended after retention");
    assert!(checked.inserted);
    let check = checked.record.check.as_ref().expect("stored check");
    assert_eq!(check.evidence, EVIDENCE);
    assert_eq!(check.evidence_sha256, Sha256Digest::of(EVIDENCE.as_bytes()));
    assert_eq!(check.recorded, checked.journal);
    let metadata = metadata();
    let started = writer
        .record_preparation_worktree(worktree_input(&checked.record, &metadata), permission())
        .expect("checked Git intent");
    let report = report(&started.record);
    let complete = writer
        .record_preparation_commit(completion_input(&started.record, &report), permission())
        .expect("checked completion");
    writer.close().expect("close first lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    assert_eq!(
        Sqlite
            .inspect_preparation(&path, data.input().id, options())
            .expect("independent read")
            .record,
        Some(complete.record.clone())
    );
    let mut writer = Sqlite
        .acquire_writer(root.path(), complete.journal, writer_options(options()))
        .expect("new generation");
    let before = fs::read(&path).expect("advanced journal bytes");
    let replay = writer
        .record_preparation_check(input(&retained, EVIDENCE), permission())
        .expect("exact replay after completion");
    assert!(!replay.inserted);
    assert_eq!(replay.record, complete.record);
    assert!(replay.journal.writer_fence > check.recorded.writer_fence);
    assert_eq!(fs::read(&path).expect("no replay rewrite"), before);
    writer.close().expect("close replay lifetime");
}

#[test]
fn changed_check_or_original_coordinates_conflict_without_replacement() {
    for change in 0..3 {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let checked = writer
            .record_preparation_check(input(&retained, EVIDENCE), permission())
            .expect("original check");
        let mut changed = retained.clone();
        if change == 1 {
            changed.intent.created.writer_fence += 1;
        }
        if change == 2 {
            changed
                .retention
                .as_mut()
                .expect("retention")
                .recorded
                .writer_fence += 1;
        }
        let evidence = if change == 0 {
            "{\"exit\":0}"
        } else {
            EVIDENCE
        };
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("original bytes");
        let error = writer
            .record_preparation_check(input(&changed, evidence), permission())
            .expect_err("exact original binding required");
        assert!(matches!(error.cause, Error::PreparationConflict));
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        assert!(
            matches!(writer.inspect(), Err(Error::Changed)),
            "failed transaction invalidates writer"
        );
        assert_eq!(fs::read(&path).expect("no replacement"), before);
        assert_eq!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .expect("independent preserved history")
                .record,
            Some(checked.record)
        );
    }
}

#[test]
fn cannot_add_check_retroactively_or_omit_it_from_worktree_or_completion() {
    for phase in 0..6 {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let record = if phase == 0 {
            retained
        } else {
            writer
                .record_preparation_check(input(&retained, EVIDENCE), permission())
                .expect("check")
                .record
        };
        let metadata = metadata();
        let record = if phase != 1 {
            writer
                .record_preparation_worktree(worktree_input(&record, &metadata), permission())
                .expect("initial Git intent")
                .record
        } else {
            record
        };
        let report = report(&record);
        let record = if phase == 5 {
            writer
                .record_preparation_commit(completion_input(&record, &report), permission())
                .expect("original checked completion")
                .record
        } else {
            record
        };
        let changed = record.check.clone().map(|mut check| {
            check.recorded.writer_fence += 1;
            check
        });
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("prior history");
        let error = match phase {
            0 => writer.record_preparation_check(input(&record, EVIDENCE), permission()),
            1 | 2 => writer.record_preparation_worktree(
                PreparationWorktreeInput {
                    check: None,
                    ..worktree_input(&record, &metadata)
                },
                permission(),
            ),
            3 => writer.record_preparation_commit(
                PreparationCommitInput {
                    check: None,
                    ..completion_input(&record, &report)
                },
                permission(),
            ),
            4 => writer.record_preparation_worktree(
                PreparationWorktreeInput {
                    check: changed.as_ref(),
                    ..worktree_input(&record, &metadata)
                },
                permission(),
            ),
            _ => writer.record_preparation_commit(
                PreparationCommitInput {
                    check: changed.as_ref(),
                    ..completion_input(&record, &report)
                },
                permission(),
            ),
        }
        .expect_err("exact optional check must match history before append or replay");
        if phase == 3 {
            assert!(
                matches!(error.cause, Error::PreparationVerification),
                "completion cannot omit its recipe's check reference"
            );
        } else {
            assert!(matches!(error.cause, Error::PreparationConflict));
        }
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        assert_eq!(
            fs::read(&path).expect("no retroactive or omitted binding"),
            before
        );
        assert_eq!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .expect("original retained history")
                .record,
            Some(record)
        );
    }
}

#[test]
fn native_constraint_failure_rolls_back_check_row_before_independent_readback() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let retained = retained(&mut writer, &data);
    let header = writer.inspect().expect("current generation").header;
    writer
        .close()
        .expect("release writer before fixture native transaction");
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("retention bytes");
    let mut connection = Connection::open(&path).expect("disposable transaction connection");
    let transaction = connection.transaction().expect("real transaction");
    crate::preparation::check::insert(&transaction, input(&retained, EVIDENCE), header)
        .expect("first native row");
    assert!(
        crate::preparation::check::insert(&transaction, input(&retained, EVIDENCE), header)
            .is_err(),
        "immutable primary key refuses a second row"
    );
    transaction.rollback().expect("rollback unsuccessful batch");
    connection.close().expect("close native fixture");
    assert_eq!(
        fs::read(&path).expect("rollback preserves exact journal bytes"),
        before
    );
    assert_eq!(
        Sqlite
            .inspect_preparation(&path, data.input().id, options())
            .expect("independent absence after rollback")
            .record,
        Some(retained)
    );
}

#[test]
fn check_preflight_enforces_permission_bounds_json_digest_and_retention() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let retained = retained(&mut writer, &data);
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("before pure validation");
    let denied = writer
        .record_preparation_check(
            input(&retained, EVIDENCE),
            PreparationWriteOptions {
                allow_record: false,
            },
        )
        .expect_err("explicit effect permission");
    assert_eq!(denied.effect, PreparationEffect::NotAttempted);
    for evidence in ["not JSON", "{\"a\":1,\"\\u0061\":2}"] {
        assert!(matches!(
            writer
                .record_preparation_check(input(&retained, evidence), permission())
                .expect_err("strict JSON")
                .cause,
            Error::PreparationVerification
        ));
    }
    let mut wrong = input(&retained, EVIDENCE);
    wrong.evidence_sha256 = Sha256Digest::of(b"different bytes");
    assert!(matches!(
        writer
            .record_preparation_check(wrong, permission())
            .expect_err("exact digest")
            .cause,
        Error::PreparationVerification
    ));
    let oversized = format!(
        "\"{}\"",
        "x".repeat(usize::try_from(options().value_bytes).expect("fixture bound"))
    );
    assert!(matches!(
        writer
            .record_preparation_check(input(&retained, &oversized), permission())
            .expect_err("record bound")
            .cause,
        Error::StorageLimit
    ));
    assert_eq!(
        fs::read(&path).expect("pure refusals preserve bytes"),
        before
    );
    writer
        .inspect()
        .expect("pure refusals do not invalidate writer");
    writer.close().expect("close validation lifetime");

    let (root, mut writer) = fixture();
    let first = writer
        .record_preparation_intent(data.input(), permission())
        .expect("intent only");
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("intent without retention");
    let mut claimed = retained;
    claimed.intent = first.record.intent.clone();
    let failure = writer
        .record_preparation_check(input(&claimed, EVIDENCE), permission())
        .expect_err("supplied retention is not durable retention");
    assert!(matches!(failure.cause, Error::PreparationConflict));
    assert_eq!(fs::read(&path).expect("no partial check"), before);
    assert_eq!(
        Sqlite
            .inspect_preparation(&path, data.input().id, options())
            .expect("read absent optional check")
            .record,
        Some(first.record)
    );
}

#[test]
fn corrupted_check_bytes_prerequisites_and_generation_order_fail_without_repair() {
    for corruption in 0..9 {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let checked = writer
            .record_preparation_check(input(&retained, EVIDENCE), permission())
            .expect("valid check");
        let metadata = metadata();
        let worktree = writer
            .record_preparation_worktree(worktree_input(&checked.record, &metadata), permission())
            .expect("valid checked intent");
        writer
            .close()
            .expect("close before fixture-only corruption");
        // Advance so a check placed after the original worktree is within the
        // current header, but still violates the cross-record history order.
        let writer = Sqlite
            .acquire_writer(root.path(), worktree.journal, writer_options(options()))
            .expect("next generation");
        writer.close().expect("close advanced generation");
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("disposable corruption connection");
        match corruption {
            0 => {
                connection
                    .execute("UPDATE gazette_preparation_checks SET evidence='{}'", [])
                    .expect("change evidence only");
            }
            1 => {
                let malformed = "{\"x\":1,\"\\u0078\":2}";
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .expect("fixture-only coordinated corruption");
                connection
                    .execute(
                        "UPDATE gazette_preparation_checks SET evidence=?1,evidence_sha256=?2",
                        params![malformed, Sha256Digest::of(malformed.as_bytes()).as_bytes()],
                    )
                    .expect("digest-correct duplicate JSON");
                connection
                    .execute(
                        "UPDATE gazette_preparation_worktrees SET check_sha256=?1",
                        [Sha256Digest::of(malformed.as_bytes()).as_bytes()],
                    )
                    .expect("matching reference still cannot qualify invalid JSON");
                let violations: i64 = connection
                    .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get(0)
                    })
                    .expect("intact native reference");
                assert_eq!(violations, 0);
            }
            2 => {
                connection
                    .execute(
                        "UPDATE gazette_preparation_checks SET recorded_journal=zeroblob(32)",
                        [],
                    )
                    .expect("wrong journal identity");
            }
            3 => {
                connection
                    .execute(
                        "UPDATE gazette_preparation_checks SET recorded_fence=recorded_fence+1",
                        [],
                    )
                    .expect("check newer than worktree");
            }
            4 => {
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .expect("fixture-only FK bypass");
                connection
                    .execute("DELETE FROM gazette_preparation_retentions", [])
                    .expect("orphan real check");
                let violations: i64 = connection
                    .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get(0)
                    })
                    .expect("actual prerequisite violation");
                assert!(violations > 0);
            }
            5 => {
                connection
                    .execute(
                        "UPDATE gazette_preparation_retentions SET recorded_fence=recorded_fence+1",
                        [],
                    )
                    .expect("check older than retention");
            }
            6 => {
                assert!(
                    connection
                        .execute("DELETE FROM gazette_preparation_checks", [])
                        .is_err(),
                    "native FK prevents loss of referenced evidence"
                );
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .expect("fixture-only check loss");
                connection
                    .execute("DELETE FROM gazette_preparation_checks", [])
                    .expect("model externally lost evidence");
            }
            7 => {
                connection
                    .execute(
                        "UPDATE gazette_preparation_worktrees SET check_sha256=NULL",
                        [],
                    )
                    .expect("null reference corruption");
            }
            _ => {
                assert!(
                    connection
                        .execute(
                            "UPDATE gazette_preparation_worktrees SET check_sha256=zeroblob(32)",
                            []
                        )
                        .is_err(),
                    "native FK prevents substitution"
                );
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .expect("fixture-only bad reference");
                connection
                    .execute(
                        "UPDATE gazette_preparation_worktrees SET check_sha256=zeroblob(32)",
                        [],
                    )
                    .expect("model mismatched reference");
            }
        }
        connection.close().expect("close corruption fixture");
        let before = fs::read(&path).expect("corrupted bytes");
        assert!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .is_err(),
            "corruption {corruption}"
        );
        assert_eq!(
            fs::read(&path).expect("read-only refusal does not repair"),
            before
        );
    }
}
