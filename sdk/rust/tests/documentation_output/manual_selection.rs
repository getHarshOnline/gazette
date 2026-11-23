//! The existing report names retained pages omitted from authored navigation.

use super::*;

#[test]
fn repository_root_manual_combines_selected_readme_docs_and_linked_licence_only() {
    let fixture = OperationFixture::new("Documentation");
    git2::Repository::init(fixture.source.path()).unwrap();
    fs::create_dir(fixture.source.path().join("docs")).unwrap();
    fs::write(
        fixture.source.path().join("docs/start.md"),
        "# Start\n\n[Licence](../LICENSE)\n",
    )
    .unwrap();
    fs::write(
        fixture.source.path().join("README.md"),
        "# Repository\n\n[Guide](docs/start.md)\n",
    )
    .unwrap();
    fs::write(fixture.source.path().join("LICENSE"), "selected licence").unwrap();
    fs::write(
        fixture.source.path().join("unselected.txt"),
        "not published",
    )
    .unwrap();
    fs::write(fixture.source.path().join(".gitignore"), "output/\n").unwrap();
    fs::create_dir(fixture.source.path().join("output")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.source.path().join("output/tool")).unwrap();
    let configured = fixture.configuration
        .replace("kind='filesystem'", "kind='working-repository'")
        .replace("path='start.md'", "path='docs/start.md'")
        .replace("pages=[", "pages=[{title='Repository',route='manual/overview',input={kind='file',path='README.md'}},");
    fs::write(fixture.workspace.path().join("gazette.toml"), configured).unwrap();
    let input = fixture.local();
    let site = "site:one".parse().unwrap();
    operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(|execution| {
        let Execution::Complete { assembly, .. } = execution else {
            panic!("root manual assembly")
        };
        assert!(
            assembly
                .output()
                .file(&"manual/overview.md".parse().unwrap())
                .is_some()
        );
        assert!(
            assembly
                .output()
                .file(&"manual/start.md".parse().unwrap())
                .is_some()
        );
        assert_eq!(
            assembly
                .output()
                .file(&"manual/assets/LICENSE".parse().unwrap()),
            Some(&b"selected licence"[..])
        );
        assert!(
            assembly
                .output()
                .inventory()
                .entries()
                .iter()
                .all(|entry| !entry.path.as_str().contains("unselected")
                    && !entry.path.as_str().contains("output/tool"))
        );
    });
}

#[test]
fn working_repository_manual_retains_linked_assets_under_the_same_git_selection() {
    let fixture = OperationFixture::new("Documentation");
    git2::Repository::init(fixture.source.path()).unwrap();
    fs::create_dir(fixture.source.path().join("docs")).unwrap();
    fs::write(
        fixture.source.path().join("docs/start.md"),
        "# Start\n\n![Diagram](diagram.png)\n",
    )
    .unwrap();
    fs::write(
        fixture.source.path().join("docs/diagram.png"),
        b"linked image",
    )
    .unwrap();
    fs::write(
        fixture.source.path().join("docs/unlinked.png"),
        b"unselected",
    )
    .unwrap();
    fs::write(fixture.source.path().join(".gitignore"), "output/\n").unwrap();
    fs::create_dir(fixture.source.path().join("docs/output")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.source.path().join("docs/output/tool")).unwrap();
    let configured = fixture.configuration.replace(
        "kind='filesystem'",
        "kind='working-repository',subdirectory='docs'",
    );
    fs::write(fixture.workspace.path().join("gazette.toml"), configured).unwrap();
    let input = fixture.local();
    let site = "site:one".parse().unwrap();
    operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(|execution| {
        let Execution::Complete { assembly, .. } = execution else {
            panic!("working manual assembly")
        };
        assert_eq!(
            assembly
                .output()
                .file(&"manual/assets/diagram.png".parse().unwrap()),
            Some(&b"linked image"[..])
        );
        assert!(
            assembly
                .output()
                .file(&"manual/assets/unlinked.png".parse().unwrap())
                .is_none()
        );
    });
    fs::write(
        fixture.source.path().join(".gitignore"),
        "output/\ndiagram.png\n",
    )
    .unwrap();
    operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(|execution| {
        assert!(
            !matches!(execution, Execution::Complete { .. }),
            "an ignored linked asset must not be silently admitted"
        );
    });
}

#[test]
fn manual_selection_reports_omissions_and_discovers_only_linked_companions() {
    let fixture = OperationFixture::new("Documentation");
    fs::write(
        fixture.source.path().join("start.md"),
        "# Start\n\n![Diagram](diagram.png)\n",
    )
    .unwrap();
    fs::write(fixture.source.path().join("diagram.png"), b"linked image").unwrap();
    fs::write(fixture.source.path().join("unlinked.png"), b"not selected").unwrap();
    fs::write(fixture.source.path().join("draft.md"), "# Draft\n").unwrap();
    let input = fixture.local();
    let site = "site:one".parse().unwrap();
    operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(|execution| {
        let Execution::Complete { report, assembly } = execution else {
            panic!("complete manual assembly")
        };
        let value = serde_json::to_value(&report).unwrap();
        validate_schema(&value);
        assert_eq!(
            value["unselected_manual_pages"],
            json!({"source:manual":["draft.md"]})
        );
        assert!(
            assembly
                .output()
                .file(&"manual/assets/diagram.png".parse().unwrap())
                .is_some()
        );
        assert!(
            assembly
                .output()
                .file(&"manual/assets/unlinked.png".parse().unwrap())
                .is_none()
        );
        assert!(
            assembly
                .output()
                .file(&"manual/draft.md".parse().unwrap())
                .is_none()
        );
        assert_eq!(report.manual.len(), 1);
    });
}

#[test]
fn imported_reference_selections_do_not_become_omitted_manual_pages() {
    for selection in [
        "{kind='file',path='reference/index.md'}",
        "{kind='directory',path='reference'}",
    ] {
        let fixture = OperationFixture::new("Documentation");
        fs::create_dir(fixture.source.path().join("reference")).unwrap();
        fs::write(
            fixture.source.path().join("reference/index.md"),
            "# Reference\n",
        )
        .unwrap();
        fs::write(
            fixture.source.path().join("reference-other.md"),
            "# Other\n",
        )
        .unwrap();
        let reference = format!(
            "navigation=[{{kind='reference',slug='reference',title='API',input={{kind='artifact',source='source:manual',format='markdown',selection={selection}}}}},"
        );
        let configured = fixture.configuration.replace("navigation=[", &reference);
        fs::write(fixture.workspace.path().join("gazette.toml"), configured).unwrap();
        let input = fixture.local();
        let site = "site:one".parse().unwrap();
        operation(&input, &site, None, RetentionSelection::None).execute_with_delivery(
            |execution| {
                let Execution::Complete { report, .. } = execution else {
                    panic!("complete manual and reference assembly")
                };
                let value = serde_json::to_value(&report).unwrap();
                validate_schema(&value);
                assert_eq!(
                    value["unselected_manual_pages"],
                    json!({"source:manual":["reference-other.md"]})
                );
            },
        );
    }
}
