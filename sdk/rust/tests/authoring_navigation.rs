//! Actual native Markdown associations through the public authoring operation.

use gazette_sdk::{
    authoring::{self, Analysis, Input, Limits, NavigationError},
    source::{EntryPath, Precision, Sha256Digest},
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn analyze(files: &[(&str, &str)], limits: Limits) -> Result<Analysis, authoring::Error> {
    authoring::analyze(
        Input {
            path: "workspace.toml".parse().expect("workspace fixture path"),
            bytes: WORKSPACE.as_bytes(),
        },
        files.iter().map(|(path, text)| Input {
            path: path.parse().expect("selected fixture path"),
            bytes: text.as_bytes(),
        }),
        limits,
    )
}

fn framed(body: &str) -> String {
    format!(
        "+++\r\n[content]\r\nid='content:one'\r\nsite='site:one'\r\ntitle='One'\r\nlifecycle='draft'\r\n[content.structure]\r\nkind='page'\r\nfamily={{kind='article'}}\r\n+++\r\n{body}"
    )
}

#[test]
fn native_definition_and_all_references_share_exact_framed_source_locations() {
    let body = "😀 First[^é] and second[^é].\r\n\r\n[^é]: *actual note*\r\n";
    let page = framed(body);
    let analysis = analyze(&[("page.md", &page)], Limits::default()).expect("framed page analysis");
    assert!(analysis.structurally_valid());
    let path: EntryPath = "page.md".parse().expect("page fixture path");
    let navigation = analysis
        .footnote_navigation(
            &path,
            &page,
            page.find("[^é]").expect("Unicode reference") + 2,
        )
        .expect("current-source navigation")
        .expect("source reference resolves");
    assert_eq!(navigation.path, &path);
    assert_eq!(navigation.source_sha256, Sha256Digest::of(page.as_bytes()));
    assert_eq!(navigation.definition.path, &path);
    assert_eq!(
        navigation.definition.source_sha256,
        navigation.source_sha256
    );
    assert!(
        navigation
            .references
            .iter()
            .all(|value| value.path == &path && value.source_sha256 == navigation.source_sha256)
    );
    assert_eq!(navigation.occurrence.span.slice(&page), Some("[^é]"));
    assert_eq!(
        navigation.definition.location.precision,
        Precision::Construct
    );
    assert!(
        navigation
            .definition
            .location
            .span
            .slice(&page)
            .expect("definition inside exact source")
            .contains("actual note")
    );
    assert_eq!(navigation.references.len(), 2);
    assert!(
        navigation
            .references
            .iter()
            .all(|location| location.location.span.slice(&page) == Some("[^é]"))
    );
    assert!(
        navigation.references[0].location.span.start < navigation.references[1].location.span.start
    );
    let from_definition = analysis
        .footnote_navigation(&path, &page, navigation.definition.location.span.start)
        .expect("definition query")
        .expect("native declaration association");
    assert_eq!(from_definition.references, navigation.references);
    assert_eq!(from_definition.definition, navigation.definition);

    // Native source facts and full-file authoring projection have separate coordinates.
    let parsed = gazette_sdk::markdown::parse(body, Default::default()).expect("native body parse");
    let inspected = parsed.inspect().expect("bounded native inspection");
    assert_eq!(parsed.footnote_definitions(), inspected.footnotes);
    assert_eq!(parsed.footnote_references(), inspected.footnote_references);
    let body_start = page.len() - body.len();
    assert_eq!(
        navigation.definition.location.span.start,
        body_start + inspected.footnotes[0].location.span.start
    );
    assert_eq!(
        navigation.references[1].location.span.start,
        body_start + inspected.footnote_references[1].location.span.start
    );
}

#[test]
fn equal_labels_do_not_join_documents_and_code_is_not_a_reference() {
    let one = "One[^n].\n\n[^n]: FIRST owner\n\n```md\n[^n]\n```\n";
    let two = "Two[^n].\n\n[^n]: SECOND owner\n";
    let analysis = analyze(&[("one.md", one), ("two.md", two)], Limits::default())
        .expect("two selected sources");
    // Unframed content requires publication binding, but native source navigation remains useful.
    assert!(!analysis.structurally_valid());
    for (path, source, expected) in [("one.md", one, "FIRST"), ("two.md", two, "SECOND")] {
        let path: EntryPath = path.parse().expect("selected document path");
        let nav = analysis
            .footnote_navigation(
                &path,
                source,
                source.find("[^n]").expect("document-local reference"),
            )
            .expect("document-local navigation")
            .expect("native association exists");
        assert_eq!(nav.references.len(), 1);
        assert!(
            nav.definition
                .location
                .span
                .slice(source)
                .expect("definition within its own source")
                .contains(expected)
        );
    }
    assert!(
        analysis
            .footnote_navigation(
                &"one.md".parse().expect("first document path"),
                one,
                one.rfind("[^n]").expect("fenced code spelling")
            )
            .expect("valid source position in code")
            .is_none()
    );
}

#[test]
fn queries_refuse_wrong_preimages_positions_and_failed_current_corpus() {
    let page = "😀 Body[^n].\n\n[^n]: detail\n";
    let path: EntryPath = "page.md".parse().expect("page fixture path");
    let good = analyze(&[("page.md", page)], Limits::default()).expect("current analysis");
    for (path, text, offset) in [
        (path.clone(), page.to_owned(), 1),
        (path.clone(), page.to_owned(), page.len() + 1),
        (path.clone(), format!("{page}\n"), 0),
        (
            "unknown.md".parse().expect("unselected path"),
            page.to_owned(),
            0,
        ),
    ] {
        assert_eq!(
            good.footnote_navigation(&path, &text, offset)
                .expect_err("invalid binding or position must refuse"),
            NavigationError::InvalidSource
        );
    }
    assert!(
        good.footnote_navigation(&path, page, page.len())
            .expect("EOF is a valid source boundary")
            .is_none()
    );
    let failed = analyze(
        &[("page.md", page), ("broken.md", "+++\n[broken\n+++\n")],
        Limits::default(),
    )
    .expect("failed corpus retains explicit analysis evidence");
    assert_eq!(
        failed
            .footnote_navigation(&path, page, page.find("[^n]").expect("source reference"))
            .expect_err("failed current corpus cannot return old navigation"),
        NavigationError::Unavailable
    );
    let changed = page.replace("[^n]", "[^changed]");
    let current =
        analyze(&[("page.md", &changed)], Limits::default()).expect("replacement analysis");
    assert_eq!(
        current
            .footnote_navigation(&path, page, 0)
            .expect_err("old source is not the replacement preimage"),
        NavigationError::InvalidSource
    );
    assert!(
        current
            .footnote_navigation(
                &path,
                &changed,
                changed.find("[^changed]").expect("replacement reference")
            )
            .expect("replacement source query")
            .is_some()
    );
}

#[test]
fn complete_occurrence_and_label_budgets_fail_without_partial_analysis() {
    let page = "One[^é] and two[^é].\n\n[^é]: note\n";
    let workspace = gazette_sdk::workspace::ParsedWorkspace::parse(WORKSPACE, Default::default())
        .expect("native workspace navigation baseline");
    let workspace_path = "workspace.toml".parse().expect("workspace path");
    let targets: Vec<_> = workspace
        .navigation()
        .symbols()
        .iter()
        .filter_map(|symbol| {
            gazette_sdk::authored::AuthoredReferenceDeclaration::workspace(
                symbol,
                &workspace_path,
                WORKSPACE,
            )
            .expect("source-bound native completion declaration")
        })
        .collect();
    let occurrences = workspace.navigation().occurrence_count() + targets.len();
    let bytes = workspace.navigation().key_bytes()
        + targets
            .iter()
            .map(|target| target.fact_bytes())
            .sum::<usize>();
    let native = gazette_sdk::markdown::parse(page, Default::default()).expect("native page facts");
    let page_occurrences = native.footnote_definitions().len() + native.footnote_references().len();
    let page_bytes = native
        .footnote_definitions()
        .iter()
        .map(|note| note.name.len())
        .chain(
            native
                .footnote_references()
                .iter()
                .map(|note| note.name.len() + note.definition.as_deref().map_or(0, str::len)),
        )
        .sum::<usize>();
    for limits in [
        Limits {
            navigation_occurrences: occurrences + page_occurrences - 1,
            ..Default::default()
        },
        Limits {
            navigation_bytes: bytes + page_bytes - 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            analyze(&[("page.md", page)], limits),
            Err(authoring::Error::Limit(_))
        ));
    }
    assert!(
        analyze(
            &[("page.md", page)],
            Limits {
                navigation_occurrences: occurrences + page_occurrences,
                navigation_bytes: bytes + page_bytes,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        analyze(
            &[("one.md", page), ("two.md", page)],
            Limits {
                navigation_occurrences: occurrences + page_occurrences * 2 - 1,
                ..Default::default()
            }
        ),
        Err(authoring::Error::Limit(_))
    ));
}

#[test]
fn nested_references_choose_the_inner_association_and_unused_definitions_remain_source_facts() {
    let source = "Main[^a].\n\n[^a]: Contains[^b].\n\n[^b]: Inner note\n\n[^unused]: Unused note\n";
    let analysis =
        analyze(&[("page.md", source)], Limits::default()).expect("nested native source analysis");
    let path: EntryPath = "page.md".parse().expect("page fixture path");
    let inner = analysis
        .footnote_navigation(
            &path,
            source,
            source.find("[^b]").expect("nested reference"),
        )
        .expect("nested source query")
        .expect("inner native association");
    assert_eq!(inner.occurrence.span.slice(source), Some("[^b]"));
    assert!(
        inner
            .definition
            .location
            .span
            .slice(source)
            .expect("inner definition within source")
            .contains("Inner note")
    );
    assert_eq!(inner.references.len(), 1);
    let unused = analysis
        .footnote_navigation(
            &path,
            source,
            source.find("[^unused]").expect("unused declaration"),
        )
        .expect("unused declaration query")
        .expect("unused native definition remains visible");
    assert!(
        unused
            .definition
            .location
            .span
            .slice(source)
            .expect("unused definition within source")
            .contains("Unused note")
    );
    assert!(unused.references.is_empty());
}

#[test]
fn missing_associations_and_duplicate_definitions_never_choose_arbitrary_targets() {
    let missing = "There is no declaration[^absent].\n";
    let analysis =
        analyze(&[("missing.md", missing)], Limits::default()).expect("undefined source analysis");
    assert!(
        analysis
            .footnote_navigation(
                &"missing.md".parse().expect("undefined source path"),
                missing,
                missing.find("[^absent]").expect("undefined spelling")
            )
            .expect("ordinary source query")
            .is_none()
    );
    let duplicate = "Ref[^n].\n\n[^n]: first\n\n[^n]: second\n";
    let parsed = gazette_sdk::markdown::parse(duplicate, Default::default())
        .expect("duplicate native source parse");
    assert_eq!(
        parsed.footnote_definitions().len(),
        2,
        "actual duplicate native definitions"
    );
    let analysis = analyze(&[("duplicate.md", duplicate)], Limits::default())
        .expect("duplicate association analysis");
    assert_eq!(
        analysis
            .footnote_navigation(
                &"duplicate.md".parse().expect("duplicate document path"),
                duplicate,
                duplicate.find("[^n]").expect("ambiguous reference")
            )
            .expect_err("duplicate native definitions must remain ambiguous"),
        NavigationError::Ambiguous
    );
}
