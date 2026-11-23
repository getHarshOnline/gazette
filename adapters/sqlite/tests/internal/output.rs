//! Shared output sealing preserves residue on late failures instead of overwriting it.

use super::{OutputDirectory, RANDOM_CHARACTERS, open_directory};
use crate::Sqlite;
use gazette_core::store::{
    BackupOptions, BackupVerifyOptions, Error, InitializeOptions, OutputLimits, ReadOptions,
    RestoreOptions, SessionOptions,
};
use gazette_ports::store::{BackupReader, RestorePreparer, StoreBackup, StoreInitializer};
use std::{
    ffi::OsStr,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, symlink},
    },
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[test]
fn output_directory_handles_reject_regular_files_and_final_symlink_aliases() {
    let root = tempdir().expect("private fixture");
    let path = root.path().canonicalize().expect("trusted parent");
    assert!(
        open_directory(&path)
            .expect("native directory")
            .metadata()
            .expect("opened metadata")
            .is_dir()
    );
    let file = path.join("regular");
    fs::write(&file, b"not a directory").expect("fixture file");
    assert!(open_directory(&file).is_err());
    let alias = path.join("alias");
    symlink(&path, &alias).expect("same-directory alias");
    assert!(open_directory(&alias).is_err());
    assert_eq!(fs::read(&file).expect("preserved"), b"not a directory");
}

#[test]
fn a_conflicting_receipt_is_preserved_and_the_failed_directory_remains_reported() {
    let parent = tempdir().expect("private parent");
    let limits = OutputLimits {
        directory_bytes: generated_bytes(parent.path(), "fixture-"),
    };
    let output = OutputDirectory::create(parent.path(), "fixture-", limits)
        .expect("production output owner");
    fs::write(
        output.path().join("store.sqlite"),
        b"unverified database residue",
    )
    .expect("fixture bytes");
    fs::write(output.path().join("restore.json"), b"foreign receipt").expect("interference");
    let error = output
        .seal("restore.json", &serde_json::json!({"new":true}))
        .expect_err("do not replace foreign inventory");
    assert!(matches!(error, Error::Changed));
    let failure = output.failure(error);
    let path = failure.directory.expect("reported residue");
    assert_eq!(path.as_os_str().as_bytes().len(), limits.directory_bytes);
    drop(output);
    assert_eq!(
        fs::read(path.join("restore.json")).expect("retained"),
        b"foreign receipt"
    );
    assert_eq!(
        fs::read(path.join("store.sqlite")).expect("retained"),
        b"unverified database residue"
    );
}

fn private_child(parent: &Path, name: &OsStr) -> PathBuf {
    let path = parent.join(name);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .expect("private fixture directory");
    path
}

fn read_options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..ReadOptions::default()
    }
}

fn initialize_options(directory_bytes: usize) -> InitializeOptions {
    InitializeOptions {
        allow_create: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read: read_options(),
        },
        output: OutputLimits { directory_bytes },
        ..InitializeOptions::default()
    }
}

fn generated_bytes(parent: &Path, prefix: &str) -> usize {
    parent
        .canonicalize()
        .expect("resolved fixture parent")
        .join(prefix)
        .as_os_str()
        .as_bytes()
        .len()
        + RANDOM_CHARACTERS
}

#[test]
fn initialization_bounds_canonical_unix_bytes_before_creation_and_retains_late_failure() {
    let root = tempdir().expect("fixture root");
    let long = private_child(root.path(), OsStr::new("long-canonical-directory"));
    let name = OsStr::new("native-雪-\"\\\n");
    let selected = private_child(&long, name);
    let alias = root.path().join("a");
    symlink(&long, &alias).expect("short ancestor alias");
    let supplied = alias.join(name);
    let canonical = selected.canonicalize().expect("actual native directory");
    let exact = canonical.as_os_str().as_bytes().len();
    assert!(
        canonical
            .to_str()
            .expect("native Unicode path")
            .chars()
            .count()
            < exact
    );
    assert!(supplied.as_os_str().as_bytes().len() < exact);

    for limit in [
        0,
        1024 * 1024 + 1,
        supplied.as_os_str().as_bytes().len(),
        exact - 1,
    ] {
        let error = Sqlite
            .initialize(&supplied, initialize_options(limit))
            .expect_err("invalid or insufficient resolved-directory allowance");
        assert!(matches!(error.cause, Error::InvalidLimits));
        assert!(
            error.directory.is_none(),
            "refusal predates output creation"
        );
        assert_eq!(
            fs::read_dir(&selected)
                .expect("preserved empty root")
                .count(),
            0
        );
    }
    let receipt = Sqlite
        .initialize(&supplied, initialize_options(exact))
        .expect("exact native byte allowance");
    assert_eq!(
        receipt.directory.as_os_str().as_bytes(),
        canonical.as_os_str().as_bytes()
    );
    assert_eq!(receipt.directory.as_os_str().as_bytes().len(), exact);
    assert!(receipt.database.is_file());

    let partial = private_child(&long, OsStr::new("partial-雨-\"\\\n"));
    let partial = partial.canonicalize().expect("resolved failure directory");
    let limit = partial.as_os_str().as_bytes().len();
    let error = Sqlite
        .initialize(
            &partial,
            InitializeOptions {
                database_bytes: 1,
                ..initialize_options(limit)
            },
        )
        .expect_err("database budget fails after exclusive file creation");
    assert!(matches!(error.cause, Error::StorageLimit));
    let retained = error
        .directory
        .expect("late failure preserves exact directory");
    assert_eq!(retained, partial);
    assert_eq!(retained.as_os_str().as_bytes().len(), limit);
    assert!(retained.join(crate::journal::DATABASE).is_file());
}

#[test]
fn backup_and_restore_enforce_generated_native_bytes_without_losing_partial_output() {
    let root = tempdir().expect("fixture root");
    let source_root = private_child(root.path(), OsStr::new("source"));
    let initial = Sqlite
        .initialize(
            &source_root,
            initialize_options(OutputLimits::default().directory_bytes),
        )
        .expect("real source journal");
    let source_before = fs::read(&initial.database).expect("original source bytes");
    let parent = private_child(root.path(), OsStr::new("long-backup-parent-雪-\"\\\n"));
    let alias = root.path().join("b");
    symlink(&parent, &alias).expect("short backup alias");
    let exact = generated_bytes(&parent, "gazette-backup-");
    let backup_options = BackupOptions {
        allow_create: true,
        read: read_options(),
        output: OutputLimits {
            directory_bytes: exact,
        },
        ..BackupOptions::default()
    };
    let alias_bytes =
        alias.join("gazette-backup-").as_os_str().as_bytes().len() + RANDOM_CHARACTERS;
    assert!(alias_bytes < exact);
    for limit in [0, 1024 * 1024 + 1, alias_bytes, exact - 1] {
        let error = Sqlite
            .backup(
                &initial.database,
                &alias,
                BackupOptions {
                    output: OutputLimits {
                        directory_bytes: limit,
                    },
                    ..backup_options
                },
            )
            .expect_err("backup refuses before output creation");
        assert!(matches!(error.cause, Error::InvalidLimits));
        assert!(error.directory.is_none());
        assert_eq!(
            fs::read_dir(&parent).expect("empty backup parent").count(),
            0
        );
        assert_eq!(
            fs::read(&initial.database).expect("source preserved"),
            source_before
        );
    }
    let backup = Sqlite
        .backup(&initial.database, &alias, backup_options)
        .expect("exact byte-bound backup");
    assert_eq!(backup.directory.as_os_str().as_bytes().len(), exact);
    assert!(
        backup
            .directory
            .to_str()
            .expect("native Unicode path")
            .chars()
            .count()
            < exact
    );
    let verification = BackupVerifyOptions {
        read: read_options(),
        ..BackupVerifyOptions::default()
    };
    Sqlite
        .verify_backup(&backup.directory, backup.manifest_sha256, verification)
        .expect("native verification accepts Unicode and escaped/control characters");

    let recoveries = private_child(root.path(), OsStr::new("long-recovery-parent-雨-\"\\\n"));
    let restore_alias = root.path().join("r");
    symlink(&recoveries, &restore_alias).expect("short recovery alias");
    let restore_bytes = generated_bytes(&recoveries, "gazette-restore-");
    let restore_options = RestoreOptions {
        allow_prepare: true,
        verification,
        output: OutputLimits {
            directory_bytes: restore_bytes,
        },
    };
    let alias_bytes = restore_alias
        .join("gazette-restore-")
        .as_os_str()
        .as_bytes()
        .len()
        + RANDOM_CHARACTERS;
    assert!(alias_bytes < restore_bytes);
    for limit in [0, 1024 * 1024 + 1, alias_bytes, restore_bytes - 1] {
        let error = Sqlite
            .prepare_restore(
                &backup.directory,
                backup.manifest_sha256,
                &restore_alias,
                RestoreOptions {
                    output: OutputLimits {
                        directory_bytes: limit,
                    },
                    ..restore_options
                },
            )
            .expect_err("restore refuses before output creation");
        assert!(matches!(error.cause, Error::InvalidLimits));
        assert!(error.directory.is_none());
        assert_eq!(
            fs::read_dir(&recoveries)
                .expect("empty recovery parent")
                .count(),
            0
        );
    }
    let restored = Sqlite
        .prepare_restore(
            &backup.directory,
            backup.manifest_sha256,
            &restore_alias,
            restore_options,
        )
        .expect("exact byte-bound detached restore");
    assert_eq!(
        restored.directory.as_os_str().as_bytes().len(),
        restore_bytes
    );
    assert!(
        restored
            .directory
            .to_str()
            .expect("native Unicode path")
            .chars()
            .count()
            < restore_bytes
    );
    assert_eq!(
        restored.preparation.source_manifest_sha256,
        backup.manifest_sha256
    );
    assert_eq!(
        fs::read(restored.directory.join(crate::journal::DATABASE)).expect("restored bytes"),
        fs::read(backup.directory.join(crate::journal::DATABASE)).expect("backup bytes")
    );

    let failed = Sqlite
        .backup(
            &initial.database,
            &alias,
            BackupOptions {
                pages_per_step: 1,
                steps: 1,
                ..backup_options
            },
        )
        .expect_err("single page budget cannot copy complete schema");
    assert!(matches!(failed.cause, Error::BackupLimit));
    let retained = failed.directory.expect("partial copy remains located");
    assert_eq!(retained.as_os_str().as_bytes().len(), exact);
    assert_ne!(retained, backup.directory);
    assert!(retained.join(crate::journal::DATABASE).is_file());
    assert!(
        !retained.join("backup.json").exists(),
        "failure is not sealed success"
    );
    assert_eq!(
        fs::read(&initial.database).expect("source remains untouched"),
        source_before
    );
}
