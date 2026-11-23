//! Native selected manual reads, including commit verification.
use super::*;

struct Manual {
    root: TempDir,
    config: String,
    commit: String,
}

impl Manual {
    fn new(pinned: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path()).unwrap();
        let mut index = repo.index().unwrap();
        for (path, bytes) in [
            ("docs/index.md", b"# Guide\n\n[Component](component/README.md#details)\n\n[Fixture][data]\n\n[data]: fixtures/input.json\n".as_slice()),
            ("docs/component/README.md", b"# Component\n\n## Details\n\n![Example](../images/example.svg)\n"),
            ("docs/fixtures/input.json", b"{\"example\":true}"),
            ("docs/images/example.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            ("docs/unrelated.bin", &[b'x'; 8192]),
            ("docs/omitted.md", b"# Omitted\n\n[Not read](absent.md)\n"),
            (".gitignore", b"docs/ignored.json\n"),
        ] {
            let file = root.path().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, bytes).unwrap();
            index.add_path(std::path::Path::new(path)).unwrap();
        }
        std::os::unix::fs::symlink("missing", root.path().join("docs/unrelated-link")).unwrap();
        index
            .add_path(std::path::Path::new("docs/unrelated-link"))
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "manual", &tree, &[])
            .unwrap()
            .to_string();
        fs::write(root.path().join("docs/ignored.json"), b"ignored").unwrap();
        let location = if pinned {
            format!(
                "kind='repository',root={:?},subdirectory='docs',commit='{commit}'",
                root.path().to_str().unwrap()
            )
        } else {
            format!(
                "kind='filesystem',root={:?},subdirectory='docs'",
                root.path().to_str().unwrap()
            )
        };
        let config = format!(
            r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[projects]]
id='project:one'
lifecycle='active'
visibility='public'
availability='available'
[[sources]]
id='source:manual'
location={{{location}}}
[[brands]]
id='brand:one'
name='Manual'
[[brands.publications]]
site='site:one'
[brands.publications.documentation_repository]
name='manual-docs'
path={destination:?}
[brands.publications.documentation_repository.build]
target={{kind='mintlify',theme='mint',primary_color='#123abc'}}
[[brands.publications.documentation_repository.build.modules]]
project='project:one'
slug='manual'
title='Manual'
manual={{source='source:manual'}}
navigation=[{{kind='manual',title='Guide',pages=[
{{title='Guide',route='manual',input={{kind='file',path='index.md'}}}},
{{title='Component',route='manual/component',input={{kind='file',path='component/README.md'}}}}
]}}]
"#,
            destination = root.path().join("not-created").to_str().unwrap()
        );
        Self {
            root,
            config,
            commit,
        }
    }

    fn acquire(&self) -> Result<build::AcquiredInputs, build::DocumentationError> {
        build::acquire_documentation(
            &self.config,
            &"site:one".parse().unwrap(),
            Default::default(),
            source::PreparationLimits {
                sources: 1,
                entries: 32,
                bytes: 2048,
                per_source: source::RevisionLimits {
                    content: source::Limits {
                        file_bytes: 1024,
                        total_bytes: 2048,
                        entries: 32,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        )
    }
}

#[test]
fn selected_manual_reads_keep_routes_and_companions_without_unrelated_payloads() {
    for pinned in [false, true] {
        let fixture = Manual::new(pinned);
        let inputs = fixture
            .acquire()
            .expect("unrelated oversized blob and symlink stay unopened");
        let input = &inputs.sources()[&"source:manual".parse().unwrap()];
        let paths: Vec<_> = input
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "component",
                "component/README.md",
                "fixtures",
                "fixtures/input.json",
                "images",
                "images/example.svg",
                "index.md"
            ]
        );
        if let source::AcquiredInput::Repository(input) = input {
            assert!(input.verification().is_selected_only());
            assert_eq!(input.verification().commit().as_str(), fixture.commit);
        } else {
            assert!(!pinned);
        }
        let assembled = build::assemble_documentation(&inputs, Default::default()).unwrap();
        let guide = std::str::from_utf8(
            assembled
                .output()
                .file(&"manual.md".parse().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(guide.contains("/manual/component#details"));
        assert!(guide.contains("/manual/assets/fixtures/input.json"));
        assert!(
            assembled
                .output()
                .file(&"manual/assets/images/example.svg".parse().unwrap())
                .is_some()
        );
    }
}

#[test]
fn selected_manual_reads_refuse_ignored_missing_unsafe_and_escaping_companions() {
    for link in [
        "ignored.json",
        "missing.json",
        "unrelated-link",
        "../outside.json",
    ] {
        let fixture = Manual::new(false);
        fs::write(
            fixture.root.path().join("docs/index.md"),
            format!("# Guide\n\n[Input]({link})\n"),
        )
        .unwrap();
        assert!(fixture.acquire().is_err(), "must refuse {link}");
    }
}

#[test]
fn selected_manual_reads_check_selected_bytes_revision_and_file_limits() {
    let mut fixture = Manual::new(true);
    fs::write(
        fixture.root.path().join("docs/fixtures/input.json"),
        b"changed",
    )
    .unwrap();
    assert!(
        fixture.acquire().is_err(),
        "selected companion differs from commit"
    );
    fixture.config = fixture.config.replace(&fixture.commit, &"0".repeat(40));
    assert!(fixture.acquire().is_err(), "wrong checked-out revision");
    let fixture = Manual::new(false);
    fs::write(
        fixture.root.path().join("docs/fixtures/input.json"),
        vec![b'x'; 1025],
    )
    .unwrap();
    assert!(fixture.acquire().is_err(), "linked bytes retain file limit");
}

#[test]
fn selected_manual_metadata_reports_omissions_and_marks_unsafe_enumeration_incomplete() {
    let fixture = Manual::new(false);
    let inputs = fixture.acquire().unwrap();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    assert_eq!(assembly.unselected_manual_pages_incomplete().len(), 1);
    fs::remove_file(fixture.root.path().join("docs/unrelated-link")).unwrap();
    fs::write(
        fixture.root.path().join(".gitignore"),
        b"docs/ignored.json\ndocs/ignored.md\n",
    )
    .unwrap();
    fs::write(fixture.root.path().join("docs/ignored.md"), b"# Ignored").unwrap();
    let inputs = fixture.acquire().unwrap();
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    assert!(assembly.unselected_manual_pages_incomplete().is_empty());
    let report = serde_json::to_value(assembly.unselected_manual_pages()).unwrap();
    assert_eq!(report, serde_json::json!({"source:manual":["omitted.md"]}));
    assert!(
        inputs.sources()[&"source:manual".parse().unwrap()]
            .file(&"omitted.md".parse().unwrap())
            .is_none()
    );
}

#[test]
fn selected_manual_include_bounds_and_expected_branch_remain_required() {
    let mut fixture = Manual::new(false);
    let repo = Repository::open(fixture.root.path()).unwrap();
    let branch = repo.head().unwrap().shorthand().unwrap().to_owned();
    fixture.config = fixture
        .config
        .replace("kind='filesystem'", "kind='included-filesystem'")
        .replace(
            "subdirectory='docs'",
            &format!("include=['docs'],expected_branch='{branch}'"),
        )
        .replace("path='index.md'", "path='docs/index.md'")
        .replace(
            "path='component/README.md'",
            "path='docs/component/README.md'",
        );
    assert!(fixture.acquire().is_ok());
    let archive = fixture.root.path().join("archive");
    fs::create_dir(&archive).unwrap();
    fs::write(
        archive.join("unselected\nchild"),
        b"unexamined by the required-entry guard",
    )
    .unwrap();
    fixture.config = fixture
        .config
        .replace("include=['docs']", "include=['docs','archive']");
    assert!(
        fixture.acquire().is_ok(),
        "an unused required directory is observed without enumerating its children"
    );
    fixture.config = fixture
        .config
        .replace("include=['docs','archive']", "include=['docs']");
    fixture.config = fixture
        .config
        .replace("include=['docs']", "include=['docs','missing']");
    assert!(
        fixture.acquire().is_err(),
        "an unused explicit include is still required"
    );
    fixture.config = fixture
        .config
        .replace("include=['docs','missing']", "include=['docs']");
    fixture.config = fixture.config.replace(
        "include=['docs']",
        "include=['docs/index.md','docs/component']",
    );
    assert!(
        fixture.acquire().is_err(),
        "linked fixtures cannot escape the configured include bounds"
    );
    fixture.config = fixture
        .config
        .replace(
            "include=['docs/index.md','docs/component']",
            "include=['docs']",
        )
        .replace(
            &format!("expected_branch='{branch}'"),
            "expected_branch='different'",
        );
    assert!(fixture.acquire().is_err(), "no branch checkout or fallback");
}

#[test]
fn selected_manual_submodule_keeps_parent_gitlink_and_child_byte_verification() {
    let mut fixture = Manual::new(true);
    let parent = tempfile::tempdir().unwrap();
    let repo = Repository::init(parent.path()).unwrap();
    let child = parent.path().join("child");
    fs::rename(fixture.root.path(), &child).unwrap();
    let mut builder = repo.treebuilder(None).unwrap();
    builder
        .insert(
            "child",
            git2::Oid::from_str(&fixture.commit).unwrap(),
            0o160000,
        )
        .unwrap();
    let tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let commit = repo
        .commit(Some("HEAD"), &signature, &signature, "parent", &tree, &[])
        .unwrap();
    fixture.config = fixture.config.replace(
        &format!("kind='repository',root={:?},subdirectory='docs',commit='{}'", fixture.root.path().to_str().unwrap(), fixture.commit),
        &format!("kind='submodule',root={:?},subdirectory='docs',parent_commit='{commit}',submodule_path='child'", parent.path().to_str().unwrap()),
    );
    let inputs = fixture
        .acquire()
        .expect("selected child inputs match the parent's gitlink");
    let source::AcquiredInput::Submodule(input) =
        &inputs.sources()[&"source:manual".parse().unwrap()]
    else {
        panic!("submodule evidence")
    };
    assert!(input.verification().source().is_selected_only());
    assert_eq!(
        input.verification().parent_commit().as_str(),
        commit.to_string()
    );
    let evidence = serde_json::to_value(inputs.inspect()).unwrap();
    assert!(evidence.to_string().contains("\"selected_only\":true"));
    fs::write(child.join("docs/fixtures/input.json"), b"changed").unwrap();
    assert!(fixture.acquire().is_err());
}

#[test]
fn selected_manual_pinned_catalog_lists_omitted_names_without_reading_their_blobs() {
    let mut fixture = Manual::new(true);
    let repo = Repository::open(fixture.root.path()).unwrap();
    fs::remove_file(fixture.root.path().join("docs/unrelated-link")).unwrap();
    let mut index = repo.index().unwrap();
    index
        .remove_path(std::path::Path::new("docs/unrelated-link"))
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let previous = repo.head().unwrap().peel_to_commit().unwrap();
    let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let commit = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "metadata fixture",
            &tree,
            &[&previous],
        )
        .unwrap()
        .to_string();
    fixture.config = fixture.config.replace(&fixture.commit, &commit);
    fixture.commit = commit;
    let inputs = fixture.acquire().expect("the unselected 8192-byte blob exceeds the 1024-byte read bound but its metadata is eligible");
    let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
    assert!(assembly.unselected_manual_pages_incomplete().is_empty());
    assert_eq!(
        serde_json::to_value(assembly.unselected_manual_pages()).unwrap(),
        serde_json::json!({"source:manual":["omitted.md"]})
    );
    assert!(
        inputs.sources()[&"source:manual".parse().unwrap()]
            .file(&"omitted.md".parse().unwrap())
            .is_none()
    );
}

#[test]
fn selected_manual_filter_metadata_does_not_spend_retained_entry_capacity() {
    for earlier_source in [false, true] {
        let mut fixture = Manual::new(false);
        fs::remove_file(fixture.root.path().join("docs/unrelated-link")).unwrap();
        fixture.config = fixture
            .config
            .lines()
            .filter(|line| !line.starts_with("{title='Component'"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            fixture.root.path().join("docs/index.md"),
            b"# Guide\n\n![Image](image.svg)\n",
        )
        .unwrap();
        fs::write(fixture.root.path().join("docs/image.svg"), b"<svg/>").unwrap();
        let early = tempfile::tempdir().unwrap();
        if earlier_source {
            fs::write(early.path().join("early.md"), b"# Early\n").unwrap();
            fixture.config.push_str(&format!(r#"
[[sources]]
id='source:earlier'
location={{kind='filesystem',root={:?}}}
[[projects]]
id='project:earlier'
lifecycle='active'
visibility='public'
availability='available'
[[brands.publications.documentation_repository.build.modules]]
project='project:earlier'
slug='early'
title='Early'
manual={{source='source:earlier'}}
navigation=[{{kind='manual',title='Early',pages=[{{title='Early',route='early',input={{kind='file',path='early.md'}}}}]}}]
"#, early.path().to_str().unwrap()));
        }
        let limits = source::PreparationLimits {
            sources: 2,
            entries: if earlier_source { 3 } else { 2 },
            bytes: 1024,
            per_source: source::RevisionLimits {
                content: source::Limits {
                    entries: if earlier_source { 32 } else { 2 },
                    file_bytes: 512,
                    total_bytes: 1024,
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        let inputs = build::acquire_documentation(&fixture.config, &"site:one".parse().unwrap(), Default::default(), limits)
            .expect("only index.md and its linked image consume the remaining two entries, despite unrelated tracked paths and two capture passes");
        assert_eq!(
            inputs.sources()[&"source:manual".parse().unwrap()]
                .inventory()
                .entries()
                .len(),
            2
        );
        let assembly = build::assemble_documentation(&inputs, Default::default()).unwrap();
        assert!(
            assembly.unselected_manual_pages_incomplete().is_empty(),
            "metadata discovery has its own bounded allowance"
        );
        let too_few = source::PreparationLimits {
            entries: limits.entries - 1,
            ..limits
        };
        assert!(
            build::acquire_documentation(
                &fixture.config,
                &"site:one".parse().unwrap(),
                Default::default(),
                too_few
            )
            .is_err()
        );
        let no_metadata = source::PreparationLimits {
            per_source: source::RevisionLimits {
                metadata_bytes: 1,
                ..limits.per_source
            },
            ..limits
        };
        assert!(
            build::acquire_documentation(
                &fixture.config,
                &"site:one".parse().unwrap(),
                Default::default(),
                no_metadata
            )
            .is_err()
        );
    }
}
