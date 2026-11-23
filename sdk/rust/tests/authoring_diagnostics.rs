//! Real selected-set failures retain their native meaning in borrowed SDK views.

use gazette_sdk::{
    authoring::{
        self, Analysis, DiagnosticError, Failure, Input, Limits, diagnostics,
        session::{Session, Source},
    },
    source::{Precision, Sha256Digest, Span},
};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const CEILING: usize = 1024 * 1024;

fn page(id: &str, relationships: &str, body: &str) -> String {
    format!(
        "+++\n{relationships}[content]\nid='{id}'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={{kind='article'}}\n+++\n{body}"
    )
}

fn analyze(workspace: &str, files: &[(&str, &str)], limits: Limits) -> Analysis {
    let input = |path: &str, bytes| Input {
        path: path.parse().expect("valid selected fixture path"),
        bytes,
    };
    authoring::analyze(
        input("workspace.toml", workspace.as_bytes()),
        files
            .iter()
            .map(|(path, text)| input(path, text.as_bytes())),
        limits,
    )
    .expect("complete native analysis, including current failures")
}

#[test]
fn broken_selected_document_link_explains_navigation_failure_at_its_source() {
    let broken = page("content:one", "", "# One\r\n\r\n[Two](two.md#missing)\r\n");
    let target = page("content:two", "", "# Two\r\n");
    let source_path = "one.md".parse().unwrap();
    let target_path = "two.md".parse().unwrap();
    let before = analyze(
        WORKSPACE,
        &[("one.md", &broken), ("two.md", &target)],
        Limits::default(),
    );
    assert!(!before.structurally_valid());
    assert!(matches!(
        before.failures(),
        [Failure::DocumentReferences(_)]
    ));
    let rows = diagnostics(&before, &source_path, &broken, CEILING).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].code, "markdown_link_resolution");
    assert_eq!(
        rows[0].location.unwrap().span.slice(&broken),
        Some("[Two](two.md#missing)")
    );
    assert!(!rows[0].selection_wide);
    assert!(
        diagnostics(&before, &target_path, &target, CEILING)
            .unwrap()
            .is_empty()
    );
    assert!(before.headings(&target_path, &target).is_some());
    assert_eq!(
        before.source_navigation(&source_path, &broken, broken.find("[Two]").unwrap()),
        Err(authoring::NavigationError::Unavailable)
    );

    let repaired = broken.replace("two.md#missing", "two.md#two");
    let after = analyze(
        WORKSPACE,
        &[("one.md", &repaired), ("two.md", &target)],
        Limits::default(),
    );
    assert!(after.structurally_valid());
    assert!(
        diagnostics(&after, &source_path, &repaired, CEILING)
            .unwrap()
            .is_empty()
    );
    let navigation = after
        .source_navigation(&source_path, &repaired, repaired.find("[Two]").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(navigation.definition.path, &target_path);
    assert_eq!(
        navigation.definition.location.span.slice(&target),
        Some("# Two")
    );
}

#[test]
fn conflicting_collection_purpose_explains_the_declaring_child_and_its_parent() {
    let root = page("collection:manual", "", "").replace(
        "kind='page'\nfamily={kind='article'}",
        "kind='collection'\npurpose='documentation'",
    );
    let child = page("collection:chapter", "", "").replace(
        "kind='page'\nfamily={kind='article'}",
        "kind='collection'\nparent='collection:manual'\npurpose='editorial'",
    );
    let analysis = analyze(
        WORKSPACE,
        &[("manual.md", &root), ("chapter.md", &child)],
        Limits::default(),
    );
    let path = "chapter.md".parse().unwrap();
    let rows = diagnostics(&analysis, &path, &child, CEILING).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "collection_purpose_conflict")
        .expect("native containment error");
    assert_eq!(row.source.unwrap().as_str(), "collection:chapter");
    assert_eq!(row.target.unwrap().as_str(), "collection:manual");
    assert!(row.message.contains("containing collection's purpose"));
    assert!(row.location.is_some());
    assert!(!row.selection_wide);
    let inherited = child.replace("purpose='editorial'\n", "");
    let recovered = analyze(
        WORKSPACE,
        &[("manual.md", &root), ("chapter.md", &inherited)],
        Limits::default(),
    );
    assert!(
        diagnostics(&recovered, &path, &inherited, CEILING)
            .unwrap()
            .iter()
            .all(|row| row.code != "collection_purpose_conflict")
    );
}

#[test]
fn incomplete_documentation_outline_names_its_unknown_and_omitted_destinations() {
    let root = page("collection:manual", "", "").replace(
        "kind='page'\nfamily={kind='article'}",
        "kind='collection'\npurpose='documentation'\nnavigation=[{group='Start here',pages=[{content='content:missing'}]}]",
    );
    let leaf = page("content:real", "collections=['collection:manual']\n", "");
    let analysis = analyze(
        WORKSPACE,
        &[("manual.md", &root), ("use.md", &leaf)],
        Limits::default(),
    );
    let path = "manual.md".parse().unwrap();
    let rows = diagnostics(&analysis, &path, &root, CEILING).unwrap();
    for (code, target, message) in [
        (
            "collection_navigation_reference",
            "content:missing",
            "does not name an existing content node",
        ),
        (
            "collection_navigation_missing",
            "content:real",
            "omits this contained destination",
        ),
    ] {
        let row = rows.iter().find(|row| row.code == code).unwrap();
        assert_eq!(row.source.unwrap().as_str(), "collection:manual");
        assert_eq!(row.target.unwrap().as_str(), target);
        assert!(row.message.contains(message));
        assert!(row.location.is_some());
        assert!(!row.selection_wide);
    }
}

#[test]
fn invalid_collection_order_identifies_the_exact_declared_child() {
    for (target, code, message) in [
        (
            "content:missing",
            "collection_order_reference",
            "does not name an existing",
        ),
        (
            "content:outside",
            "collection_order_not_child",
            "belong directly",
        ),
    ] {
        let root = page("collection:manual", "related=['content:outside']\n", "").replace(
            "kind='page'\nfamily={kind='article'}",
            &format!("kind='collection'\nmember_order=['{target}']"),
        );
        let outside = page("content:outside", "", "");
        let analysis = analyze(
            WORKSPACE,
            &[("manual.md", &root), ("outside.md", &outside)],
            Limits::default(),
        );
        let path = "manual.md".parse().unwrap();
        let rows = diagnostics(&analysis, &path, &root, CEILING).unwrap();
        let row = rows.iter().find(|row| row.code == code).unwrap();
        assert!(row.message.contains(message));
        let location = row.location.unwrap();
        assert_eq!(location.precision, Precision::Exact);
        assert_eq!(
            location.span.start,
            root.rfind(&format!("'{target}'")).unwrap()
        );
        assert_eq!(
            location.span.slice(&root),
            Some(format!("'{target}'").as_str())
        );
    }
}

#[test]
fn workspace_semantic_fields_are_borrowed_without_invented_locations() {
    let workspace = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let analysis = analyze(&workspace, &[], Limits::default());
    let [Failure::Workspace(gazette_compiler::workspace::Error::Invalid(native))] =
        analysis.failures()
    else {
        panic!("native semantic failure required")
    };
    let path = "workspace.toml".parse().expect("valid workspace path");
    let rows = diagnostics(&analysis, &path, &workspace, CEILING)
        .expect("project native workspace failures within the diagnostic ceiling");
    assert_eq!(rows.len(), native.len());
    for (row, error) in rows.iter().zip(native) {
        assert_eq!(row.code, error.code);
        assert_eq!(row.field, Some(error.field.as_str()));
        assert_eq!(row.key, error.key.as_ref());
        assert!(std::ptr::eq(
            row.field
                .expect("workspace failure retains its field")
                .as_ptr(),
            error.field.as_ptr()
        ));
        assert!(row.location.is_none());
        assert!(!row.selection_wide);
        assert_eq!(*row.source_sha256, Sha256Digest::of(workspace.as_bytes()));
    }
}

#[test]
fn expanded_child_navigation_never_highlights_a_valid_related_link_on_its_parent() {
    let root = page("collection:manual", "related=['content:outside']\n", "").replace(
        "kind='page'\nfamily={kind='article'}",
        "kind='collection'\npurpose='documentation'\nnavigation=[{content='collection:chapter'}]",
    );
    let child = page("collection:chapter", "", "").replace(
        "kind='page'\nfamily={kind='article'}",
        "kind='collection'\nparent='collection:manual'\nnavigation=[{content='content:outside'}]",
    );
    let outside = page("content:outside", "", "");
    let analysis = analyze(
        WORKSPACE,
        &[
            ("manual.md", &root),
            ("chapter.md", &child),
            ("outside.md", &outside),
        ],
        Limits::default(),
    );
    let root_path = "manual.md".parse().unwrap();
    let rows = diagnostics(&analysis, &root_path, &root, CEILING).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "collection_navigation_outside")
        .expect("expanded child navigation failure on parent");
    assert_eq!(row.target.unwrap().as_str(), "content:outside");
    assert_eq!(
        row.location.unwrap().precision,
        gazette_core::source::Precision::Construct
    );
    let child_path = "chapter.md".parse().unwrap();
    let rows = diagnostics(&analysis, &child_path, &child, CEILING).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "collection_navigation_outside")
        .expect("direct navigation failure on child");
    let location = row.location.unwrap();
    assert_eq!(location.precision, gazette_core::source::Precision::Exact);
    assert_eq!(location.span.slice(&child), Some("'content:outside'"));
}

#[test]
fn retained_only_refusal_keeps_exact_source_identity_without_inventing_a_token_span() {
    let workspace = format!(
        "{WORKSPACE}[[sources]]\nid='source:one'\nlocation={{kind='filesystem',root='/fixture'}}\n\
         [[brands]]\nid='brand:one'\nname='Example'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\n\
         authored=[{{source='source:one',target='authored/one',retained_only=['guide.md']}}]\n"
    );
    let source = page("content:guide", "", "# Guide\n");
    let analysis = analyze(
        &workspace,
        &[("authored/one/guide.md", &source)],
        Limits::default(),
    );
    let [
        Failure::Authored(gazette_compiler::authored::Error::RetainedOnly {
            path: failed,
            reason,
        }),
    ] = analysis.failures()
    else {
        panic!("a Markdown entry cannot acquire the opaque retained-only disposition")
    };
    assert_eq!(failed.as_str(), "authored/one/guide.md");
    for (name, bytes, selection_wide) in [
        ("authored/one/guide.md", source.as_str(), false),
        ("workspace.toml", workspace.as_str(), true),
    ] {
        let path = name.parse().unwrap();
        let rows = diagnostics(&analysis, &path, bytes, CEILING).unwrap();
        let [row] = rows.as_slice() else {
            panic!("one native retention refusal")
        };
        assert_eq!(row.path, &path);
        assert_eq!(row.code, "authored_retained_only");
        assert_eq!(row.message, *reason);
        assert_eq!(*row.source_sha256, Sha256Digest::of(bytes.as_bytes()));
        assert_eq!(row.selection_wide, selection_wide);
        assert!(
            row.location.is_none(),
            "path admission has no Markdown token range"
        );
    }
    let display = "guide.md".parse().unwrap();
    let rows = authoring::diagnostics_for(&analysis, failed, &display, &source, CEILING).unwrap();
    assert_eq!(rows[0].path, &display);
    assert!(
        !rows[0].selection_wide,
        "display-path projection must not change the exact native source join"
    );
}

#[test]
fn selected_home_binding_is_selection_wide_and_repaired_by_exact_workspace_overlay() {
    let workspace = format!(
        "{WORKSPACE}[[brands]]\nid='brand:one'\nname='Publication'\n\
         [[brands.publications]]\nsite='site:one'\n\
         public_base_url='https://one.example/'\nhome='content:missing'\n"
    );
    let source = page("content:home", "", "# Home\n").replace(
        "[content.structure]",
        "route={path='/'}\n[content.structure]",
    );
    let analysis = analyze(&workspace, &[("home.md", &source)], Limits::default());
    let [Failure::Authored(gazette_compiler::authored::Error::PublicationMetadata(native))] =
        analysis.failures()
    else {
        panic!("selected native home binding must fail against the retained graph")
    };
    for (path, bytes) in [
        ("workspace.toml", workspace.as_str()),
        ("home.md", source.as_str()),
    ] {
        let path = path.parse().unwrap();
        let rows = diagnostics(&analysis, &path, bytes, CEILING).unwrap();
        let [row] = rows.as_slice() else {
            panic!("one complete shared metadata diagnostic")
        };
        assert_eq!(row.code, native.code());
        assert_eq!(row.message, native.reason);
        assert_eq!(row.source, Some(&native.site));
        assert_eq!(row.target, Some(&native.id));
        assert!(row.selection_wide);
        assert!(row.location.is_none(), "no fabricated Markdown origin");
        assert_eq!(*row.source_sha256, Sha256Digest::of(bytes.as_bytes()));
    }
    let mut session = Session::new(
        Source {
            id: "workspace".into(),
            path: "workspace.toml".parse().unwrap(),
            text: workspace.clone(),
        },
        [Source {
            id: "home".into(),
            path: "home.md".parse().unwrap(),
            text: source,
        }],
        Default::default(),
    )
    .unwrap();
    assert!(!session.analysis().as_ref().unwrap().structurally_valid());
    let repaired = workspace.replace("home='content:missing'", "home='content:home'");
    assert!(session.open("workspace", 2, repaired).unwrap());
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
    assert!(!session.change("workspace", 1, workspace).unwrap());
    assert!(session.analysis().as_ref().unwrap().structurally_valid());
}

#[test]
fn frontmatter_parser_ranges_are_lifted_once_against_exact_crlf_unicode_source() {
    let source = "+++\r\n# é 😀\r\n[broken\r\n+++\r\n# Body\r\n";
    let analysis = analyze(WORKSPACE, &[("é/bad.md", source)], Limits::default());
    let [
        Failure::Corpus(gazette_compiler::corpus::Error::File {
            error:
                gazette_compiler::corpus::FileError::Document(
                    gazette_compiler::document::Error::Metadata {
                        error:
                            gazette_compiler::toml::Error::Invalid {
                                span: Some(span),
                                message,
                            },
                        offset,
                    },
                ),
            ..
        }),
    ] = analysis.failures()
    else {
        panic!("native located TOML error required")
    };
    let path = "é/bad.md".parse().expect("valid Unicode fixture path");
    let rows = diagnostics(&analysis, &path, source, CEILING)
        .expect("project located frontmatter failure against its exact source");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.code, "toml_invalid");
    assert_eq!(row.message, message);
    let location = row
        .location
        .expect("frontmatter failure retains its checked location");
    assert_eq!(location.precision, Precision::Construct);
    assert_eq!(
        location.span,
        span.in_parent(
            source,
            Span::checked(source, *offset..source.len())
                .expect("native metadata offset starts a valid source range")
        )
        .expect("native metadata span lifts into the full source")
    );
    assert!(location.span.slice(source).is_some());
    assert!(std::ptr::eq(row.message.as_ptr(), message.as_ptr()));
    assert!(
        diagnostics(
            &analysis,
            &"workspace.toml".parse().expect("valid workspace path"),
            WORKSPACE,
            CEILING
        )
        .expect("unaffected workspace has a valid empty diagnostic projection")
        .is_empty()
    );
}

#[test]
fn heading_failure_uses_full_file_coordinates_not_a_fake_exact_token() {
    let source =
        page("content:one", "", "# Héllo {#same}\n# Again {#same}\n").replace('\n', "\r\n");
    let analysis = analyze(WORKSPACE, &[("page.md", &source)], Limits::default());
    let path = "page.md".parse().expect("valid page path");
    let rows = diagnostics(&analysis, &path, &source, CEILING)
        .expect("project duplicate heading failure against current source");
    let row = rows
        .iter()
        .find(|row| row.code == "markdown_anchor")
        .expect("real heading allocator failure");
    let location = row.location.expect("containing heading evidence");
    assert_eq!(location.precision, Precision::Construct);
    assert_eq!(location.span.slice(&source), Some("# Again {#same}"));
    assert!(!row.selection_wide);
}

#[test]
fn pre_theme_renderability_diagnostics_match_effective_footnote_support() {
    let (body, code, reason, occurrence) = (
        "<script>unsafe()</script>\n",
        "markdown_raw_html_unavailable",
        "raw HTML requires an admitted renderer",
        "<script>unsafe()</script>",
    );
    let source = page("content:one", "", body).replace('\n', "\r\n");
    let analysis = analyze(WORKSPACE, &[("render.md", &source)], Limits::default());
    assert!(
        analysis
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Renderability { .. }))
    );
    let path = "render.md".parse().expect("path");
    let rows = diagnostics(&analysis, &path, &source, CEILING).expect("shared SDK projection");
    let row = rows
        .iter()
        .find(|row| row.message == reason)
        .expect("exact native renderability diagnostic");
    assert_eq!(row.code, code);
    let location = row.location.expect("native source occurrence");
    assert_eq!(location.precision, Precision::Construct);
    assert!(
        location
            .span
            .slice(&source)
            .expect("full-file checked range")
            .contains(occurrence),
        "{} did not identify {occurrence:?}",
        location.span.slice(&source).unwrap()
    );
    assert!(!row.selection_wide);

    for body in [
        "note[^outer]\n\n[^outer]: nested[^inner]\n\n[^inner]: target\n",
        "![label[^note]](https://example.invalid/image.png)\n\n[^note]: detail\n",
    ] {
        let source = page("content:one", "", body).replace('\n', "\r\n");
        let analysis = analyze(WORKSPACE, &[("render.md", &source)], Limits::default());
        assert!(
            !analysis
                .failures()
                .iter()
                .any(|failure| matches!(failure, Failure::Renderability { .. })),
            "supported footnote structure must not become a renderability failure"
        );
        assert!(
            diagnostics(
                &analysis,
                &"render.md".parse().expect("path"),
                &source,
                CEILING,
            )
            .expect("shared SDK projection")
            .iter()
            .all(|row| row.code != "markdown_adapter"),
            "supported footnote structure must not retain the old parser diagnostic"
        );
    }
}

#[test]
fn every_duplicate_declaration_stays_bound_to_its_own_named_source() {
    let first = page("content:duplicate", "", "# One\n");
    let second = first.replace("# One", "# Two").replace('\n', "\r\n");
    let analysis = analyze(
        WORKSPACE,
        &[("a.md", &first), ("z.md", &second)],
        Limits::default(),
    );
    for (path, source) in [("a.md", &first), ("z.md", &second)] {
        let selected_path = path.parse().expect("valid duplicate declaration path");
        let rows = diagnostics(&analysis, &selected_path, source, CEILING)
            .expect("project each duplicate declaration at its own source");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.code, "content_duplicate");
        assert_eq!(row.path.as_str(), path);
        assert_eq!(*row.source_sha256, Sha256Digest::of(source.as_bytes()));
        assert_eq!(
            row.source
                .expect("duplicate failure retains its source key")
                .as_str(),
            "content:duplicate"
        );
        let location = row
            .location
            .expect("duplicate declaration has a checked origin");
        assert!(
            location
                .span
                .slice(source)
                .expect("declaration origin slices its exact source")
                .starts_with("[content]")
        );
        assert_eq!(location.precision, Precision::Construct);
        let encoded = serde_json::to_value(row).expect("borrowed diagnostic serializes");
        assert_eq!(encoded["path"], path);
        assert_eq!(encoded["source"], "content:duplicate");
        assert_eq!(encoded["location"]["precision"], "Construct");
    }
}

#[test]
fn missing_cross_document_endpoint_keeps_native_source_and_target_keys() {
    let source = page(
        "content:one",
        "collections=['collection:missing']\n",
        "# One\n",
    );
    let analysis = analyze(WORKSPACE, &[("page.md", &source)], Limits::default());
    let path = "page.md".parse().expect("valid page path");
    let rows = diagnostics(&analysis, &path, &source, CEILING)
        .expect("project missing relationship endpoint against current source");
    let row = rows
        .iter()
        .find(|row| {
            row.target
                .is_some_and(|target| target.as_str() == "collection:missing")
        })
        .expect("native missing endpoint");
    assert_eq!(
        row.source
            .expect("endpoint failure retains its source key")
            .as_str(),
        "content:one"
    );
    let location = row
        .location
        .expect("the missing reference has an exact scalar");
    assert_eq!(location.precision, Precision::Exact);
    assert_eq!(location.span.slice(&source), Some("'collection:missing'"));
    assert!(!row.selection_wide);
}

#[test]
fn reference_locations_decode_escapes_without_selecting_a_matching_title() {
    let source = page(
        "content:one",
        "collections=[\"collection:miss\\u0069ng\"]\n",
        "# One\n",
    )
    .replace("title='Guide'", "title='collection:missing'");
    let analysis = analyze(WORKSPACE, &[("page.md", &source)], Limits::default());
    let path = "page.md".parse().unwrap();
    let rows = diagnostics(&analysis, &path, &source, CEILING).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "content_reference")
        .unwrap();
    let location = row.location.unwrap();
    assert_eq!(location.precision, Precision::Exact);
    assert_eq!(
        location.span.slice(&source),
        Some("\"collection:miss\\u0069ng\"")
    );
    assert!(row.explanation(&source).contains("collection:missing"));
}

#[test]
fn ambiguous_reference_occurrences_keep_the_declaration_location() {
    let source = page(
        "content:one",
        "collections=['collection:missing']\nrelated=['collection:missing']\n",
        "# One\n",
    );
    let analysis = analyze(WORKSPACE, &[("page.md", &source)], Limits::default());
    let path = "page.md".parse().unwrap();
    let rows = diagnostics(&analysis, &path, &source, CEILING).unwrap();
    let row = rows
        .iter()
        .find(|row| row.code == "content_reference")
        .unwrap();
    assert_eq!(row.location.unwrap().precision, Precision::Construct);
}

#[test]
fn selection_wide_resource_failure_is_explicit_and_has_no_location() {
    let source = page("content:one", "", "# One\n");
    let mut limits = Limits::default();
    limits.content.nodes = 0;
    let analysis = analyze(WORKSPACE, &[("page.md", &source)], limits);
    let path = "page.md".parse().expect("valid page path");
    let rows = diagnostics(&analysis, &path, &source, CEILING)
        .expect("project selection-wide resource failure without a fabricated location");
    let row = rows
        .iter()
        .find(|row| row.code == "content_limit")
        .expect("native complete-set limit");
    assert!(row.selection_wide);
    assert!(row.location.is_none());
}

#[test]
fn changed_or_unknown_sources_and_exhausted_projection_return_no_partial_rows() {
    let source = page(
        "content:one",
        "collections=['collection:missing']\n",
        "# One\n",
    );
    let analysis = analyze(WORKSPACE, &[("page.md", &source)], Limits::default());
    let path = "page.md".parse().expect("valid selected page path");
    assert_eq!(
        diagnostics(&analysis, &path, "changed", CEILING)
            .expect_err("changed source must not yield diagnostics"),
        DiagnosticError::InvalidSource
    );
    assert_eq!(
        diagnostics(
            &analysis,
            &"missing.md".parse().expect("valid unselected path"),
            &source,
            CEILING
        )
        .expect_err("unselected source must not yield diagnostics"),
        DiagnosticError::InvalidSource
    );
    assert_eq!(
        diagnostics(&analysis, &path, &source, 0)
            .expect_err("exhausted projection ceiling must refuse partial rows"),
        DiagnosticError::Limit
    );
    let rows = diagnostics(&analysis, &path, &source, CEILING)
        .expect("unchanged selected source projects within the ceiling");
    assert!(!rows.is_empty());
}

#[test]
fn modular_workspace_syntax_diagnostics_belong_to_the_failed_document() {
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
    let site = "[[sites]\n";
    let session = Session::new_with_workspace_documents(
        Source {
            id: "root".into(),
            path: "gazette.toml".parse().expect("root path"),
            text: root.into(),
        },
        [Source {
            id: "site".into(),
            path: "sites/one.toml".parse().expect("site path"),
            text: site.into(),
        }],
        [],
        [],
        Default::default(),
    )
    .expect("selected modular source set");
    let analysis = session
        .analysis()
        .as_ref()
        .expect("current failure analysis");
    let path = "sites/one.toml".parse().expect("site path");
    let rows = diagnostics(analysis, &path, site, CEILING).expect("document-local diagnostic");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path.as_str(), "sites/one.toml");
    assert_eq!(rows[0].code, "toml_invalid");
    assert!(rows[0].location.is_some());
    assert!(!rows[0].selection_wide);
    assert!(
        diagnostics(
            analysis,
            &"gazette.toml".parse().expect("root path"),
            root,
            CEILING,
        )
        .expect("root diagnostics")
        .is_empty()
    );
}

#[test]
fn artwork_refusal_borrows_original_toml_yaml_variant_and_source_digest() {
    let workspace = format!(
        "{WORKSPACE}[[sources]]\nid='source:one'\nlocation={{kind='filesystem',root='/not-read'}}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/'\nauthored=[{{source='source:one',target='one',static_assets={{route_prefix='/assets/'}}}}]\n"
    );
    let toml = page(
        "content:guide",
        "# é 😀\n[listing_artwork]\ndefault={source='source:absent',entry='media/image.webp'}\n",
        "# Body\n",
    )
    .replace('\n', "\r\n");
    let yaml="format: gazette-landing/v1\nlisting_artwork:\n  default: {source: 'source:absent', entry: media/image.webp}\ncontent:\n  id: content:guide\n  site: site:one\n  title: Guide\n  lifecycle: draft\n  structure: {kind: collection}\nsections:\n- body: A retained account.\n".to_owned();
    for (name, source) in [("one/guide.md", toml), ("one/guide.landing.yaml", yaml)] {
        let analysis = analyze(&workspace, &[(name, &source)], Default::default());
        let [Failure::Authored(gazette_compiler::authored::Error::Artwork(native))] =
            analysis.failures()
        else {
            panic!("one native variant refusal")
        };
        let path = name.parse().unwrap();
        let rows = diagnostics(&analysis, &path, &source, CEILING).unwrap();
        let [row] = rows.as_slice() else {
            panic!("one original-source diagnostic")
        };
        assert_eq!(row.code, native.code());
        assert_eq!(row.source, Some(&native.content));
        assert_eq!(*row.source_sha256, Sha256Digest::of(source.as_bytes()));
        let location = row.location.unwrap();
        assert!(
            location
                .span
                .slice(&source)
                .unwrap()
                .contains("source:absent")
        );
        assert!(!row.selection_wide);
        assert_eq!(native.variant, "default");
    }
}
