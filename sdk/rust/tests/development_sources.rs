//! Native working-byte capture preserves the declared Git baseline without verifying dirty bytes.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use gazette_adapter_fs::FileSystem;
use gazette_adapter_git::Git;
use gazette_application::{
    build,
    source::{self, AcquiredInput, DevelopmentBaseline, SourceSelection},
};
use gazette_core::source::{CommitId, Error, RevisionLimits};
use tempfile::{TempDir, tempdir};

use gazette_test_support as native;

#[path = "development_sources/branches.rs"]
mod branches;

struct Fixture {
    directory: TempDir,
    commit: CommitId,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempdir().expect("disposable native source repository");
        let commit = initialize(directory.path());
        Self { directory, commit }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn selection(&self) -> SourceSelection {
        SourceSelection::Repository {
            root: self.root().to_owned(),
            subdirectory: "docs".parse().expect("selected subtree"),
            commit: self.commit.clone(),
        }
    }

    fn config(&self) -> String {
        format!(
            "[[principals]]\nid='owner:one'\nkind='person'\n\
            [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
            [[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
            [[brands]]\nid='brand:one'\nname='Fixture'\n\
            [[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:shared',target='one'}}]\n\
            [[brands.publications]]\nsite='site:two'\nauthored=[{{source='source:shared',target='two'}}]\n\
            [[sources]]\nid='source:shared'\nlocation={{kind='repository',root={:?},subdirectory='docs',commit='{}'}}\n",
            self.root().to_str().expect("UTF-8 fixture path"),
            self.commit
        )
    }
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new(native::git())
        .args(arguments)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .output()
        .expect("disposable native Git operation");
    assert!(
        output.status.success(),
        "Git fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 native Git result")
        .trim()
        .to_owned()
}

fn commit(root: &Path) -> CommitId {
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "source fixture",
        ],
    );
    git(root, &["rev-parse", "HEAD"])
        .parse()
        .expect("exact native fixture commit")
}

fn initialize(root: &Path) -> CommitId {
    fs::create_dir_all(root.join("docs")).expect("selected tree");
    fs::write(root.join("docs/page.md"), b"# Baseline\r\n").expect("baseline document");
    fs::write(root.join("docs/remove.md"), b"Removed later\n").expect("deletion fixture");
    fs::write(root.join("docs/rename.bin"), [0, 128, 255]).expect("rename fixture");
    git(root, &["init", "--quiet"]);
    git(root, &["add", "docs"]);
    commit(root)
}

// Include native Git administration, not just selected publication payloads.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut snapshot = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("native fixture directory") {
            let entry = entry.expect("native fixture entry");
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("confined snapshot path")
                .to_owned();
            let kind = entry.file_type().expect("native entry kind");
            if kind.is_dir() {
                snapshot.insert(relative, None);
                pending.push(path);
            } else {
                assert!(
                    kind.is_file(),
                    "snapshot contains regular native files only"
                );
                snapshot.insert(relative, Some(fs::read(path).expect("native bytes")));
            }
        }
    }
    snapshot
}

#[test]
fn working_edits_additions_deletions_and_renames_are_retained_not_verified() {
    let fixture = Fixture::new();
    let subtree = "docs".parse().expect("subtree");
    let strict = source::acquire(
        &FileSystem,
        &Git,
        fixture.root(),
        &subtree,
        &fixture.commit,
        RevisionLimits::default(),
    )
    .expect("clean baseline verification");
    let baseline = strict.verification().inventory().identity();
    let clean_development = AcquiredInput::Development(
        source::acquire_development(
            &FileSystem,
            &Git,
            &fixture.selection(),
            RevisionLimits::default(),
        )
        .expect("explicit development capture also permits unchanged working bytes"),
    );
    assert!(clean_development.is_development());
    assert_eq!(clean_development.inventory().identity(), baseline);
    fs::write(fixture.root().join("docs/page.md"), b"# Edited\r\n").expect("dirty source");
    fs::remove_file(fixture.root().join("docs/remove.md")).expect("explicit fixture deletion");
    fs::rename(
        fixture.root().join("docs/rename.bin"),
        fixture.root().join("docs/moved.bin"),
    )
    .expect("fixture rename");
    fs::write(fixture.root().join("docs/new.md"), b"New\n").expect("untracked source");
    let before = snapshot(fixture.root());
    assert!(matches!(
        source::acquire(
            &FileSystem,
            &Git,
            fixture.root(),
            &subtree,
            &fixture.commit,
            RevisionLimits::default()
        ),
        Err(Error::ContentMismatch(_))
    ));
    let observed = source::acquire_development(
        &FileSystem,
        &Git,
        &fixture.selection(),
        RevisionLimits::default(),
    )
    .expect("explicit dirty capture");
    match observed.baseline() {
        DevelopmentBaseline::Repository {
            commit,
            subdirectory,
            tree,
        } => {
            assert_eq!(commit, &fixture.commit);
            assert_eq!(subdirectory, &subtree);
            assert_eq!(*tree, baseline);
        }
        DevelopmentBaseline::Submodule { .. } => {
            panic!("repository must not invent gitlink lineage")
        }
    }
    assert_ne!(observed.inventory().identity(), baseline);
    for (path, expected) in [
        ("page.md", b"# Edited\r\n".as_slice()),
        ("new.md", b"New\n".as_slice()),
        ("moved.bin", &[0, 128, 255]),
    ] {
        assert_eq!(
            observed.file(&path.parse().expect("observed path")),
            Some(expected)
        );
    }
    for path in ["remove.md", "rename.bin"] {
        assert!(
            observed
                .file(&path.parse().expect("deleted path"))
                .is_none()
        );
    }
    assert_eq!(
        snapshot(fixture.root()),
        before,
        "capture and strict refusal write no source or Git state"
    );
    let input = AcquiredInput::Development(observed);
    assert!(input.is_development());
    let evidence =
        serde_json::to_value(build::SourceEvidence::from(&input)).expect("honest source evidence");
    assert_eq!(evidence["kind"], "development");
    assert!(evidence.get("verification").is_none());
    assert_eq!(
        evidence["observation"]["baseline"]["commit"],
        fixture.commit.as_str()
    );
    assert!(evidence["observation"].get("contents").is_none());
    fs::write(fixture.root().join("docs/page.md"), b"Later mutation").expect("later editor change");
    assert_eq!(
        input.file(&"page.md".parse().expect("path")),
        Some(b"# Edited\r\n".as_slice())
    );
}

#[test]
fn development_publications_share_capture_and_preserve_configuration_and_strict_default() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root().join("docs/page.md"),
        b"Dirty shared capture\n",
    )
    .expect("dirty bytes");
    let config = fixture.config();
    let sites = [
        "site:two".parse().expect("second site"),
        "site:one".parse().expect("first site"),
    ];
    let before = snapshot(fixture.root());
    let strict = build::acquire_publications(
        &FileSystem,
        &Git,
        &config,
        &sites,
        Default::default(),
        Default::default(),
    )
    .expect_err("strict publication acquisition remains pinned");
    assert_eq!(strict.code(), "source_content_mismatch");
    let acquired = build::acquire_development_publications(
        &FileSystem,
        &Git,
        &config,
        &sites,
        Default::default(),
        Default::default(),
    )
    .expect("explicit development publication capture");
    assert_eq!(acquired.sources().len(), 1);
    assert!(
        acquired
            .sources()
            .values()
            .all(AcquiredInput::is_development)
    );
    let first = acquired.sites()[&sites[0]].as_ref().expect("first site");
    let second = acquired.sites()[&sites[1]].as_ref().expect("second site");
    assert!(std::ptr::eq(first.sources(), second.sources()));
    assert!(std::ptr::eq(first.compilation(), second.compilation()));
    assert_eq!(
        serde_json::to_value(first.inspect()).expect("native inspection")["sources"]["source:shared"]
            ["kind"],
        "development"
    );
    assert_eq!(snapshot(fixture.root()), before);
    let bounded = build::acquire_development_publications(
        &FileSystem,
        &Git,
        &config,
        &sites,
        Default::default(),
        build::PublicationSetAcquisitionLimits {
            sources: source::PreparationLimits {
                sources: 0,
                entries: 100,
                bytes: 1024,
                per_source: RevisionLimits::default(),
            },
            ..Default::default()
        },
    )
    .expect_err("same source count preflight");
    assert_eq!(bounded.code(), "composition_source_limit");
    assert_eq!(snapshot(fixture.root()), before);
}

#[test]
fn development_does_not_advance_head_or_accept_a_missing_committed_subtree() {
    let fixture = Fixture::new();
    let advanced = commit(fixture.root());
    assert_ne!(advanced, fixture.commit);
    let before = snapshot(fixture.root());
    assert!(matches!(
        source::acquire_development(
            &FileSystem,
            &Git,
            &fixture.selection(),
            RevisionLimits::default()
        ),
        Err(Error::RevisionMismatch { .. })
    ));
    assert_eq!(snapshot(fixture.root()), before);
    fs::create_dir(fixture.root().join("uncommitted")).expect("uncommitted directory");
    fs::write(fixture.root().join("uncommitted/page.md"), b"New subtree")
        .expect("uncommitted subtree bytes");
    let selection = SourceSelection::Repository {
        root: fixture.root().to_owned(),
        subdirectory: "uncommitted".parse().expect("literal subtree"),
        commit: advanced,
    };
    let before = snapshot(fixture.root());
    assert!(
        source::acquire_development(&FileSystem, &Git, &selection, RevisionLimits::default())
            .is_err(),
        "a working-only subtree has no selected committed baseline"
    );
    assert_eq!(snapshot(fixture.root()), before);
}

#[test]
fn development_submodule_preserves_parent_and_gitlink_and_rejects_changed_child_or_parent_head() {
    let fixture = Fixture::new();
    let child_root = fixture.root().join("child");
    let child = initialize(&child_root);
    git(
        fixture.root(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{child},child"),
        ],
    );
    let parent = commit(fixture.root());
    let selection = SourceSelection::Submodule {
        parent: fixture.root().to_owned(),
        parent_commit: parent.clone(),
        submodule_path: "child".parse().expect("gitlink"),
        subdirectory: "docs".parse().expect("child subtree"),
    };
    let baseline = source::acquire_submodule(
        &FileSystem,
        &Git,
        fixture.root(),
        &parent,
        &"child".parse().expect("gitlink"),
        &"docs".parse().expect("subtree"),
        RevisionLimits::default(),
    )
    .expect("clean exact gitlink baseline");
    fs::write(child_root.join("docs/page.md"), b"Dirty child\n").expect("child editor bytes");
    let before = snapshot(fixture.root());
    let observed =
        source::acquire_development(&FileSystem, &Git, &selection, RevisionLimits::default())
            .expect("dirty child with unchanged exact parent/gitlink");
    match observed.baseline() {
        DevelopmentBaseline::Submodule {
            parent_commit,
            submodule_path,
            commit,
            subdirectory,
            tree,
        } => {
            assert_eq!(parent_commit, &parent);
            assert_eq!(commit, &child);
            assert_eq!(submodule_path.to_string(), "child");
            assert_eq!(subdirectory.to_string(), "docs");
            assert_eq!(
                *tree,
                baseline.verification().source().inventory().identity()
            );
        }
        DevelopmentBaseline::Repository { .. } => panic!("gitlink lineage must remain distinct"),
    }
    assert_eq!(snapshot(fixture.root()), before);
    let child_advanced = commit(&child_root);
    let before = snapshot(fixture.root());
    let error =
        source::acquire_development(&FileSystem, &Git, &selection, RevisionLimits::default())
            .expect_err("unchanged parent still selects old child");
    assert!(
        matches!(error, Error::RevisionMismatch { expected, actual, .. }
        if expected == child.as_str() && actual == child_advanced.as_str())
    );
    assert_eq!(snapshot(fixture.root()), before);
    git(&child_root, &["reset", "--soft", child.as_str()]);
    let parent_advanced = commit(fixture.root());
    let before = snapshot(fixture.root());
    let error =
        source::acquire_development(&FileSystem, &Git, &selection, RevisionLimits::default())
            .expect_err("parent pin cannot advance implicitly");
    assert!(
        matches!(error, Error::RevisionMismatch { expected, actual, .. }
        if expected == parent.as_str() && actual == parent_advanced.as_str())
    );
    assert_eq!(snapshot(fixture.root()), before);
}

#[test]
fn development_capture_keeps_native_source_and_git_metadata_ceilings() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.root());
    for limits in [
        RevisionLimits {
            content: gazette_core::source::Limits {
                file_bytes: 1,
                ..Default::default()
            },
            ..Default::default()
        },
        RevisionLimits {
            content: gazette_core::source::Limits {
                entries: 1,
                ..Default::default()
            },
            ..Default::default()
        },
        RevisionLimits {
            content: gazette_core::source::Limits {
                total_bytes: 1,
                ..Default::default()
            },
            ..Default::default()
        },
        RevisionLimits {
            metadata_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            source::acquire_development(&FileSystem, &Git, &fixture.selection(), limits),
            Err(Error::Limit { .. })
        ));
        assert_eq!(snapshot(fixture.root()), before);
    }
}

#[cfg(unix)]
#[test]
fn development_never_follows_an_unsafe_selected_link() {
    let fixture = Fixture::new();
    let outside = tempdir().expect("unselected fixture source");
    fs::write(outside.path().join("secret"), b"Not selected").expect("unselected bytes");
    std::os::unix::fs::symlink(
        outside.path().join("secret"),
        fixture.root().join("docs/link"),
    )
    .expect("unsafe source fixture");
    let outside_before = snapshot(outside.path());
    assert!(matches!(
        source::acquire_development(
            &FileSystem,
            &Git,
            &fixture.selection(),
            RevisionLimits::default()
        ),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(snapshot(outside.path()), outside_before);
    assert_eq!(
        fs::read_link(fixture.root().join("docs/link")).expect("link preserved"),
        outside.path().join("secret")
    );
}
