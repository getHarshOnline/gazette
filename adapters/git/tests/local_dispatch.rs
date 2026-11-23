//! Explicit synchronous local transactions over real objects and locked native refs.

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

use gazette_adapter_git::{Dispatcher, Git};
use gazette_core::{
    process::{CaptureRequest, Captured, Cleanup, Error as ProcessError},
    release::{
        CommitMetadata, CommitOptions, CommitSignature, DispatchEffect, DispatchFailure,
        DispatchObservation, DispatchOptions, DispatchProfile, DispatchRequest, DispatchStatus,
        GitSelection, ParentSource, RemoteState, Target, WorktreeOptions,
    },
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use gazette_ports::{
    process::CaptureRunner,
    release::{ReleaseCommits, ReleaseDispatcher, ReleaseWorktrees},
};
use git2::{Oid, Repository};
use tempfile::TempDir;

struct NoProcess;
impl CaptureRunner for NoProcess {
    fn capture(&self, _: CaptureRequest<'_>) -> Result<Captured, ProcessError> {
        panic!("local transaction must not invoke any subprocess runner")
    }
}

struct Fixture {
    directory: TempDir,
    destination: Repository,
    staged: PathBuf,
    output: CapturedSource,
    metadata: CommitMetadata,
    target: Target,
}

impl Fixture {
    fn new(parented: bool) -> Self {
        let directory = tempfile::tempdir().expect("disposable local transaction");
        let root = directory.path().canonicalize().expect("absolute fixture");
        let destination = Repository::init_bare(root.join("destination.git")).expect("bare target");
        let data = b"Exact local publication\n";
        let path: EntryPath = "index.html".parse().expect("output path");
        let mut inventory = InventoryBuilder::new(Limits::default());
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: data.len() as u64,
                    sha256: Sha256Digest::of(data),
                },
            )
            .expect("bounded output entry");
        let output =
            CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, data.to_vec())]))
                .expect("exact retained output");
        let signature = CommitSignature {
            name: "Explicit fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 0,
            offset_minutes: 0,
        };
        let mut metadata = CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Root\n".into(),
        };
        if parented {
            let parent = Git
                .prepare_commit(
                    destination.path(),
                    output.as_input(),
                    &metadata,
                    CommitOptions {
                        allow_write_objects: true,
                        ..Default::default()
                    },
                )
                .expect("real parent objects")
                .commit;
            destination
                .reference(
                    "refs/heads/main",
                    Oid::from_str(parent.as_str()).expect("parent"),
                    false,
                    "initial fixture",
                )
                .expect("initial target ref");
            metadata.parent = Some(parent);
        }
        metadata.message = "Publication\n".into();
        let staged = root.join("staged");
        fs::create_dir(&staged).expect("private stage");
        fs::write(staged.join("index.html"), data).expect("actual publication output");
        let parent_source = ParentSource::Bare {
            path: root.join("destination.git"),
        };
        let prepared = Git
            .prepare_worktree(
                &staged,
                output.as_input(),
                &metadata,
                parented.then_some(&parent_source),
                WorktreeOptions {
                    allow_initialize: true,
                    ..Default::default()
                },
            )
            .expect("independent prepared worktree");
        let target = Target {
            ordinal: 0,
            domain: "fixture.invalid".into(),
            remote: destination.path().to_str().expect("target path").into(),
            branch: "main".into(),
            refspec: format!("{}:refs/heads/main", prepared.commit.commit),
            expected_remote_oid_or_unborn: metadata
                .parent
                .clone()
                .map_or(RemoteState::Unborn, |oid| RemoteState::Commit { oid }),
            prepared_release_receipt_ref: Sha256Digest::of(b"fixture receipt"),
            local_commit_oid: prepared.commit.commit,
            output_digest: output.inventory().identity().sha256(),
        };
        Self {
            directory,
            destination,
            staged,
            output,
            metadata,
            target,
        }
    }

    fn dispatch(
        &self,
        options: DispatchOptions,
    ) -> Result<DispatchObservation, gazette_core::release::DispatchError> {
        let selection = GitSelection {
            program: PathBuf::from("/nonexistent-native-provider"),
            isolation: PathBuf::from("/nonexistent-isolation"),
            credentials: None,
        };
        Dispatcher {
            runner: &NoProcess,
            selection: &selection,
        }
        .dispatch(
            DispatchRequest {
                repository: &self.staged,
                target: &self.target,
                metadata: &self.metadata,
                output: self.output.inventory().identity(),
            },
            options,
        )
    }

    fn reference(&self) -> Option<Oid> {
        match self.destination.find_reference("refs/heads/main") {
            Ok(reference) => Some(reference.target().expect("direct ref")),
            Err(error) if error.code() == git2::ErrorCode::NotFound => None,
            Err(error) => panic!("unexpected native reference error: {error}"),
        }
    }
}

fn options() -> DispatchOptions {
    DispatchOptions {
        profile: DispatchProfile::LocalTransaction,
        allow_dispatch: true,
        ..Default::default()
    }
}

#[test]
fn explicit_local_initial_and_parented_updates_transfer_exact_objects_without_processes() {
    for parented in [false, true] {
        let fixture = Fixture::new(parented);
        let original_source = fs::read(fixture.staged.join(".git/config")).expect("source config");
        let original_target =
            fs::read(fixture.destination.path().join("config")).expect("target config");
        let receipt = fixture
            .dispatch(options())
            .expect("actual native transaction");
        assert_eq!(receipt.status, DispatchStatus::Updated);
        assert_eq!(receipt.exit_code, None);
        assert_eq!(receipt.cleanup, Cleanup::NotStarted);
        let desired = Oid::from_str(fixture.target.local_commit_oid.as_str()).expect("desired ref");
        assert_eq!(fixture.reference(), Some(desired));
        let verified = Git
            .verify_commit(
                fixture.destination.path(),
                &fixture.target.local_commit_oid,
                fixture.output.inventory().identity(),
                &fixture.metadata,
                options().worktree.output,
            )
            .expect("independent exact native commit and publication output readback");
        assert_eq!(verified.commit, fixture.target.local_commit_oid);
        let tree = fixture
            .destination
            .find_commit(desired)
            .expect("native commit")
            .tree()
            .expect("native tree");
        let blob = fixture
            .destination
            .find_blob(tree.get_name("index.html").expect("emitted file").id())
            .expect("transferred blob");
        assert_eq!(blob.content(), b"Exact local publication\n");
        let again = fixture
            .dispatch(options())
            .expect("already-present native observation");
        assert_eq!(again.status, DispatchStatus::AlreadyPresent);
        assert_eq!(
            again,
            fixture.dispatch(options()).expect("stable repeat identity")
        );
        assert_ne!(again.response_sha256, receipt.response_sha256);
        assert_eq!(
            fs::read(fixture.staged.join(".git/config")).expect("unchanged source config"),
            original_source
        );
        assert_eq!(
            fs::read(fixture.destination.path().join("config")).expect("unchanged target config"),
            original_target
        );
        assert!(
            !fixture
                .destination
                .path()
                .join("refs/heads/main.lock")
                .exists()
        );
    }
}

#[test]
fn mismatched_old_ref_and_held_native_ref_lock_never_overwrite_concurrent_state() {
    let fixture = Fixture::new(true);
    let mut metadata = fixture.metadata.clone();
    metadata.message = "Concurrent publication\n".into();
    let other = Git
        .prepare_commit(
            fixture.destination.path(),
            fixture.output.as_input(),
            &metadata,
            CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("real conflicting commit")
        .commit;
    let other = Oid::from_str(other.as_str()).expect("conflicting OID");
    fixture
        .destination
        .reference("refs/heads/main", other, true, "concurrent change")
        .expect("actual movement");
    assert_eq!(
        fixture
            .dispatch(options())
            .expect("native old-state rejection")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(fixture.reference(), Some(other));

    let mut transaction = fixture
        .destination
        .transaction()
        .expect("independent transaction");
    transaction
        .lock_ref("refs/heads/main")
        .expect("concurrent native lock");
    let error = fixture
        .dispatch(options())
        .expect_err("another native owner holds exact ref lock");
    assert_eq!(error.cleanup, Cleanup::NotStarted);
    assert_eq!(fixture.reference(), Some(other));
    drop(transaction);
}

#[test]
fn installed_receiver_hooks_and_custom_configuration_are_refused_without_execution_or_rewrite() {
    for policy in ["hook", "hooks-path", "receive", "include", "logging"] {
        let fixture = Fixture::new(true);
        let original_ref = fixture.reference();
        let marker = fixture.directory.path().join("must-not-run");
        if policy == "hook" {
            let hook = fixture.destination.path().join("hooks/pre-receive");
            fs::create_dir_all(hook.parent().expect("hooks directory")).expect("fixture directory");
            fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))
                .expect("receiver fixture");
            fs::set_permissions(hook, fs::Permissions::from_mode(0o700))
                .expect("executable fixture");
        } else {
            let mut config = fixture.destination.config().expect("local config");
            let key = match policy {
                "hooks-path" => "core.hookspath",
                "receive" => "receive.denyNonFastForwards",
                "include" => "include.path",
                _ => "core.logallrefupdates",
            };
            config
                .set_str(
                    key,
                    if policy == "include" {
                        "/missing-external-policy"
                    } else {
                        "true"
                    },
                )
                .expect("explicit unsupported policy");
        }
        let original_config =
            fs::read(fixture.destination.path().join("config")).expect("original config bytes");
        let error = fixture
            .dispatch(options())
            .expect_err("no silent receiver-policy bypass");
        assert_eq!(error.failure, DispatchFailure::Unsupported, "{policy}");
        assert_eq!(error.effect, DispatchEffect::NotStarted);
        assert_eq!(fixture.reference(), original_ref);
        assert_eq!(
            fs::read(fixture.destination.path().join("config")).expect("preserved config"),
            original_config
        );
        assert!(!marker.exists());
    }
}

#[test]
fn unsafe_destinations_overlapping_sources_and_unsupported_urls_are_refused() {
    for kind in ["worktree", "symlink", "overlap", "network"] {
        let mut fixture = Fixture::new(false);
        fixture.target.remote = match kind {
            "worktree" => fixture.staged.to_str().expect("worktree").into(),
            "symlink" => {
                let link = fixture.directory.path().join("alias");
                symlink(fixture.destination.path(), &link).expect("unsafe alias fixture");
                link.to_str().expect("alias").into()
            }
            "overlap" => fixture
                .staged
                .join(".git")
                .to_str()
                .expect("overlap")
                .into(),
            _ => "https://example.invalid/remote.git".into(),
        };
        let error = fixture
            .dispatch(options())
            .expect_err("refuse unsupported target without effect");
        assert_eq!(error.effect, DispatchEffect::NotStarted, "{kind}");
        assert_eq!(error.cleanup, Cleanup::NotStarted);
        assert_eq!(fixture.reference(), None);
    }
}

#[test]
fn permissions_receipt_bounds_and_corrupt_prepared_objects_refuse_before_ref_mutation() {
    let fixture = Fixture::new(false);
    let before = fixture.reference();
    for selected in [
        DispatchOptions {
            allow_dispatch: false,
            ..options()
        },
        DispatchOptions {
            max_response_bytes: 1,
            ..options()
        },
    ] {
        let error = fixture
            .dispatch(selected)
            .expect_err("explicit permission and evidence limits");
        assert_eq!(error.effect, DispatchEffect::NotStarted);
        assert_eq!(fixture.reference(), before);
    }
    let oid = fixture.target.local_commit_oid.as_str();
    let object = fixture
        .staged
        .join(".git/objects")
        .join(&oid[..2])
        .join(&oid[2..]);
    fs::set_permissions(&object, fs::Permissions::from_mode(0o600)).expect("owned corrupt fixture");
    fs::write(object, b"corrupt").expect("corrupt exact prepared bytes");
    assert_eq!(
        fixture
            .dispatch(options())
            .expect_err("verify exact source before transfer")
            .failure,
        DispatchFailure::LocalState
    );
    assert_eq!(fixture.reference(), before);
}
