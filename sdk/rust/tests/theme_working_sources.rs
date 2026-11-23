//! Theme discovery omits Git-ignored tooling without weakening explicit source selection.

use std::{fs, os::unix::fs::symlink, path::Path};

use gazette_sdk::{build, source::PreparationLimits};

const MANIFEST: &str = "package='theme:base'\nnamespace='base'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn config(root: &Path, location: &str, shared: bool) -> String {
    let shared = if shared {
        "[[brands]]\nid='brand:one'\nname='Shared'\n[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:theme',target='content'}]\n"
    } else {
        ""
    };
    format!(
        "[composition]\ntheme={{package='theme:base'}}\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:theme'\nlocation={{root={:?},{location}}}\n\
         [[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='presentation.toml'\n{shared}",
        root.to_str().unwrap(),
    )
}

fn acquire(config: &str) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_composition(
        config,
        &"site:one".parse().unwrap(),
        Default::default(),
        limits(),
    )
}

fn limits() -> PreparationLimits {
    PreparationLimits {
        sources: 8,
        entries: 10_000,
        bytes: 64 << 20,
        per_source: Default::default(),
    }
}

fn fixture(root: &Path, subtree: &str) {
    let theme = root.join(subtree);
    write(&theme, "presentation.toml", MANIFEST);
    write(&theme, "templates/page.html", "{{ page.title }}");
    write(
        root,
        ".gitignore",
        "node_modules/\nignored/\n*.tmp\n!keep.tmp\n",
    );
    write(&theme, "keep.tmp", "intentional exception");
    write(&theme, "drop.tmp", "ignored");
    write(&theme, ".hidden", "retained hidden input");
    write(&theme, "nested/.gitignore", "local/\n");
    fs::create_dir_all(theme.join("nested/local")).unwrap();
    symlink("missing", theme.join("nested/local/tool")).unwrap();
    fs::create_dir_all(theme.join("node_modules/.bin")).unwrap();
    symlink("missing", theme.join("node_modules/.bin/playwright-core")).unwrap();
}

#[test]
fn ignored_tooling_is_skipped_before_native_type_checks_at_root_and_subtree() {
    for subtree in ["", "theme"] {
        let root = tempfile::tempdir().unwrap();
        git2::Repository::init(root.path()).unwrap();
        fixture(root.path(), subtree);
        let location = if subtree.is_empty() {
            "kind='filesystem'".to_owned()
        } else {
            format!("kind='filesystem',subdirectory='{subtree}'")
        };
        let inputs = acquire(&config(root.path(), &location, false)).unwrap();
        let source = &inputs.sources()[&"source:theme".parse().unwrap()];
        for path in [
            "presentation.toml",
            "templates/page.html",
            ".hidden",
            "keep.tmp",
        ] {
            assert!(source.file(&path.parse().unwrap()).is_some(), "{path}");
        }
        for path in ["node_modules", "nested/local", "drop.tmp", ".git"] {
            assert!(
                source.inventory().entries().iter().all(|entry| {
                    entry.path.as_str() != path
                        && !entry.path.as_str().starts_with(&format!("{path}/"))
                }),
                "{path}"
            );
        }
        symlink("missing", root.path().join(subtree).join("unsafe-link")).unwrap();
        let error = acquire(&config(root.path(), &location, false)).unwrap_err();
        assert!(error.to_string().contains("unsafe-link"), "{error}");
    }
}

#[test]
fn tracked_inputs_survive_ignore_rules_and_literal_exclusions_still_apply() {
    let root = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(root.path()).unwrap();
    fixture(root.path(), "");
    write(root.path(), "ignored/tracked.css", "body {}");
    symlink("missing", root.path().join("ignored/tool")).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("ignored/tracked.css")).unwrap();
    index.write().unwrap();
    let inputs = acquire(&config(root.path(), "kind='filesystem'", false)).unwrap();
    let source = &inputs.sources()[&"source:theme".parse().unwrap()];
    assert_eq!(
        source.file(&"ignored/tracked.css".parse().unwrap()),
        Some(&b"body {}"[..])
    );
    let inputs = acquire(&config(
        root.path(),
        "kind='filtered-filesystem',exclude=['ignored/tracked.css']",
        false,
    ))
    .unwrap();
    let source = &inputs.sources()[&"source:theme".parse().unwrap()];
    assert!(
        source
            .file(&"ignored/tracked.css".parse().unwrap())
            .is_none()
    );
}

#[test]
fn explicit_and_content_shared_selections_do_not_apply_theme_ignores() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    fixture(root.path(), "");
    let explicit =
        "kind='included-filesystem',include=['presentation.toml','templates','drop.tmp']";
    let inputs = acquire(&config(root.path(), explicit, false)).unwrap();
    assert!(
        inputs.sources()[&"source:theme".parse().unwrap()]
            .file(&"drop.tmp".parse().unwrap())
            .is_some()
    );
    for (location, shared) in [
        (
            "kind='included-filesystem',include=['presentation.toml','templates','node_modules']",
            false,
        ),
        ("kind='filesystem'", true),
    ] {
        let error = acquire(&config(root.path(), location, shared)).unwrap_err();
        assert!(error.to_string().contains("unsupported entry"), "{error}");
        if !shared {
            assert!(error.to_string().contains("playwright-core"), "{error}");
        }
    }
}

#[test]
fn non_repository_sources_keep_ordinary_capture_and_ignore_discovery_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "");
    let error = acquire(&config(root.path(), "kind='filesystem'", false)).unwrap_err();
    assert!(error.to_string().contains("unsupported entry"), "{error}");
    git2::Repository::init(root.path()).unwrap();
    let mut limits = limits();
    limits.per_source.metadata_bytes = 0;
    let error = build::acquire_composition(
        &config(root.path(), "kind='filesystem'", false),
        &"site:one".parse().unwrap(),
        Default::default(),
        limits,
    )
    .unwrap_err();
    assert!(error.to_string().contains("working path bytes"), "{error}");
}

#[test]
fn pinned_repository_capture_still_requires_the_complete_selected_tree() {
    let root = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(root.path()).unwrap();
    fixture(root.path(), "theme");
    let mut index = repo.index().unwrap();
    for path in ["theme/presentation.toml", "theme/templates/page.html"] {
        index.add_path(Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let commit = repo
        .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
        .unwrap();
    let location = format!("kind='repository',subdirectory='theme',commit='{commit}'");
    let error = acquire(&config(root.path(), &location, false)).unwrap_err();
    assert!(error.to_string().contains("unsupported entry"), "{error}");
}
