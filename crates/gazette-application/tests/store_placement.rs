//! Private store output remains disjoint from every declared Website lifecycle root.

use gazette_adapter_fs::FileSystem;
use gazette_application::store::{
    self, BackupRequest, LifecycleProtectedRoots, PrepareRestoreRequest,
};
use gazette_core::{
    source::Sha256Digest,
    store::{BackupOptions, BackupReceipt, Error, OutputError, RestoreOptions, RestoreReceipt},
};
use gazette_ports::store::{RestorePreparer, StoreBackup};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct RefusingProvider {
    calls: AtomicUsize,
}

impl StoreBackup for RefusingProvider {
    fn backup(
        &self,
        _source: &Path,
        _parent: &Path,
        _options: BackupOptions,
    ) -> Result<BackupReceipt, OutputError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(Error::StoreExists.into())
    }
}

impl RestorePreparer for RefusingProvider {
    fn prepare_restore(
        &self,
        _backup: &Path,
        _manifest_sha256: Sha256Digest,
        _parent: &Path,
        _options: RestoreOptions,
    ) -> Result<RestoreReceipt, OutputError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(Error::StoreExists.into())
    }
}

fn directory(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir(&path).expect("fixture directory");
    path
}

fn protected(groups: &[Vec<PathBuf>; 4]) -> LifecycleProtectedRoots {
    LifecycleProtectedRoots {
        authored: groups[0].clone(),
        staged: groups[1].clone(),
        published: groups[2].clone(),
        live_journals: groups[3].clone(),
    }
}

fn backup(provider: &RefusingProvider, parent: &Path, groups: &[Vec<PathBuf>; 4]) -> OutputError {
    store::backup(
        provider,
        &FileSystem,
        BackupRequest {
            source: Path::new("unused-by-refusing-provider.sqlite"),
            output_parent: parent,
            protected: protected(groups),
            options: BackupOptions {
                allow_create: true,
                read: gazette_core::store::ReadOptions {
                    allow_coordination_files: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        },
    )
    .expect_err("fixture provider never succeeds")
}

fn restore(provider: &RefusingProvider, parent: &Path, groups: &[Vec<PathBuf>; 4]) -> OutputError {
    store::prepare_restore(
        provider,
        &FileSystem,
        PrepareRestoreRequest {
            backup: Path::new("unused-by-refusing-provider-backup"),
            manifest_sha256: Sha256Digest::of(b"fixture"),
            output_parent: parent,
            protected: protected(groups),
            options: RestoreOptions {
                allow_prepare: true,
                verification: gazette_core::store::BackupVerifyOptions {
                    read: gazette_core::store::ReadOptions {
                        allow_coordination_files: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        },
    )
    .expect_err("fixture provider never succeeds")
}

type Operation = fn(&RefusingProvider, &Path, &[Vec<PathBuf>; 4]) -> OutputError;

#[test]
fn backup_and_restore_refuse_every_lifecycle_root_in_both_ancestry_directions_before_provider() {
    for operation in [backup as Operation, restore as Operation] {
        for category in 0..4 {
            for relation in ["equal", "inside", "containing"] {
                let fixture = tempfile::tempdir().expect("fixture root");
                let base = fixture.path().canonicalize().expect("fixture identity");
                let mut groups = [
                    vec![directory(&base, "authored")],
                    vec![directory(&base, "staged")],
                    vec![directory(&base, "published")],
                    vec![directory(&base, "journal")],
                ];
                let parent = match relation {
                    "equal" => groups[category][0].clone(),
                    "inside" => directory(&groups[category][0], "private-output"),
                    "containing" => {
                        let parent = directory(&base, "private-output");
                        groups[category][0] = directory(&parent, "protected");
                        parent
                    }
                    _ => unreachable!(),
                };
                let provider = RefusingProvider::default();
                let error = operation(&provider, &parent, &groups);
                assert!(matches!(error.cause, Error::UnsafePath));
                assert_eq!(error.directory, None);
                assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            }
        }
    }
}

#[test]
fn disjoint_private_parent_reaches_each_native_provider_once() {
    let fixture = tempfile::tempdir().expect("fixture root");
    let base = fixture.path().canonicalize().expect("fixture identity");
    let groups = [
        vec![directory(&base, "authored")],
        vec![directory(&base, "staged")],
        vec![directory(&base, "published")],
        vec![directory(&base, "journal")],
    ];
    let parent = directory(&base, "private-output");

    for operation in [backup as Operation, restore as Operation] {
        let provider = RefusingProvider::default();
        let error = operation(&provider, &parent, &groups);
        assert!(matches!(error.cause, Error::StoreExists));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}
