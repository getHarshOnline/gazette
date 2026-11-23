//! Local retained output becomes an independent Git worktree, not a release receipt.

use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use gazette_sdk::{release_commit as release, store};
use git2::{Oid, Repository};
use tempfile::TempDir;

use super::preparation::{published_fixture, render};
use super::{Fixture, HOME, Value, acquire, build, bytes, fs, source};

pub(super) struct Retained {
    _parent: TempDir,
    pub(super) journal: TempDir,
    pub(super) database: PathBuf,
    pub(super) receipt: store::PreparationReceipt,
    pub(super) root: PathBuf,
}

impl Retained {
    pub(super) fn new(rendered: &build::PublicationRender<'_>) -> Self {
        let private = || {
            tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()
                .expect("disposable private directory")
        };
        let journal = private();
        let parent = private();
        let session = store::SessionOptions {
            allow_local_reservation: true,
            read: store::ReadOptions {
                allow_coordination_files: true,
                ..Default::default()
            },
        };
        let initialized = store::initialize(
            journal.path(),
            store::InitializeOptions {
                allow_create: true,
                session,
                ..Default::default()
            },
        )
        .expect("real SQLite baseline");
        let receipt = build::prepare_stage(
            rendered,
            build::PreparationRequest {
                journal: journal.path(),
                expected: initialized.header,
                parent: parent.path(),
                name: "deployment",
                id: source::Sha256Digest::of(b"connected local worktree fixture"),
            },
            build::PreparationOptions {
                allow_stage: true,
                record: store::PreparationWriteOptions { allow_record: true },
                writer: store::WriterOptions {
                    allow_generation_advance: true,
                    session,
                },
                ..Default::default()
            },
        )
        .expect("durable intent and retained native stage");
        let root = receipt.record.intent.address.path();
        Self {
            _parent: parent,
            journal,
            database: initialized.database,
            receipt,
            root,
        }
    }

    fn request<'a>(
        &'a self,
        metadata: &'a release::CommitMetadata,
        protected: &'a [PathBuf],
    ) -> release::WorktreeRequest<'a> {
        release::WorktreeRequest {
            root: &self.root,
            expected: self.receipt.record.intent.output,
            metadata,
            parent_source: None,
            protected_roots: protected,
        }
    }

    fn protected(&self, fixture: &Fixture) -> [PathBuf; 2] {
        [
            fixture.directory.path().to_owned(),
            self.journal.path().to_owned(),
        ]
    }
}

pub(super) fn metadata() -> release::CommitMetadata {
    let signature = release::CommitSignature {
        name: "Local fixture".into(),
        email: "fixture@example.invalid".into(),
        seconds: 100,
        offset_minutes: 330,
    };
    release::CommitMetadata {
        parent: None,
        author: signature.clone(),
        committer: signature,
        message: "Exact retained publication\n".into(),
    }
}

fn options() -> release::WorktreeOptions {
    release::WorktreeOptions {
        allow_initialize: true,
        ..Default::default()
    }
}

fn assert_commit(
    stage: &Retained,
    report: &release::WorktreeReport,
    output: &source::CapturedSource,
) {
    let repository = Repository::open(&stage.root).expect("independent native Git");
    assert!(!repository.is_bare());
    assert!(!repository.is_worktree());
    assert!(repository.head_detached().expect("detached state"));
    assert_eq!(repository.remotes().expect("no remote dependency").len(), 0);
    let oid = Oid::from_str(report.commit.commit.as_str()).expect("exact commit");
    assert_eq!(repository.head().expect("HEAD").target(), Some(oid));
    let commit = repository
        .find_commit(oid)
        .expect("existing exact commit object");
    assert_eq!(commit.parent_count(), 0);
    let tree = commit.tree().expect("complete Git tree");
    let index = repository.index().expect("native index");
    let mut files = 0;
    for entry in output.inventory().entries() {
        if let source::Content::File { .. } = entry.content {
            files += 1;
            let path = std::path::Path::new(entry.path.as_str());
            let leaf = tree
                .get_path(path)
                .expect("every output belongs to the commit");
            let indexed = index
                .get_path(path, 0)
                .expect("every output belongs to the index");
            assert_eq!(indexed.id, leaf.id());
            assert_eq!(indexed.mode, 0o100644);
            assert_eq!(
                repository
                    .find_blob(leaf.id())
                    .expect("native blob")
                    .content(),
                bytes(output, entry.path.as_str())
            );
        }
    }
    assert_eq!(index.len(), files);
}

#[test]
fn retained_publication_becomes_exact_independent_worktree_without_recompilation() {
    const BINARY: &[u8] = b"%PDF-1.7\n\0\xffretained payload";
    const LINES: &[u8] = b"first\r\nsecond\r\n";
    let home = format!("{HOME}\n[Download](payload.pdf) [Text](lines.txt)\n");
    let fixture = published_fixture(&[
        ("one/home.md", home.as_bytes()),
        ("one/payload.pdf", BINARY),
        ("one/lines.txt", LINES),
    ]);
    let config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("real compiler and Theme output");
    let stage = Retained::new(&rendered);
    fs::write(
        fixture.directory.path().join("one/home.md"),
        "UNACQUIRED SOURCE",
    )
    .expect("later native edit");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "UNACQUIRED THEME",
    )
    .expect("later template edit");
    let metadata = metadata();
    let protected = stage.protected(&fixture);
    let request = stage.request(&metadata, &protected);
    let prepared = release::prepare_worktree(request, options()).expect("local worktree creation");
    assert_eq!(prepared.output, *rendered.site().output().inventory());
    assert_eq!(prepared.git.commit.output, request.expected);
    assert_commit(&stage, &prepared.git, rendered.site().output());
    for path in ["index.html", "guide/index.html", "old.html", "model.json"] {
        assert_eq!(
            fs::read(stage.root.join(path)).expect("retained native output"),
            bytes(rendered.site().output(), path)
        );
    }
    assert_eq!(
        fs::read(stage.root.join("assets/payload.pdf")).expect("binary output"),
        BINARY
    );
    assert_eq!(
        fs::read(stage.root.join("assets/lines.txt")).expect("CRLF output"),
        LINES
    );
    let model: Value =
        serde_json::from_slice(&fs::read(stage.root.join("model.json")).expect("machine bytes"))
            .expect("shared model");
    assert_eq!(model["site"], "site:one");
    let inspected =
        release::inspect_worktree(request, &prepared.git.commit.commit, Default::default())
            .expect("paired independent Git and filesystem read-back");
    assert_eq!(inspected.git, prepared.git);
    assert_eq!(inspected.output, prepared.output);
    assert_eq!(
        fs::read(fixture.directory.path().join("one/home.md")).expect("source untouched"),
        b"UNACQUIRED SOURCE"
    );
    let history = store::inspect_preparation(
        &stage.database,
        stage.receipt.record.intent.id,
        Some(&stage.receipt.journal),
        store::ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
    )
    .expect("staging journal remains separate");
    assert_eq!(history.record, Some(stage.receipt.record));
}

#[test]
fn valid_commit_does_not_hide_ignored_untracked_or_changed_worktree_bytes() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let protected = stage.protected(&fixture);
    let request = stage.request(&metadata, &protected);
    let prepared = release::prepare_worktree(request, options()).expect("worktree");
    fs::create_dir_all(stage.root.join(".git/info")).expect("native ignore metadata");
    fs::write(stage.root.join(".git/info/exclude"), "ignored.tmp\n")
        .expect("explicit local ignored file");
    for name in ["ignored.tmp", "untracked.tmp"] {
        let path = stage.root.join(name);
        fs::write(&path, b"must not silently disappear").expect("extra output");
        if name == "ignored.tmp" {
            let repo = Repository::open(&stage.root).expect("Git");
            assert!(
                repo.status_file(std::path::Path::new(name))
                    .expect("ignored status")
                    .is_ignored()
            );
        }
        assert_commit(&stage, &prepared.git, rendered.site().output());
        let failure =
            release::inspect_worktree(request, &prepared.git.commit.commit, Default::default())
                .expect_err("complete inventory rejects extras even with a valid commit");
        assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
        assert_eq!(
            fs::read(&path).expect("no cleanup"),
            b"must not silently disappear"
        );
        fs::remove_file(path).expect("remove only this test's extra file");
    }
    fs::write(stage.root.join("guide/index.html"), "changed output")
        .expect("alter one working file");
    assert_commit(&stage, &prepared.git, rendered.site().output());
    release::inspect_worktree(request, &prepared.git.commit.commit, Default::default())
        .expect_err("valid object is not valid live output");
    assert_eq!(
        fs::read(stage.root.join("guide/index.html")).expect("no repair"),
        b"changed output"
    );
}

#[test]
fn existing_metadata_and_reinitialization_refuse_without_adoption_or_cleanup() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    for kind in ["file", "directory", "prepared"] {
        let stage = Retained::new(&rendered);
        let metadata = metadata();
        let protected = stage.protected(&fixture);
        let request = stage.request(&metadata, &protected);
        let git = stage.root.join(".git");
        let report = match kind {
            "file" => {
                fs::write(&git, "unrelated administrative file").expect("existing metadata file");
                None
            }
            "directory" => {
                fs::create_dir(&git).expect("existing metadata directory");
                fs::write(git.join("sentinel"), "unrelated metadata").expect("sentinel");
                None
            }
            _ => Some(
                release::prepare_worktree(request, options()).expect("initial native worktree"),
            ),
        };
        let before = if git.is_dir() {
            Some(source::inspect(&git, None, Default::default()).expect("metadata inventory"))
        } else {
            None
        };
        let failure = release::prepare_worktree(request, options())
            .expect_err("existing metadata is never adopted");
        assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
        if let Some(before) = before {
            assert_eq!(
                source::inspect(&git, None, Default::default()).expect("unchanged metadata"),
                before
            );
        } else {
            assert_eq!(
                fs::read(&git).expect("metadata file preserved"),
                b"unrelated administrative file"
            );
        }
        assert_eq!(
            fs::read(stage.root.join("model.json")).expect("output preserved"),
            bytes(rendered.site().output(), "model.json")
        );
        if let Some(report) = report {
            assert_commit(&stage, &report.git, rendered.site().output());
        }
    }
}

#[test]
fn permission_pin_limits_and_protected_root_refusals_precede_metadata() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let protected = stage.protected(&fixture);
    let empty = tempfile::tempdir().expect("different exact tree");
    let wrong_pin = source::inspect(empty.path(), None, Default::default())
        .expect("empty tree")
        .identity();
    let overlapping = [stage.root.clone()];
    for case in 0..5 {
        let mut request = stage.request(&metadata, &protected);
        let mut options = options();
        match case {
            0 => options.allow_initialize = false,
            1 => request.expected = wrong_pin,
            2 => request.protected_roots = &overlapping,
            3 => options.limits.output.content.total_bytes = 0,
            _ => options.storage_bytes = 0,
        }
        let failure =
            release::prepare_worktree(request, options).expect_err("explicit pre-write gate");
        assert_eq!(
            failure.effect,
            release::WorktreeEffect::NotAttempted,
            "case {case}"
        );
        assert!(!stage.root.join(".git").exists());
        assert_eq!(
            source::inspect(&stage.root, None, Default::default())
                .expect("untouched retained output"),
            *rendered.site().output().inventory()
        );
    }
    let database = fs::read(&stage.database).expect("protected journal bytes");
    let source_head =
        fs::read(fixture.directory.path().join(".git/HEAD")).expect("protected source HEAD");
    for root in [fixture.directory.path(), stage.journal.path()] {
        let request = release::WorktreeRequest {
            root,
            ..stage.request(&metadata, &protected)
        };
        let failure = release::prepare_worktree(request, options())
            .expect_err("actual source and journal roots cannot become worktrees");
        assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
    }
    assert_eq!(
        fs::read(&stage.database).expect("journal preserved"),
        database
    );
    assert_eq!(
        fs::read(fixture.directory.path().join(".git/HEAD")).expect("source HEAD preserved"),
        source_head
    );
    assert!(!stage.journal.path().join(".git").exists());
}

#[test]
fn late_storage_bound_keeps_commit_metadata_and_output_for_explicit_recovery() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let protected = stage.protected(&fixture);
    let request = stage.request(&metadata, &protected);
    let failure = release::prepare_worktree(
        request,
        release::WorktreeOptions {
            storage_bytes: 1,
            ..options()
        },
    )
    .expect_err("native storage synchronization exceeds tiny nonzero ceiling");
    let release::WorktreeEffect::CommitCreated(commit) = failure.effect else {
        panic!("expected known created commit, got {failure:?}");
    };
    assert!(stage.root.join(".git").is_dir());
    assert_eq!(
        fs::read(stage.root.join("model.json")).expect("retained bytes survive failure"),
        bytes(rendered.site().output(), "model.json")
    );
    let observed = release::inspect_worktree(request, &commit, Default::default())
        .expect("explicit later inspection, not proof that failed synchronization succeeded");
    assert_eq!(observed.output, *rendered.site().output().inventory());
    assert_commit(&stage, &observed.git, rendered.site().output());
}
