//! Modular workspace acquisition selects one exact native document set.

use std::{cell::Cell, fs, path::Path};

use gazette_adapter_fs::FileSystem;
use gazette_application::configuration;
use gazette_compiler::workspace::{DeclarationKind, FieldQuery, SourceRoot};
use gazette_core::source::{CapturedSource, EntryPath, Limits};
use gazette_ports::source::SelectedSourceCapture;

const ROOT: &str = "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
const SITE: &str = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

struct ForbiddenReader;

impl SelectedSourceCapture for ForbiddenReader {
    fn capture_selected(
        &self,
        _root: &Path,
        _paths: &[EntryPath],
        _limits: Limits,
    ) -> Result<CapturedSource, gazette_core::source::Error> {
        panic!("a relative root must be rejected before native acquisition")
    }
}

#[test]
fn relative_root_is_rejected_before_native_acquisition() {
    let workspace: EntryPath = "gazette.toml".parse().expect("root path");
    let error = configuration::load(
        &ForbiddenReader,
        Path::new("relative-root"),
        &workspace,
        configuration::Limits::default(),
    )
    .expect_err("cwd-dependent root");
    assert_eq!(error.code(), "workspace_root_not_absolute");
}

#[test]
fn portable_source_roots_resolve_only_against_the_selected_workspace() {
    let mut logical_digest = None;
    for workspace in [
        tempfile::tempdir().expect("first workspace"),
        tempfile::tempdir().expect("second workspace"),
    ] {
        fs::create_dir(workspace.path().join("sites")).expect("site module directory");
        fs::write(workspace.path().join("gazette.toml"), ROOT).expect("root document");
        fs::write(workspace.path().join("sites/one.toml"), SITE).expect("site document");
        let loaded = configuration::load(
            &FileSystem,
            workspace.path(),
            &"gazette.toml".parse().expect("root path"),
            configuration::Limits::default(),
        )
        .expect("portable workspace");
        let digest = loaded.documents().compilation.source_sha256;
        let resolved = loaded.into_resolved();
        assert_eq!(
            resolved
                .source_root(&".".parse::<SourceRoot>().expect("workspace root"))
                .expect("selected native root"),
            workspace.path()
        );
        assert_eq!(
            resolved
                .source_root(
                    &"sources/content"
                        .parse::<SourceRoot>()
                        .expect("relative source root"),
                )
                .expect("selected native root"),
            workspace.path().join("sources/content")
        );
        if let Some(expected) = logical_digest {
            assert_eq!(
                digest, expected,
                "native relocation must not enter workspace identity"
            );
        } else {
            logical_digest = Some(digest);
        }
    }
}

#[test]
fn detached_configuration_never_resolves_workspace_relative_sources_from_cwd() {
    let resolved = configuration::resolve(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
        gazette_compiler::toml::Limits::default(),
    )
    .expect("detached workspace");
    let error = resolved
        .source_root(
            &"sources/content"
                .parse::<SourceRoot>()
                .expect("relative source root"),
        )
        .expect_err("detached bytes have no selected native root");
    assert_eq!(
        error.to_string(),
        "workspace-relative source root requires a selected native workspace; load the workspace from an explicit root instead of supplying detached TOML bytes"
    );
    assert_eq!(
        resolved
            .source_root(
                &"/external/project"
                    .parse::<SourceRoot>()
                    .expect("explicit external root"),
            )
            .expect("external roots do not need workspace context"),
        Path::new("/external/project")
    );
}

#[test]
fn exact_native_document_set_resolves_once_without_sibling_discovery() {
    let fixture = tempfile::tempdir().expect("workspace root");
    fs::create_dir(fixture.path().join("sites")).expect("site module directory");
    fs::write(fixture.path().join("gazette.toml"), ROOT).expect("root document");
    fs::write(fixture.path().join("sites/one.toml"), SITE).expect("site document");
    fs::write(fixture.path().join("unselected.toml"), b"\xff").expect("invalid unselected sibling");
    let root: EntryPath = "gazette.toml".parse().expect("root path");

    let first = configuration::load(
        &FileSystem,
        fixture.path(),
        &root,
        configuration::Limits::default(),
    )
    .expect("strict modular workspace");
    let second = configuration::load(
        &FileSystem,
        fixture.path(),
        &root,
        configuration::Limits::default(),
    )
    .expect("deterministic replay");
    assert_eq!(
        first.documents().compilation.source_sha256,
        second.documents().compilation.source_sha256
    );
    assert_eq!(
        first
            .documents()
            .documents
            .iter()
            .map(|document| document.path.as_str())
            .collect::<Vec<_>>(),
        ["gazette.toml", "sites/one.toml"]
    );
    assert_eq!(
        first
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["gazette.toml", "sites", "sites/one.toml"]
    );
    let parsed = gazette_compiler::workspace::ParsedWorkspaceDocuments::parse(
        &root,
        [
            (&root, ROOT.as_bytes()),
            (
                &"sites/one.toml".parse().expect("site path"),
                SITE.as_bytes(),
            ),
        ],
        gazette_compiler::toml::SetLimits::default(),
    )
    .expect("origin-bearing compilation");
    let origin = parsed
        .origin(&FieldQuery {
            kind: DeclarationKind::Site,
            identity: "site:one".parse().expect("site identity"),
            field: vec![gazette_compiler::toml::PathSegment::Key("publisher".into())],
        })
        .expect("module-local field origin");
    assert_eq!(origin.document.as_str(), "sites/one.toml");
}

struct ChangingRoot {
    calls: Cell<usize>,
}

impl SelectedSourceCapture for ChangingRoot {
    fn capture_selected(
        &self,
        root: &Path,
        paths: &[EntryPath],
        limits: Limits,
    ) -> Result<CapturedSource, gazette_core::source::Error> {
        let result = FileSystem.capture_selected(root, paths, limits)?;
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call == 0 {
            fs::write(
                root.join("gazette.toml"),
                format!("{ROOT}# changed between selection and complete capture\n"),
            )
            .expect("change disposable root");
        }
        Ok(result)
    }
}

#[test]
fn root_change_between_include_selection_and_complete_capture_fails_closed() {
    let fixture = tempfile::tempdir().expect("workspace root");
    fs::create_dir(fixture.path().join("sites")).expect("site module directory");
    fs::write(fixture.path().join("gazette.toml"), ROOT).expect("root document");
    fs::write(fixture.path().join("sites/one.toml"), SITE).expect("site document");
    let root: EntryPath = "gazette.toml".parse().expect("root path");
    let reader = ChangingRoot {
        calls: Cell::new(0),
    };

    let error = configuration::load(
        &reader,
        fixture.path(),
        &root,
        configuration::Limits::default(),
    )
    .expect_err("a changed selector document cannot choose the compiled set");
    assert_eq!(error.code(), "source_changed");
    assert_eq!(reader.calls.get(), 2);
}

#[test]
fn machine_binding_is_captured_once_without_changing_portable_configuration_identity() {
    let mut logical_digest = None;
    let mut binding_digest = None;
    for root in [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()] {
        fs::create_dir(root.path().join("sites")).unwrap();
        fs::create_dir(root.path().join("tools")).unwrap();
        let config = format!(
            "machine_binding='tools/studio.local.toml'\n{ROOT}[[sources]]\nid='source:bound'\nlocation={{kind='filesystem',root='machine:documents/Project',subdirectory='docs'}}\n[[sources]]\nid='source:unbound'\nlocation={{kind='filesystem',root='machine:missing'}}\n[[sources]]\nid='source:relative'\nlocation={{kind='filesystem',root='local'}}\n"
        );
        fs::write(root.path().join("gazette.toml"), &config).unwrap();
        fs::write(root.path().join("sites/one.toml"), SITE).unwrap();
        let binding = format!(
            "[instance]\nname='Unrelated studio setting'\n[roots]\ndocuments={:?}\nbuilds='outputs'\n",
            root.path().join("documents").to_str().unwrap()
        );
        let binding_path: EntryPath = "tools/studio.local.toml".parse().unwrap();
        fs::write(root.path().join(binding_path.as_str()), &binding).unwrap();
        let loaded = configuration::load(
            &FileSystem,
            root.path(),
            &"gazette.toml".parse().unwrap(),
            configuration::Limits::default(),
        )
        .unwrap();
        assert_eq!(loaded.source(&binding_path), Some(binding.as_bytes()));
        let native = serde_json::to_value(&loaded).unwrap();
        assert_eq!(native["native_root"], serde_json::json!(root.path()));
        assert_eq!(
            native["source_roots"]["source:bound"],
            serde_json::json!(root.path().join("documents/Project"))
        );
        assert_eq!(
            native["source_roots"]["source:relative"],
            serde_json::json!(root.path().join("local"))
        );
        assert!(native["source_roots"].get("source:unbound").is_none());
        assert_eq!(loaded.source_roots().len(), 2);
        assert!(
            !root.path().join("documents").exists(),
            "projection must not require source acquisition"
        );

        assert_eq!(
            loaded.documents().documents.len(),
            2,
            "binding is not a configuration module"
        );
        assert!(
            loaded
                .inventory()
                .entries()
                .iter()
                .any(|entry| entry.path == binding_path)
        );
        let digest = loaded.documents().compilation.source_sha256;
        if let Some(previous) = logical_digest {
            assert_eq!(
                digest, previous,
                "machine paths do not enter logical identity"
            );
        }
        logical_digest = Some(digest);
        let digest = gazette_core::source::Sha256Digest::of(binding.as_bytes());
        if let Some(previous) = binding_digest {
            assert_ne!(
                digest, previous,
                "distinct binding bytes remain in acquisition evidence"
            );
        }
        binding_digest = Some(digest);
        // Resolution must use captured values even if the original binding changes.
        fs::write(
            root.path().join(binding_path.as_str()),
            "[roots]\ndocuments='/changed'\n",
        )
        .unwrap();
        let resolved = loaded.into_resolved();
        for (coordinate, expected) in [
            ("machine:documents", root.path().join("documents")),
            (
                "machine:documents/Projects/one",
                root.path().join("documents/Projects/one"),
            ),
            (
                "machine:builds/site",
                root.path().join("tools/outputs/site"),
            ),
        ] {
            assert_eq!(
                resolved.source_root(&coordinate.parse().unwrap()).unwrap(),
                expected
            );
        }
        assert!(
            resolved
                .source_root(&"machine:missing".parse().unwrap())
                .is_err()
        );
        let logical = serde_json::to_string(resolved.compilation()).unwrap();
        assert!(!logical.contains(root.path().to_str().unwrap()));
    }
}

#[test]
fn missing_invalid_or_detached_machine_bindings_never_use_ambient_paths() {
    let root = tempfile::tempdir().unwrap();
    let config = format!(
        "machine_binding='studio.local.toml'\n{SITE}[[principals]]\nid='owner:one'\nkind='person'\n"
    );
    fs::write(root.path().join("gazette.toml"), &config).unwrap();
    let load = || {
        configuration::load(
            &FileSystem,
            root.path(),
            &"gazette.toml".parse().unwrap(),
            configuration::Limits::default(),
        )
    };
    assert!(
        load().is_err(),
        "missing binding cannot fall back to cwd or another file"
    );
    for invalid in [
        "[roots]\ndocuments=42",
        "[roots]\ndocuments=''",
        "[roots]\ndocuments='x'\ndocuments='y'",
    ] {
        fs::write(root.path().join("studio.local.toml"), invalid).unwrap();
        assert_eq!(load().unwrap_err().code(), "workspace_machine_binding");
    }
    let detached = configuration::resolve(&config, Default::default()).unwrap();
    assert!(
        detached
            .source_root(&"machine:documents/page".parse().unwrap())
            .is_err()
    );
}

#[test]
fn identical_relative_bindings_keep_distinct_native_projection_origins() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut results = Vec::new();
    for root in [first.path(), second.path()] {
        fs::write(
            root.join("studio.local.toml"),
            "[roots]\ncontent='content'\n",
        )
        .unwrap();
        fs::write(root.join("gazette.toml"), format!(
            "machine_binding='studio.local.toml'\n{SITE}[[principals]]\nid='owner:one'\nkind='person'\n[[sources]]\nid='source:one'\nlocation={{kind='filesystem',root='machine:content',subdirectory='docs'}}\n"
        )).unwrap();
        let loaded = configuration::load(
            &FileSystem,
            root,
            &"gazette.toml".parse().unwrap(),
            Default::default(),
        )
        .unwrap();
        let projection = serde_json::to_value(&loaded).unwrap();
        assert_eq!(projection["native_root"], serde_json::json!(root));
        assert_eq!(
            projection["source_roots"]["source:one"],
            serde_json::json!(root.join("content"))
        );
        let resolved = loaded.into_resolved();
        let evidence = serde_json::to_value(resolved.evidence()).unwrap();
        assert_eq!(evidence["native_root"], projection["native_root"]);
        results.push((projection, evidence));
    }
    assert_eq!(results[0].0["documents"], results[1].0["documents"]);
    assert_ne!(results[0].0["source_roots"], results[1].0["source_roots"]);
    assert_ne!(results[0].1, results[1].1);
}
