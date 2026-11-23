//! Real anchored `SQLite` transitions. Bodies/provider observations are supplied
//! representation fixtures, not signature, Git, remote-lock or publication proof.

#[path = "attempt.rs"]
mod attempts;
#[path = "reviewed_release.rs"]
mod reviewed_release;

use super::*;
use crate::LocalCoordinator;
use gazette_core::{
    release::{
        Authorization, AuthorizationSchema, Binding, CommitMetadata, CommitReport, CommitSignature,
        RefAdvertisement, RefComparison, RemoteState, Target, TargetReadback, TargetReadbackRow,
    },
    source::TreeIdentity,
    stage::Address,
    store::{
        BackupOptions, InitializeOptions, LocalCoordinatorBinding, LocalCoordinatorState,
        PreparationCommitInput, PreparationInput, PreparationRetentionInput,
        PreparationWorktreeInput, PreparationWriteOptions, ReleaseAuthorizationInput,
        ReleaseAuthorizationWriteOptions, ReleaseConsumptionInput, ReleaseConsumptionOptions,
        ReleaseLeaseOptions, ReleaseWriteEffect, RestoreAdoptionOptions, RestoreOptions,
        SessionOptions, WriterOptions,
    },
};
use gazette_ports::store::{
    JournalWriter, PreparationWriter, ReleaseAuthorizationWriter, ReleaseConsumptionWriter,
    ReleaseCoordinatorReader, RestoreAdopter, RestoreAdoptionRequest, RestorePreparer, StoreBackup,
    StoreInitializer, WriterConnector,
};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}
fn session(read: ReadOptions) -> SessionOptions {
    SessionOptions {
        allow_local_reservation: true,
        read,
    }
}
fn writer_options(read: ReadOptions) -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: session(read),
    }
}
fn lease_permission() -> ReleaseLeaseOptions {
    ReleaseLeaseOptions {
        allow_acquire: true,
    }
}
fn consume_permission() -> ReleaseConsumptionOptions {
    ReleaseConsumptionOptions {
        allow_consume: true,
    }
}
fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private native fixture")
}

struct Fixture {
    root: tempfile::TempDir,
    anchor: tempfile::TempDir,
    binding: LocalCoordinatorBinding,
    initial: JournalHeader,
}
impl Fixture {
    fn new() -> Self {
        let root = private();
        let anchor = private();
        let initial = Sqlite
            .initialize(
                root.path(),
                InitializeOptions {
                    allow_create: true,
                    session: session(options()),
                    ..Default::default()
                },
            )
            .expect("fresh journal");
        let seal = LocalCoordinator::enroll(
            anchor.path(),
            root.path(),
            "coordinator fixture",
            Sha256Digest::of(b"fixture principal"),
            session(options()),
            true,
        )
        .expect("fresh anchored enrollment");
        Self {
            root,
            anchor,
            binding: seal.binding,
            initial: initial.header,
        }
    }
    fn database(&self) -> PathBuf {
        Sqlite::journal_database(self.root.path())
    }
    fn writer(&self, expected: JournalHeader, read: ReadOptions) -> crate::Writer {
        LocalCoordinator::new(self.anchor.path().to_owned(), self.binding.clone(), true)
            .acquire_writer(self.root.path(), expected, writer_options(read))
            .expect("actual anchored generation")
    }
}

fn pending(writer: &mut crate::Writer, database: &Path, label: &str) -> ReleaseAuthorizationRecord {
    let write = PreparationWriteOptions { allow_record: true };
    let mut ids = Vec::new();
    let mut targets = Vec::new();
    for ordinal in 0..2_u32 {
        let id = Sha256Digest::of(format!("{label}-{ordinal}").as_bytes());
        let output: TreeIdentity = serde_json::from_value(serde_json::json!({"format": TreeIdentity::FORMAT, "sha256": Sha256Digest::of(&[ordinal as u8])})).expect("typed claimed output");
        let address = Address {
            parent: "/private/consumption-fixtures".into(),
            name: format!("{label}-{ordinal}"),
        };
        let intent = writer
            .record_preparation_intent(
                PreparationInput {
                    id,
                    address: &address,
                    destination: None,
                    output,
                    entries: 1,
                    bytes: 7,
                    evidence: "{}",
                    evidence_sha256: Sha256Digest::of(b"{}"),
                },
                write,
            )
            .expect("native preparation intent");
        let retention = writer
            .record_preparation_retention(
                PreparationRetentionInput {
                    intent: &intent.record.intent,
                    output,
                    entries: 1,
                    bytes: 7,
                },
                write,
            )
            .expect("native retention");
        let signature = CommitSignature {
            name: "Fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 1,
            offset_minutes: 0,
        };
        let metadata = CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Exact fixture\n".into(),
        };
        let worktree = writer
            .record_preparation_worktree(
                PreparationWorktreeInput {
                    intent: &retention.record.intent,
                    retention: retention.record.retention.as_ref().expect("retained"),
                    check: None,
                    metadata: &metadata,
                    parent_source: None,
                },
                write,
            )
            .expect("native recipe");
        let report = CommitReport {
            commit: format!("{:040x}", ordinal + 1)
                .parse()
                .expect("complete OID"),
            output,
            parent: None,
            entries: 1,
            bytes: 7,
        };
        writer
            .record_preparation_commit(
                PreparationCommitInput {
                    intent: &worktree.record.intent,
                    retention: worktree.record.retention.as_ref().expect("retained"),
                    check: None,
                    worktree: worktree.record.worktree.as_ref().expect("recipe"),
                    report: &report,
                },
                write,
            )
            .expect("native completion");
        targets.push(Target {
            ordinal,
            domain: format!("site{ordinal}.invalid"),
            remote: format!("/private/remote-{ordinal}.git"),
            branch: "main".into(),
            refspec: format!("{}:refs/heads/main", report.commit),
            expected_remote_oid_or_unborn: RemoteState::Commit {
                oid: "ffffffffffffffffffffffffffffffffffffffff"
                    .parse()
                    .expect("reviewed prestate"),
            },
            prepared_release_receipt_ref: Sha256Digest::of(b"application receipt"),
            local_commit_oid: report.commit,
            output_digest: output.sha256(),
        });
        ids.push(id);
    }
    let header = writer.inspect().expect("actual held header").header;
    let hash = Sha256Digest::of(b"representation, not signature");
    let body = Authorization {
        schema_id: AuthorizationSchema::WebsiteReleaseBatchAuthorizationV1,
        record_id: label.into(),
        nonce: label.into(),
        coordinator_id: "coordinator fixture".into(),
        journal_id: header.journal_id,
        journal_epoch: header.journal_epoch,
        writer_fence: header.writer_fence,
        ordered_targets: targets.clone(),
        validation_evidence_digest: hash,
        disclosure_evidence_digest: hash,
        policy_evidence_digest: hash,
        scanner_revision: "fixture".into(),
        generic_rule_set_digest: hash,
        website_policy_projection_digest: hash,
        approved_at: 1,
        not_before: 1,
        expires_at: 10,
        canonical_body_sha256: hash,
        publisher_authorizations: vec![],
    };
    let binding = Binding {
        canonical_body_sha256: hash,
        coordinator_id: body.coordinator_id.clone(),
        journal: header,
        ordered_targets: targets,
        publishers: vec![],
        authority_frontier_sha256: hash,
        revocation_frontier_sha256: hash,
        supersession_frontier_sha256: hash,
        observed_at: 2,
        emergency_stop: false,
    };
    let raw = serde_json::to_string_pretty(&body).expect("exact received bytes");
    writer
        .record_release_authorization(
            database,
            ReleaseAuthorizationInput {
                authorization: &raw,
                binding: &binding.clone().into(),
                preparations_sha256: hash,
                preparation_ids: &ids,
            },
            ReleaseAuthorizationWriteOptions { allow_record: true },
        )
        .expect("real pending registration")
        .record
}

fn readbacks(record: &ReleaseAuthorizationRecord) -> Vec<TargetReadbackRow> {
    record
        .binding
        .ordered_targets()
        .iter()
        .map(|target| {
            let RemoteState::Commit { oid } = &target.expected_remote_oid_or_unborn else {
                panic!("fixture prestate")
            };
            let advertisement = RefAdvertisement::Commit { oid: oid.clone() };
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
fn input<'a>(
    registration: &'a ReleaseAuthorizationRecord,
    leases: &'a ReleaseLeases,
    rows: &'a [TargetReadbackRow],
) -> ReleaseConsumptionInput<'a> {
    ReleaseConsumptionInput {
        registration,
        binding: &registration.binding,
        leases,
        readbacks: rows,
        preparations_sha256: registration.preparations_sha256,
    }
}

#[test]
fn anchored_complete_leases_and_consumption_survive_reopen_without_second_consumption() {
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let registration = pending(&mut writer, &path, "first");
    let leases = writer
        .acquire_release_leases("first", lease_permission())
        .expect("complete native lease set");
    assert_eq!(leases.targets.len(), 2);
    assert!(leases.targets.iter().all(|lease| lease.generation == 1));
    let before = fs::read(&path).expect("before lease replay");
    assert_eq!(
        writer
            .acquire_release_leases("first", lease_permission())
            .expect("same held set replay"),
        leases
    );
    assert_eq!(fs::read(&path).expect("lease replay unchanged"), before);
    let rows = readbacks(&registration);
    let receipt = writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .expect("one actual consumption");
    assert_eq!(receipt.record.targets.len(), 2);
    let header = receipt.journal;
    writer.close().expect("seal consumed state");
    let before = fs::read(&path).expect("before historical inspection");
    assert_eq!(
        Sqlite
            .inspect_release_consumption(&path, "first", options())
            .expect("original full history")
            .record,
        Some(receipt.record.clone())
    );
    assert_eq!(fs::read(&path).expect("no historical mutation"), before);
    let mut next = fixture.writer(header, options());
    assert_eq!(
        next.inspect_release_consumption("first")
            .expect("old intent still inspectable")
            .record,
        Some(receipt.record)
    );
    assert!(
        next.consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission()
        )
        .is_err(),
        "old coordinates cannot authorize another consumption"
    );
    next.close().expect("pure stale holder refusal");
}

#[test]
fn restore_adoption_refuses_nonterminal_consumed_release_before_touching_successor() {
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let registration = pending(&mut writer, &path, "nonterminal-restore");
    let leases = writer
        .acquire_release_leases("nonterminal-restore", lease_permission())
        .expect("complete native lease set");
    let rows = readbacks(&registration);
    writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .expect("valid consumed release without terminal settlement");
    writer.close().expect("seal nonterminal consumed history");
    let LocalCoordinatorState::Clean { seal, .. } =
        LocalCoordinator::inspect(fixture.anchor.path(), &fixture.binding, session(options()))
            .expect("latest clean consumed state")
    else {
        panic!("closed writer must leave a clean coordinator")
    };

    let backups = private();
    let backup = Sqlite
        .backup(
            &path,
            backups.path(),
            BackupOptions {
                allow_create: true,
                read: options(),
                ..Default::default()
            },
        )
        .expect("complete native backup");
    let restores = private();
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
    let successor = private();
    let error = Sqlite
        .adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: fixture.anchor.path(),
                expected: &seal.binding,
                previous: &seal,
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
        )
        .expect_err("nonterminal release history cannot move to another binding");
    assert!(matches!(error, Error::ReleaseReconciliationRequired));
    assert_eq!(
        fs::read_dir(successor.path())
            .expect("untouched successor")
            .count(),
        0
    );
    let state =
        LocalCoordinator::inspect(fixture.anchor.path(), &fixture.binding, session(options()))
            .expect("unchanged clean coordinator");
    assert_eq!(
        state,
        LocalCoordinatorState::Clean {
            seal,
            adoption: None,
        }
    );
}

#[test]
fn replacing_derived_registry_preserves_consumed_intent_and_original_leases() {
    use gazette_core::{
        publication::{
            Key, Principal, PrincipalKind,
            content::{Declaration, Graph},
            registry::{Projection, Site},
        },
        store::{RegistryInput, RegistryWriteOptions},
    };
    use gazette_ports::store::{
        RegistryReader, RegistryWriter, ReleaseAttemptReader, ReleaseReconciliationReader,
    };
    use std::collections::BTreeMap;

    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let registration = pending(&mut writer, &path, "first");
    let leases = writer
        .acquire_release_leases("first", lease_permission())
        .expect("original complete leases");
    let rows = readbacks(&registration);
    let consumed = writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .expect("original complete consumed intent");
    let attempt_history = attempts::retained_fixture(&mut writer, &path, &consumed.record);
    let reconciliation_history = Sqlite
        .inspect_release_reconciliations(&path, "first", options())
        .expect("original reconciliation history")
        .records;
    assert_eq!(reconciliation_history.len(), 1);
    let graph = Graph::resolve(
        Declaration {
            nodes: vec![],
            memberships: vec![],
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        Default::default(),
    )
    .expect("empty content graph");
    let site: Key = "site:one".parse().expect("site key");
    let mut previous = None;
    for name in ["First principal", "Revised principal"] {
        let principal = Principal {
            id: "person:one".parse().expect("principal key"),
            kind: PrincipalKind::Person,
            name: Some(name.into()),
            website: None,
            same_as: Vec::new(),
        };
        let model = Projection {
            principals: BTreeMap::from([(&principal.id, &principal)]),
            projects: BTreeMap::new(),
            taxonomies: BTreeMap::new(),
            extensions: BTreeMap::new(),
            brands: BTreeMap::new(),
            sites: BTreeMap::from([(
                &site,
                Site {
                    publisher: &principal.id,
                    rights_holder: None,
                    default_author: None,
                    roles: &[],
                    deployable: false,
                    brand: None,
                    public_base_url: None,
                    repository: None,
                    discussion: None,
                },
            )]),
            placements: vec![],
            content: &graph,
            routes: BTreeMap::new(),
            provenance: BTreeMap::new(),
        };
        let replacement = writer
            .replace_registry(
                RegistryInput {
                    model: &model,
                    documents: &BTreeMap::new(),
                    semantic_sha256: model.semantic_sha256(1 << 20).expect("model identity"),
                    configuration_sha256: Sha256Digest::of(name.as_bytes()),
                },
                RegistryWriteOptions {
                    allow_replace: true,
                },
            )
            .expect("real complete-set registry replacement");
        assert_eq!(replacement.journal, consumed.journal);
        assert_eq!(
            Sqlite
                .inspect_release_reconciliations(&path, "first", options())
                .expect("registry replacement preserves reconciliations")
                .records,
            reconciliation_history
        );
        assert_eq!(
            Sqlite
                .inspect_release_attempts(&path, "first", options())
                .expect("registry replacement preserves complete and incomplete attempts")
                .attempts,
            attempt_history
        );
        assert_ne!(previous, Some(replacement.registry.rows_sha256));
        previous = Some(replacement.registry.rows_sha256);
        assert_eq!(
            Sqlite
                .inspect_registry(&path, options())
                .expect("registry read-back")
                .registry,
            Some(replacement.registry)
        );
        assert_eq!(
            writer
                .inspect_release_consumption("first")
                .expect("held intent inspection")
                .record,
            Some(consumed.record.clone())
        );
        assert_eq!(
            Sqlite
                .inspect_release_leases(&path, "first", options())
                .expect("independent original leases")
                .record,
            Some(leases.clone())
        );
    }
    writer.close().expect("seal same anchored lifetime");
    let before = fs::read(&path).expect("sealed native bytes");
    assert_eq!(
        Sqlite
            .inspect_release_reconciliations(&path, "first", options())
            .expect("reopened reconciliations after registry replacement")
            .records,
        reconciliation_history
    );
    assert_eq!(
        Sqlite
            .inspect_release_attempts(&path, "first", options())
            .expect("reopened attempts after registry replacement")
            .attempts,
        attempt_history
    );
    assert_eq!(
        Sqlite
            .inspect_release_consumption(&path, "first", options())
            .expect("reopened complete intent")
            .record,
        Some(consumed.record)
    );
    assert_eq!(
        Sqlite
            .inspect_release_leases(&path, "first", options())
            .expect("reopened original leases")
            .record,
        Some(leases)
    );
    assert_eq!(fs::read(&path).expect("read-only native bytes"), before);
}

#[test]
fn exact_second_consumption_refuses_and_preserves_single_batch() {
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let record = pending(&mut writer, &path, "first");
    let leases = writer
        .acquire_release_leases("first", lease_permission())
        .expect("leases");
    let rows = readbacks(&record);
    let original = writer
        .consume_release_authorization(&path, input(&record, &leases, &rows), consume_permission())
        .expect("first consumed");
    let error = writer
        .consume_release_authorization(&path, input(&record, &leases, &rows), consume_permission())
        .expect_err("one use");
    assert!(matches!(error.cause, Error::ReleaseAlreadyConsumed));
    assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted);
    drop(writer);
    assert_eq!(
        Sqlite
            .inspect_release_consumption(&path, "first", options())
            .expect("one original")
            .record,
        Some(original.record)
    );
}

#[test]
fn ordinary_and_wrong_coordinator_writers_never_acquire_release_leases() {
    let fixture = Fixture::new();
    let mut ordinary = Sqlite
        .acquire_writer(
            fixture.root.path(),
            fixture.initial,
            writer_options(options()),
        )
        .expect("ordinary native writer");
    assert!(matches!(
        ordinary.inspect_release_coordinator(),
        Err(Error::CoordinatorMismatch)
    ));
    let error = ordinary
        .acquire_release_leases("absent", lease_permission())
        .expect_err("ordinary cannot lease");
    assert!(matches!(error.cause, Error::CoordinatorMismatch));
    assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted);
    ordinary
        .close()
        .expect("ordinary close is not anchor repair");
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let original = pending(&mut writer, &path, "source");
    let mut binding = original.binding.clone();
    binding.as_signed_mut().unwrap().coordinator_id = "wrong coordinator".into();
    let mut body = original
        .as_input()
        .validate()
        .expect("body")
        .as_signed()
        .unwrap()
        .clone();
    body.record_id = "wrong".into();
    body.nonce = "wrong".into();
    body.coordinator_id = binding.coordinator_id().clone();
    let raw = serde_json::to_string(&body).expect("wrong holder body");
    writer
        .record_release_authorization(
            &path,
            ReleaseAuthorizationInput {
                authorization: &raw,
                binding: &binding,
                preparations_sha256: original.preparations_sha256,
                preparation_ids: &original.preparation_ids,
            },
            ReleaseAuthorizationWriteOptions { allow_record: true },
        )
        .expect("pending custody does not grant holder authority");
    assert!(
        writer
            .acquire_release_leases("wrong", lease_permission())
            .is_err()
    );
}

#[test]
fn same_generation_competitor_refuses_and_new_generation_only_supersedes_pending_holder() {
    for consume in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.database();
        let mut writer = fixture.writer(fixture.initial, options());
        let first = pending(&mut writer, &path, "first");
        let leases = writer
            .acquire_release_leases("first", lease_permission())
            .expect("first holder");
        if consume {
            writer
                .consume_release_authorization(
                    &path,
                    input(&first, &leases, &readbacks(&first)),
                    consume_permission(),
                )
                .expect("unsettled consumption");
        }
        let header = writer.inspect().expect("retain exact generation").header;
        writer
            .close()
            .expect("seal before explicit next generation");
        let mut writer = fixture.writer(header, options());
        pending(&mut writer, &path, "next");
        let next = writer.acquire_release_leases("next", lease_permission());
        if consume {
            assert!(matches!(
                next.expect_err("unresolved consumed holder cannot be stolen")
                    .cause,
                Error::ReleaseLeaseConflict
            ));
        } else {
            assert!(
                next.expect("older pending-only holder may yield")
                    .targets
                    .iter()
                    .all(|lease| lease.generation == 2)
            );
            writer.close().expect("seal successor");
            assert_eq!(
                Sqlite
                    .inspect_release_leases(&path, "first", options())
                    .expect("historical superseded leases remain readable")
                    .record,
                Some(leases)
            );
        }
    }
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    pending(&mut writer, &path, "first");
    pending(&mut writer, &path, "competitor");
    writer
        .acquire_release_leases("first", lease_permission())
        .expect("first lease");
    let error = writer
        .acquire_release_leases("competitor", lease_permission())
        .expect_err("same-fence competitor");
    assert!(matches!(error.cause, Error::ReleaseLeaseConflict));
    assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted);
}

#[test]
fn changed_leases_prestate_registration_or_database_never_consumes() {
    for change in [
        "generation",
        "partial",
        "prestate",
        "registration",
        "database",
        "permission",
    ] {
        let fixture = Fixture::new();
        let path = fixture.database();
        let mut writer = fixture.writer(fixture.initial, options());
        let mut record = pending(&mut writer, &path, "first");
        let mut leases = writer
            .acquire_release_leases("first", lease_permission())
            .expect("leases");
        let mut rows = readbacks(&record);
        match change {
            "generation" => leases.targets[0].generation += 1,
            "partial" => {
                rows.pop();
            }
            "prestate" => {
                rows[0].readback = TargetReadback::Observed {
                    advertisement: RefAdvertisement::NotAdvertised,
                    comparison: RefComparison::Indeterminate,
                };
            }
            "registration" => record.authorization.push(' '),
            _ => {}
        }
        let before = fs::read(&path).expect("before refused consume");
        let selected = if change == "database" {
            path.with_file_name("other.sqlite")
        } else {
            path.clone()
        };
        let permission = if change == "permission" {
            Default::default()
        } else {
            consume_permission()
        };
        let error = writer
            .consume_release_authorization(&selected, input(&record, &leases, &rows), permission)
            .expect_err(change);
        assert_eq!(error.effect, ReleaseWriteEffect::NotAttempted, "{change}");
        drop(writer);
        assert_eq!(fs::read(&path).expect("no consume bytes"), before);
        assert!(
            Sqlite
                .inspect_release_consumption(&path, "first", options())
                .expect("no intent")
                .record
                .is_none()
        );
    }
}

#[test]
fn failed_target_fk_rolls_back_batch_and_preceding_target_intent() {
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let registration = pending(&mut writer, &path, "first");
    let leases = writer
        .acquire_release_leases("first", lease_permission())
        .expect("leases");
    let mut record =
        ReleaseConsumption::from_input(input(&registration, &leases, &readbacks(&registration)))
            .expect("representation only");
    writer.close().expect("close before native FK test");
    record.targets[1].lease_generation += 1;
    let mut connection = Connection::open(&path).expect("native fixture connection");
    connection
        .execute_batch("PRAGMA foreign_keys=ON")
        .expect("enforced FKs");
    {
        let transaction = connection.transaction().expect("atomic production owner");
        assert!(insert_consumption(&transaction, &record, options()).is_err());
        let rows: i64 = transaction
            .query_row(
                "SELECT count(*) FROM gazette_release_target_intents",
                [],
                |row| row.get(0),
            )
            .expect("first target was inserted");
        assert_eq!(rows, 1);
    }
    assert!(!consumed(&connection, "first").expect("batch rolled back"));
    let rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM gazette_release_target_intents",
            [],
            |row| row.get(0),
        )
        .expect("no target remains");
    assert_eq!(rows, 0);
}

#[test]
fn corrupted_domains_partial_rows_and_payloads_fail_without_repair() {
    for change in [
        "batch-domain",
        "target-domain",
        "target-missing",
        "lease-missing",
        "body",
        "unknown",
    ] {
        let fixture = Fixture::new();
        let path = fixture.database();
        let mut writer = fixture.writer(fixture.initial, options());
        let registration = pending(&mut writer, &path, "first");
        let leases = writer
            .acquire_release_leases("first", lease_permission())
            .expect("leases");
        let receipt = writer
            .consume_release_authorization(
                &path,
                input(&registration, &leases, &readbacks(&registration)),
                consume_permission(),
            )
            .expect("complete intent");
        writer.close().expect("seal before corruption");
        let connection = Connection::open(&path).expect("controlled corruption");
        connection
            .execute_batch("PRAGMA foreign_keys=OFF")
            .expect("deliberate historical corruption fixture");
        match change {
            "batch-domain" => {
                let bytes = encode(&receipt.record, options()).expect("original bytes");
                connection
                    .execute(
                        "UPDATE gazette_release_consumptions SET payload_sha256=?1",
                        [digest(TARGET_DOMAIN, &bytes).as_bytes()],
                    )
                    .expect("wrong record domain");
            }
            "target-domain" => {
                connection.execute("UPDATE gazette_release_target_intents SET payload_sha256=zeroblob(32) WHERE ordinal=0", []).expect("wrong target hash");
            }
            "target-missing" => {
                connection
                    .execute(
                        "DELETE FROM gazette_release_target_intents WHERE ordinal=1",
                        [],
                    )
                    .expect("partial target set");
            }
            "lease-missing" => {
                connection
                    .execute(
                        "DELETE FROM gazette_release_lease_history WHERE ordinal=1",
                        [],
                    )
                    .expect("partial lease history");
            }
            change => {
                let mut value = serde_json::to_value(&receipt.record).expect("body");
                if change == "unknown" {
                    value["extra"] = true.into();
                } else {
                    value["authorization_id"] = "different".into();
                }
                let bytes = serde_json::to_vec(&value).expect("changed bytes");
                connection
                    .execute(
                        "UPDATE gazette_release_consumptions SET payload=?1,payload_sha256=?2",
                        params![bytes, digest(BATCH_DOMAIN, &bytes).as_bytes()],
                    )
                    .expect("rehash tamper");
            }
        }
        connection.close().expect("corruptor closed");
        let before = fs::read(&path).expect("corrupted bytes");
        assert!(
            Sqlite
                .inspect_release_consumption(&path, "first", options())
                .is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&path).expect("no repair"), before);
    }
}

#[test]
fn committed_consumption_survives_failed_bounded_delivery_and_dirty_anchor() {
    use gazette_core::release::{KeyState, PublisherBinding, PublisherRole};
    let fixture = Fixture::new();
    let path = fixture.database();
    let size = fs::metadata(&path).expect("fresh native allocation").len();
    let ceiling = size + 64 * 1024;
    let bounded = ReadOptions {
        storage_bytes: ceiling,
        ..options()
    };
    let mut writer = fixture.writer(fixture.initial, bounded);
    let registration = pending(&mut writer, &path, "first");
    let leases = writer
        .acquire_release_leases("first", lease_permission())
        .expect("leases within selected ceiling");
    let rows = readbacks(&registration);
    let mut binding = registration.binding.clone();
    // The adapter stores bounded representation, not key validity. Application
    // tests separately exercise genuine signatures; this stresses retained bytes.
    binding
        .as_signed_mut()
        .unwrap()
        .publishers
        .push(PublisherBinding {
            publisher_id: "fixture".into(),
            publisher_key_id: "fixture".into(),
            publisher_key_epoch: 1,
            authority_reference: Sha256Digest::of(b"fixture"),
            role: PublisherRole::Publisher,
            authorized_target_ordinals: vec![0, 1],
            public_key: "x".repeat(usize::try_from(ceiling / 2).expect("bounded fixture")),
            state: KeyState::Active,
        });
    let input = ReleaseConsumptionInput {
        binding: &binding,
        ..input(&registration, &leases, &rows)
    };
    encode(&input, bounded).expect("complete input itself fits before insertion");
    let error = writer
        .consume_release_authorization(&path, input, consume_permission())
        .expect_err("independent read-back exceeds storage ceiling after commit");
    assert!(matches!(error.cause, Error::StorageLimit));
    assert_eq!(error.effect, ReleaseWriteEffect::Unverified);
    assert!(
        writer.close().is_err(),
        "failed write cannot seal anchored lifetime"
    );
    let recovered = Sqlite
        .inspect_release_consumption(&path, "first", options())
        .expect("independent wider-bound historical read");
    let record = recovered
        .record
        .expect("lost delivery did not undo consumption");
    assert_eq!(record.binding, binding);
    assert_eq!(record.targets.len(), 2);
    assert_eq!(record.created, leases.coordinator.journal);
    assert_eq!(
        Sqlite
            .inspect_release_leases(&path, "first", options())
            .expect("dirty custody does not erase original lease history")
            .record,
        Some(leases)
    );
}
