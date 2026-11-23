//! Test the native transaction's own checks, not only the outer read-only preflight.

use super::*;
use gazette_core::store::{InitializeOptions, SessionOptions};
use gazette_ports::store::StoreInitializer;
use std::os::unix::fs::PermissionsExt;

fn fixture() -> (tempfile::TempDir, Writer) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable journal");
    let options = WriterOptions {
        allow_generation_advance: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read: ReadOptions {
                allow_coordination_files: true,
                ..ReadOptions::default()
            },
        },
    };
    let initial = Sqlite
        .initialize(
            root.path(),
            InitializeOptions {
                allow_create: true,
                session: options.session,
                ..InitializeOptions::default()
            },
        )
        .expect("production initialization");
    let writer = Sqlite
        .acquire_writer(root.path(), initial.header, options)
        .expect("production acquisition");
    (root, writer)
}

#[test]
fn immediate_transaction_rechecks_coordinates_changed_after_read_preflight() {
    for (sql, code) in [
        (
            "UPDATE gazette_journal SET journal_id=zeroblob(32);",
            "store_journal_mismatch",
        ),
        (
            "UPDATE gazette_journal SET journal_epoch=journal_epoch+1;",
            "store_journal_mismatch",
        ),
        (
            "UPDATE gazette_journal SET writer_fence=writer_fence+1;",
            "store_journal_mismatch",
        ),
        ("PRAGMA user_version=0;", "store_journal_schema"),
    ] {
        let (root, mut writer) = fixture();
        assert_eq!(
            writer.inspect().expect("previous read preflight").header,
            writer.header
        );
        let path = root.path().join(journal::DATABASE);
        Connection::open(&path)
            .expect("uncoordinated fixture writer")
            .execute_batch(sql)
            .expect("state changed after preflight");
        let before = fs::read(&path).expect("changed committed bytes");
        let mut effect = GenerationEffect::NotAttempted;
        // Calling the actual native transaction separately makes this fixture
        // fail if its check is removed while the outer inspection still exists.
        let error = writer
            .advance(&mut effect)
            .expect_err("transaction must independently reject stale binding");
        assert_eq!(error.code(), code);
        assert_eq!(effect, GenerationEffect::NotAttempted);
        assert_eq!(fs::read(&path).expect("no generation update"), before);
        assert_eq!(
            fs::read_dir(root.path())
                .expect("no leftover write sidecars")
                .count(),
            1
        );
    }
}

#[test]
fn generation_update_cas_itself_checks_identity_epoch_and_previous_generation() {
    for coordinate in ["identity", "epoch", "generation"] {
        let (root, writer) = fixture();
        let path = root.path().join(journal::DATABASE);
        let before = fs::read(&path).expect("baseline");
        let mut expected = writer.header;
        match coordinate {
            "identity" => expected.journal_id[0] ^= 1,
            "epoch" => expected.journal_epoch += 1,
            "generation" => expected.writer_fence += 1,
            _ => unreachable!("closed fixture coordinate set"),
        }
        let mut connection = Connection::open(&path).expect("native test connection");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("native write transaction");
        assert!(matches!(
            journal::advance_generation(&transaction, expected),
            Err(Error::JournalMismatch)
        ));
        transaction.commit().expect("commit exact zero-row update");
        connection.close().expect("close native fixture");
        assert_eq!(fs::read(&path).expect("CAS changed nothing"), before);
    }
}
