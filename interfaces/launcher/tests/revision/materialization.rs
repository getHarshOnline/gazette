//! Actual acquisition, staging and pinned read-back; no marker-based ownership shortcut.

use std::{
    fs,
    os::unix::fs::symlink,
    path::Path,
    process::{Command, Output},
};

use gazette_sdk::{
    markdown,
    source::{self, Content, GeneratedTreeBinding, Limits, Sha256Digest, StageError},
};
use git2::Repository;
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

use super::{Fixture, context, limits, request};

#[test]
fn included_source_manifest_preserves_the_selection_and_captured_tree() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/guide.md"), "# Guide\n").unwrap();
    fs::write(root.path().join("private.md"), "excluded").unwrap();
    let repo = Repository::init(root.path()).unwrap();
    let tree = repo.treebuilder(None).unwrap().write().unwrap();
    let commit = super::super::commit_tree(&repo, tree, "manual branch");
    repo.branch("manuals", &repo.find_commit(commit).unwrap(), false)
        .unwrap();
    repo.set_head("refs/heads/manuals").unwrap();
    let input = source::input::parse(
        &json!({
            "target": "manual", "project": "project", "deployment": "local",
            "source": {"kind": "included_filesystem", "root": root.path(), "include": ["docs"], "expected_branch": "manuals"}
        })
        .to_string(),
    )
    .unwrap();
    let (context, requests) =
        source::input::bind("workspace".into(), "site".into(), vec![input]).unwrap();
    let prepared = source::prepare(
        requests,
        source::PreparationLimits {
            bytes: 8,
            ..limits()
        },
    )
    .unwrap();
    let authored = tempdir().unwrap();
    let parent = tempdir().unwrap();
    let tree = source::stage(
        &prepared,
        &context,
        authored.path(),
        source::StageLocation::Fresh(parent.path()),
        Limits::default(),
    )
    .unwrap();
    let pin = manifest_pin(&tree, "manual");
    let binding = context.binding(&"manual".parse().unwrap()).unwrap();
    let verified = source::verify_generated_tree(
        &tree.path().join("manual"),
        &binding,
        pin,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        verified.manifest().source(),
        &source::GeneratedSource::IncludedFilesystem {
            include: vec!["docs".parse().unwrap()],
            expected_branch: Some("manuals".into()),
        }
    );
    assert_eq!(
        verified.source().inventory(),
        prepared.placements()[0].source().inventory()
    );
    assert!(!tree.path().join("manual/private.md").exists());
    assert_eq!(
        fs::read(tree.path().join("manual/docs/guide.md")).unwrap(),
        b"# Guide\n"
    );
    tree.close().unwrap();
}

struct Materialized {
    tree: source::StagedTree,
    prepared: source::PreparedSourceSet,
    binding: GeneratedTreeBinding,
    pin: Sha256Digest,
    _repo: Fixture,
    _authored: TempDir,
    _parent: TempDir,
}

impl Materialized {
    fn new() -> Self {
        let repo = Fixture::new();
        let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
        let context = context(&prepared);
        let binding = context
            .binding(&"project".parse().expect("target"))
            .expect("binding");
        let authored = tempdir().expect("authored");
        let parent = tempdir().expect("staging");
        let tree = source::stage(
            &prepared,
            &context,
            authored.path(),
            source::StageLocation::Fresh(parent.path()),
            Limits::default(),
        )
        .expect("stage");
        let pin = manifest_pin(&tree, "project");
        Self {
            tree,
            prepared,
            binding,
            pin,
            _repo: repo,
            _authored: authored,
            _parent: parent,
        }
    }

    fn root(&self) -> std::path::PathBuf {
        self.tree.path().join("project")
    }

    fn cli(&self, extra: &[&str]) -> Output {
        command(&self.root(), &self.binding, self.pin)
            .args(extra)
            .output()
            .expect("Gazette")
    }
}

pub(super) fn manifest_pin(tree: &source::StagedTree, target: &str) -> Sha256Digest {
    let path = format!("{target}/{}", source::GENERATED_TREE_MANIFEST);
    let entry = tree
        .inventory()
        .entries()
        .iter()
        .find(|entry| entry.path.as_str() == path)
        .expect("staging evidence");
    match entry.content {
        Content::File { sha256, .. } => sha256,
        Content::Directory => panic!("manifest must be a file"),
    }
}

fn command(root: &Path, binding: &GeneratedTreeBinding, pin: Sha256Digest) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("verify-generated-tree")
        .arg(root)
        .arg("--expected-binding")
        .arg(serde_json::to_string(binding).expect("binding JSON"))
        .arg("--manifest-sha256")
        .arg(pin.to_string());
    command
}

fn failure(output: &Output, reason: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "no partial report: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(reason),
        "{output:?}"
    );
}

#[test]
fn independently_pinned_materialization_drives_sdk_cli_and_compiler_without_live_rereads() {
    let fixture = Materialized::new();
    let root = fixture.root();
    let before = source::inspect(&root, None, Limits::default()).expect("before");
    let verified =
        source::verify_generated_tree(&root, &fixture.binding, fixture.pin, Limits::default())
            .expect("pinned read-back");
    assert_eq!(
        verified.source().inventory(),
        fixture.prepared.placements()[0].source().inventory()
    );
    assert_eq!(verified.manifest().binding(), &fixture.binding);
    assert_eq!(
        verified.manifest().source(),
        &source::GeneratedSource::Repository {
            commit: fixture._repo.commit.to_string(),
            docs_path: "docs".into(),
        }
    );
    assert!(
        verified
            .source()
            .file(&source::GENERATED_TREE_MANIFEST.parse().expect("metadata"))
            .is_none()
    );
    let output = fixture.cli(&[]);
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report");
    assert_eq!(
        report,
        json!({"manifest": verified.manifest(), "inventory": verified.source().inventory()})
    );
    assert_eq!(
        source::inspect(&root, None, Limits::default()).expect("after"),
        before
    );
    let marker = fs::read_to_string(root.join(source::GENERATED_TREE_MANIFEST)).expect("manifest");
    assert!(!marker.contains(fixture.tree.path().to_str().expect("UTF-8")));
    assert!(!marker.contains(fixture._repo.root.path().to_str().expect("UTF-8")));
    fs::write(root.join("readme.md"), b"later bytes").expect("later change");
    let bytes = verified
        .source()
        .file(&"readme.md".parse().expect("path"))
        .expect("retained");
    let parsed = markdown::ParsedMarkdown::parse(
        std::str::from_utf8(bytes).expect("UTF-8"),
        markdown::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("compiler");
    assert_eq!(parsed.render_html().expect("HTML"), "<p>abc</p>\n");
    failure(&fixture.cli(&[]), "do not match their pinned source tree");
    assert_eq!(
        fs::read(root.join("readme.md")).expect("preserved"),
        b"later bytes"
    );
}

#[test]
fn every_binding_and_independent_pin_is_required_without_mutation() {
    let fixture = Materialized::new();
    let before = source::inspect(&fixture.root(), None, Limits::default()).expect("before");
    for field in ["workspace", "site", "project", "deployment", "target"] {
        let mut binding = serde_json::to_value(&fixture.binding).expect("binding");
        binding[field] = json!("wrong");
        let binding = serde_json::from_value(binding).expect("typed binding");
        failure(
            &command(&fixture.root(), &binding, fixture.pin)
                .output()
                .expect("Gazette"),
            "binding mismatch",
        );
    }
    failure(
        &command(
            &fixture.root(),
            &fixture.binding,
            Sha256Digest::of(b"unrelated"),
        )
        .output()
        .expect("Gazette"),
        "pin mismatch",
    );
    assert_eq!(
        source::inspect(&fixture.root(), None, Limits::default()).expect("after"),
        before
    );
}

#[test]
fn changed_missing_extra_and_unsafe_documentation_never_yield_verified_bytes() {
    for mutation in [
        "changed",
        "missing",
        "extra",
        "empty directory",
        "symlink",
        "missing marker",
        "marker directory",
    ] {
        let fixture = Materialized::new();
        let root = fixture.root();
        let marker = root.join(source::GENERATED_TREE_MANIFEST);
        match mutation {
            "changed" => fs::write(root.join("readme.md"), b"xyz").expect("changed"),
            "missing" => fs::remove_file(root.join("readme.md")).expect("remove fixture file"),
            "extra" => fs::write(root.join("extra.md"), b"unknown").expect("extra"),
            "empty directory" => fs::create_dir(root.join("extra")).expect("directory"),
            "symlink" => symlink(root.join("readme.md"), root.join("link")).expect("link"),
            "missing marker" => fs::remove_file(&marker).expect("remove fixture marker"),
            _ => {
                fs::remove_file(&marker).expect("remove fixture marker");
                fs::create_dir(&marker).expect("directory marker");
            }
        }
        let output = fixture.cli(&[]);
        failure(&output, "no files changed");
        assert!(root.is_dir());
        if mutation == "extra" {
            assert_eq!(
                fs::read(root.join("extra.md")).expect("retained"),
                b"unknown"
            );
        }
        if mutation == "symlink" {
            assert!(
                fs::symlink_metadata(root.join("link"))
                    .expect("retained")
                    .is_symlink()
            );
        }
    }
}

#[test]
fn malformed_pinned_metadata_is_not_structural_acceptance() {
    let fixture = Materialized::new();
    let path = fixture.root().join(source::GENERATED_TREE_MANIFEST);
    let original = fs::read(&path).expect("metadata");
    let value: Value = serde_json::from_slice(&original).expect("JSON");
    let mut cases = Vec::new();
    for branch in [
        "",
        " ",
        "HEAD",
        "../main",
        "topic.lock",
        "main^",
        "main\n",
        &"x".repeat(1025),
    ] {
        let mut malformed = value.clone();
        malformed["source"] = json!({
            "kind": "included_filesystem", "include": ["readme.md"], "expected_branch": branch,
        });
        cases.push(serde_json::to_vec(&malformed).expect("fixture encoding"));
    }
    for field in ["format", "generator_version", "tree_format"] {
        let mut malformed = value.clone();
        malformed[field] = json!("unsupported");
        cases.push(serde_json::to_vec(&malformed).expect("fixture encoding"));
    }
    for (field, replacement) in [
        ("commit", json!("bad")),
        ("docs_path", json!("../escape")),
        ("extra", json!(true)),
    ] {
        let mut malformed = value.clone();
        malformed["source"][field] = replacement;
        cases.push(serde_json::to_vec(&malformed).expect("fixture encoding"));
    }
    for (field, replacement) in [
        ("workspace", json!("")),
        ("site", json!(" padded")),
        ("project", json!("x".repeat(513))),
        ("deployment", json!("control\n")),
        ("target", json!("%70roject")),
    ] {
        let mut malformed = value.clone();
        malformed["binding"][field] = replacement;
        cases.push(serde_json::to_vec(&malformed).expect("fixture encoding"));
    }
    for (field, replacement) in [("extra", json!(true)), ("source_tree_sha256", json!("bad"))] {
        let mut malformed = value.clone();
        malformed[field] = replacement;
        cases.push(serde_json::to_vec(&malformed).expect("fixture encoding"));
    }
    cases.push(
        format!(
            "{{\"format\":\"duplicate\",{}",
            std::str::from_utf8(&original[1..]).expect("UTF-8")
        )
        .into_bytes(),
    );
    cases.push(b"{}".to_vec());
    cases.push(vec![b' '; 16 * 1024 + 1]);
    for malformed in cases {
        fs::write(&path, &malformed).expect("malformed fixture");
        // Deliberately pin malformed bytes to exercise decoding, not to claim
        // that hashing a suspect marker grants trust or ownership.
        let output = command(
            &fixture.root(),
            &fixture.binding,
            Sha256Digest::of(&malformed),
        )
        .output()
        .expect("Gazette");
        failure(&output, "source_inventory");
        assert_eq!(fs::read(&path).expect("preserved"), malformed);
    }
}

#[test]
fn manifest_bytes_and_entries_consume_the_same_complete_tree_limits() {
    let fixture = Materialized::new();
    let inventory = source::inspect(&fixture.root(), None, Limits::default()).expect("tree");
    let bytes = fs::metadata(fixture.root().join(source::GENERATED_TREE_MANIFEST))
        .expect("marker")
        .len();
    for (arg, limit) in [
        ("--max-entries", "1".into()),
        ("--max-depth", "0".into()),
        ("--max-file-bytes", (bytes - 1).to_string()),
        (
            "--max-total-bytes",
            (inventory.total_bytes() - 1).to_string(),
        ),
    ] {
        failure(&fixture.cli(&[arg, &limit]), "source_limit");
    }
    let output = fixture.cli(&[
        "--max-entries",
        "2",
        "--max-depth",
        "1",
        "--max-file-bytes",
        &bytes.to_string(),
        "--max-total-bytes",
        &inventory.total_bytes().to_string(),
    ]);
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn reserved_metadata_collisions_refuse_check_and_stage_before_writes() {
    for (name, directory) in [
        (source::GENERATED_TREE_MANIFEST, false),
        (".GAZETTE-SOURCE.JSON", false),
        (source::GENERATED_TREE_MANIFEST, true),
    ] {
        let mut fixture = Fixture::new();
        let repo = Repository::open(fixture.root.path()).expect("repository");
        let original = repo.blob(b"abc").expect("original blob");
        let (object, mode) = if directory {
            fs::create_dir(fixture.root.path().join("docs").join(name))
                .expect("reserved directory");
            (
                repo.treebuilder(None)
                    .expect("tree")
                    .write()
                    .expect("empty tree"),
                0o040000,
            )
        } else {
            fs::write(fixture.root.path().join("docs").join(name), b"marker")
                .expect("reserved file");
            (repo.blob(b"marker").expect("blob"), 0o100644)
        };
        fixture.commit = super::super::commit_docs(
            &repo,
            &[("readme.md", original, 0o100644), (name, object, mode)],
            "reserved collision",
        );
        let prepared = source::prepare(
            vec![request("project", &fixture)],
            source::PreparationLimits {
                bytes: 9,
                ..limits()
            },
        )
        .expect("acquire native collision");
        let context = context(&prepared);
        let authored = tempdir().expect("authored");
        let parent = tempdir().expect("staging");
        let before =
            source::inspect(fixture.root.path(), None, Limits::default()).expect("source before");
        assert!(matches!(
            source::check_destination(&prepared, &context, authored.path(), Limits::default()),
            Err(source::DestinationError::Collision { .. })
        ));
        assert!(matches!(
            source::stage(
                &prepared,
                &context,
                authored.path(),
                source::StageLocation::Fresh(parent.path()),
                Limits::default()
            ),
            Err(StageError::Preflight(
                source::DestinationError::Collision { .. }
            ))
        ));
        assert!(
            fs::read_dir(parent.path())
                .expect("no stage")
                .next()
                .is_none()
        );
        assert_eq!(
            source::inspect(fixture.root.path(), None, Limits::default()).expect("source after"),
            before
        );
    }
}

#[test]
fn incomplete_or_invalid_explicit_context_precedes_native_reads_and_effects() {
    let fixture = Fixture::new();
    let prepared = source::prepare(vec![request("project", &fixture)], limits()).expect("prepare");
    let absent = fixture.root.path().join("absent");
    let staging = tempdir().expect("staging");
    for mutation in [
        "missing",
        "extra",
        "wrong target",
        "empty workspace",
        "empty site",
        "invalid project",
    ] {
        let mut context = context(&prepared);
        match mutation {
            "missing" => context.placements.clear(),
            "extra" => {
                context.placements.insert(
                    "extra".parse().expect("target"),
                    source::PlacementIdentity {
                        project: "extra".into(),
                        deployment: "extra".into(),
                    },
                );
            }
            "wrong target" => {
                let binding = context.placements.pop_first().expect("entry").1;
                context
                    .placements
                    .insert("wrong".parse().expect("target"), binding);
            }
            "empty workspace" => context.workspace.clear(),
            "empty site" => context.site.clear(),
            _ => {
                context
                    .placements
                    .values_mut()
                    .next()
                    .expect("identity")
                    .project = "\n".into()
            }
        }
        assert!(matches!(
            source::check_destination(&prepared, &context, &absent, Limits::default()),
            Err(source::DestinationError::Observation(
                source::Error::InvalidInventory { .. }
            ))
        ));
        assert!(matches!(
            source::stage(
                &prepared,
                &context,
                &absent,
                source::StageLocation::Fresh(staging.path()),
                Limits::default()
            ),
            Err(StageError::Preflight(
                source::DestinationError::Observation(source::Error::InvalidInventory { .. })
            ))
        ));
        assert!(
            fs::read_dir(staging.path())
                .expect("no stage")
                .next()
                .is_none()
        );
    }
}

#[test]
fn cli_requires_explicit_evidence_and_rejects_malformed_bindings_before_reading() {
    let temp = tempdir().expect("fixture");
    let missing = temp.path().join("missing");
    for args in [
        vec![],
        vec!["--manifest-sha256", "bad"],
        vec![
            "--expected-binding",
            "[\"workspace\",\"site\",\"project\",\"deployment\",\"target\"]",
            "--manifest-sha256",
            &"0".repeat(64),
        ],
        vec![
            "--expected-binding",
            "{}",
            "--manifest-sha256",
            &"0".repeat(64),
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("verify-generated-tree")
            .arg(&missing)
            .args(args)
            .output()
            .expect("Gazette");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let help = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["verify-generated-tree", "--help"])
        .output()
        .expect("help");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("UTF-8");
    for required in [
        "no writes",
        "--expected-binding",
        "--manifest-sha256",
        "--max-total-bytes",
    ] {
        assert!(help.contains(required), "{help}");
    }
    assert!(!missing.exists());
}
