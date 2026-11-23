//! Native immutable append/replay, independent inspection and failure boundaries.

use super::*;
use gazette_core::store::{
    CheckpointEffect, CheckpointWriteOptions, InitializeOptions, SessionOptions, WriterOptions,
};
use gazette_ports::store::{CheckpointWriter, JournalWriter, StoreInitializer, WriterConnector};
use std::{fs, os::unix::fs::PermissionsExt};

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn writer_options(read: ReadOptions) -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read,
        },
    }
}

fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private native fixture");
    let initial = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: writer_options(options()).session,
                ..Default::default()
            },
        )
        .expect("actual initial schema");
    let writer = Sqlite
        .acquire_writer(root.path(), initial.header, writer_options(options()))
        .expect("actual held writer");
    (root, writer)
}

fn data() -> (CommitMetadata, CommitReport) {
    let author = CommitSignature {
        name: "Exact Author".into(),
        email: "author@example.test".into(),
        seconds: 1234,
        offset_minutes: -330,
    };
    let metadata = CommitMetadata {
        parent: None,
        author: author.clone(),
        committer: author,
        message: "Exact message\n".into(),
    };
    let report = CommitReport {
        commit: "1111111111111111111111111111111111111111".parse().expect("full OID"),
        output: serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"exact tree")})).expect("typed claimed tree pin"),
        parent: None, entries: 3, bytes: 123,
    };
    (metadata, report)
}

fn input<'a>(metadata: &'a CommitMetadata, commit: &'a CommitReport) -> CheckpointInput<'a> {
    CheckpointInput {
        id: Sha256Digest::of(b"checkpoint one"),
        repository: "/private/local-publication.git",
        metadata,
        commit,
    }
}

fn permission() -> CheckpointWriteOptions {
    CheckpointWriteOptions { allow_record: true }
}

#[test]
fn append_reopen_and_exact_replay_preserve_original_creation_and_native_rows() {
    let (root, mut writer) = fixture();
    let (metadata, report) = data();
    let input = input(&metadata, &report);
    let receipt = writer
        .record_checkpoint(input, permission())
        .expect("native append and independent readback");
    assert_eq!(receipt.checkpoint.created, receipt.journal);
    assert!(same_payload(&receipt.checkpoint, input));
    writer.close().expect("close verified lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("retained bytes");
    let observed = Sqlite
        .inspect_checkpoint(&path, input.id, options())
        .expect("read-only reopening");
    assert_eq!(observed.checkpoint, Some(receipt.checkpoint.clone()));
    assert_eq!(fs::read(&path).expect("read-only native bytes"), before);
    assert!(
        Sqlite
            .inspect_checkpoint(&path, Sha256Digest::of(b"missing"), options())
            .expect("exact absence")
            .checkpoint
            .is_none()
    );
    let mut writer = Sqlite
        .acquire_writer(root.path(), receipt.journal, writer_options(options()))
        .expect("next generation");
    let acquired = writer.inspect().expect("current generation").header;
    let before_replay = fs::read(&path).expect("advanced generation bytes");
    let replay = writer
        .record_checkpoint(input, permission())
        .expect("same intent replay");
    assert_eq!(replay.checkpoint, receipt.checkpoint);
    assert_eq!(replay.journal, acquired);
    assert_eq!(acquired.writer_fence, receipt.journal.writer_fence + 1);
    assert_eq!(
        fs::read(&path).expect("replay performs no insert or rewrite"),
        before_replay
    );
    writer.close().expect("close replay lifetime");
}

#[test]
fn changed_payload_conflicts_without_replacing_original_and_invalidates_lifetime() {
    for change in [
        "repository",
        "message",
        "commit",
        "output",
        "entries",
        "bytes",
        "author",
        "parent",
    ] {
        let (root, mut writer) = fixture();
        let (metadata, report) = data();
        let original = writer
            .record_checkpoint(input(&metadata, &report), permission())
            .expect("original evidence");
        let mut changed_metadata = metadata.clone();
        let mut changed_report = report.clone();
        match change {
            "message" => changed_metadata.message.push('!'),
            "commit" => changed_report.commit = "2222222222222222222222222222222222222222".parse().expect("second OID"),
            "output" => changed_report.output = serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"changed")})).expect("other tree"),
            "entries" => changed_report.entries += 1,
            "bytes" => changed_report.bytes += 1,
            "author" => changed_metadata.author.offset_minutes += 1,
            "parent" => { changed_metadata.parent = Some(report.commit.clone()); changed_report.parent = changed_metadata.parent.clone(); },
            "repository" => (),
            _ => unreachable!("closed changed-field fixture"),
        }
        let mut changed = input(&changed_metadata, &changed_report);
        if change == "repository" {
            changed.repository = "/private/other.git";
        }
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("original native rows");
        let error = writer
            .record_checkpoint(changed, permission())
            .expect_err("same ID cannot change intent");
        assert!(matches!(error.cause, Error::CheckpointConflict), "{change}");
        assert_eq!(error.effect, CheckpointEffect::NotAttempted);
        assert!(matches!(writer.inspect(), Err(Error::Changed)));
        assert!(writer.close().is_err());
        assert_eq!(fs::read(&path).expect("preserved original"), before);
        assert_eq!(
            Sqlite
                .inspect_checkpoint(&path, changed.id, options())
                .expect("independent retained record")
                .checkpoint,
            Some(original.checkpoint)
        );
    }
}

#[test]
fn malformed_or_unpermitted_input_fails_before_writable_access() {
    let (root, mut writer) = fixture();
    let (mut metadata, mut report) = data();
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("before checkpoint attempts");
    let error = writer
        .record_checkpoint(input(&metadata, &report), Default::default())
        .expect_err("explicit checkpoint permission");
    assert!(matches!(error.cause, Error::CheckpointPermission));
    for repository in [
        "relative/repository",
        "/",
        "/private/../other.git",
        "/private/bad\0.git",
    ] {
        let mut invalid = input(&metadata, &report);
        invalid.repository = repository;
        assert!(matches!(
            writer
                .record_checkpoint(invalid, permission())
                .expect_err("invalid locator")
                .cause,
            Error::CheckpointVerification
        ));
    }
    metadata.author.seconds = -1;
    assert!(
        writer
            .record_checkpoint(input(&metadata, &report), permission())
            .is_err()
    );
    metadata.author.seconds = 1;
    report.parent = Some(report.commit.clone());
    assert!(
        writer
            .record_checkpoint(input(&metadata, &report), permission())
            .is_err()
    );
    report.parent = None;
    report.bytes = u64::MAX;
    assert!(matches!(
        writer
            .record_checkpoint(input(&metadata, &report), permission())
            .expect_err("SQL integer ceiling")
            .cause,
        Error::StorageLimit
    ));
    assert_eq!(fs::read(&path).expect("no checkpoint mutation"), before);
    writer
        .close()
        .expect("preflight refusals do not consume lifetime");
}

#[test]
fn malformed_rows_and_wrong_original_generation_are_refused_without_repair() {
    for sql in [
        "UPDATE gazette_local_commit_checkpoints SET repository='relative'",
        "UPDATE gazette_local_commit_checkpoints SET commit_oid='not-an-oid'",
        "UPDATE gazette_local_commit_checkpoints SET author_name=' unnormalized'",
        "UPDATE gazette_local_commit_checkpoints SET created_journal=zeroblob(32)",
        "UPDATE gazette_local_commit_checkpoints SET created_epoch=created_epoch+1",
        "UPDATE gazette_local_commit_checkpoints SET created_fence=created_fence+1",
        "UPDATE gazette_local_commit_checkpoints SET created_schema=created_schema+1",
    ] {
        let (root, mut writer) = fixture();
        let (metadata, report) = data();
        let input = input(&metadata, &report);
        writer
            .record_checkpoint(input, permission())
            .expect("valid native evidence");
        writer.close().expect("release native reservation");
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("external fixture mutation");
        connection
            .execute_batch(sql)
            .expect("controlled malformed record");
        connection.close().expect("close fixture corruption");
        let before = fs::read(&path).expect("corrupt bytes");
        assert!(
            Sqlite
                .inspect_checkpoint(&path, input.id, options())
                .is_err(),
            "{sql}"
        );
        assert_eq!(fs::read(&path).expect("no repair on read"), before);
    }
}

#[test]
fn native_insert_failure_rolls_back_without_erasing_prior_checkpoint() {
    let (root, mut writer) = fixture();
    let (metadata, report) = data();
    let input = input(&metadata, &report);
    let original = writer
        .record_checkpoint(input, permission())
        .expect("original checkpoint");
    writer
        .close()
        .expect("release lifetime before native failure injection");
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("native old state");
    let mut connection = Connection::open(&path).expect("native transaction fixture");
    crate::configure_connection(&connection, options()).expect("actual connection policy");
    connection.authorizer(Some(|context: rusqlite::hooks::AuthContext<'_>| {
        if matches!(context.action, rusqlite::hooks::AuthAction::Insert { table_name } if table_name == "gazette_local_commit_checkpoints") { rusqlite::hooks::Authorization::Deny } else { rusqlite::hooks::Authorization::Allow }
    })).expect("deny actual insertion");
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("native transaction");
    let mut next = input;
    next.id = Sha256Digest::of(b"second checkpoint");
    assert!(insert(&transaction, next, original.journal).is_err());
    transaction
        .rollback()
        .expect("native failed transaction rollback");
    connection.close().expect("close fixture connection");
    assert_eq!(fs::read(&path).expect("original bytes survive"), before);
    assert_eq!(
        Sqlite
            .inspect_checkpoint(&path, input.id, options())
            .expect("original remains readable")
            .checkpoint,
        Some(original.checkpoint)
    );
}

#[test]
fn committed_checkpoint_survives_failed_bounded_readback_with_generation_intact() {
    let (root, mut initial) = fixture();
    let header = initial.inspect().expect("previous generation").header;
    initial.close().expect("close measurement lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    let initial_bytes = fs::metadata(&path).expect("measured database size").len();
    let connection = Connection::open(&path).expect("measure native pages");
    let page_bytes: u32 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .expect("page size");
    let free_pages: i64 = connection
        .pragma_query_value(None, "freelist_count", |row| row.get(0))
        .expect("free pages");
    assert_eq!(free_pages, 0);
    connection.close().expect("close measurements");
    let bounded = ReadOptions {
        storage_bytes: initial_bytes,
        ..options()
    };
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options(bounded))
        .expect("acquire within file ceiling");
    let acquired = writer
        .inspect()
        .expect("current acquired coordinates")
        .header;
    let (mut metadata, report) = data();
    let text_bytes = initial_bytes / 2;
    assert!(text_bytes > u64::from(page_bytes) * 2);
    metadata.message = "m".repeat(usize::try_from(text_bytes).expect("bounded fixture text"));
    let input = input(&metadata, &report);
    validate(input, bounded).expect("record fits input bounds");
    let error = writer
        .record_checkpoint(input, permission())
        .expect_err("committed growth exceeds readback ceiling");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, CheckpointEffect::Unverified);
    assert!(writer.inspect().is_err());
    assert!(writer.close().is_err());
    assert!(fs::metadata(&path).expect("committed size").len() > initial_bytes);
    let observed = Sqlite
        .inspect_checkpoint(&path, input.id, options())
        .expect("independent sufficient-bound read");
    assert_eq!(observed.journal.header, acquired);
    let record = observed
        .checkpoint
        .expect("committed record is not rolled back");
    assert_eq!(record.created, acquired);
    assert!(same_payload(&record, input));
}

#[test]
fn backup_verification_and_detached_restore_preserve_history_without_adopting_it() {
    use gazette_core::store::{BackupOptions, BackupVerifyOptions, RestoreOptions};
    use gazette_ports::store::{BackupReader, RestorePreparer, StoreBackup};

    let (root, mut writer) = fixture();
    let (metadata, report) = data();
    let input = input(&metadata, &report);
    let recorded = writer
        .record_checkpoint(input, permission())
        .expect("committed local evidence");
    writer.close().expect("close before custody operations");
    let path = root.path().join(crate::journal::DATABASE);
    let original_bytes = fs::read(&path).expect("original source bytes");
    let parent = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("independent private recovery parent");
    let backup = Sqlite
        .backup(
            &path,
            parent.path(),
            BackupOptions {
                allow_create: true,
                read: options(),
                ..Default::default()
            },
        )
        .expect("native consistent backup");
    let verification = BackupVerifyOptions {
        read: options(),
        ..Default::default()
    };
    let verified = Sqlite
        .verify_backup(&backup.directory, backup.manifest_sha256, verification)
        .expect("retained manifest and native backup verification");
    assert_eq!(verified.snapshot, backup.snapshot);
    let prepared = Sqlite
        .prepare_restore(
            &backup.directory,
            backup.manifest_sha256,
            parent.path(),
            RestoreOptions {
                allow_prepare: true,
                verification,
                ..RestoreOptions::default()
            },
        )
        .expect("detached recovery copy");
    assert_ne!(prepared.directory, root.path());
    for copied in [&backup.directory, &prepared.directory] {
        let observed = Sqlite
            .inspect_checkpoint(&copied.join(crate::journal::DATABASE), input.id, options())
            .expect("independent historical checkpoint read");
        assert_eq!(observed.checkpoint, Some(recorded.checkpoint.clone()));
        assert_eq!(
            observed.journal.header, recorded.journal,
            "copied coordinates are retained history, not fresh fencing"
        );
    }
    assert_eq!(
        fs::read(&path).expect("original is not activated or replaced"),
        original_bytes
    );
}
