//! Exact latest-clean restore adoption; inputs survive and replay is evidence-bound.

use super::*;
use gazette_core::store::{
    BackupOptions, LocalCoordinatorBinding, LocalCoordinatorState, RestoreAdoptionOptions,
    RestoreAdoptionSource, RestoreAdoptionTarget, RestoreOptions,
};
use gazette_ports::store::{RestoreAdopter, RestoreAdoptionRequest, RestorePreparer, StoreBackup};
use rusqlite::{Connection, TransactionBehavior};

fn prepared(
    fixture: &Fixture,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    gazette_core::store::RestoreReceipt,
) {
    let backups = private();
    let backup = Sqlite
        .backup(
            &fixture.database(),
            backups.path(),
            BackupOptions {
                allow_create: true,
                read: session().read,
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
                    read: session().read,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("detached exact restore");
    (backups, restores, restore)
}

fn adoption_options() -> RestoreAdoptionOptions {
    RestoreAdoptionOptions {
        allow_adopt: true,
        allow_anchor_write: true,
        read: session().read,
        ..Default::default()
    }
}

fn begin_adoption(
    fixture: &Fixture,
    restore: &gazette_core::store::RestoreReceipt,
    successor: &tempfile::TempDir,
) -> (RestoreAdoptionSource, RestoreAdoptionTarget) {
    let source = RestoreAdoptionSource {
        restore_root: restore.directory.canonicalize().expect("canonical restore"),
        receipt_sha256: restore.receipt_sha256,
        source_manifest_sha256: restore.preparation.source_manifest_sha256,
        database_sha256: restore.preparation.sha256,
        content_sha256: fixture.seal.content_sha256,
    };
    let binding = LocalCoordinatorBinding {
        journal_root: successor
            .path()
            .canonicalize()
            .expect("canonical successor"),
        ..fixture.seal.binding.clone()
    };
    let next = RestoreAdoptionTarget {
        binding,
        header: gazette_core::store::JournalHeader {
            writer_fence: fixture.seal.header.writer_fence + 1,
            ..fixture.seal.header
        },
    };
    let anchor = Anchor::acquire(fixture.anchor.path(), adoption_options().read)
        .expect("held coordinator anchor");
    let observed = anchor.read().expect("current clean anchor");
    anchor
        .write(
            LocalCoordinatorState::Adopting {
                previous: fixture.seal.clone(),
                source: source.clone(),
                next: next.clone(),
                transaction: None,
            },
            Some(&observed),
        )
        .expect("durable adoption intent");
    (source, next)
}

fn copy_restore(restore: &gazette_core::store::RestoreReceipt, successor: &tempfile::TempDir) {
    fs::copy(
        restore.directory.join(journal::DATABASE),
        successor.path().join(journal::DATABASE),
    )
    .expect("exact copied restore stage");
}

fn advance_copy(fixture: &Fixture, successor: &tempfile::TempDir) {
    let mut connection = Connection::open(successor.path().join(journal::DATABASE))
        .expect("copied successor connection");
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("successor transaction");
    journal::advance_generation(&transaction, fixture.seal.header)
        .expect("one successor fence advance");
    let expected = snapshot::capture(&transaction, session().read).expect("expected next state");
    let anchor = Anchor::acquire(fixture.anchor.path(), session().read).expect("held anchor");
    let observed = anchor.read().expect("adopting anchor");
    let LocalCoordinatorState::Adopting {
        previous,
        source,
        next,
        ..
    } = observed.state.clone()
    else {
        panic!("adoption intent remains active")
    };
    anchor
        .write(
            LocalCoordinatorState::Adopting {
                previous,
                source,
                next,
                transaction: Some(expected),
            },
            Some(&observed),
        )
        .expect("persist expected successor before commit");
    transaction.commit().expect("committed successor advance");
    connection.close().expect("closed successor connection");
}

#[test]
fn latest_clean_restore_adopts_once_and_exact_replay_returns_same_successor() {
    let fixture = Fixture::new();
    let (_backups, _restores, restore) = prepared(&fixture);
    let old_bytes = fs::read(fixture.database()).expect("old journal bytes");
    let restore_bytes = fs::read(restore.directory.join(journal::DATABASE)).expect("restore bytes");
    let successor = private();

    let first = Sqlite
        .adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: fixture.anchor.path(),
                expected: &fixture.seal.binding,
                previous: &fixture.seal,
                restore_root: &restore.directory,
                receipt_sha256: restore.receipt_sha256,
                successor_root: successor.path(),
            },
            adoption_options(),
        )
        .expect("verified latest-clean adoption");
    assert_eq!(
        first.successor.header.writer_fence,
        fixture.seal.header.writer_fence + 1
    );
    assert_eq!(
        first.successor.binding.journal_root,
        successor
            .path()
            .canonicalize()
            .expect("canonical successor")
    );
    assert_eq!(
        fs::read(fixture.database()).expect("old remains"),
        old_bytes
    );
    assert_eq!(
        fs::read(restore.directory.join(journal::DATABASE)).expect("restore remains"),
        restore_bytes
    );
    let second = Sqlite
        .adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: fixture.anchor.path(),
                expected: &fixture.seal.binding,
                previous: &fixture.seal,
                restore_root: &restore.directory,
                receipt_sha256: restore.receipt_sha256,
                successor_root: successor.path(),
            },
            adoption_options(),
        )
        .expect("exact replay");
    assert_eq!(second, first);
    let state =
        LocalCoordinator::inspect(fixture.anchor.path(), &first.successor.binding, session())
            .expect("successor clean state");
    assert!(matches!(
        state,
        LocalCoordinatorState::Clean {
            adoption: Some(_),
            ..
        }
    ));
}

#[test]
fn adoption_replays_every_provable_crash_stage_without_recopying_unknown_residue() {
    for stage in ["intent", "copied", "advanced"] {
        let fixture = Fixture::new();
        let (_backups, _restores, restore) = prepared(&fixture);
        let successor = private();
        let old_bytes = fs::read(fixture.database()).expect("old journal bytes");
        let restore_bytes =
            fs::read(restore.directory.join(journal::DATABASE)).expect("restore bytes");
        let (_, expected) = begin_adoption(&fixture, &restore, &successor);
        if matches!(stage, "copied" | "advanced") {
            copy_restore(&restore, &successor);
        }
        if stage == "advanced" {
            advance_copy(&fixture, &successor);
        }

        let receipt = Sqlite
            .adopt_restore(
                RestoreAdoptionRequest {
                    anchor_root: fixture.anchor.path(),
                    expected: &fixture.seal.binding,
                    previous: &fixture.seal,
                    restore_root: &restore.directory,
                    receipt_sha256: restore.receipt_sha256,
                    successor_root: successor.path(),
                },
                adoption_options(),
            )
            .unwrap_or_else(|error| panic!("{stage} replay must close: {error}"));
        assert_eq!(receipt.successor.binding, expected.binding, "{stage}");
        assert_eq!(receipt.successor.header, expected.header, "{stage}");
        assert_eq!(
            fs::read(fixture.database()).expect("old remains"),
            old_bytes
        );
        assert_eq!(
            fs::read(restore.directory.join(journal::DATABASE)).expect("restore remains"),
            restore_bytes
        );
        assert!(matches!(
            LocalCoordinator::inspect(fixture.anchor.path(), &receipt.successor.binding, session())
                .expect("successor clean anchor"),
            LocalCoordinatorState::Clean {
                adoption: Some(_),
                ..
            }
        ));
    }

    let fixture = Fixture::new();
    let (_backups, _restores, restore) = prepared(&fixture);
    let successor = private();
    begin_adoption(&fixture, &restore, &successor);
    let residue = b"interrupted copy is preserved";
    let residue_path = successor.path().join(journal::DATABASE);
    fs::write(&residue_path, residue).expect("partial copy residue");
    fs::set_permissions(&residue_path, fs::Permissions::from_mode(0o600))
        .expect("private partial copy");
    let error = Sqlite
        .adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: fixture.anchor.path(),
                expected: &fixture.seal.binding,
                previous: &fixture.seal,
                restore_root: &restore.directory,
                receipt_sha256: restore.receipt_sha256,
                successor_root: successor.path(),
            },
            adoption_options(),
        )
        .expect_err("unproved partial copy must stop");
    assert!(
        matches!(
            error,
            Error::RestoreAdoptionVerification | Error::BackupVerification | Error::Provider { .. }
        ),
        "unexpected partial-copy classification: {error:?}"
    );
    assert_eq!(fs::read(residue_path).expect("residue retained"), residue);
    assert!(matches!(
        fixture.state(),
        LocalCoordinatorState::Adopting { .. }
    ));
}

#[test]
fn stale_or_dirty_frontier_never_adopts_or_touches_fresh_successor() {
    let fixture = Fixture::new();
    let (_backups, _restores, restore) = prepared(&fixture);
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("advance current frontier");
    let newer = writer.inspect().expect("new header").header;
    writer.close().expect("new clean seal");
    assert!(newer.writer_fence > fixture.seal.header.writer_fence);
    let successor = private();
    let error = Sqlite
        .adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: fixture.anchor.path(),
                expected: &fixture.seal.binding,
                previous: &fixture.seal,
                restore_root: &restore.directory,
                receipt_sha256: restore.receipt_sha256,
                successor_root: successor.path(),
            },
            adoption_options(),
        )
        .expect_err("older clean frontier cannot be adopted");
    assert!(matches!(
        error,
        Error::CoordinatorState | Error::CoordinatorMismatch
    ));
    assert_eq!(
        fs::read_dir(successor.path()).expect("fresh root").count(),
        0
    );

    let LocalCoordinatorState::Clean { seal, .. } = fixture.state() else {
        panic!("current frontier remains clean")
    };
    let dirty = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), seal.header, options())
        .expect("dirty held generation");
    drop(dirty);
    let another = private();
    let error = Sqlite
        .adopt_restore(
            RestoreAdoptionRequest {
                anchor_root: fixture.anchor.path(),
                expected: &seal.binding,
                previous: &seal,
                restore_root: &restore.directory,
                receipt_sha256: restore.receipt_sha256,
                successor_root: another.path(),
            },
            adoption_options(),
        )
        .expect_err("in-progress custody cannot adopt");
    assert!(matches!(error, Error::CoordinatorState));
    assert_eq!(fs::read_dir(another.path()).expect("fresh root").count(), 0);
}
