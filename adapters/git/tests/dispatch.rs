//! Real disposable receive-pack updates and exact-prestate refusal, never public effects.

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use gazette_adapter_git::{Dispatcher, Git, RemoteReader};
use gazette_adapter_process::Runner;
use gazette_core::{
    process::Cleanup,
    release::{
        CommitMetadata, CommitOptions, CommitSignature, DispatchEffect, DispatchFailure,
        DispatchOptions, DispatchRequest, DispatchStatus, GitCredentialSelection, GitSelection,
        ParentSource, RefAdvertisement, RemoteReadFailure, RemoteReadOptions, RemoteState, Target,
        WorktreeOptions,
    },
    source::{
        CapturedSource, CommitId, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest,
    },
};
use gazette_ports::release::{
    ReleaseCommits, ReleaseDispatcher, ReleaseWorktrees, RemoteReader as _,
};
use git2::{Oid, Repository};
use tempfile::TempDir;

use gazette_test_support as native;

#[path = "dispatch/https.rs"]
mod https;

struct Fixture {
    root: TempDir,
    isolation: PathBuf,
    deployment: PathBuf,
    remote: Repository,
    input: CapturedSource,
    metadata: CommitMetadata,
    target: Target,
    ancestor: Option<CommitId>,
}

fn recipe() -> CommitMetadata {
    let signature = CommitSignature {
        name: "Fixture".into(),
        email: "fixture@example.invalid".into(),
        seconds: 10,
        offset_minutes: 0,
    };
    CommitMetadata {
        parent: None,
        author: signature.clone(),
        committer: signature,
        message: "Fixture root\n".into(),
    }
}

impl Fixture {
    fn new() -> Self {
        Self::with_parent(true)
    }

    fn with_parent(parented: bool) -> Self {
        let root = tempfile::tempdir().expect("disposable dispatch fixture");
        let root_path = fs::canonicalize(root.path()).expect("exact native fixture root");
        let isolation = root_path.join("isolation");
        fs::create_dir(&isolation).expect("explicit existing isolation");
        fs::set_permissions(&isolation, fs::Permissions::from_mode(0o700))
            .expect("private isolation");
        let remote_path = root_path.join("destination.git");
        let remote = Repository::init_bare(&remote_path).expect("disposable bare receiver");
        let bytes = b"Exact retained publication\n";
        let path: EntryPath = "index.html".parse().expect("portable output path");
        let mut inventory = InventoryBuilder::new(Limits::default());
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("complete bounded inventory");
        let input = CapturedSource::from_parts(
            inventory.finish(),
            BTreeMap::from([(path, bytes.to_vec())]),
        )
        .expect("retained output");
        let ancestor = Git
            .prepare_commit(
                remote.path(),
                input.as_input(),
                &recipe(),
                CommitOptions {
                    allow_write_objects: true,
                    ..Default::default()
                },
            )
            .expect("real ancestor objects")
            .commit;
        let parent = if parented {
            let mut current = recipe();
            current.parent = Some(ancestor.clone());
            current.message = "Current reviewed destination\n".into();
            Git.prepare_commit(
                remote.path(),
                input.as_input(),
                &current,
                CommitOptions {
                    allow_write_objects: true,
                    ..Default::default()
                },
            )
            .expect("real reviewed parent objects")
            .commit
        } else {
            ancestor.clone()
        };
        if parented {
            remote
                .reference(
                    "refs/heads/main",
                    Oid::from_str(parent.as_str()).expect("parent OID"),
                    false,
                    "fixture initial state",
                )
                .expect("real advertised initial state");
        }
        let mut metadata = recipe();
        metadata.parent = parented.then(|| parent.clone());
        metadata.message = "Reviewed publication\n".into();
        let deployment = root_path.join("deployment");
        fs::create_dir(&deployment).expect("separate staged output");
        fs::write(deployment.join("index.html"), bytes).expect("actual output bytes");
        let prepared = Git
            .prepare_worktree(
                &deployment,
                input.as_input(),
                &metadata,
                parented.then_some(&ParentSource::Bare {
                    path: remote_path.clone(),
                }),
                WorktreeOptions {
                    allow_initialize: true,
                    ..Default::default()
                },
            )
            .expect("real independent parented preparation");
        let target = Target {
            ordinal: 0,
            domain: "fixture.invalid".into(),
            remote: remote_path.to_str().expect("exact local receiver").into(),
            branch: "main".into(),
            refspec: format!("{}:refs/heads/main", prepared.commit.commit),
            expected_remote_oid_or_unborn: if parented {
                RemoteState::Commit { oid: parent }
            } else {
                RemoteState::Unborn
            },
            prepared_release_receipt_ref: Sha256Digest::of(b"fixture receipt"),
            local_commit_oid: prepared.commit.commit,
            output_digest: input.inventory().identity().sha256(),
        };
        Self {
            root,
            isolation,
            deployment,
            remote,
            input,
            metadata,
            target,
            ancestor: parented.then_some(ancestor),
        }
    }

    fn request(&self) -> DispatchRequest<'_> {
        DispatchRequest {
            repository: &self.deployment,
            target: &self.target,
            metadata: &self.metadata,
            output: self.input.inventory().identity(),
        }
    }

    fn dispatch(
        &self,
        program: &Path,
        options: DispatchOptions,
    ) -> Result<gazette_core::release::DispatchObservation, gazette_core::release::DispatchError>
    {
        self.dispatch_with_credentials(program, None, options)
    }

    fn dispatch_with_credentials(
        &self,
        program: &Path,
        credentials: Option<gazette_core::release::GitCredentialSelection>,
        options: DispatchOptions,
    ) -> Result<gazette_core::release::DispatchObservation, gazette_core::release::DispatchError>
    {
        let selection = GitSelection {
            program: program.to_owned(),
            isolation: self.isolation.clone(),
            credentials,
        };
        Dispatcher {
            runner: &Runner,
            selection: &selection,
        }
        .dispatch(self.request(), options)
    }

    fn observe(&self) -> RefAdvertisement {
        let selection = GitSelection {
            program: native::git().to_owned(),
            isolation: self.isolation.clone(),
            credentials: None,
        };
        RemoteReader {
            runner: &Runner,
            selection: &selection,
        }
        .observe_remote(
            &self.target.remote,
            &self.target.branch,
            RemoteReadOptions {
                allow_read: true,
                timeout: Duration::from_secs(10),
                ..Default::default()
            },
        )
        .expect("independent native read-back")
    }

    fn sibling(&self) -> CommitId {
        let mut metadata = self.metadata.clone();
        metadata.message = "Independent concurrent publication\n".into();
        Git.prepare_commit(
            self.remote.path(),
            self.input.as_input(),
            &metadata,
            CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("real concurrent commit")
        .commit
    }

    fn executable(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.path().join(name);
        executable(&path, body);
        path
    }
}

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("disposable native fixture script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("executable fixture");
}

fn options() -> DispatchOptions {
    DispatchOptions {
        allow_dispatch: true,
        timeout: Duration::from_secs(10),
        ..Default::default()
    }
}

#[test]
fn exact_parent_updates_one_ref_and_already_present_is_not_a_second_cas() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.clone().expect("signed parent")
        }
    );
    let report = fixture
        .dispatch(native::git(), options())
        .expect("real exact-parent receive-pack update");
    assert_eq!(report.status, DispatchStatus::Updated);
    assert_eq!(report.cleanup, Cleanup::Reaped);
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
    assert_eq!(
        fixture.remote.references().expect("receiver refs").count(),
        1
    );
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect("native up-to-date return")
            .status,
        DispatchStatus::AlreadyPresent
    );
    assert_eq!(
        fixture
            .remote
            .references()
            .expect("unchanged receiver refs")
            .count(),
        1
    );
    assert!(!fixture.deployment.join(".git/FETCH_HEAD").exists());
}

#[test]
fn receive_pack_uses_one_exact_expected_old_lease_and_non_force_refspec() {
    let fixture = Fixture::new();
    let arguments = fixture.root.path().join("push-arguments");
    let wrapper = fixture.executable(
        "record-push",
        &format!(
            "printf '%s\\n' \"$@\" > {}\nexec {} \"$@\"",
            native::shell(&arguments),
            native::shell(native::git())
        ),
    );
    let report = fixture
        .dispatch(&wrapper, options())
        .expect("exact-lease native update");
    assert_eq!(report.status, DispatchStatus::Updated);
    let arguments = fs::read_to_string(arguments).expect("complete native argument capture");
    let arguments = arguments.lines().collect::<Vec<_>>();
    assert!(arguments.contains(&"push"));
    assert!(arguments.contains(&fixture.target.refspec.as_str()));
    let expected_lease = format!(
        "--force-with-lease=refs/heads/main:{}",
        fixture
            .metadata
            .parent
            .as_ref()
            .expect("reviewed expected destination")
    );
    assert_eq!(
        arguments
            .iter()
            .filter(|argument| argument.starts_with("--force"))
            .copied()
            .collect::<Vec<_>>(),
        vec![expected_lease.as_str()],
        "only the exact expected-old safety lease is admitted: {arguments:?}"
    );
    assert!(
        !fixture.target.refspec.starts_with('+'),
        "the update refspec remains ordinary and cannot authorize non-fast-forward publication"
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
}

#[test]
fn root_publication_creates_only_the_absent_selected_branch() {
    let fixture = Fixture::with_parent(false);
    assert_eq!(fixture.observe(), RefAdvertisement::Absent);
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect("native initial push")
            .status,
        DispatchStatus::Updated
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
    assert_eq!(fixture.remote.references().expect("native refs").count(), 1);
    assert_eq!(
        fixture
            .remote
            .find_commit(
                Oid::from_str(fixture.target.local_commit_oid.as_str()).expect("native OID")
            )
            .expect("native commit")
            .parent_count(),
        0
    );
}

#[test]
fn initial_publication_does_not_overwrite_a_concurrently_created_branch() {
    let fixture = Fixture::with_parent(false);
    assert_eq!(fixture.observe(), RefAdvertisement::Absent);
    let concurrent = fixture.sibling();
    fixture
        .remote
        .reference(
            "refs/heads/main",
            Oid::from_str(concurrent.as_str()).expect("native OID"),
            false,
            "independent branch creation",
        )
        .expect("concurrent publication");
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect("native non-force create rejection")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit { oid: concurrent }
    );
}

#[test]
fn movement_after_independent_read_refuses_without_overwriting_concurrent_commit() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.clone().expect("original parent")
        }
    );
    let concurrent = fixture.sibling();
    fixture
        .remote
        .reference(
            "refs/heads/main",
            Oid::from_str(concurrent.as_str()).expect("concurrent OID"),
            true,
            "fixture concurrent owner",
        )
        .expect("independent movement before dispatch");
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect("complete native non-fast-forward report")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit { oid: concurrent }
    );
}

#[test]
fn ancestor_rewind_after_independent_read_is_refused_despite_fast_forward_target() {
    let fixture = Fixture::new();
    let reviewed = fixture.metadata.parent.clone().expect("reviewed parent");
    let ancestor = fixture.ancestor.clone().expect("strict ancestor");
    assert_ne!(reviewed, ancestor);
    fixture
        .remote
        .reference(
            "refs/heads/main",
            Oid::from_str(ancestor.as_str()).expect("ancestor OID"),
            true,
            "fixture deliberate concurrent rollback",
        )
        .expect("independent rewind before dispatch");
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect("exact expected-old lease rejection report")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit { oid: ancestor }
    );
}

#[test]
fn movement_inside_receive_pack_after_advertisement_is_atomically_refused() {
    let fixture = Fixture::new();
    let concurrent = fixture.sibling();
    let hook = fixture.remote.path().join("hooks/pre-receive");
    fs::create_dir_all(hook.parent().expect("fixture hooks directory"))
        .expect("controlled receiver hook fixture");
    // This receiver-owned fixture gate runs after command negotiation and pack
    // transfer, before receive-pack's ref transaction. It changes real native state,
    // not the provider response. Production never installs or changes receiver hooks.
    executable(
        &hook,
        &format!(
            "unset GIT_QUARANTINE_PATH GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES\nwhile read old new reference; do\n {} update-ref \"$reference\" {concurrent} \"$old\" || exit 7\ndone\nexit 0",
            native::shell(native::git())
        ),
    );
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect("native concurrent-update rejection report")
            .status,
        DispatchStatus::Rejected
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit { oid: concurrent }
    );
}

#[test]
fn permissions_incompatible_recipe_config_and_corrupt_objects_refuse_before_launch() {
    let missing = Path::new("/not-an-installed-dispatch-provider");
    let mut fixture = Fixture::new();
    let denied = fixture
        .dispatch(missing, DispatchOptions::default())
        .expect_err("explicit effect permission");
    assert_eq!(denied.failure, DispatchFailure::Permission);
    assert_eq!(denied.effect, DispatchEffect::NotStarted);
    fixture.metadata.parent = None;
    assert_eq!(
        fixture
            .dispatch(missing, options())
            .expect_err("incompatible parent recipe")
            .failure,
        DispatchFailure::Selection
    );
    let fixture = Fixture::new();
    fs::write(
        fixture.deployment.join(".git/config"),
        "[push]\nfollowTags=true\n",
    )
    .expect("changed owned config fixture");
    assert_eq!(
        fixture
            .dispatch(missing, options())
            .expect_err("ambient push configuration refused")
            .failure,
        DispatchFailure::LocalState
    );
    let fixture = Fixture::new();
    let id = fixture.target.local_commit_oid.as_str();
    let object = fixture
        .deployment
        .join(".git/objects")
        .join(&id[..2])
        .join(&id[2..]);
    fs::set_permissions(&object, fs::Permissions::from_mode(0o600))
        .expect("writable disposable commit");
    fs::write(object, b"corrupt").expect("corrupt actual commit bytes");
    assert_eq!(
        fixture
            .dispatch(missing, options())
            .expect_err("actual commit corruption rejected")
            .failure,
        DispatchFailure::LocalState
    );
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.metadata.parent.expect("unchanged parent")
        }
    );
}

#[test]
fn timeout_and_lost_native_response_remain_uncertain_without_automatic_retry() {
    let fixture = Fixture::new();
    let stalled = fixture.executable("stalled", "while :; do :; done");
    let error = fixture
        .dispatch(
            &stalled,
            DispatchOptions {
                timeout: Duration::from_millis(20),
                ..options()
            },
        )
        .expect_err("bounded process deadline");
    assert_eq!(error.failure, DispatchFailure::Timeout);
    assert_eq!(error.effect, DispatchEffect::Unverified);
    assert_eq!(error.cleanup, Cleanup::Reaped);
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture
                .metadata
                .parent
                .clone()
                .expect("unchanged before real dispatch")
        }
    );
    let lost = fixture.executable(
        "lost-response",
        &format!(
            "{} \"$@\" >/dev/null\nexit $?",
            native::shell(native::git())
        ),
    );
    let error = fixture
        .dispatch(&lost, options())
        .expect_err("actual effect without usable response");
    assert_eq!(error.failure, DispatchFailure::Response);
    assert_eq!(error.effect, DispatchEffect::Unverified);
    assert_eq!(
        fixture.observe(),
        RefAdvertisement::Commit {
            oid: fixture.target.local_commit_oid.clone()
        }
    );
}

#[test]
fn network_requires_permission_ssh_is_unsupported_and_extra_refspecs_are_refused() {
    let mut fixture = Fixture::new();
    fixture.target.remote = "https://example.invalid/destination.git".into();
    assert_eq!(
        fixture
            .dispatch(native::git(), options())
            .expect_err("no network permission")
            .failure,
        DispatchFailure::Permission
    );
    fixture.target.remote = "ssh://example.invalid/destination.git".into();
    assert_eq!(
        fixture
            .dispatch(
                native::git(),
                DispatchOptions {
                    allow_network: true,
                    ..options()
                }
            )
            .expect_err("network permission does not admit SSH")
            .failure,
        DispatchFailure::Unsupported
    );
    fixture.target.remote = fixture.remote.path().to_str().expect("fixture path").into();
    for refspec in [
        format!("+{}", fixture.target.refspec),
        ":refs/heads/main".into(),
        "refs/*:refs/*".into(),
    ] {
        fixture.target.refspec = refspec;
        let error = fixture
            .dispatch(native::git(), options())
            .expect_err("no force/deletion/wildcard refspec");
        assert_eq!(error.failure, DispatchFailure::Selection);
        assert_eq!(error.effect, DispatchEffect::NotStarted);
    }
}
