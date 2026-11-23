//! Manual source links and the rendered manual share anchor names and source locations.

use super::*;

fn workspace() -> String {
    format!(
        "{WORKSPACE}{}",
        r##"
[[projects]]
id='project:manual'
lifecycle='active'
visibility='public'
availability='unavailable'
[[sources]]
id='source:manual'
location={kind='filesystem',root='/never-read/manual'}
[[brands]]
id='brand:manual'
name='Manual'
[[brands.publications]]
site='site:one'
documentation_repository={name='manual',path='/never-written/manual',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:manual',slug='manual',title='Manual',manual={source='source:manual'},navigation=[{kind='manual',title='Manual',pages=[{title='Start',route='manual/start',input={kind='file',path='start.md'}},{title='Details',route='manual/details',input={kind='file',path='details.md'}}]}]}]}}
"##
    )
}

fn manuals(start: &str, details: &str) -> Analysis {
    authoring::analyze_with_manuals_and_package_baselines(
        input("workspace.toml", workspace().as_bytes()),
        authoring::SelectedCorpus {
            asset_metadata: Default::default(),
            documents: vec![],
            manuals: vec![
                manual_baseline("source:manual", "start.md", start.as_bytes()),
                manual_baseline("source:manual", "details.md", details.as_bytes()),
            ],
            page_inputs: vec![],
        },
        [],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("bounded manual analysis")
}

fn path(analysis: &Analysis, name: &str) -> EntryPath {
    analysis
        .manual_analysis_path(&"source:manual".parse().unwrap(), &name.parse().unwrap())
        .unwrap()
        .clone()
}

#[test]
fn named_html_and_colliding_heading_links_agree_with_manual_projection() {
    let start = "# Start 🦀\r\n\r\n[Alias](details.md#ready) [Name](details.md#caf%C3%A9) [Heading](details.md#ready-1) [Other](details.md#other)\r\n";
    let details = "<a id=\"ready\"></a>\r\n\r\n<a name=\"café\"></a>\r\n\r\n# Ready\r\n\r\n## Other\r\n\r\n[Alias](#ready) [Name](#caf%C3%A9) [Local heading](#ready-1) [Other heading](#other)\r\n";
    let analysis = manuals(start, details);
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    analysis
        .documentation_authoring(&gazette_adapter_markdown::Comrak)
        .expect("the same fragments close in the assembled manual");
    let start_path = path(&analysis, "start.md");
    let details_path = path(&analysis, "details.md");
    let headings = analysis.headings(&details_path, details).unwrap();
    assert_eq!(
        headings
            .values
            .iter()
            .map(|heading| heading.id.as_str())
            .collect::<Vec<_>>(),
        ["ready-1", "other"]
    );
    for (source_path, source, label, expected) in [
        (&start_path, start, "[Heading]", "# Ready"),
        (&start_path, start, "[Other]", "## Other"),
        (&details_path, details, "[Local heading]", "# Ready"),
        (&details_path, details, "[Other heading]", "## Other"),
    ] {
        let navigation = analysis
            .source_navigation(source_path, source, source.find(label).unwrap())
            .expect("complete reference set")
            .expect("heading definition");
        assert_eq!(navigation.definition.path, &details_path);
        assert_eq!(
            navigation.definition.source_sha256,
            Sha256Digest::of(details.as_bytes())
        );
        assert_eq!(
            navigation.definition.location.span.slice(details),
            Some(expected)
        );
        assert_eq!(
            navigation.definition.location.precision,
            Precision::Construct
        );
    }
    for (source_path, source) in [(&start_path, start), (&details_path, details)] {
        for label in ["[Alias]", "[Name]"] {
            assert!(
                analysis
                    .source_navigation(source_path, source, source.find(label).unwrap())
                    .expect("valid named anchor leaves navigation available")
                    .is_none(),
                "a named HTML anchor is not a heading definition"
            );
        }
        assert!(analysis.source_links(source_path, source, 8192).is_ok());
    }
    assert_eq!(
        analysis.manual_baseline(
            &"source:manual".parse().unwrap(),
            &"details.md".parse().unwrap()
        ),
        Some(details.as_bytes())
    );
}

#[test]
fn missing_or_malformed_manual_fragments_keep_complete_failure_results() {
    for fragment in ["", "missing", "bad%ZZ", "%FF"] {
        for local in [false, true] {
            let target = if local { "" } else { "details.md" };
            let start =
                format!("# Start\n\n[Broken]({target}#{fragment}) [Good](details.md#details)\n");
            let details = "# Details\n\n<a id=\"alias\"></a>\n";
            let analysis = manuals(&start, details);
            assert!(!analysis.structurally_valid(), "{local}: {fragment}");
            let start_path = path(&analysis, "start.md");
            if local {
                assert!(analysis.failures().iter().any(|failure| matches!(failure,
                    Failure::Renderability { path, body_span, error: gazette_compiler::markdown::Error::LinkResolution { span, .. } }
                    if path == &start_path && span.in_parent(&start, *body_span).and_then(|span| span.slice(&start)).is_some_and(|text| text.starts_with("[Broken]"))
                )), "local failure retains its exact link construct: {:?}", analysis.failures());
            } else {
                assert!(
                    analysis.source_links(&start_path, &start, 8192).is_err(),
                    "no successful cross-document subset"
                );
                assert!(
                    analysis
                        .failures()
                        .iter()
                        .any(|failure| matches!(failure, Failure::DocumentReferences(_)))
                );
            }
        }
    }
}

#[test]
fn manual_anchor_admission_rejects_bad_declarations_at_their_source() {
    for (details, rejected) in [
        (
            "<a id=\"bad name\"></a>\n\n# Details\n",
            "<a id=\"bad name\">",
        ),
        (
            "<script>alert(1)</script>\n\n# Details\n",
            "<script>alert(1)</script>",
        ),
        (
            "<a id=\"same\"></a>\n\n# Details {#same}\n",
            "# Details {#same}",
        ),
        (
            "<a id=\"same\"></a>\n\n<a name=\"same\"></a>\n\n# Details\n",
            "<a name=\"same\">",
        ),
    ] {
        let analysis = manuals("# Start\n\n[Target](details.md#details)\n", details);
        assert!(!analysis.structurally_valid(), "{details}");
        let target = path(&analysis, "details.md");
        assert!(analysis.failures().iter().any(|failure| matches!(failure,
            Failure::Headings { path, body_span, error: gazette_compiler::markdown::Error::Anchor { span, .. } }
            if path == &target && span.in_parent(details, *body_span).and_then(|span| span.slice(details)).is_some_and(|text| text.trim() == rejected)
        )), "bad declaration keeps its containing construct: {:?}", analysis.failures());
        assert!(
            analysis
                .documentation_authoring(&gazette_adapter_markdown::Comrak)
                .is_err()
        );
    }
}

#[test]
fn the_same_html_in_an_authored_publication_remains_refused() {
    let body = "# Article\n\n<a id=\"alias\"></a>\n\n[Alias](#alias)\n";
    let analysis = analyze(
        WORKSPACE,
        &[("article.md", body.as_bytes())],
        Limits::default(),
    )
    .unwrap();
    assert!(!analysis.structurally_valid());
    assert!(analysis.failures().iter().any(|failure| matches!(
        failure,
        Failure::Renderability {
            error: gazette_compiler::markdown::Error::RawHtml(_),
            ..
        }
    )));
}
