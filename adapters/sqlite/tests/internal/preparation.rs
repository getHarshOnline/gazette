//! Native two-phase history, exact replay and read-only recovery; no staging effects.

use super::*;
use gazette_core::store::{
    InitializeOptions, PreparationEffect, PreparationWriteOptions, SessionOptions, WriterOptions,
};
use gazette_ports::store::{JournalWriter, PreparationWriter, StoreInitializer, WriterConnector};
use std::{fs, os::unix::fs::PermissionsExt};

#[path = "preparation_worktree.rs"]
mod worktree;

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

fn permission() -> PreparationWriteOptions {
    PreparationWriteOptions { allow_record: true }
}

fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private SQLite fixture");
    let initial = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: writer_options(options()).session,
                ..Default::default()
            },
        )
        .expect("actual fresh schema");
    let writer = Sqlite
        .acquire_writer(root.path(), initial.header, writer_options(options()))
        .expect("actual held writer");
    (root, writer)
}

struct Data {
    address: Address,
    destination: Option<PublicationDestination>,
    output: TreeIdentity,
    evidence: String,
}

impl Data {
    fn new() -> Self {
        Self {
            // The store accepts a native owner's canonical locator but never opens it.
            address: Address { parent: "/private/selected-stages".into(), name: "exact-child".into() },
            destination: None,
            output: serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"complete output")})).expect("claimed typed output identity"),
            evidence: "{\n  \"source\": \"PRIVATE exact evidence\", \"revision\": 1\n}\n".into(),
        }
    }
    fn input(&self) -> PreparationInput<'_> {
        PreparationInput {
            id: Sha256Digest::of(b"preparation one"),
            address: &self.address,
            destination: self.destination.as_ref(),
            output: self.output,
            entries: 3,
            bytes: 123,
            evidence: &self.evidence,
            evidence_sha256: Sha256Digest::of(self.evidence.as_bytes()),
        }
    }
}

fn retention(intent: &PreparationIntent) -> PreparationRetentionInput<'_> {
    PreparationRetentionInput {
        intent,
        output: intent.output,
        entries: intent.entries,
        bytes: intent.bytes,
    }
}

fn destination() -> PublicationDestination {
    PublicationDestination::Git {
        remote: "ssh://git@example.test/private/publication.git".into(),
        branch: "publication/site".into(),
    }
}

#[test]
fn destination_changes_and_local_only_changes_never_replace_intent() {
    for change in ["remote", "branch", "remove", "add"] {
        let (root, mut writer) = fixture();
        let mut data = Data::new();
        if change != "add" {
            data.destination = Some(destination());
        }
        let original = writer
            .record_preparation_intent(data.input(), permission())
            .expect("original delivery selection");
        match change {
            "remove" => data.destination = None,
            "add" => data.destination = Some(destination()),
            field => {
                let PublicationDestination::Git { remote, branch } =
                    data.destination.as_mut().expect("selected Git destination");
                if field == "remote" {
                    remote.push_str("-other");
                } else {
                    branch.push_str("-other");
                }
            }
        }
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("original database bytes");
        let failure = writer
            .record_preparation_intent(data.input(), permission())
            .expect_err("same ID cannot change selected destination");
        assert!(
            matches!(failure.cause, Error::PreparationConflict),
            "{change}"
        );
        assert_eq!(failure.effect, PreparationEffect::NotAttempted);
        drop(writer);
        assert_eq!(fs::read(&path).expect("no replacement"), before);
        assert_eq!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .expect("original intent remains")
                .record,
            Some(original.record)
        );
    }
}

#[test]
fn persisted_destination_requires_strict_canonical_valid_typed_json() {
    for corrupt in [
        "null",
        "not JSON",
        r#"{"kind":"git","remote":"x","remote":"y","branch":"main"}"#,
        r#"{"kind":"git","remote":"x","branch":"main","extra":true}"#,
        r#"{"kind":"other","remote":"x","branch":"main"}"#,
        r#"{"kind":"git","remote":"x","branch":"../main"}"#,
        r#"{"kind":"git", "remote":"x","branch":"main"}"#,
        r#"{"branch":"main","kind":"git","remote":"x"}"#,
    ] {
        let (root, mut writer) = fixture();
        let mut data = Data::new();
        data.destination = Some(destination());
        writer
            .record_preparation_intent(data.input(), permission())
            .expect("valid selected destination");
        writer
            .close()
            .expect("close before deliberate fixture corruption");
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("fixture corruptor");
        connection
            .execute(
                "UPDATE gazette_preparation_intents SET destination=?1",
                [corrupt],
            )
            .expect("replace only fixture payload");
        connection.close().expect("close corruptor");
        let before = fs::read(&path).expect("corrupt bytes");
        assert!(
            matches!(
                Sqlite.inspect_preparation(&path, data.input().id, options()),
                Err(Error::PreparationVerification)
            ),
            "{corrupt}"
        );
        assert_eq!(
            fs::read(&path).expect("read never repairs destination"),
            before
        );
    }
}

#[test]
fn destination_serialized_bytes_share_intent_budget_before_decode_or_append() {
    let (root, mut writer) = fixture();
    let mut data = Data::new();
    // Quotes require JSON escaping, so raw field lengths are insufficient.
    data.destination = Some(PublicationDestination::Git {
        remote: "\"".repeat(100),
        branch: "main".into(),
    });
    let encoded = serde_json::to_string(data.destination.as_ref().expect("destination"))
        .expect("native canonical JSON");
    let base = 256
        + data.address.parent.to_str().expect("UTF-8 parent").len()
        + data.address.name.len()
        + data.evidence.len();
    let exact = i32::try_from(base + encoded.len()).expect("small fixture ceiling");
    let read = ReadOptions {
        value_bytes: exact,
        ..options()
    };
    validate(data.input(), read).expect("exact serialized ceiling accepted");
    assert!(matches!(
        validate(
            data.input(),
            ReadOptions {
                value_bytes: exact - 1,
                ..read
            }
        ),
        Err(Error::StorageLimit)
    ));
    let recorded = writer
        .record_preparation_intent(data.input(), permission())
        .expect("bounded destination persisted");
    writer.close().expect("release writer");
    let path = root.path().join(crate::journal::DATABASE);
    // Exercise the actual row decoder with its borrowed native payload extent.
    let connection = Connection::open(&path).expect("read fixture database");
    assert_eq!(
        super::read(&connection, data.input().id, recorded.journal, read)
            .expect("exact read budget")
            .expect("intent")
            .intent
            .destination,
        data.destination
    );
    assert!(matches!(
        super::read(
            &connection,
            data.input().id,
            recorded.journal,
            ReadOptions {
                value_bytes: exact - 1,
                ..read
            }
        ),
        Err(Error::StorageLimit)
    ));
    connection.close().expect("close reader");
    // Keep SQLite's native schema-loading ceiling independent from the
    // application payload refusal exercised by this later append.
    let writer_read = options();
    let mut writer = Sqlite
        .acquire_writer(root.path(), recorded.journal, writer_options(writer_read))
        .expect("bounded writer");
    data.destination = Some(PublicationDestination::Git {
        remote: "\"".repeat(usize::try_from(writer_read.value_bytes).expect("positive byte limit")),
        branch: "main".into(),
    });
    let before = fs::read(&path).expect("before oversized append");
    let failure = writer
        .record_preparation_intent(data.input(), permission())
        .expect_err("serialized destination exceeds invocation ceiling");
    assert!(matches!(failure.cause, Error::StorageLimit));
    assert_eq!(failure.effect, PreparationEffect::NotAttempted);
    writer
        .close()
        .expect("preflight refusal leaves writer usable");
    assert_eq!(fs::read(&path).expect("no oversized row"), before);
}

#[test]
fn held_preparation_lookup_preserves_bytes_and_refuses_corrupt_history() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let first = writer
        .record_preparation_intent(data.input(), permission())
        .expect("exact recorded intent");
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("held journal bytes");
    let found = writer
        .inspect_preparation(first.record.intent.id)
        .expect("held-generation lookup");
    assert_eq!(found.journal.header, first.journal);
    assert_eq!(found.record, Some(first.record));
    assert!(
        writer
            .inspect_preparation(Sha256Digest::of(b"never recorded"))
            .expect("missing is a read result, not an insertion")
            .record
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("lookup writes no rows"), before);
    // Disposable external writer deliberately violates the exclusive-owner premise.
    let connection = Connection::open(&path).expect("fixture-only corruptor");
    connection
        .execute_batch("UPDATE gazette_preparation_intents SET evidence='{}'")
        .expect("invalidate the retained evidence digest");
    connection.close().expect("close fixture corruptor");
    let corrupt = fs::read(&path).expect("corrupt fixture bytes");
    assert!(writer.inspect_preparation(data.input().id).is_err());
    assert!(matches!(writer.inspect(), Err(Error::Changed)));
    assert!(matches!(
        writer.inspect_preparation(data.input().id),
        Err(Error::Changed)
    ));
    assert!(writer.close().is_err());
    assert_eq!(
        fs::read(&path).expect("no repair on failed lookup"),
        corrupt
    );
}

#[test]
fn intent_retention_reopen_and_replay_preserve_exact_evidence_and_original_coordinates() {
    let (root, mut writer) = fixture();
    let mut data = Data::new();
    data.destination = Some(destination());
    let first = writer
        .record_preparation_intent(data.input(), permission())
        .expect("intent before any filesystem stage");
    assert!(first.inserted);
    assert_eq!(first.record.intent.destination, data.destination);
    assert!(first.record.retention.is_none());
    assert_eq!(
        first.record.intent.evidence.as_bytes(),
        data.evidence.as_bytes()
    );
    assert_eq!(first.record.intent.created, first.journal);
    assert!(matches!(
        writer
            .record_preparation_retention(retention(&first.record.intent), Default::default())
            .expect_err("retention has its own explicit record permission")
            .cause,
        Error::PreparationPermission
    ));
    let complete = writer
        .record_preparation_retention(retention(&first.record.intent), permission())
        .expect("separate retention transaction");
    assert!(complete.inserted);
    assert_eq!(complete.record.intent, first.record.intent);
    assert_eq!(
        complete
            .record
            .retention
            .as_ref()
            .expect("retention")
            .recorded,
        complete.journal
    );
    writer.close().expect("close durable lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("retained database");
    let observed = Sqlite
        .inspect_preparation(&path, data.input().id, options())
        .expect("independent read-only reopen");
    assert_eq!(observed.record, Some(complete.record.clone()));
    assert!(
        Sqlite
            .inspect_preparation(&path, Sha256Digest::of(b"missing"), options())
            .expect("explicit absence")
            .record
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("no read mutation"), before);
    let mut writer = Sqlite
        .acquire_writer(root.path(), complete.journal, writer_options(options()))
        .expect("next writer generation");
    let acquired = writer.inspect().expect("current coordinates").header;
    let advanced = fs::read(&path).expect("advanced journal bytes");
    let replay = writer
        .record_preparation_intent(data.input(), permission())
        .expect("intent replay includes historical retention");
    assert!(!replay.inserted);
    assert_eq!(replay.record, complete.record);
    assert_eq!(replay.journal, acquired);
    assert_eq!(
        replay.journal.writer_fence,
        complete.journal.writer_fence + 1
    );
    let replay = writer
        .record_preparation_retention(retention(&first.record.intent), permission())
        .expect("retention replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, complete.record);
    assert_eq!(
        fs::read(&path).expect("neither replay rewrites rows"),
        advanced
    );
    writer.close().expect("close replay lifetime");
}

#[test]
fn changed_intent_conflicts_without_replacement_and_invalidates_writer() {
    for field in ["address", "output", "entries", "bytes", "evidence"] {
        let (root, mut writer) = fixture();
        let mut data = Data::new();
        let original = writer
            .record_preparation_intent(data.input(), permission())
            .expect("original intent");
        match field {
            "address" => data.address.name.push_str("-other"),
            "output" => data.output = serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"other output")})).expect("other typed identity"),
            "evidence" => data.evidence.push(' '),
            _ => (),
        }
        let mut changed = data.input();
        if field == "entries" {
            changed.entries += 1;
        }
        if field == "bytes" {
            changed.bytes += 1;
        }
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("original bytes");
        let error = writer
            .record_preparation_intent(changed, permission())
            .expect_err("immutable same-ID intent");
        assert!(matches!(error.cause, Error::PreparationConflict), "{field}");
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        assert!(matches!(writer.inspect(), Err(Error::Changed)));
        assert!(writer.close().is_err());
        assert_eq!(fs::read(&path).expect("original is not replaced"), before);
        assert_eq!(
            Sqlite
                .inspect_preparation(&path, changed.id, options())
                .expect("original independent history")
                .record,
            Some(original.record)
        );
    }
}

#[test]
fn retention_requires_exact_existing_intent_and_matching_complete_output() {
    for field in [
        "absent",
        "created",
        "destination",
        "output",
        "entries",
        "bytes",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let first = writer
            .record_preparation_intent(data.input(), permission())
            .expect("original intent");
        let mut intent = first.record.intent.clone();
        if field == "absent" {
            intent.id = Sha256Digest::of(b"absent intent");
        }
        if field == "created" {
            intent.created.writer_fence += 1;
        }
        if field == "destination" {
            intent.destination = Some(destination());
        }
        let mut observed = retention(&intent);
        if field == "output" {
            observed.output = serde_json::from_value(serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":Sha256Digest::of(b"wrong output")})).expect("different output");
        }
        if field == "entries" {
            observed.entries += 1;
        }
        if field == "bytes" {
            observed.bytes += 1;
        }
        let path = root.path().join(crate::journal::DATABASE);
        let before = fs::read(&path).expect("intent-only bytes");
        let error = writer
            .record_preparation_retention(observed, permission())
            .expect_err("no inferred retention");
        assert!(
            matches!(
                error.cause,
                Error::PreparationVerification | Error::PreparationConflict
            ),
            "{field}"
        );
        assert_eq!(error.effect, PreparationEffect::NotAttempted);
        drop(writer);
        assert_eq!(fs::read(&path).expect("no retention row inserted"), before);
        assert_eq!(
            Sqlite
                .inspect_preparation(&path, data.input().id, options())
                .expect("intent remains")
                .record,
            Some(first.record)
        );
    }
}

#[test]
fn permission_shape_digest_and_resource_refusals_precede_record_writes() {
    let (root, mut writer) = fixture();
    let mut data = Data::new();
    let path = root.path().join(crate::journal::DATABASE);
    let before = fs::read(&path).expect("initial writer bytes");
    assert!(matches!(
        writer
            .record_preparation_intent(data.input(), Default::default())
            .expect_err("explicit permission")
            .cause,
        Error::PreparationPermission
    ));
    for parent in [
        "relative",
        "/private/../stages",
        "/private/./stages",
        "/private//stages",
        "/private/stages/",
    ] {
        data.address.parent = parent.into();
        assert!(
            matches!(
                writer
                    .record_preparation_intent(data.input(), permission())
                    .expect_err("canonical spelling")
                    .cause,
                Error::PreparationVerification
            ),
            "{parent}"
        );
    }
    data = Data::new();
    for name in ["", ".", "..", "a/b", "x/", "bad\0name"] {
        data.address.name = name.into();
        assert!(
            writer
                .record_preparation_intent(data.input(), permission())
                .is_err(),
            "{name}"
        );
    }
    data = Data::new();
    let mut bad = data.input();
    bad.evidence_sha256 = Sha256Digest::of(b"wrong exact bytes");
    assert!(matches!(
        writer
            .record_preparation_intent(bad, permission())
            .expect_err("digest binding")
            .cause,
        Error::PreparationVerification
    ));
    for evidence in ["not JSON", "{\"a\":1,\"\\u0061\":2}"] {
        data.evidence = evidence.into();
        assert!(
            writer
                .record_preparation_intent(data.input(), permission())
                .is_err()
        );
    }
    data = Data::new();
    let mut huge = data.input();
    huge.entries = u64::MAX;
    assert!(matches!(
        writer
            .record_preparation_intent(huge, permission())
            .expect_err("native integer ceiling")
            .cause,
        Error::StorageLimit
    ));
    data.evidence = format!(
        "\"{}\"",
        "x".repeat(usize::try_from(options().value_bytes).expect("bounded fixture"))
    );
    assert!(matches!(
        writer
            .record_preparation_intent(data.input(), permission())
            .expect_err("retained row ceiling")
            .cause,
        Error::StorageLimit
    ));
    assert_eq!(fs::read(&path).expect("no SQL data changes"), before);
    writer
        .close()
        .expect("pure preflight refusals preserve lifetime");
    let missing = root.path().join("absent.sqlite");
    assert!(
        Sqlite
            .inspect_preparation(&missing, Sha256Digest::of(b"absent"), options())
            .is_err()
    );
    assert!(!missing.exists());
}

#[test]
fn malformed_stored_evidence_retention_and_coordinates_fail_read_only_without_repair() {
    for sql in [
        "UPDATE gazette_preparation_intents SET parent='relative'",
        "UPDATE gazette_preparation_intents SET evidence='{}'",
        "UPDATE gazette_preparation_intents SET created_journal=zeroblob(32)",
        "UPDATE gazette_preparation_intents SET created_fence=created_fence+1",
        "UPDATE gazette_preparation_retentions SET bytes=bytes+1",
        "UPDATE gazette_preparation_retentions SET recorded_epoch=recorded_epoch+1",
        "UPDATE gazette_preparation_retentions SET intent_id=randomblob(32)",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let first = writer
            .record_preparation_intent(data.input(), permission())
            .expect("valid intent");
        writer
            .record_preparation_retention(retention(&first.record.intent), permission())
            .expect("valid retention");
        writer
            .close()
            .expect("release before external fixture corruption");
        let path = root.path().join(crate::journal::DATABASE);
        let connection = Connection::open(&path).expect("fixture-only external SQL writer");
        let orphan = sql == "UPDATE gazette_preparation_retentions SET intent_id=randomblob(32)";
        if orphan {
            // Deliberately model an external writer bypassing constraints. This
            // affects only this disposable connection, never Gazette's reader.
            connection
                .pragma_update(None, "foreign_keys", false)
                .expect("disable enforcement only to construct orphan fixture");
            assert!(
                !connection
                    .pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))
                    .expect("verify fixture-only enforcement setting")
            );
        }
        connection
            .execute_batch(sql)
            .expect("controlled corrupt field");
        if orphan {
            let table: String = connection
                .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
                .expect("verify actual orphan retention exists");
            assert_eq!(table, "gazette_preparation_retentions");
        }
        connection.close().expect("close corruption fixture");
        let before = fs::read(&path).expect("corrupt bytes");
        let error = Sqlite
            .inspect_preparation(&path, data.input().id, options())
            .expect_err("corrupt historical evidence must fail read-only inspection");
        if orphan {
            assert!(
                matches!(error, Error::JournalVerification),
                "{sql}: {error}"
            );
        }
        assert_eq!(fs::read(&path).expect("inspection never repairs"), before);
    }
}

#[test]
fn committed_intent_survives_failed_readback_without_false_rollback_claim() {
    let (root, mut initial) = fixture();
    let header = initial.inspect().expect("old generation").header;
    initial.close().expect("close measurement lifetime");
    let path = root.path().join(crate::journal::DATABASE);
    let initial_bytes = fs::metadata(&path).expect("native initial size").len();
    let bounded = ReadOptions {
        storage_bytes: initial_bytes,
        ..options()
    };
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options(bounded))
        .expect("bounded generation acquisition");
    let acquired = writer.inspect().expect("current generation").header;
    let mut data = Data::new();
    data.evidence = format!(
        "{{\"retained\":\"{}\"}}",
        "x".repeat(usize::try_from(initial_bytes / 2).expect("bounded growth fixture"))
    );
    validate(data.input(), bounded).expect("input fits logical row limit");
    let error = writer
        .record_preparation_intent(data.input(), permission())
        .expect_err("committed growth exceeds independent readback ceiling");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, PreparationEffect::Unverified);
    assert!(writer.inspect().is_err());
    assert!(writer.close().is_err());
    let observed = Sqlite
        .inspect_preparation(&path, data.input().id, options())
        .expect("fresh sufficient-bound inspection");
    assert_eq!(observed.journal.header, acquired);
    let record = observed.record.expect("committed intent remains");
    assert_eq!(record.intent.created, acquired);
    assert_eq!(record.intent.as_input(), data.input());
    assert!(
        record.retention.is_none(),
        "no native staging or retention invented"
    );
}

#[test]
fn backup_and_detached_restore_preserve_preparation_and_check_without_registry_ownership() {
    use gazette_core::store::{BackupOptions, BackupVerifyOptions, RestoreOptions};
    use gazette_ports::store::{BackupReader, RestorePreparer, StoreBackup};
    let (root, mut writer) = fixture();
    let data = Data::new();
    let first = writer
        .record_preparation_intent(data.input(), permission())
        .expect("intent");
    let complete = writer
        .record_preparation_retention(retention(&first.record.intent), permission())
        .expect("retention");
    let evidence = "{\"private\":\"retained output check\"}";
    let complete = writer
        .record_preparation_check(
            gazette_core::store::PreparationCheckInput {
                intent: &complete.record.intent,
                retention: complete.record.retention.as_ref().expect("retained output"),
                evidence,
                evidence_sha256: Sha256Digest::of(evidence.as_bytes()),
            },
            permission(),
        )
        .expect("check preserved by backup and detached restore");
    writer.close().expect("close before backup");
    assert!(
        crate::schema::registry_tables()
            .expect("registry-owned table catalog")
            .iter()
            .all(|table| !table.name.starts_with("gazette_preparation_"))
    );
    let path = root.path().join(crate::journal::DATABASE);
    let original = fs::read(&path).expect("original history");
    let parent = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("independent backup parent");
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
        .expect("actual native backup");
    let verification = BackupVerifyOptions {
        read: options(),
        ..Default::default()
    };
    Sqlite
        .verify_backup(&backup.directory, backup.manifest_sha256, verification)
        .expect("manifest-bound native backup");
    let restored = Sqlite
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
        .expect("detached copy only");
    for copy in [&backup.directory, &restored.directory] {
        let inspection = Sqlite
            .inspect_preparation(
                &copy.join(crate::journal::DATABASE),
                data.input().id,
                options(),
            )
            .expect("copied exact history");
        assert_eq!(inspection.record, Some(complete.record.clone()));
        assert_eq!(inspection.journal.header, complete.journal);
    }
    assert_eq!(fs::read(&path).expect("original not replaced"), original);
}
