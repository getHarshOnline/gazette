//! Actual reserved `SQLite` history; stored observations never invoke an output writer.

use super::*;
use gazette_core::{
    destination::OutputObservation,
    store::{
        BackupOptions, InitializeOptions, MaterializationCompletionInput, MaterializationEffect,
        MaterializationWriteOptions, SessionOptions, WriterOptions,
    },
};
use gazette_ports::store::{
    JournalWriter, MaterializationReader, MaterializationWriter, StoreBackup, StoreInitializer,
    WriterConnector,
};
use std::{fs, os::unix::fs::PermissionsExt};

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}
fn writer_options() -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read: options(),
        },
    }
}
fn permission() -> MaterializationWriteOptions {
    MaterializationWriteOptions { allow_record: true }
}
fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private fixture")
}
fn fixture() -> (tempfile::TempDir, crate::Writer) {
    let root = private();
    let initial = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: writer_options().session,
                ..Default::default()
            },
        )
        .expect("fresh native schema");
    let writer = Sqlite
        .acquire_writer(root.path(), initial.header, writer_options())
        .expect("reserved native writer");
    (root, writer)
}
fn manifest(text: &[u8]) -> Inventory {
    let mut builder = InventoryBuilder::new(Limits::default());
    builder
        .insert(
            "index.html".parse().expect("path"),
            Content::File {
                bytes: text.len() as u64,
                sha256: Sha256Digest::of(text),
            },
        )
        .expect("complete file");
    builder.finish()
}
struct Data {
    site: Key,
    binding: OutputBinding,
    observed: Inventory,
    expected: Inventory,
    generated: Inventory,
    protected: Vec<PathBuf>,
    evidence: String,
}
impl Data {
    fn new() -> Self {
        Self {
            site: "site:one".parse().expect("site key"),
            binding: OutputBinding::new(
                "/private/exact-output".into(),
                OutputIdentity::new(1, 2),
                None,
            )
            .expect("native fact shape"),
            observed: InventoryBuilder::new(Limits::default()).finish(),
            expected: manifest(b"rendered"),
            generated: manifest(b"rendered"),
            protected: vec!["/private/selected-source".into()],
            evidence: "{\"build\":\"exact retained render\"}".into(),
        }
    }
    fn input(&self, id: &[u8], previous: Option<Sha256Digest>) -> MaterializationInput<'_> {
        MaterializationInput {
            id: Sha256Digest::of(id),
            site: &self.site,
            binding: &self.binding,
            previous,
            observed: &self.observed,
            expected: &self.expected,
            generated: &self.generated,
            protected: &self.protected,
            evidence: &self.evidence,
            evidence_sha256: Sha256Digest::of(self.evidence.as_bytes()),
        }
    }
    fn read_back(&self) -> OutputObservation {
        OutputObservation::new(manifest(b"rendered"), self.binding.clone())
    }
}
fn database(root: &tempfile::TempDir) -> PathBuf {
    Sqlite::journal_database(root.path())
}

#[test]
fn exact_replay_reopen_and_atomic_completed_head_preserve_original_coordinates() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let first = writer
        .record_materialization_intent(data.input(b"first", None), permission())
        .expect("reserved intent");
    assert!(first.inserted);
    assert!(first.record.completion.is_none());
    let replay = writer
        .record_materialization_intent(data.input(b"first", None), permission())
        .expect("history only replay");
    assert!(!replay.inserted);
    assert_eq!(first.record, replay.record);
    let frontier = writer
        .inspect_materialization(data.binding.canonical_root())
        .expect("pending frontier")
        .frontier;
    assert!(frontier.head.is_none());
    assert_eq!(
        frontier.active.as_ref().map(|record| record.intent.id),
        Some(first.record.intent.id)
    );
    let header = writer.inspect().expect("current writer header").header;
    writer.close().expect("clean writer close");
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options())
        .expect("new held generation");
    let original = writer
        .inspect_materialization(data.binding.canonical_root())
        .expect("persisted active")
        .frontier
        .active
        .expect("complete intent");
    assert_eq!(original, first.record);
    let read_back = data.read_back();
    let completion = writer
        .record_materialization_completion(
            MaterializationCompletionInput {
                intent: &original.intent,
                read_back: &read_back,
            },
            permission(),
        )
        .expect("atomic completion");
    assert!(completion.inserted);
    assert_eq!(
        completion.record.intent.created,
        first.record.intent.created
    );
    assert!(
        completion
            .record
            .completion
            .as_ref()
            .expect("read-back completion")
            .recorded
            .writer_fence
            > first.record.intent.created.writer_fence
    );
    let replay = writer
        .record_materialization_completion(
            MaterializationCompletionInput {
                intent: &original.intent,
                read_back: &read_back,
            },
            permission(),
        )
        .expect("completion replay");
    assert!(!replay.inserted);
    assert_eq!(replay.record, completion.record);
    let second = writer
        .record_materialization_intent(
            data.input(b"second", Some(original.intent.id)),
            permission(),
        )
        .expect("exact prior head reserved");
    let frontier = writer
        .inspect_materialization(data.binding.canonical_root())
        .expect("head plus pending")
        .frontier;
    assert_eq!(
        frontier.head.as_ref().map(|record| record.intent.id),
        Some(original.intent.id)
    );
    assert_eq!(
        frontier.active.as_ref().map(|record| record.intent.id),
        Some(second.record.intent.id)
    );
    writer
        .close()
        .expect("historical pending is not a SQL custody failure");
    let read = MaterializationReader::inspect_materialization(
        &Sqlite,
        &database(&root),
        data.binding.canonical_root(),
        options(),
    )
    .expect("independent historical read");
    assert_eq!(read.frontier, frontier);
    let mut next = Sqlite
        .acquire_writer(root.path(), read.journal.header, writer_options())
        .expect("independently reacquired generation");
    let refused = next
        .record_materialization_intent(data.input(b"third", Some(original.intent.id)), permission())
        .expect_err("durable active slot blocks another lifetime");
    assert!(matches!(refused.cause, Error::MaterializationPending));
    assert_eq!(refused.effect, MaterializationEffect::NotAttempted);
}

#[test]
fn competing_start_stale_head_and_changed_exact_replay_refuse_without_history_effect() {
    for case in ["active", "head", "payload", "binding"] {
        let (root, mut writer) = fixture();
        let mut data = Data::new();
        let first = writer
            .record_materialization_intent(data.input(b"first", None), permission())
            .expect("first intent");
        if case == "head" || case == "binding" {
            writer
                .record_materialization_completion(
                    MaterializationCompletionInput {
                        intent: &first.record.intent,
                        read_back: &data.read_back(),
                    },
                    permission(),
                )
                .expect("confirmed head");
        }
        if case == "payload" {
            data.evidence = "{\"changed\":true}".into();
        }
        if case == "binding" {
            data.binding = OutputBinding::new(
                data.binding.canonical_root().into(),
                OutputIdentity::new(1, 3),
                None,
            )
            .expect("same path, replacement root facts");
        }
        let error = writer
            .record_materialization_intent(
                data.input(
                    if case == "payload" {
                        b"first"
                    } else {
                        b"second"
                    },
                    if case == "binding" {
                        Some(first.record.intent.id)
                    } else {
                        None
                    },
                ),
                permission(),
            )
            .expect_err("no second effect permission");
        assert_eq!(error.effect, MaterializationEffect::NotAttempted);
        assert!(matches!(
            error.cause,
            Error::MaterializationPending | Error::MaterializationConflict
        ));
        drop(writer);
        let connection = Connection::open(database(&root)).expect("native inspect fixture");
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM gazette_materialization_intents",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("count"),
            1
        );
    }
}

#[test]
fn missing_permission_wrong_binding_and_divergent_readback_never_append_completion() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let error = writer
        .record_materialization_intent(
            data.input(b"first", None),
            MaterializationWriteOptions::default(),
        )
        .expect_err("explicit permission");
    assert!(matches!(error.cause, Error::MaterializationPermission));
    assert_eq!(error.effect, MaterializationEffect::NotAttempted);
    let first = writer
        .record_materialization_intent(data.input(b"first", None), permission())
        .expect("intent after permission refusal");
    for read_back in [
        OutputObservation::new(manifest(b"different"), data.binding.clone()),
        OutputObservation::new(
            manifest(b"rendered"),
            OutputBinding::new(
                data.binding.canonical_root().into(),
                OutputIdentity::new(1, 3),
                None,
            )
            .expect("replaced root shape"),
        ),
    ] {
        let error = writer
            .record_materialization_completion(
                MaterializationCompletionInput {
                    intent: &first.record.intent,
                    read_back: &read_back,
                },
                permission(),
            )
            .expect_err("exact result required");
        assert!(matches!(error.cause, Error::MaterializationVerification));
        assert_eq!(error.effect, MaterializationEffect::NotAttempted);
    }
    writer
        .close()
        .expect("pure validation refusals do not invalidate holder");
    let inspection = MaterializationReader::inspect_materialization(
        &Sqlite,
        &database(&root),
        data.binding.canonical_root(),
        options(),
    )
    .expect("still pending");
    assert!(inspection.frontier.active.is_some());
    assert!(inspection.frontier.head.is_none());
}

#[test]
fn native_manifest_digest_structure_lineage_and_slot_corruption_fail_closed() {
    for case in [
        "identity",
        "parent",
        "count",
        "evidence",
        "binding",
        "lineage",
        "header",
        "slot",
        "duplicate-key",
    ] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let original = writer
            .record_materialization_intent(data.input(b"first", None), permission())
            .expect("actual intent");
        writer
            .close()
            .expect("close before adversarial local corruption");
        let connection = Connection::open(database(&root)).expect("fixture corruption connection");
        if case == "slot" {
            connection
                .execute("DELETE FROM gazette_materialization_frontiers", [])
                .expect("missing slot");
        } else {
            let text: String = connection
                .query_row(
                    "SELECT payload FROM gazette_materialization_intents",
                    [],
                    |row| row.get(0),
                )
                .expect("stored bytes");
            let mut value: serde_json::Value = serde_json::from_str(&text).expect("fixture JSON");
            match case {
                "identity" => {
                    value["expected"]["identity"]["sha256"] =
                        serde_json::json!(Sha256Digest::of(b"forged"))
                }
                "parent" => {
                    value["expected"]["entries"][0]["path"] =
                        serde_json::json!("missing/index.html")
                }
                "count" => value["expected"]["total_bytes"] = serde_json::json!(999),
                "evidence" => value["evidence"] = serde_json::json!("{}"),
                "binding" => {
                    value["binding"]["canonical_root"] = serde_json::json!("/private/other")
                }
                "lineage" => value["previous"] = serde_json::json!(Sha256Digest::of(b"absent")),
                "header" => value["created"]["writer_fence"] = serde_json::json!(0),
                "duplicate-key" => {}
                _ => unreachable!("enumerated fixture"),
            }
            let mut altered = serde_json::to_string(&value).expect("altered fixture bytes");
            // Preserve the native canonical representation for facts checked after
            // reconstruction, so rejection cannot merely be map-order normalization.
            if case == "lineage" || case == "header" {
                let mut intent = original.record.intent;
                if case == "lineage" {
                    intent.previous = Some(Sha256Digest::of(b"absent"));
                } else {
                    intent.created.writer_fence = 0;
                }
                altered = serde_json::to_string(&intent).expect("canonical corrupted history");
            }
            if case == "duplicate-key" {
                altered = altered.replacen("{", "{\"id\":\"duplicated\",", 1);
            }
            connection
                .execute(
                    "UPDATE gazette_materialization_intents SET payload=?1,payload_sha256=?2",
                    params![altered, Sha256Digest::of(altered.as_bytes()).as_bytes()],
                )
                .expect("self-consistent row hash is not semantic proof");
        }
        connection.close().expect("closed adversarial connection");
        assert!(
            matches!(
                MaterializationReader::inspect_materialization(
                    &Sqlite,
                    &database(&root),
                    data.binding.canonical_root(),
                    options()
                ),
                Err(Error::MaterializationVerification)
            ),
            "{case}"
        );
    }
}

#[test]
fn corrupted_completion_and_rewound_head_are_not_accepted_as_ownership() {
    for case in ["identity", "entries", "bytes", "journal", "head"] {
        let (root, mut writer) = fixture();
        let data = Data::new();
        let first = writer
            .record_materialization_intent(data.input(b"first", None), permission())
            .expect("actual intent");
        let completed = writer
            .record_materialization_completion(
                MaterializationCompletionInput {
                    intent: &first.record.intent,
                    read_back: &data.read_back(),
                },
                permission(),
            )
            .expect("actual complete head");
        if case == "head" {
            let second = writer
                .record_materialization_intent(
                    data.input(b"second", Some(first.record.intent.id)),
                    permission(),
                )
                .expect("second exact intent");
            writer
                .record_materialization_completion(
                    MaterializationCompletionInput {
                        intent: &second.record.intent,
                        read_back: &data.read_back(),
                    },
                    permission(),
                )
                .expect("new completed head");
        }
        writer.close().expect("close before corruption");
        let connection = Connection::open(database(&root)).expect("exact fixture database");
        if case == "head" {
            connection
                .execute(
                    "UPDATE gazette_materialization_frontiers SET head=?1",
                    [first.record.intent.id.as_bytes()],
                )
                .expect("rewound head still has valid foreign keys");
        } else {
            let mut completion = completed
                .record
                .completion
                .expect("original completed payload");
            match case {
                "identity" => completion.output = manifest(b"forged").identity(),
                "entries" => completion.entries += 1,
                "bytes" => completion.bytes += 1,
                "journal" => completion.recorded.journal_id = [0; 32],
                _ => unreachable!("enumerated fixture"),
            }
            let text = serde_json::to_string(&completion).expect("canonical forged completion");
            connection
                .execute(
                    "UPDATE gazette_materialization_completions SET payload=?1,payload_sha256=?2",
                    params![text, Sha256Digest::of(text.as_bytes()).as_bytes()],
                )
                .expect("hash-consistent corrupt history");
        }
        connection.close().expect("close mutation connection");
        assert!(
            matches!(
                MaterializationReader::inspect_materialization(
                    &Sqlite,
                    &database(&root),
                    data.binding.canonical_root(),
                    options()
                ),
                Err(Error::MaterializationVerification)
            ),
            "{case}"
        );
    }
}

#[test]
fn bounded_payload_and_whole_history_refuse_instead_of_returning_prefix() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let first = writer
        .record_materialization_intent(data.input(b"first", None), permission())
        .expect("intent");
    writer
        .record_materialization_completion(
            MaterializationCompletionInput {
                intent: &first.record.intent,
                read_back: &data.read_back(),
            },
            permission(),
        )
        .expect("completion");
    writer
        .record_materialization_intent(
            data.input(b"second", Some(first.record.intent.id)),
            permission(),
        )
        .expect("second intent");
    writer.close().expect("close");
    let error = MaterializationReader::inspect_materialization(
        &Sqlite,
        &database(&root),
        data.binding.canonical_root(),
        ReadOptions {
            value_bytes: 256,
            ..options()
        },
    )
    .expect_err("bounded row before allocation");
    assert!(matches!(
        error,
        Error::StorageLimit | Error::Provider { .. }
    ));
    let connection = Connection::open(database(&root)).expect("bounded semantic fixture");
    let current = crate::journal::inspect(&connection)
        .expect("native header")
        .header;
    let length: i64 = connection
        .query_row(
            "SELECT length(payload) FROM gazette_materialization_intents LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("actual row size");
    let length = u64::try_from(length).expect("nonnegative native row size");
    assert!(matches!(
        frontier(
            &connection,
            data.binding.canonical_root(),
            current,
            ReadOptions {
                storage_bytes: length + 256,
                ..options()
            }
        ),
        Err(Error::StorageLimit)
    ));
}

#[test]
fn exact_native_backup_retains_completed_history_and_pending_blocks_adoption_gate() {
    let (root, mut writer) = fixture();
    let data = Data::new();
    let first = writer
        .record_materialization_intent(data.input(b"first", None), permission())
        .expect("intent");
    writer
        .record_materialization_completion(
            MaterializationCompletionInput {
                intent: &first.record.intent,
                read_back: &data.read_back(),
            },
            permission(),
        )
        .expect("completion");
    let before = writer
        .inspect_materialization(data.binding.canonical_root())
        .expect("complete frontier")
        .frontier;
    writer.close().expect("close");
    let backups = private();
    let backup = Sqlite
        .backup(
            &database(&root),
            backups.path(),
            BackupOptions {
                allow_create: true,
                read: options(),
                ..Default::default()
            },
        )
        .expect("native online snapshot");
    let copied = MaterializationReader::inspect_materialization(
        &Sqlite,
        &backup.directory.join(crate::journal::DATABASE),
        data.binding.canonical_root(),
        options(),
    )
    .expect("backup has exact semantic history");
    assert_eq!(copied.frontier, before);
    let connection = Connection::open(database(&root)).expect("gate snapshot");
    require_terminal(&connection, options()).expect("completed output permits adoption gate");
    let header = crate::journal::inspect(&connection).expect("header").header;
    connection.close().expect("close reader");
    let mut writer = Sqlite
        .acquire_writer(root.path(), header, writer_options())
        .expect("next held writer");
    writer
        .record_materialization_intent(
            data.input(b"second", Some(first.record.intent.id)),
            permission(),
        )
        .expect("unresolved next operation");
    writer
        .close()
        .expect("uncertain filesystem phase is retained");
    let connection = Connection::open(database(&root)).expect("adoption gate snapshot");
    assert!(matches!(
        require_terminal(&connection, options()),
        Err(Error::MaterializationPending)
    ));
}

#[test]
fn real_coordinated_restore_adoption_refuses_unresolved_output_before_successor_effects() {
    use gazette_core::store::{LocalCoordinatorState, RestoreAdoptionOptions, RestoreOptions};
    use gazette_ports::store::{RestoreAdopter, RestoreAdoptionRequest, RestorePreparer};
    for completed in [false, true] {
        let journal = private();
        let anchor = private();
        let successor = private();
        Sqlite
            .initialize(
                journal.path(),
                InitializeOptions {
                    allow_create: true,
                    session: writer_options().session,
                    ..Default::default()
                },
            )
            .expect("fresh journal");
        let initial = crate::LocalCoordinator::enroll(
            anchor.path(),
            journal.path(),
            "configured-output-test",
            Sha256Digest::of(b"exact local principal"),
            writer_options().session,
            true,
        )
        .expect("native coordinator enrollment");
        let connector =
            crate::LocalCoordinator::new(anchor.path().into(), initial.binding.clone(), true);
        let mut writer = connector
            .acquire_writer(journal.path(), initial.header, writer_options())
            .expect("coordinated reserved writer");
        let protected = writer.materialization_roots().expect("actual held roots");
        assert_eq!(
            protected,
            vec![
                journal.path().canonicalize().expect("native journal"),
                anchor.path().canonicalize().expect("native anchor")
            ]
        );
        let mut data = Data::new();
        data.protected = protected;
        let intent = writer
            .record_materialization_intent(data.input(b"coordinated", None), permission())
            .expect("intent under native coordinator");
        if completed {
            writer
                .record_materialization_completion(
                    MaterializationCompletionInput {
                        intent: &intent.record.intent,
                        read_back: &data.read_back(),
                    },
                    permission(),
                )
                .expect("completed filesystem observation");
        }
        writer
            .close()
            .expect("exact SQL custody closes cleanly even when output phase is pending");
        let before = crate::LocalCoordinator::inspect(
            anchor.path(),
            &initial.binding,
            writer_options().session,
        )
        .expect("retained clean SQL frontier");
        let LocalCoordinatorState::Clean { seal, .. } = &before else {
            panic!("closed SQL lifetime is clean")
        };
        let backups = private();
        let restores = private();
        let backup = Sqlite
            .backup(
                &database(&journal),
                backups.path(),
                BackupOptions {
                    allow_create: true,
                    read: options(),
                    ..Default::default()
                },
            )
            .expect("exact private backup");
        let restore = Sqlite
            .prepare_restore(
                &backup.directory,
                backup.manifest_sha256,
                restores.path(),
                RestoreOptions {
                    allow_prepare: true,
                    verification: gazette_core::store::BackupVerifyOptions {
                        read: options(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("detached exact restore");
        let result = Sqlite.adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: anchor.path(),
                expected: &initial.binding,
                previous: seal,
                restore_root: &restore.directory,
                receipt_sha256: restore.receipt_sha256,
                successor_root: successor.path(),
            },
            RestoreAdoptionOptions {
                allow_adopt: true,
                allow_anchor_write: true,
                read: options(),
                ..Default::default()
            },
        );
        if completed {
            let adopted =
                result.expect("terminal output history does not forbid explicit native adoption");
            let copied = MaterializationReader::inspect_materialization(
                &Sqlite,
                &adopted
                    .successor
                    .binding
                    .journal_root
                    .join(crate::journal::DATABASE),
                data.binding.canonical_root(),
                options(),
            )
            .expect("completed history survives exact successor copy");
            assert_eq!(
                copied.frontier.head.as_ref().map(|record| record.intent.id),
                Some(intent.record.intent.id)
            );
            assert!(copied.frontier.active.is_none());
        } else {
            assert!(matches!(result, Err(Error::MaterializationPending)));
            assert_eq!(
                fs::read_dir(successor.path())
                    .expect("fresh successor remains")
                    .count(),
                0
            );
            assert_eq!(
                crate::LocalCoordinator::inspect(
                    anchor.path(),
                    &initial.binding,
                    writer_options().session
                )
                .expect("anchor unchanged"),
                before
            );
        }
        assert!(backup.directory.join(crate::journal::DATABASE).is_file());
        assert!(restore.directory.join(crate::journal::DATABASE).is_file());
        assert!(database(&journal).is_file());
    }
}
