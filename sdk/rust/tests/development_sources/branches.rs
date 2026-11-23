//! Branch requirements surround working-byte capture without selecting a revision.

use std::{cell::Cell, path::Path};

use gazette_adapter_fs::FileSystem;
use gazette_adapter_git::Git;
use gazette_application::source::{self, PlacementRequest, PreparationLimits, SourceSelection};
use gazette_core::source::{
    CapturedSource, EntryPath, Error, Inventory, Limits, RevisionLimits, Subdirectory,
};
use gazette_ports::source::{RevisionReader, SourceCapture, SourceReader};

use super::{Fixture, snapshot};

fn request(root: &Path, expected: &str) -> PlacementRequest {
    PlacementRequest {
        target: "manual".parse().unwrap(),
        source: SourceSelection::IncludedFilesystem {
            root: root.to_owned(),
            include: vec!["docs".parse().unwrap()],
            expected_branch: Some(expected.into()),
        },
    }
}

fn limits() -> PreparationLimits {
    PreparationLimits {
        sources: 1,
        entries: 100,
        bytes: 4096,
        per_source: RevisionLimits::default(),
    }
}

#[test]
fn branch_validation_preserves_worktree_and_git_metadata_and_never_searches_ancestors() {
    let fixture = Fixture::new();
    let repo = git2::Repository::open(fixture.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("manuals/topic", &commit, false).unwrap();
    repo.set_head("refs/heads/manuals/topic").unwrap();
    std::fs::write(fixture.root().join("docs/page.md"), b"# Held edits\n").unwrap();
    let before = snapshot(fixture.root());
    let prepared = source::prepare(
        &FileSystem,
        &Git,
        vec![request(fixture.root(), "manuals/topic")],
        limits(),
    )
    .unwrap();
    assert_eq!(
        prepared.placements()[0]
            .source()
            .file(&"docs/page.md".parse().unwrap()),
        Some(b"# Held edits\n".as_slice())
    );
    assert!(matches!(
        prepared.placements()[0].source(),
        source::AcquiredInput::Filesystem(_)
    ));
    for expected in [
        "other",
        "manuals",
        "",
        "HEAD",
        "../manuals",
        "manuals^",
        "manuals/topic.lock",
        "manuals topic",
    ] {
        assert!(
            Git.verify_branch(fixture.root(), expected).is_err(),
            "{expected}"
        );
    }
    assert!(
        Git.verify_branch(&fixture.root().join("docs"), "manuals/topic")
            .is_err()
    );
    assert_eq!(snapshot(fixture.root()), before);
}

struct SwitchDuringCapture {
    reads: Cell<usize>,
}

impl SourceReader for SwitchDuringCapture {
    fn read(&self, _: &Path, _: Option<&Subdirectory>, _: Limits) -> Result<Inventory, Error> {
        panic!("included capture does not use inventory-only reads")
    }
}

impl SourceCapture for SwitchDuringCapture {
    fn capture_filtered(
        &self,
        root: &Path,
        subtree: Option<&Subdirectory>,
        exclude: &[EntryPath],
        filter: &dyn gazette_ports::source::SourceFilter,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        self.reads.set(self.reads.get() + 1);
        let captured = FileSystem.capture_filtered(root, subtree, exclude, filter, limits)?;
        git2::Repository::open(root)
            .unwrap()
            .set_head("refs/heads/other")
            .unwrap();
        Ok(captured)
    }

    fn capture(
        &self,
        _: &Path,
        _: Option<&Subdirectory>,
        _: Limits,
    ) -> Result<CapturedSource, Error> {
        panic!("included capture does not read the complete root")
    }

    fn capture_including(
        &self,
        root: &Path,
        include: &[EntryPath],
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        self.reads.set(self.reads.get() + 1);
        let captured = FileSystem.capture_including(root, include, limits)?;
        git2::Repository::open(root)
            .unwrap()
            .set_head("refs/heads/other")
            .unwrap();
        Ok(captured)
    }
}

#[test]
fn mismatch_prevents_capture_and_a_branch_switch_during_capture_discards_the_result() {
    for working_repository in [false, true] {
        let fixture = Fixture::new();
        let repo = git2::Repository::open(fixture.root()).unwrap();
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        for branch in ["manuals", "other"] {
            repo.branch(branch, &commit, false).unwrap();
        }
        repo.set_head("refs/heads/other").unwrap();
        let reader = SwitchDuringCapture {
            reads: Cell::new(0),
        };
        for expected_reads in [0, 1] {
            if expected_reads == 1 {
                repo.set_head("refs/heads/manuals").unwrap();
            }
            let mut request = request(fixture.root(), "manuals");
            if working_repository {
                request.source = SourceSelection::WorkingRepository {
                    root: fixture.root().to_owned(),
                    subdirectory: Some("docs".parse().unwrap()),
                    expected_branch: Some("manuals".into()),
                };
            }
            let error = source::prepare(&reader, &Git, vec![request], limits()).unwrap_err();
            assert!(matches!(
                error,
                source::PreparationError::Source {
                    error: Error::BranchMismatch { .. },
                    ..
                }
            ));
            assert_eq!(reader.reads.get(), expected_reads);
        }
    }
}

#[test]
fn branch_requirement_refuses_non_repository_unborn_and_bare_sources() {
    let root = tempfile::tempdir().unwrap();
    assert!(Git.verify_branch(root.path(), "manuals").is_err());
    let repo = git2::Repository::init(root.path()).unwrap();
    repo.set_head("refs/heads/manuals").unwrap();
    assert!(Git.verify_branch(root.path(), "manuals").is_err());
    let bare = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(bare.path()).unwrap();
    assert!(Git.verify_branch(bare.path(), "manuals").is_err());
}
