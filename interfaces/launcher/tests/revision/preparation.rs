//! Whole-source-set preparation through the SDK and the same native Git fixtures.

use std::fs;

use gazette_sdk::source::{
    self, AcquiredInput, PlacementRequest, PreparationError, PreparationLimits, PreparedSourceSet,
    RevisionLimits, SourceSelection,
};
use serde_json::Value;

use super::{Fixture, ParentFixture};

#[path = "destination.rs"]
mod destination;

#[path = "stage.rs"]
mod stage;

#[path = "stage_cli.rs"]
mod stage_cli;

#[path = "materialization.rs"]
mod materialization;

#[path = "retention.rs"]
mod retention;

fn context(prepared: &PreparedSourceSet) -> source::StageContext {
    source::StageContext {
        workspace: "fixture-workspace".into(),
        site: "fixture-site".into(),
        placements: prepared
            .placements()
            .iter()
            .map(|placement| {
                let target = placement.request().target.clone();
                let identity = source::PlacementIdentity {
                    project: format!("fixture-project:{target}"),
                    deployment: format!("fixture-deployment:{target}"),
                };
                (target, identity)
            })
            .collect(),
    }
}

fn request(target: &str, fixture: &Fixture) -> PlacementRequest {
    PlacementRequest {
        target: target.parse().expect("target"),
        source: SourceSelection::Repository {
            root: fixture.root.path().to_owned(),
            subdirectory: "docs".parse().expect("docs"),
            commit: fixture.commit.to_string().parse().expect("commit"),
        },
    }
}

fn limits() -> PreparationLimits {
    PreparationLimits {
        sources: 2,
        entries: 2,
        bytes: 6,
        per_source: RevisionLimits::default(),
    }
}

fn proofs(set: &PreparedSourceSet) -> Vec<(String, Value)> {
    set.placements()
        .iter()
        .map(|placement| {
            let proof = match placement.source() {
                AcquiredInput::Filesystem(source) => serde_json::to_value(source.inventory()),
                AcquiredInput::Repository(source) => serde_json::to_value(source.verification()),
                AcquiredInput::Submodule(source) => serde_json::to_value(source.verification()),
                AcquiredInput::Development(_) => {
                    panic!("strict preparation returned development bytes")
                }
            }
            .expect("proof");
            (placement.request().target.to_string(), proof)
        })
        .collect()
}

#[test]
fn filesystem_sources_retain_one_complete_tree_without_git_evidence() {
    let root = tempfile::tempdir().expect("local authored root");
    fs::create_dir(root.path().join("authored")).expect("confined subtree");
    fs::write(root.path().join("authored/readme.md"), b"local").expect("authored input");
    let prepared = source::prepare(
        vec![PlacementRequest {
            target: "content".parse().expect("target"),
            source: SourceSelection::Filesystem {
                root: root.path().to_owned(),
                subdirectory: Some("authored".parse().expect("subdirectory")),
            },
        }],
        PreparationLimits {
            sources: 1,
            entries: 2,
            bytes: 5,
            per_source: RevisionLimits::default(),
        },
    )
    .expect("complete retained filesystem tree");
    let input = prepared.placements()[0].source();
    assert!(matches!(input, AcquiredInput::Filesystem(_)));
    assert_eq!(
        input.file(&"readme.md".parse().expect("entry")),
        Some(b"local".as_slice())
    );
    let before = input.inventory().identity();
    fs::write(root.path().join("authored/readme.md"), b"later").expect("later edit");
    assert_eq!(input.inventory().identity(), before);
    assert_eq!(
        input.file(&"readme.md".parse().expect("entry")),
        Some(b"local".as_slice())
    );
}

#[test]
fn mixed_sources_prepare_in_target_order_with_proof_parity_and_no_source_writes() {
    let repo = Fixture::new();
    let parent = ParentFixture::new();
    let before_repo = source::inspect(repo.root.path(), None, source::Limits::default())
        .expect("repository before");
    let before_parent = source::inspect(parent.root.path(), None, source::Limits::default())
        .expect("parent before");
    let direct = request("z/repository", &repo);
    let child = PlacementRequest {
        target: "/a/child/".parse().expect("child target"),
        source: SourceSelection::Submodule {
            parent: parent.root.path().to_owned(),
            parent_commit: parent.parent.to_string().parse().expect("parent pin"),
            submodule_path: "packages/tool".parse().expect("gitlink"),
            subdirectory: "docs".parse().expect("docs"),
        },
    };
    let forward = source::prepare(vec![direct.clone(), child.clone()], limits())
        .expect("complete forward set");
    let reversed = source::prepare(vec![child, direct], limits()).expect("complete reversed set");
    assert_eq!(proofs(&forward), proofs(&reversed));
    let repo_output = repo.inspect(&[]);
    let parent_output = parent.inspect(&[]);
    assert!(repo_output.status.success(), "{repo_output:?}");
    assert!(parent_output.status.success(), "{parent_output:?}");
    assert_eq!(
        proofs(&forward),
        vec![
            (
                "a/child".into(),
                serde_json::from_slice(&parent_output.stdout).expect("parent CLI")
            ),
            (
                "z/repository".into(),
                serde_json::from_slice(&repo_output.stdout).expect("repo CLI")
            ),
        ]
    );
    for placement in forward.placements() {
        assert_eq!(placement.source().inventory().total_bytes(), 3);
        assert_eq!(
            placement
                .source()
                .file(&"readme.md".parse().expect("entry")),
            Some(b"abc".as_slice())
        );
    }
    assert_eq!(
        before_repo,
        source::inspect(repo.root.path(), None, source::Limits::default()).expect("repo after")
    );
    assert_eq!(
        before_parent,
        source::inspect(parent.root.path(), None, source::Limits::default()).expect("parent after")
    );
}

#[test]
fn normalized_duplicate_and_ancestor_targets_fail_before_native_reads() {
    let fixture = Fixture::new();
    for targets in [
        vec!["/docs/", "%64ocs"],
        vec!["a/b", "a"],
        vec!["a/b", "a-b", "a"],
    ] {
        let requests = targets
            .iter()
            .map(|target| {
                let mut selected = request(target, &fixture);
                let SourceSelection::Repository { root, .. } = &mut selected.source else {
                    unreachable!()
                };
                *root = fixture.root.path().join("intentionally-absent");
                selected
            })
            .collect();
        let error = source::prepare(
            requests,
            PreparationLimits {
                sources: 3,
                ..limits()
            },
        )
        .expect_err("target preflight, not a missing-source error");
        let PreparationError::Overlap { first, second } = error else {
            panic!("{error:?}")
        };
        assert_eq!(
            first.as_str(),
            if targets.len() == 2 && targets[0] == "/docs/" {
                "docs"
            } else {
                "a"
            }
        );
        assert_eq!(
            second.as_str(),
            if first.as_str() == "docs" {
                "docs"
            } else {
                "a/b"
            }
        );
    }
}

#[test]
fn source_count_is_checked_before_any_source_read() {
    let fixture = Fixture::new();
    let mut selected = request("docs", &fixture);
    let SourceSelection::Repository { root, .. } = &mut selected.source else {
        unreachable!()
    };
    *root = fixture.root.path().join("intentionally-absent");
    assert!(matches!(
        source::prepare(
            vec![selected],
            PreparationLimits {
                sources: 0,
                ..limits()
            }
        ),
        Err(PreparationError::TooManySources { limit: 0 })
    ));
}

#[test]
fn late_missing_dirty_or_wrong_pin_source_never_returns_an_earlier_subset() {
    for failure in ["missing", "dirty", "wrong pin"] {
        let first = Fixture::new();
        let last = Fixture::new();
        let mut last_request = request("z", &last);
        let SourceSelection::Repository { root, commit, .. } = &mut last_request.source else {
            unreachable!()
        };
        match failure {
            "missing" => *root = last.root.path().join("intentionally-absent"),
            "dirty" => {
                fs::write(last.root.path().join("docs/readme.md"), b"xyz").expect("dirty fixture")
            }
            _ => {
                *commit = "1111111111111111111111111111111111111111"
                    .parse()
                    .expect("wrong pin")
            }
        }
        let before =
            source::inspect(first.root.path(), None, source::Limits::default()).expect("before");
        let error = source::prepare(vec![last_request, request("a", &first)], limits())
            .expect_err("complete set must fail");
        let PreparationError::Source { target, error } = error else {
            panic!("{error:?}")
        };
        assert_eq!(target.as_str(), "z");
        match failure {
            "dirty" => assert!(matches!(error, source::Error::ContentMismatch(_))),
            "wrong pin" => assert!(matches!(error, source::Error::RevisionMismatch { .. })),
            _ => assert!(matches!(error, source::Error::Io { .. })),
        }
        assert_eq!(
            before,
            source::inspect(first.root.path(), None, source::Limits::default()).expect("after")
        );
        assert_eq!(
            fs::read(last.root.path().join("docs/readme.md")).expect("last source"),
            if failure == "dirty" { b"xyz" } else { b"abc" }
        );
    }
}

#[test]
fn aggregate_entry_and_byte_limits_apply_to_every_retained_placement() {
    let fixture = Fixture::new();
    let requests = vec![request("a", &fixture), request("b", &fixture)];
    let exact = source::prepare(requests.clone(), limits()).expect("exact complete-set bounds");
    assert_eq!(exact.placements().len(), 2);
    for bound in [
        PreparationLimits {
            entries: 1,
            ..limits()
        },
        PreparationLimits {
            bytes: 5,
            ..limits()
        },
    ] {
        let error = source::prepare(requests.clone(), bound).expect_err("aggregate exhausted");
        let PreparationError::Source { target, error } = error else {
            panic!("{error:?}")
        };
        assert_eq!(target.as_str(), "b");
        assert!(matches!(error, source::Error::Limit { .. }), "{error:?}");
    }
}

#[test]
fn whole_set_allowance_does_not_relax_each_sources_limits() {
    let fixture = Fixture::new();
    for per_source in [
        RevisionLimits {
            content: source::Limits {
                file_bytes: 2,
                ..source::Limits::default()
            },
            ..RevisionLimits::default()
        },
        RevisionLimits {
            content: source::Limits {
                total_bytes: 2,
                ..source::Limits::default()
            },
            ..RevisionLimits::default()
        },
        RevisionLimits {
            metadata_bytes: 0,
            ..RevisionLimits::default()
        },
    ] {
        let error = source::prepare(
            vec![request("a", &fixture)],
            PreparationLimits {
                per_source,
                ..limits()
            },
        )
        .expect_err("per-source bound");
        assert!(
            matches!(
                error,
                PreparationError::Source {
                    error: source::Error::Limit { .. },
                    ..
                }
            ),
            "{error:?}"
        );
    }
}

#[test]
fn explicitly_empty_source_set_is_structurally_empty_not_certification() {
    let set = source::prepare(
        Vec::new(),
        PreparationLimits {
            sources: 0,
            entries: 0,
            bytes: 0,
            per_source: RevisionLimits::default(),
        },
    )
    .expect("no requested placements");
    assert!(set.placements().is_empty());
}
