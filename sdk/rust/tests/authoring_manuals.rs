//! Authored manuals share editor analysis while retaining a distinct semantic role.

use gazette_sdk::{
    authoring::{
        SymbolKind,
        session::{Error, ManualSource, Session, Source},
    },
    source::Sha256Digest,
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn source(id: &str, path: &str, text: &str) -> Source {
    Source {
        id: id.into(),
        path: path.parse().expect("logical path"),
        text: text.into(),
    }
}

fn manual_source(source_key: &str, id: &str, path: &str, text: &str) -> ManualSource {
    ManualSource {
        source: source_key.parse().expect("configured source key"),
        input: source(id, path, text),
    }
}

#[test]
fn manual_role_survives_overlays_and_projects_exact_markdown_evidence() {
    let manual = "# Manual 🦀\r\n\r\n[Details](details.md#details)\r\n";
    let details = "# Details\r\n";
    let mut session = Session::new_with_manuals_and_package_baselines(
        source("workspace", "workspace.toml", WORKSPACE),
        [],
        [
            manual_source("source:manual", "details", "manual/details.md", details),
            manual_source("source:manual", "manual", "manual/index.md", manual),
        ],
        [],
        Default::default(),
    )
    .expect("distinct manual session");
    assert!(session.is_manual("manual"));
    assert!(session.is_manual("details"));
    let analysis = session.analysis().as_ref().expect("current analysis");
    assert_eq!(analysis.manuals().count(), 2);
    let symbols = session
        .search_symbols("manual", 1 << 16)
        .expect("manual heading search");
    assert_eq!(symbols.symbols.len(), 1);
    assert_eq!(symbols.symbols[0].kind, SymbolKind::Heading);
    assert_eq!(symbols.symbols[0].document, "manual");
    assert_eq!(
        symbols.symbols[0].source.source_sha256,
        Sha256Digest::of(manual.as_bytes())
    );
    let manual_path = analysis
        .manual_analysis_path(
            &"source:manual".parse().expect("source"),
            &"manual/index.md".parse().expect("manual path"),
        )
        .expect("qualified manual path");
    let target = analysis
        .source_navigation(
            manual_path,
            manual,
            manual.find("[Details]").expect("manual link"),
        )
        .expect("source navigation")
        .expect("selected target");
    assert_eq!(
        target.definition.path,
        analysis
            .manual_analysis_path(
                &"source:manual".parse().expect("source"),
                &"manual/details.md".parse().expect("manual path"),
            )
            .expect("qualified details path")
    );

    let changed = "😀\r\n\r\n# Changed Manual\r\n";
    assert!(
        session
            .open("manual", 7, changed.into())
            .expect("manual overlay")
    );
    assert!(session.is_manual("manual"));
    assert_eq!(
        session
            .search_symbols("changed", 1 << 16)
            .expect("changed manual heading")
            .symbols[0]
            .version,
        Some(7)
    );
}

#[test]
fn manual_role_scopes_equal_capture_paths_by_source_and_rejects_duplicate_bindings() {
    Session::new_with_manuals_and_package_baselines(
        source("workspace", "workspace.toml", WORKSPACE),
        [source("document", "same.md", "# Document")],
        [
            manual_source("source:one", "one", "same.md", "# Manual one"),
            manual_source("source:two", "two", "same.md", "# Manual two"),
        ],
        [],
        Default::default(),
    )
    .expect("configured source identity scopes capture-relative paths");

    assert!(matches!(
        Session::new_with_manuals_and_package_baselines(
            source("workspace", "workspace.toml", WORKSPACE),
            [],
            [
                manual_source("source:one", "one", "same.md", "# Manual one"),
                manual_source("source:one", "two", "same.md", "# Manual two"),
            ],
            [],
            Default::default(),
        ),
        Err(Error::Selection(
            "duplicate manual source/path binding or identifier"
        ))
    ));
}
