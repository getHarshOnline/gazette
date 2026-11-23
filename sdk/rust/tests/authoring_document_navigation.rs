//! Exact selected-source links through the real application and overlay session.

use gazette_sdk::{
    authoring::{
        self, Analysis, Input, Limits, NavigationError,
        session::{Session, Source},
    },
    source::{EntryPath, Precision, Sha256Digest},
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn analyze(files: &[(&str, &str)], limits: Limits) -> Result<Analysis, authoring::Error> {
    authoring::analyze(
        Input {
            path: "workspace.toml".parse().expect("workspace path"),
            bytes: WORKSPACE.as_bytes(),
        },
        files.iter().map(|(path, text)| Input {
            path: path.parse().expect("selected path"),
            bytes: text.as_bytes(),
        }),
        limits,
    )
}

fn framed(id: &str, body: &str) -> String {
    format!(
        "+++\r\n[content]\r\nid='{id}'\r\nsite='site:one'\r\ntitle='Page'\r\nlifecycle='draft'\r\n[content.structure]\r\nkind='page'\r\nfamily={{kind='article'}}\r\n+++\r\n{body}"
    )
}

#[test]
fn selected_document_links_use_native_anchors_and_complete_source_bound_reverse_sets() {
    let target_body =
        "# Héllo 😀 {#\"é\"}\r\n\r\n[local](#%C3%A9)\r\n\r\n# Repeat\r\n\r\n# Repeat\r\n";
    let target = framed("content:target", target_body);
    let start = framed(
        "content:start",
        "😀 [target](../target%20name.md#%C3%A9)\r\n\r\n[duplicate](../target%20name.md#repeat-1)\r\n\r\n[document](../target%20name.md)\r\n",
    );
    let sibling = "[another](target%20name.md?view=1#é)\n";
    let analysis = analyze(
        &[
            ("docs/nested/start.md", &start),
            ("docs/target name.md", &target),
            ("docs/sibling.md", sibling),
        ],
        Default::default(),
    )
    .expect("selected analysis");
    let source_path: EntryPath = "docs/nested/start.md".parse().expect("source path");
    let target_path: EntryPath = "docs/target name.md".parse().expect("target path");
    let nav = analysis
        .source_navigation(
            &source_path,
            &start,
            start.find("[target]").expect("cross link"),
        )
        .expect("current query")
        .expect("selected target");
    assert_eq!(nav.path, &source_path);
    assert_eq!(nav.source_sha256, Sha256Digest::of(start.as_bytes()));
    assert_eq!(nav.definition.path, &target_path);
    assert_eq!(
        nav.definition.source_sha256,
        Sha256Digest::of(target.as_bytes())
    );
    assert_eq!(nav.definition.location.precision, Precision::Construct);
    assert_eq!(
        nav.definition.location.span.slice(&target),
        Some("# Héllo 😀 {#\"é\"}")
    );
    let expected = [
        (
            "docs/nested/start.md",
            Sha256Digest::of(start.as_bytes()),
            "[target](../target%20name.md#%C3%A9)",
        ),
        (
            "docs/sibling.md",
            Sha256Digest::of(sibling.as_bytes()),
            "[another](target%20name.md?view=1#é)",
        ),
        (
            "docs/target name.md",
            Sha256Digest::of(target.as_bytes()),
            "[local](#%C3%A9)",
        ),
    ];
    assert_eq!(nav.references.len(), expected.len());
    for (value, (path, digest, spelling)) in nav.references.iter().zip(expected) {
        assert_eq!(value.path.as_str(), path);
        assert_eq!(value.source_sha256, digest);
        let source = match path {
            "docs/nested/start.md" => &start,
            "docs/sibling.md" => sibling,
            _ => &target,
        };
        assert_eq!(value.location.span.slice(source), Some(spelling));
    }
    let reverse = analysis
        .source_navigation(&target_path, &target, nav.definition.location.span.start)
        .expect("target declaration query")
        .expect("complete reverse association");
    assert_eq!(reverse.definition, nav.definition);
    assert_eq!(reverse.references, nav.references);
    let duplicate = analysis
        .source_navigation(
            &source_path,
            &start,
            start.find("[duplicate]").expect("duplicate link"),
        )
        .expect("query")
        .expect("allocated duplicate anchor");
    assert_eq!(
        duplicate.definition.location.span.start,
        target.rfind("# Repeat").expect("second heading")
    );
    let whole = analysis
        .source_navigation(
            &source_path,
            &start,
            start.find("[document]").expect("document link"),
        )
        .expect("query")
        .expect("whole selected body");
    assert_eq!(whole.definition.path, &target_path);
    assert_eq!(
        whole.definition.location.span.slice(&target),
        Some(target_body)
    );
    assert_eq!(whole.definition.location.precision, Precision::Construct);
    assert_eq!(whole.references.len(), 1);
}

#[test]
fn images_external_roots_unselected_and_nonheading_anchors_do_not_infer_targets() {
    let source = "![image](target.md#target) [external](https://example.invalid/target.md#target) [root](/target.md#target) [unselected](other.md#target) [note](target.md#fn-note-6e)\n";
    let target = "# Target\n\nNote[^n].\n\n[^n]: note\n";
    let analysis = analyze(
        &[("source.md", source), ("target.md", target)],
        Default::default(),
    )
    .expect("selected analysis");
    let path = "source.md".parse().expect("path");
    for spelling in ["![image]", "[external]", "[root]", "[unselected]", "[note]"] {
        assert!(
            analysis
                .source_navigation(&path, source, source.find(spelling).expect("excluded link"))
                .expect("valid excluded occurrence")
                .is_none(),
            "must not infer {spelling}"
        );
    }
}

#[test]
fn target_overlays_rebind_unchanged_links_and_close_restores_selected_baseline() {
    let source = "[go](target.md#target)\n";
    let target = "# Target\n";
    let mut session = Session::new(
        Source {
            id: "workspace".into(),
            path: "workspace.toml".parse().expect("path"),
            text: WORKSPACE.into(),
        },
        [
            Source {
                id: "source".into(),
                path: "source.md".parse().expect("path"),
                text: source.into(),
            },
            Source {
                id: "target".into(),
                path: "target.md".parse().expect("path"),
                text: target.into(),
            },
        ],
        Default::default(),
    )
    .expect("explicit selected session");
    let path = "source.md".parse().expect("path");
    let baseline = session
        .analysis()
        .as_ref()
        .expect("analysis")
        .source_navigation(&path, source, 0)
        .expect("query")
        .expect("target")
        .definition
        .location;
    let changed = "😀 intro\n\n# Target\n";
    assert!(
        session
            .open("target", 1, changed.into())
            .expect("target overlay")
    );
    let nav = session
        .analysis()
        .as_ref()
        .expect("current analysis")
        .source_navigation(&path, source, 0)
        .expect("unchanged source query")
        .expect("changed target");
    assert_eq!(
        nav.definition.source_sha256,
        Sha256Digest::of(changed.as_bytes())
    );
    assert_eq!(
        nav.definition.location.span.start,
        changed.find("# Target").expect("new heading location")
    );
    assert_ne!(nav.definition.location, baseline);
    assert!(
        session
            .change("target", 2, "# Removed\n".into())
            .expect("remove target anchor")
    );
    assert_eq!(
        session
            .analysis()
            .as_ref()
            .expect("current evidence")
            .source_navigation(&path, source, 0)
            .expect_err("no old target after edit"),
        NavigationError::Unavailable
    );
    assert!(session.close("target").expect("restore selected baseline"));
    let restored = session
        .analysis()
        .as_ref()
        .expect("restored analysis")
        .source_navigation(&path, source, 0)
        .expect("query")
        .expect("restored target");
    assert_eq!(restored.definition.location, baseline);
    assert_eq!(
        restored.definition.source_sha256,
        Sha256Digest::of(target.as_bytes())
    );
    assert_eq!(
        session
            .analysis()
            .as_ref()
            .expect("analysis")
            .source_navigation(&path, "[changed](target.md#target)\n", 0)
            .expect_err("stale queried source refused"),
        NavigationError::InvalidSource
    );
}

#[test]
fn invalid_selected_target_prevents_partial_references_but_preserves_footnotes() {
    let target = "# Target\n";
    for link in [
        "target.md#missing",
        "target.md#bad%ZZ",
        "target.md#%ff",
        "../target.md#target",
    ] {
        let source =
            format!("[good](target.md#target) [bad]({link}) Note[^n].\n\n[^n]: independent note\n");
        let analysis = analyze(
            &[("source.md", &source), ("target.md", target)],
            Default::default(),
        )
        .expect("retain failed associations");
        let path = "source.md".parse().expect("path");
        assert_eq!(
            analysis
                .source_navigation(&path, &source, 0)
                .expect_err("no partial cross-document set"),
            NavigationError::Unavailable
        );
        let note = analysis
            .source_navigation(&path, &source, source.find("[^n]").expect("footnote"))
            .expect("independent source evidence")
            .expect("footnote target");
        assert_eq!(note.definition.path, &path);
        assert!(
            note.definition
                .location
                .span
                .slice(&source)
                .expect("note range")
                .contains("independent note")
        );
    }
}

#[test]
fn complete_selected_reference_budget_refuses_without_partial_analysis() {
    let source = "[go](target.md#target)\n".repeat(40);
    let target = "# Target\n";
    let limits = Limits {
        navigation_occurrences: 32,
        ..Default::default()
    };
    assert!(
        analyze(
            &[("source.md", "No links\n"), ("target.md", target)],
            limits
        )
        .is_ok()
    );
    assert!(matches!(
        analyze(&[("source.md", &source), ("target.md", target)], limits),
        Err(authoring::Error::Limit(_))
    ));
    let complete = analyze(
        &[("source.md", &source), ("target.md", target)],
        Default::default(),
    )
    .expect("sufficient complete budget");
    let nav = complete
        .source_navigation(&"source.md".parse().expect("path"), &source, 0)
        .expect("query")
        .expect("target");
    assert_eq!(nav.references.len(), 40);
}
