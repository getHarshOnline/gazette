//! Real immutable Git-intent/completion transactions over the existing journal.

use super::*;
use gazette_core::{
    release::{CommitMetadata, CommitReport, CommitSignature, ParentSource},
    store::{PreparationCommitInput, PreparationRecord, PreparationWorktreeInput},
};

#[path = "preparation_check.rs"]
mod check;

fn metadata() -> CommitMetadata {
    let signature = CommitSignature {
        name: "Explicit author".into(),
        email: "fixture@example.invalid".into(),
        seconds: 10,
        offset_minutes: 330,
    };
    CommitMetadata {
        parent: None,
        author: signature.clone(),
        committer: signature,
        message: "Exact recipe\n".into(),
    }
}

fn retained(writer: &mut crate::Writer, data: &Data) -> PreparationRecord {
    let intent = writer
        .record_preparation_intent(data.input(), permission())
        .expect("stage intent");
    writer
        .record_preparation_retention(retention(&intent.record.intent), permission())
        .expect("durable stage retention")
        .record
}

fn worktree_input<'a>(
    record: &'a PreparationRecord,
    metadata: &'a CommitMetadata,
) -> PreparationWorktreeInput<'a> {
    PreparationWorktreeInput {
        intent: &record.intent,
        retention: record.retention.as_ref().expect("retention fixture"),
        check: record.check.as_ref(),
        metadata,
        parent_source: None,
    }
}

fn report(record: &PreparationRecord) -> CommitReport {
    CommitReport {
        commit: "1111111111111111111111111111111111111111"
            .parse()
            .expect("complete native coordinate"),
        output: record.intent.output,
        parent: None,
        entries: usize::try_from(record.intent.entries).expect("fixture entries"),
        bytes: record.intent.bytes,
    }
}

fn completion_input<'a>(
    record: &'a PreparationRecord,
    report: &'a CommitReport,
) -> PreparationCommitInput<'a> {
    PreparationCommitInput {
        intent: &record.intent,
        retention: record.retention.as_ref().expect("retention fixture"),
        check: record.check.as_ref(),
        worktree: record.worktree.as_ref().expect("worktree intent fixture"),
        report,
    }
}

#[test]
fn ordered_intent_completion_reopen_and_old_snapshot_replay_preserve_coordinates() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let retained = retained(&mut writer, &data);
    let metadata = metadata();
    let first = writer
        .record_preparation_worktree(worktree_input(&retained, &metadata), permission())
        .expect("Git pre-write intent");
    assert!(first.inserted);
    assert!(
        first
            .record
            .worktree
            .as_ref()
            .expect("stored intent")
            .completion
            .is_none()
    );
    let report = report(&first.record);
    let complete = writer
        .record_preparation_commit(completion_input(&first.record, &report), permission())
        .expect("verified native completion");
    assert!(complete.inserted);
    let historical = complete.record.worktree.as_ref().expect("worktree history");
    assert_eq!(historical.created, first.journal);
    assert_eq!(
        historical.completion.as_ref().expect("completed").recorded,
        complete.journal
    );
    writer.close().expect("close first lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    assert_eq!(
        Sqlite
            .inspect_preparation(&path, data.input().id, options())
            .expect("read-only history")
            .record,
        Some(complete.record.clone())
    );
    let mut writer = Sqlite
        .acquire_writer(root.path(), complete.journal, writer_options(options()))
        .expect("new lifetime");
    let before = fs::read(&path).expect("advanced generation bytes");
    let replay = writer
        .record_preparation_worktree(worktree_input(&retained, &metadata), permission())
        .expect("exact recipe replay");
    assert!(!replay.inserted);
    assert!(replay.journal.writer_fence > historical.created.writer_fence);
    assert_eq!(replay.record, complete.record);
    let replay = writer
        .record_preparation_commit(completion_input(&first.record, &report), permission())
        .expect("original pre-completion snapshot replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, complete.record);
    let replay = writer
        .record_preparation_commit(completion_input(&complete.record, &report), permission())
        .expect("current completion snapshot replay");
    assert!(!replay.inserted);
    assert_eq!(
        fs::read(&path).expect("replays do not replace records"),
        before
    );
    writer.close().expect("close replay lifetime");
}

#[test]
fn missing_or_changed_stage_and_retention_cannot_authorize_git_intent() {
    for change in [
        "absent-stage",
        "absent-retention",
        "intent",
        "retention",
        "recipe-conflict",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let mut record = retained(&mut writer, &data);
        let mut metadata = metadata();
        if change == "recipe-conflict" {
            writer
                .record_preparation_worktree(worktree_input(&record, &metadata), permission())
                .expect("initial recipe");
            metadata.message.push_str("changed");
        }
        if change == "absent-stage" {
            record.intent.id = Sha256Digest::of(b"missing");
            record.retention.as_mut().expect("retention").intent_id = record.intent.id;
        }
        if change == "absent-retention" {
            let mut input = data.input();
            input.id = Sha256Digest::of(b"stage without retention");
            let absent = writer
                .record_preparation_intent(input, permission())
                .expect("stage-only intent");
            record.intent = absent.record.intent;
            record
                .retention
                .as_mut()
                .expect("supplied but unstored retention")
                .intent_id = record.intent.id;
        }
        if change == "intent" {
            record.intent.created.writer_fence += 1;
        }
        if change == "retention" {
            record
                .retention
                .as_mut()
                .expect("retention")
                .recorded
                .writer_fence += 1;
        }
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("exact prior records");
        let error = writer
            .record_preparation_worktree(worktree_input(&record, &metadata), permission())
            .expect_err("no inferred or replaced prerequisite");
        assert!(
            matches!(
                error.cause,
                Error::PreparationConflict | Error::PreparationVerification
            ),
            "{change}"
        );
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        drop(writer);
        assert_eq!(fs::read(path).expect("no record mutation"), before);
    }
}

#[test]
fn completion_requires_exact_git_intent_and_complete_matching_result() {
    for change in [
        "missing",
        "created",
        "metadata",
        "output",
        "entries",
        "bytes",
        "parent",
        "zero",
        "changed-completion",
        "claimed-completion",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let metadata = metadata();
        let mut record = writer
            .record_preparation_worktree(worktree_input(&retained, &metadata), permission())
            .expect("original recipe")
            .record;
        let mut report = report(&record);
        if change == "changed-completion" || change == "claimed-completion" {
            record = writer
                .record_preparation_commit(completion_input(&record, &report), permission())
                .expect("original completion")
                .record;
        }
        match change {
            "missing" => {
                let mut input = data.input(); input.id = Sha256Digest::of(b"other stage");
                let stage = writer.record_preparation_intent(input, permission()).expect("other stage");
                let stage = writer.record_preparation_retention(retention(&stage.record.intent), permission()).expect("other retained stage");
                record.intent = stage.record.intent; record.retention = stage.record.retention;
            }
            "created" => record.worktree.as_mut().expect("worktree").created.writer_fence += 1,
            "metadata" => record.worktree.as_mut().expect("worktree").metadata.message.push('x'),
            "output" => report.output = serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"wrong output")})).expect("different output"),
            "entries" => report.entries += 1,
            "bytes" => report.bytes += 1,
            "parent" => report.parent = Some("2222222222222222222222222222222222222222".parse().expect("parent")),
            "zero" => report.commit = "0000000000000000000000000000000000000000".parse().expect("typed zero is not admitted"),
            "changed-completion" => report.commit = "2222222222222222222222222222222222222222".parse().expect("changed result"),
            "claimed-completion" => record.worktree.as_mut().expect("worktree").completion.as_mut().expect("completion").recorded.writer_fence += 1,
            _ => (),
        }
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("exact prior records");
        let error = writer
            .record_preparation_commit(completion_input(&record, &report), permission())
            .expect_err("complete exact prerequisite/result binding");
        assert!(
            matches!(
                error.cause,
                Error::PreparationConflict | Error::PreparationVerification
            ),
            "{change}"
        );
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        drop(writer);
        assert_eq!(fs::read(path).expect("no replacement"), before);
    }
}

#[test]
fn worktree_permissions_and_recipe_bounds_precede_insertions() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let retained = retained(&mut writer, &data);
    let metadata = metadata();
    assert!(matches!(
        writer
            .record_preparation_worktree(worktree_input(&retained, &metadata), Default::default())
            .expect_err("explicit intent permission")
            .cause,
        Error::PreparationPermission
    ));
    let record = writer
        .record_preparation_worktree(worktree_input(&retained, &metadata), permission())
        .expect("allowed intent")
        .record;
    assert!(matches!(
        writer
            .record_preparation_commit(
                completion_input(&record, &report(&record)),
                Default::default()
            )
            .expect_err("explicit completion permission")
            .cause,
        Error::PreparationPermission
    ));
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("intent bytes");
    let mut huge = metadata.clone();
    huge.message = "x".repeat(usize::try_from(options().value_bytes).expect("native value limit"));
    assert!(matches!(
        writer
            .record_preparation_worktree(worktree_input(&retained, &huge), permission())
            .expect_err("bounded recipe")
            .cause,
        Error::StorageLimit
    ));
    assert_eq!(fs::read(path).expect("no insertion"), before);
}

#[test]
fn corrupted_completion_and_historical_coordinates_fail_without_repair() {
    for sql in [
        "UPDATE gazette_preparation_worktrees SET created_fence=999",
        "UPDATE gazette_preparation_worktrees SET created_journal=zeroblob(32)",
        "UPDATE gazette_preparation_worktrees SET metadata='{}'",
        "UPDATE gazette_preparation_commits SET bytes=124",
        "UPDATE gazette_preparation_commits SET parent_oid='2222222222222222222222222222222222222222'",
        "UPDATE gazette_preparation_commits SET recorded_fence=999",
        "DELETE FROM gazette_preparation_worktrees",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let record = writer
            .record_preparation_worktree(worktree_input(&retained, &metadata()), permission())
            .expect("intent")
            .record;
        writer
            .record_preparation_commit(completion_input(&record, &report(&record)), permission())
            .expect("completion");
        writer.close().expect("close before deliberate corruption");
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("native corruption fixture");
        connection
            .pragma_update(None, "foreign_keys", false)
            .expect("allow deliberately orphaned fixture");
        connection.execute(sql, []).expect("exact corruption");
        connection.close().expect("close corrupt fixture");
        let before = fs::read(&path).expect("corrupt bytes");
        assert!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .is_err(),
            "{sql}"
        );
        assert_eq!(fs::read(path).expect("inspection never repairs"), before);
    }
}

#[test]
fn parent_provenance_survives_completion_reopen_and_exact_replay() {
    for source in [
        ParentSource::Bare {
            path: "/private/parents/deployment.git".into(),
        },
        ParentSource::Worktree {
            path: "/private/parents/deployment".into(),
        },
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let mut metadata = metadata();
        metadata.parent = Some(
            "2222222222222222222222222222222222222222"
                .parse()
                .expect("exact parent pin"),
        );
        let first = writer
            .record_preparation_worktree(
                PreparationWorktreeInput {
                    parent_source: Some(&source),
                    ..worktree_input(&retained, &metadata)
                },
                permission(),
            )
            .expect("stored exact parent provenance without filesystem access");
        assert_eq!(
            first
                .record
                .worktree
                .as_ref()
                .expect("Git intent")
                .parent_source
                .as_ref(),
            Some(&source)
        );
        let mut report = report(&first.record);
        report.parent.clone_from(&metadata.parent);
        let complete = writer
            .record_preparation_commit(completion_input(&first.record, &report), permission())
            .expect("parented completion");
        writer.close().expect("first writer closed");
        let path = root.path().join(crate::journal::DATABASE);
        assert_eq!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .expect("fresh native inspection")
                .record,
            Some(complete.record.clone())
        );
        let connection = Connection::open(&path).expect("read exact stored encoding");
        let raw: String = connection
            .query_row(
                "SELECT parent_source FROM gazette_preparation_worktrees",
                [],
                |row| row.get(0),
            )
            .expect("explicit provenance column");
        assert_eq!(
            raw,
            serde_json::to_string(&source).expect("canonical source encoding")
        );
        connection.close().expect("close native lookup");
        let mut writer = Sqlite
            .acquire_writer(root.path(), complete.journal, writer_options(options()))
            .expect("new writer generation");
        let before = fs::read(&path).expect("current durable bytes");
        let replay = writer
            .record_preparation_worktree(
                PreparationWorktreeInput {
                    parent_source: Some(&source),
                    ..worktree_input(&retained, &metadata)
                },
                permission(),
            )
            .expect("exact provenance replay");
        assert!(!replay.inserted);
        assert_eq!(replay.record, complete.record);
        assert!(replay.journal.writer_fence > first.journal.writer_fence);
        let replay = writer
            .record_preparation_commit(completion_input(&first.record, &report), permission())
            .expect("exact completed replay");
        assert!(!replay.inserted);
        assert_eq!(replay.record, complete.record);
        assert_eq!(
            fs::read(&path).expect("replay never rewrites history"),
            before
        );
        writer.close().expect("replay writer closed");
    }
}

#[test]
fn same_parent_pin_cannot_change_provenance_in_intent_or_completion() {
    for completion in [false, true] {
        for changed in [
            ParentSource::Bare {
                path: "/private/another-parent.git".into(),
            },
            ParentSource::Worktree {
                path: "/private/parent.git".into(),
            },
        ] {
            let (root, mut writer) = fixture();
            let data = Data::new();
            let retained = retained(&mut writer, &data);
            let mut metadata = metadata();
            metadata.parent = Some(
                "2222222222222222222222222222222222222222"
                    .parse()
                    .expect("unchanged pin"),
            );
            let source = ParentSource::Bare {
                path: "/private/parent.git".into(),
            };
            let first = writer
                .record_preparation_worktree(
                    PreparationWorktreeInput {
                        parent_source: Some(&source),
                        ..worktree_input(&retained, &metadata)
                    },
                    permission(),
                )
                .expect("original provenance");
            let path = root.path().join(crate::journal::DATABASE);
            let before = fs::read(&path).expect("original history");
            let failure = if completion {
                let mut supplied = first.record.clone();
                supplied
                    .worktree
                    .as_mut()
                    .expect("Git intent")
                    .parent_source = Some(changed);
                let mut report = report(&supplied);
                report.parent.clone_from(&metadata.parent);
                writer.record_preparation_commit(completion_input(&supplied, &report), permission())
            } else {
                writer.record_preparation_worktree(
                    PreparationWorktreeInput {
                        parent_source: Some(&changed),
                        ..worktree_input(&retained, &metadata)
                    },
                    permission(),
                )
            }
            .expect_err("same commit pin does not authorize a different source");
            assert!(matches!(failure.cause, Error::PreparationConflict));
            assert_eq!(failure.effect, PreparationEffect::NotAttempted);
            drop(writer);
            assert_eq!(
                fs::read(&path).expect("conflict preserves exact history"),
                before
            );
            assert_eq!(
                Sqlite
                    .inspect_preparation(&path, data.input().id, options())
                    .expect("original provenance remains inspectable")
                    .record,
                Some(first.record)
            );
        }
    }
}

#[test]
fn parent_selector_binding_grammar_and_size_fail_before_record_attempt() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let retained = retained(&mut writer, &data);
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("only retained stage history");
    for (pin, source, over_limit) in [
        (true, None, false),
        (
            false,
            Some(ParentSource::Bare {
                path: "/private/parent.git".into(),
            }),
            false,
        ),
        (
            true,
            Some(ParentSource::Bare {
                path: "relative.git".into(),
            }),
            false,
        ),
        (
            true,
            Some(ParentSource::Worktree {
                path: "/private/../parent".into(),
            }),
            false,
        ),
        (true, Some(ParentSource::Bare { path: "/".into() }), false),
        (
            true,
            Some(ParentSource::Bare {
                path: format!(
                    "/{}",
                    "x".repeat(usize::try_from(options().value_bytes).expect("fixture bound"))
                )
                .into(),
            }),
            true,
        ),
    ] {
        let mut metadata = metadata();
        if pin {
            metadata.parent = Some(
                "2222222222222222222222222222222222222222"
                    .parse()
                    .expect("valid parent"),
            );
        }
        let error = writer
            .record_preparation_worktree(
                PreparationWorktreeInput {
                    parent_source: source.as_ref(),
                    ..worktree_input(&retained, &metadata)
                },
                permission(),
            )
            .expect_err("invalid parent selection or bounded encoding");
        if over_limit {
            assert!(matches!(error.cause, Error::StorageLimit));
        } else {
            assert!(matches!(error.cause, Error::PreparationVerification));
        }
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        assert_eq!(
            fs::read(&path).expect("preflight does not mutate history"),
            before
        );
    }
    writer
        .close()
        .expect("preflight refusals preserve valid writer lifetime");
}

#[test]
fn corrupted_parent_provenance_fails_read_only_without_repair() {
    for encoded in [
        None,
        Some("null"),
        Some("{}"),
        Some("{\"kind\":\"bare\",\"kind\":\"bare\",\"path\":\"/private/parent.git\"}"),
        Some("{\"kind\":\"bare\",\"path\":\"/private/parent.git\",\"extra\":true}"),
        Some("{\"kind\":\"bare\",\"path\":\"relative.git\"}"),
        Some("{ \"kind\":\"bare\",\"path\":\"/private/parent.git\"}"),
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let retained = retained(&mut writer, &data);
        let mut metadata = metadata();
        metadata.parent = Some(
            "2222222222222222222222222222222222222222"
                .parse()
                .expect("valid pin"),
        );
        let source = ParentSource::Bare {
            path: "/private/parent.git".into(),
        };
        writer
            .record_preparation_worktree(
                PreparationWorktreeInput {
                    parent_source: Some(&source),
                    ..worktree_input(&retained, &metadata)
                },
                permission(),
            )
            .expect("valid original source");
        writer.close().expect("close before explicit corruption");
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("native corruption connection");
        connection
            .execute(
                "UPDATE gazette_preparation_worktrees SET parent_source=?1",
                [encoded],
            )
            .expect("precise stored provenance corruption");
        connection.close().expect("close corruption connection");
        let before = fs::read(&path).expect("retained corrupt bytes");
        assert!(matches!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .expect_err("no invented or repaired parent source"),
            Error::PreparationVerification
        ));
        assert_eq!(
            fs::read(&path).expect("read-only inspection preserves corruption"),
            before
        );
    }
}
