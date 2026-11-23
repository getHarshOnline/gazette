//! Real immutable Git custody refs and bounded native synchronization.

use gazette_adapter_git::Git;
use gazette_core::{
    release::{
        CommitMetadata, CommitOptions, CommitReport, CommitSignature, CustodyEffect,
        CustodyOptions, custody_reference,
    },
    source::{CapturedSource, Content, InventoryBuilder, Limits, RevisionLimits, Sha256Digest},
};
use gazette_ports::release::{ReleaseCommits, ReleaseCustody};
use git2::{Oid, Repository};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    recipe: CommitMetadata,
    report: CommitReport,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("private fixture parent");
        let root = directory.path().join("deployment.git");
        Repository::init_bare(&root).expect("fixture repository");
        let path = "index.html".parse().expect("fixture path");
        let bytes = b"literal\r\nretained\0bytes";
        let mut inventory = InventoryBuilder::new(Limits::default());
        inventory
            .insert(
                path,
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("fixture inventory");
        let source = CapturedSource::from_parts(
            inventory.finish(),
            BTreeMap::from([(
                "index.html".parse().expect("fixture payload path"),
                bytes.to_vec(),
            )]),
        )
        .expect("fixture capture");
        let signature = CommitSignature {
            name: "Fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 101,
            offset_minutes: 0,
        };
        let recipe = CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Exact fixture\n".into(),
        };
        let report = Git
            .prepare_commit(
                &root,
                source.as_input(),
                &recipe,
                CommitOptions {
                    allow_write_objects: true,
                    ..Default::default()
                },
            )
            .expect("detached fixture commit");
        Self {
            _directory: directory,
            root,
            recipe,
            report,
        }
    }

    fn retain(
        &self,
        id: Sha256Digest,
        options: CustodyOptions,
    ) -> Result<gazette_core::release::CustodyReport, gazette_core::release::CustodyError> {
        Git.retain_commit(
            &self.root,
            id,
            &self.report.commit,
            self.report.output,
            &self.recipe,
            options,
        )
    }

    fn inspect(
        &self,
        id: Sha256Digest,
    ) -> Result<gazette_core::release::CustodyReport, gazette_core::release::CustodyError> {
        Git.inspect_custody(
            &self.root,
            id,
            &self.report.commit,
            self.report.output,
            &self.recipe,
            RevisionLimits::default(),
        )
    }
}

fn allowed() -> CustodyOptions {
    CustodyOptions {
        allow_retain: true,
        ..Default::default()
    }
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut output = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        for child in fs::read_dir(path).expect("snapshot native directory") {
            let child = child.expect("snapshot native directory entry");
            if child.file_type().expect("snapshot entry type").is_dir() {
                pending.push(child.path());
            } else {
                output.insert(
                    child
                        .path()
                        .strip_prefix(root)
                        .expect("snapshot remains inside fixture")
                        .to_owned(),
                    fs::read(child.path()).expect("snapshot native file bytes"),
                );
            }
        }
    }
    output
}

#[test]
fn retention_replay_and_read_only_inspection_preserve_recipe_head_and_branches() {
    let fixture = Fixture::new();
    let repository = Repository::open_bare(&fixture.root).expect("open fixture repository");
    let oid = Oid::from_str(fixture.report.commit.as_str()).expect("exact fixture OID");
    repository
        .reference("refs/heads/unchanged", oid, false, "fixture branch")
        .expect("create fixture branch");
    let before = files(&fixture.root);
    let id = Sha256Digest::of(b"one immutable retention");
    let report = fixture
        .retain(id, allowed())
        .expect("native synchronization and readback");
    assert_eq!(report.commit, fixture.report);
    assert_eq!(report.id, id);
    assert_eq!(
        repository
            .find_reference(&custody_reference(id))
            .expect("retained custody reference")
            .target(),
        Some(oid)
    );
    let after = files(&fixture.root);
    for (path, bytes) in before {
        assert_eq!(
            after.get(&path),
            Some(&bytes),
            "existing native file changed: {}",
            path.display()
        );
    }
    assert_eq!(
        fixture
            .inspect(id)
            .expect("independent custody observation"),
        report
    );
    assert_eq!(files(&fixture.root), after, "inspection wrote native state");
    assert_eq!(
        fixture.retain(id, allowed()).expect("exact custody replay"),
        report
    );
    assert_eq!(files(&fixture.root), after, "exact replay rewrote state");
    assert!(repository.is_bare());
    assert!(repository.workdir().is_none());
    assert!(!fixture.root.join("index").exists());
}

#[test]
fn absent_permission_and_missing_reference_are_read_only_refusals() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"missing permission");
    let before = files(&fixture.root);
    assert_eq!(
        fixture
            .retain(id, CustodyOptions::default())
            .expect_err("missing retention permission")
            .effect,
        CustodyEffect::NotAttempted
    );
    assert_eq!(
        fixture
            .inspect(id)
            .expect_err("missing custody reference")
            .effect,
        CustodyEffect::NotAttempted
    );
    assert_eq!(files(&fixture.root), before);
}

#[test]
fn conflicting_and_symbolic_refs_are_never_replaced() {
    let fixture = Fixture::new();
    let repository = Repository::open_bare(&fixture.root).expect("open fixture repository");
    let original = repository
        .find_commit(Oid::from_str(fixture.report.commit.as_str()).expect("exact fixture OID"))
        .expect("existing fixture commit");
    let other = repository
        .commit(
            None,
            &original.author(),
            &original.committer(),
            "Other recipe",
            &original.tree().expect("existing fixture commit tree"),
            &[],
        )
        .expect("distinct conflicting fixture commit");
    let id = Sha256Digest::of(b"conflicting");
    repository
        .reference(&custody_reference(id), other, false, "fixture conflict")
        .expect("install conflicting fixture ref");
    let symbolic = Sha256Digest::of(b"symbolic");
    repository
        .reference_symbolic(
            &custody_reference(symbolic),
            "HEAD",
            false,
            "fixture symbolic",
        )
        .expect("install symbolic fixture ref");
    let before = files(&fixture.root);
    for id in [id, symbolic] {
        assert_eq!(
            fixture
                .retain(id, allowed())
                .expect_err("conflicting or symbolic custody refused")
                .effect,
            CustodyEffect::NotAttempted
        );
        assert!(fixture.inspect(id).is_err());
    }
    assert_eq!(files(&fixture.root), before);
}

#[test]
fn existing_storage_and_metadata_bounds_refuse_before_reference_creation() {
    let fixture = Fixture::new();
    let before = files(&fixture.root);
    for options in [
        CustodyOptions {
            storage_bytes: 0,
            ..allowed()
        },
        CustodyOptions {
            limits: RevisionLimits {
                metadata_bytes: 1,
                ..Default::default()
            },
            ..allowed()
        },
    ] {
        assert_eq!(
            fixture
                .retain(Sha256Digest::of(b"bounded"), options)
                .expect_err("preflight native storage or metadata bound")
                .effect,
            CustodyEffect::NotAttempted
        );
    }
    assert_eq!(files(&fixture.root), before);
}

#[test]
fn new_reference_exceeding_storage_bound_is_preserved_as_unverified_effect() {
    let fixture = Fixture::new();
    let original_bytes = files(&fixture.root)
        .values()
        .map(|bytes| bytes.len() as u64)
        .sum();
    let id = Sha256Digest::of(b"post write storage refusal");
    let error = fixture
        .retain(
            id,
            CustodyOptions {
                storage_bytes: original_bytes,
                ..allowed()
            },
        )
        .expect_err("new ref exceeds remaining storage bound");
    assert_eq!(error.effect, CustodyEffect::ReferenceMayRemain);
    assert!(error.reason.contains("storage byte"));
    assert_eq!(
        fixture
            .inspect(id)
            .expect("recover exact post-failure ref observation")
            .commit,
        fixture.report,
        "observation recovers exact reference, not synchronization proof"
    );
}

#[test]
fn corrupt_objects_or_changed_recipes_never_gain_custody() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"corrupt");
    let mut recipe = fixture.recipe.clone();
    recipe.message.push_str("changed");
    let mismatch = Git
        .retain_commit(
            &fixture.root,
            id,
            &fixture.report.commit,
            fixture.report.output,
            &recipe,
            allowed(),
        )
        .expect_err("changed recipe is not the verified commit");
    assert_eq!(mismatch.effect, CustodyEffect::NotAttempted);
    let oid = fixture.report.commit.as_str();
    let object = fixture.root.join("objects").join(&oid[..2]).join(&oid[2..]);
    fs::remove_file(&object).expect("remove disposable fixture object");
    fs::write(&object, b"corrupt loose object").expect("install corrupt disposable object");
    let before = files(&fixture.root);
    assert_eq!(
        fixture
            .retain(id, allowed())
            .expect_err("corrupt object refuses custody")
            .effect,
        CustodyEffect::NotAttempted
    );
    assert_eq!(files(&fixture.root), before);
}

#[test]
fn symlinked_and_hardlinked_storage_fail_without_custody() {
    for hardlink in [false, true] {
        let fixture = Fixture::new();
        let unexpected = fixture.root.join("unexpected");
        if hardlink {
            fs::hard_link(fixture.root.join("HEAD"), &unexpected).expect("fixture hardlink");
        } else {
            symlink(fixture.root.join("HEAD"), &unexpected).expect("fixture symlink");
        }
        let id = Sha256Digest::of(b"unsafe native namespace");
        assert_eq!(
            fixture
                .retain(id, allowed())
                .expect_err("unsafe storage refuses custody")
                .effect,
            CustodyEffect::NotAttempted
        );
        assert!(!fixture.root.join(custody_reference(id)).exists());
        assert!(fs::symlink_metadata(&unexpected).is_ok());
    }
}

fn orphan_log(root: &Path, id: Sha256Digest) -> PathBuf {
    let path = root.join("logs").join(custody_reference(id));
    fs::create_dir_all(path.parent().expect("fixture log parent"))
        .expect("fixture log directories");
    fs::write(&path, b"").expect("existing empty native custody log");
    path
}

#[test]
fn logging_modes_that_can_persist_ambient_attribution_refuse_before_creation() {
    for (mode, existing) in [("always", false), ("invalid-mode", false), ("true", true)] {
        let fixture = Fixture::new();
        let repository = Repository::open_bare(&fixture.root).expect("open fixture repository");
        repository
            .config()
            .expect("fixture config")
            .set_str("core.logallrefupdates", mode)
            .expect("fixture logging mode");
        let id = Sha256Digest::of(b"no ambient log effects");
        if existing {
            orphan_log(&fixture.root, id);
        }
        let before = files(&fixture.root);
        let error = fixture
            .retain(id, allowed())
            .expect_err("logging-capable creation refused");
        assert_eq!(error.effect, CustodyEffect::NotAttempted);
        assert!(error.reason.contains("reflog"));
        assert_eq!(
            files(&fixture.root),
            before,
            "refusal preserves refs, logs and configuration"
        );
    }
}

#[test]
fn inert_creation_modes_preserve_logs_and_replay_never_calls_ref_creation() {
    for (mode, existing) in [("true", false), ("false", true)] {
        let fixture = Fixture::new();
        let repository = Repository::open_bare(&fixture.root).expect("open fixture repository");
        repository
            .config()
            .expect("fixture config")
            .set_str("core.logallrefupdates", mode)
            .expect("fixture inert mode");
        let id = Sha256Digest::of(b"inert creation and exact replay");
        let log = fixture.root.join("logs").join(custody_reference(id));
        if existing {
            orphan_log(&fixture.root, id);
        }
        let before = files(&fixture.root);
        let report = fixture
            .retain(id, allowed())
            .expect("native profile cannot append a log");
        assert_eq!(log.exists(), existing);
        let after = files(&fixture.root);
        for (path, bytes) in before {
            assert_eq!(
                after.get(&path),
                Some(&bytes),
                "existing file changed: {}",
                path.display()
            );
        }
        // Logging configuration can change after retention. Exact replay performs
        // synchronization and verification only, so it need not rewrite or refuse it.
        repository
            .config()
            .expect("fixture config")
            .set_str("core.logallrefupdates", "always")
            .expect("fixture replay logging mode");
        let before_replay = files(&fixture.root);
        assert_eq!(
            fixture
                .retain(id, allowed())
                .expect("existing direct ref replay"),
            report
        );
        assert_eq!(files(&fixture.root), before_replay);
    }
}

#[test]
fn unborn_head_dependencies_direct_or_chained_cannot_gain_a_value_through_custody() {
    for chained in [false, true] {
        let fixture = Fixture::new();
        let repository = Repository::open_bare(&fixture.root).expect("open fixture repository");
        let id = Sha256Digest::of(b"HEAD dependency");
        let target = custody_reference(id);
        let head_target = if chained {
            repository
                .reference_symbolic("refs/heads/relay", &target, false, "fixture chain")
                .expect("fixture unborn relay");
            "refs/heads/relay"
        } else {
            target.as_str()
        };
        repository
            .reference_symbolic("HEAD", head_target, true, "fixture HEAD dependency")
            .expect("fixture dependent HEAD");
        let before = files(&fixture.root);
        let error = fixture
            .retain(id, allowed())
            .expect_err("HEAD must remain unrelated to new custody ref");
        assert_eq!(error.effect, CustodyEffect::NotAttempted);
        assert!(error.reason.contains("HEAD"));
        assert_eq!(files(&fixture.root), before);
    }
}
