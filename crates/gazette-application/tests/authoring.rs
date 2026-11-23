//! Current selected-set analysis through the real workspace/content/Markdown path.

#[path = "authoring/manual_anchors.rs"]
mod manual_anchors;

use gazette_application::authoring::{
    self, Analysis, ComponentCompletion, Error, Failure, Input, Limits, PackageBaseline,
    PackageBaselineKey, PackageFailure,
};
use gazette_compiler::authored;
use gazette_core::source::{EntryPath, Precision, Sha256Digest};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

const PACKAGE_WORKSPACE: &str = r#"
[composition]
theme={package='theme:root'}
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='Example'
[[brands.publications]]
site='site:one'
authored=[{source='source:content',target='content'}]
[[sources]]
id='source:content'
location={kind='filesystem',root='/not-opened/content'}
[[sources]]
id='source:foundation'
location={kind='filesystem',root='/not-opened/foundation'}
[[sources]]
id='source:root'
location={kind='filesystem',root='/not-opened/root'}
[[sources]]
id='source:plugin'
location={kind='filesystem',root='/not-opened/plugin'}
[[theme_packages]]
package='theme:foundation'
source='source:foundation'
manifest='foundation.toml'
[[theme_packages]]
package='theme:root'
source='source:root'
manifest='root.toml'
[[plugin_packages]]
package='commerce-plugin'
source='source:plugin'
manifest='plugin.toml'
"#;

const FOUNDATION_THEME: &[u8] = b"package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\n[components.notice]\ntemplate='notice.html'\n";
const ROOT_THEME: &[u8] = b"package='theme:root'\nnamespace='root'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:foundation'\n[components.callout]\ntemplate='callout.html'\n";
const COMMERCE_PLUGIN: &[u8] = b"package='commerce-plugin'\n[resources.'module:helper']\ninputs=['helper.ts']\nbuild={kind='browser-module',entry='helper.ts'}\n";

fn document(id: &str, edges: &str, structure: &str) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\n[content.structure]\n{structure}\n+++\n# Héllo 😀\n"
    )
}

fn input<'a>(path: &str, bytes: &'a [u8]) -> Input<'a> {
    Input {
        path: path.parse().expect("logical path"),
        bytes,
    }
}

fn package<'a>(package: PackageBaselineKey, bytes: &'a [u8]) -> PackageBaseline<'a> {
    PackageBaseline { package, bytes }
}

fn manual_baseline<'a>(source: &str, path: &str, bytes: &'a [u8]) -> authoring::ManualBaseline<'a> {
    authoring::ManualBaseline {
        source: source.parse().expect("manual source key"),
        path: path.parse().expect("manual path"),
        bytes,
    }
}

fn analyze(workspace: &str, files: &[(&str, &[u8])], limits: Limits) -> Result<Analysis, Error> {
    authoring::analyze(
        input("workspace.toml", workspace.as_bytes()),
        files.iter().map(|(path, bytes)| input(path, bytes)),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
}

#[test]
fn authored_manuals_keep_markdown_navigation_without_entering_publication_or_theme_semantics() {
    let article = document(
        "content:article",
        "",
        "kind='page'\nfamily={kind='article'}",
    )
    .replace("# Héllo 😀", "# Article");
    let manual = "+++\r\n[content]\r\nid='manual:not-publication-content'\r\n+++\r\n# Manual 🦀\r\n\r\n[Details](details.md#details)\r\n\r\n:::callout\r\nNot a Theme directive in this role.\r\n:::\r\n";
    let details = "# Details\r\n";
    let analysis = authoring::analyze_with_manuals_and_package_baselines(
        input("workspace.toml", WORKSPACE.as_bytes()),
        authoring::SelectedCorpus {
            asset_metadata: Default::default(),
            documents: vec![input("content/article.md", article.as_bytes())],
            manuals: vec![
                manual_baseline("source:manual", "manual/guide.md", manual.as_bytes()),
                manual_baseline("source:manual", "manual/details.md", details.as_bytes()),
            ],
            page_inputs: vec![],
        },
        [],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("manual and publication inputs remain one bounded editor selection");
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let manual_path = analysis
        .manual_analysis_path(
            &"source:manual".parse().expect("source"),
            &"manual/guide.md".parse().expect("manual path"),
        )
        .expect("qualified manual path")
        .clone();
    assert_eq!(analysis.manuals().count(), 2);
    assert_eq!(
        analysis
            .headings(&manual_path, manual)
            .expect("exact manual bytes retain headings")
            .values[0]
            .text,
        "Manual 🦀"
    );
    assert!(
        analysis
            .component_assistance(
                &manual_path,
                manual,
                manual.find("callout").expect("manual spelling"),
                1 << 16,
            )
            .expect("manual source is current")
            .is_none(),
        "manuals never bind Theme component contracts"
    );
    let navigation = analysis
        .source_navigation(
            &manual_path,
            manual,
            manual.find("[Details]").expect("manual link"),
        )
        .expect("manual navigation query")
        .expect("manual link resolves in the selected Markdown corpus");
    assert!(
        navigation
            .definition
            .path
            .as_str()
            .ends_with("/manual/details.md")
    );
    assert_eq!(
        navigation.definition.location.span.slice(details),
        Some("# Details")
    );
}

#[test]
fn authored_manual_paths_are_scoped_by_configured_source_identity() {
    let analysis = authoring::analyze_with_manuals_and_package_baselines(
        input("workspace.toml", WORKSPACE.as_bytes()),
        authoring::SelectedCorpus {
            asset_metadata: Default::default(),
            documents: vec![input("same.md", b"# Publication")],
            manuals: vec![
                manual_baseline("source:one", "same.md", b"# Manual one"),
                manual_baseline("source:two", "same.md", b"# Manual two"),
            ],
            page_inputs: vec![],
        },
        [],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("capture-relative manual paths are source-scoped");
    assert_eq!(analysis.manuals().count(), 2);
}

#[test]
fn manual_comments_use_the_documentation_grammar_without_admitting_active_html() {
    for (bytes, valid) in [
        (
            b"# Manual\n\n<!-- generated source boundary -->\n\nUse $D(\\cdot \\| \\cdot)$.\n\n```math\nB=(M_s,M_b)\n```\n".as_slice(),
            true,
        ),
        (b"# Manual\n\n<script>alert(1)</script>\n".as_slice(), false),
    ] {
        let analysis = authoring::analyze_with_manuals_and_package_baselines(
            input("workspace.toml", WORKSPACE.as_bytes()),
            authoring::SelectedCorpus {
            asset_metadata: Default::default(),
                documents: vec![],
                manuals: vec![manual_baseline("source:manual", "manual.md", bytes)],
                page_inputs: vec![],
            },
            [],
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        assert_eq!(
            analysis.structurally_valid(),
            valid,
            "{bytes:?}: {:?}",
            analysis.failures()
        );
    }
}

#[test]
fn editor_manual_companions_are_bound_without_claiming_retained_asset_bytes() {
    let workspace = format!(
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
documentation_repository={name='manual',path='/never-written/manual',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:manual',slug='manual',title='Manual',manual={source='source:manual',assets=[{input={kind='file',path='source.rs'},path='manual/source.rs.txt'}]},navigation=[{kind='manual',title='Manual',pages=[{title='Start',route='manual/start',input={kind='file',path='start.md'}}]}]}]}}
"##
    );
    for (target, valid) in [("source.rs", true), ("missing.rs", false)] {
        let body = format!("# Start\n\n[Source]({target})\n");
        let analysis = authoring::analyze_with_manuals_and_package_baselines(
            input("workspace.toml", workspace.as_bytes()),
            authoring::SelectedCorpus {
                asset_metadata: Default::default(),
                documents: vec![],
                manuals: vec![manual_baseline(
                    "source:manual",
                    "start.md",
                    body.as_bytes(),
                )],
                page_inputs: vec![],
            },
            [],
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let views = analysis.documentation_authoring(&gazette_adapter_markdown::Comrak);
        if valid {
            let views = views.expect("configured file companion is not a missing link");
            assert!(
                matches!(&views[0].closure, authoring::DocumentationClosure::Unavailable { reason } if reason.contains("companion") && reason.contains("bytes are unavailable"))
            );
        } else {
            assert!(
                views.is_err(),
                "undeclared input remains a genuine link failure"
            );
        }
    }
}

#[test]
fn rich_manual_authoring_closes_native_links_without_formatting_or_materializing_source() {
    let workspace = format!(
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
documentation_repository={name='manual',path='/never-written/manual',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:manual',slug='manual',title='Manual',manual={source='source:manual',assets=[{input={kind='file',path='source.rs'},path='manual/source.rs.txt'}]},navigation=[{kind='manual',title='Manual',pages=[{title='Start',route='manual/start',input={kind='file',path='start.md'}},{title='Details',route='manual/details',input={kind='file',path='details.md'}}]}]}]}}
"##
    );
    let details = "# Details\n\n## Ready\n\n[Return](start.md#start)\n";
    for (target, valid) in [
        ("details.md#ready", true),
        ("details.md#missing", false),
        ("missing.md", false),
        ("javascript:private", false),
    ] {
        let body = format!(
            "# Start\n\n::::steps\n:::step \"Install\"\n[Details]({target}) and [Source](source.rs).\n:::\n::::\n"
        );
        let analysis = authoring::analyze_with_manuals_and_package_baselines(
            input("workspace.toml", workspace.as_bytes()),
            authoring::SelectedCorpus {
                asset_metadata: Default::default(),
                documents: vec![],
                manuals: vec![
                    manual_baseline("source:manual", "start.md", body.as_bytes()),
                    manual_baseline("source:manual", "details.md", details.as_bytes()),
                ],
                page_inputs: vec![],
            },
            [],
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("retained rich manual analysis");
        let views = analysis.documentation_authoring(&gazette_adapter_markdown::Comrak);
        if valid {
            let views = views.expect("source closure does not require a lossy Markdown rewrite");
            assert!(
                matches!(&views[0].closure, authoring::DocumentationClosure::Unavailable { reason }
                if reason.contains("companion") && reason.contains("retained manual page routes, anchors"))
            );
            let source_key = "source:manual".parse().unwrap();
            let source_path = "start.md".parse().unwrap();
            assert_eq!(
                analysis.manual_baseline(&source_key, &source_path),
                Some(body.as_bytes())
            );
            let path = analysis
                .manual_analysis_path(&source_key, &source_path)
                .unwrap();
            let target = analysis
                .source_navigation(path, &body, body.find("[Details]").unwrap())
                .expect("current source link")
                .expect("exact selected heading");
            assert_eq!(
                target.definition.location.span.slice(details),
                Some("## Ready")
            );
            assert_eq!(
                target.definition.source_sha256,
                Sha256Digest::of(details.as_bytes())
            );
        } else {
            assert!(
                views.is_err(),
                "genuine source/anchor/scheme failure must remain: {target}"
            );
        }
    }
}

#[test]
fn changed_collection_invalidates_current_membership_and_restoration_recovers() {
    let collection = document("collection:guide", "", "kind='collection'");
    let article = document(
        "content:one",
        "collections=['collection:guide']\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let files = [
        ("collection.md", collection.as_bytes()),
        ("article.md", article.as_bytes()),
    ];
    let initial = analyze(WORKSPACE, &files, Limits::default()).expect("initial selected set");
    assert!(initial.structurally_valid());
    let changed = collection.replace("collection:guide", "collection:changed");
    let current = analyze(
        WORKSPACE,
        &[("collection.md", changed.as_bytes()), files[1]],
        Limits::default(),
    )
    .expect("invalid current analysis");
    assert!(!current.structurally_valid());
    let [
        Failure::Authored(authored::Error::Structure {
            error,
            declarations,
        }),
    ] = current.failures()
    else {
        panic!("native graph failure: {:?}", current.failures())
    };
    let gazette_core::publication::content::Error::Invalid(diagnostics) = error else {
        panic!("not a resource failure")
    };
    assert!(diagnostics.iter().any(|d| {
        d.source.as_str() == "content:one"
            && d.target
                .as_ref()
                .is_some_and(|k| k.as_str() == "collection:guide")
    }));
    assert_eq!(
        declarations[&"content:one".parse().expect("valid article identity")][0]
            .path
            .as_str(),
        "article.md"
    );
    assert_ne!(
        initial.inputs()["collection.md"],
        current.inputs()["collection.md"]
    );
    assert_eq!(
        initial.inputs()["article.md"],
        current.inputs()["article.md"]
    );
    let restored = analyze(WORKSPACE, &files, Limits::default()).expect("restored source");
    assert!(restored.structurally_valid());
    assert_eq!(restored.inputs(), initial.inputs());
}

#[test]
fn duplicate_identities_keep_both_full_document_origins_and_equal_bytes_remain_named() {
    let page = document("content:one", "", "kind='page'\nfamily={kind='article'}");
    let analysis = analyze(
        WORKSPACE,
        &[("a.md", page.as_bytes()), ("b.md", page.as_bytes())],
        Limits::default(),
    )
    .expect("native duplicate diagnostics");
    assert!(!analysis.structurally_valid());
    assert_eq!(analysis.inputs().len(), 3);
    let [Failure::Authored(authored::Error::Structure { declarations, .. })] = analysis.failures()
    else {
        panic!("both duplicate origins retained")
    };
    let origins = &declarations[&"content:one".parse().expect("valid duplicated identity")];
    assert_eq!(
        origins.iter().map(|o| o.path.as_str()).collect::<Vec<_>>(),
        ["a.md", "b.md"]
    );
    assert!(
        origins
            .iter()
            .all(|o| o.source_sha256 == Sha256Digest::of(page.as_bytes()) && o.span.is_some())
    );
}

#[test]
fn invalid_current_corpus_has_no_successful_heading_subset_or_previous_semantics() {
    let page = document("content:one", "", "kind='page'\nfamily={kind='article'}");
    let good = analyze(WORKSPACE, &[("a.md", page.as_bytes())], Limits::default())
        .expect("valid current article analysis");
    assert!(good.structurally_valid());
    let invalid = analyze(
        WORKSPACE,
        &[
            ("a.md", page.as_bytes()),
            ("bad.md", b"+++\n[broken\n+++\n# hidden"),
        ],
        Limits::default(),
    )
    .expect("current syntax failure");
    assert!(!invalid.structurally_valid());
    assert!(
        invalid
            .headings(&"a.md".parse().expect("valid article path"), &page)
            .is_none()
    );
    assert!(matches!(invalid.failures(), [Failure::Corpus(_)]));
    assert_eq!(invalid.inputs().len(), 3);
}

#[test]
fn heading_coordinates_are_bound_to_exact_full_source_and_preserve_precision() {
    let page =
        document("content:one", "", "kind='page'\nfamily={kind='article'}").replace('\n', "\r\n");
    let analysis = analyze(
        WORKSPACE,
        &[("page.md", page.as_bytes()), ("asset.bin", b"\xff")],
        Limits::default(),
    )
    .expect("analyze CRLF article with a non-document asset");
    assert!(analysis.structurally_valid());
    assert_eq!(analysis.inputs().len(), 3);
    assert!(
        analysis
            .headings(&"asset.bin".parse().expect("valid asset path"), "")
            .is_none()
    );
    assert_eq!(
        analysis.inputs()["page.md"],
        Sha256Digest::of(page.as_bytes())
    );
    let headings = analysis
        .headings(&"page.md".parse().expect("valid page path"), &page)
        .expect("current exact source retains its heading");
    assert_eq!(headings.values.len(), 1);
    let heading = &headings.values[0];
    assert_eq!(heading.precision, Precision::Construct);
    assert_eq!(
        heading
            .span
            .in_parent(&page, headings.body_span)
            .expect("heading span maps into the full CRLF document")
            .slice(&page),
        Some("# Héllo 😀")
    );
    let changed = page.replace("Héllo", "Hello");
    assert!(
        analysis
            .headings(&"page.md".parse().expect("valid page path"), &changed)
            .is_none()
    );
    assert!(
        analysis
            .headings(&"unknown.md".parse().expect("valid unselected path"), &page)
            .is_none()
    );
}

#[test]
fn invalid_workspace_keeps_only_explicitly_structural_current_corpus_help() {
    let page = document("content:one", "", "kind='page'\nfamily={kind='article'}");
    let analysis = analyze(
        "[[sites]]\nid='site:one'\npublisher='missing'\ndeployable=false",
        &[("page.md", page.as_bytes())],
        Limits::default(),
    )
    .expect("workspace failure preserves current corpus analysis");
    assert!(!analysis.structurally_valid());
    assert!(matches!(analysis.failures(), [Failure::Workspace(_)]));
    assert_eq!(
        analysis
            .headings(&"page.md".parse().expect("valid page path"), &page)
            .expect("structural heading remains available despite workspace failure")
            .values[0]
            .text,
        "Héllo 😀"
    );
}

#[test]
fn ambiguous_selections_and_aggregate_projection_exhaustion_never_return_a_subset() {
    let page = document("content:one", "", "kind='page'\nfamily={kind='article'}");
    for files in [
        vec![("workspace.toml", page.as_bytes())],
        vec![("a.md", page.as_bytes()), ("a.md", page.as_bytes())],
    ] {
        assert!(matches!(
            analyze(WORKSPACE, &files, Limits::default()),
            Err(Error::Selection(_))
        ));
    }
    for limits in [
        Limits {
            headings: 0,
            ..Limits::default()
        },
        Limits {
            heading_bytes: 0,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            analyze(WORKSPACE, &[("a.md", page.as_bytes())], limits),
            Err(Error::Limit(_))
        ));
    }
    let mut limits = Limits::default();
    limits.corpus.path_bytes = 1;
    assert!(matches!(
        analyze(WORKSPACE, &[], limits),
        Err(Error::Limit("input path bytes"))
    ));
}

#[test]
fn package_baselines_resolve_the_effective_theme_graph_for_component_help() {
    let page = format!(
        "{}\n:::notice\nDependency-owned component.\n:::\n",
        document("content:one", "", "kind='page'\nfamily={kind='article'}")
    );
    let baselines = vec![
        package(
            PackageBaselineKey::Theme("theme:foundation".parse().unwrap()),
            FOUNDATION_THEME,
        ),
        package(
            PackageBaselineKey::Theme("theme:root".parse().unwrap()),
            ROOT_THEME,
        ),
        package(
            PackageBaselineKey::Plugin("commerce-plugin".parse().unwrap()),
            COMMERCE_PLUGIN,
        ),
    ];
    let analysis = authoring::analyze_with_package_baselines(
        input("workspace.toml", PACKAGE_WORKSPACE.as_bytes()),
        [input("content/page.md", page.as_bytes())],
        baselines,
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("canonical package baselines");
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    assert_eq!(analysis.inputs().len(), 2);
    assert_eq!(analysis.package_baselines().len(), 3);
    assert_eq!(
        analysis.package_baselines()[&PackageBaselineKey::Theme("theme:root".parse().unwrap())],
        Sha256Digest::of(ROOT_THEME)
    );

    let path = "content/page.md".parse().unwrap();
    let offset = page.find("notice").expect("component name") + 2;
    let assistance = analysis
        .component_assistance(&path, &page, offset, 16 << 10)
        .expect("bounded component assistance")
        .expect("component header selected");
    let Some(ComponentCompletion::Name { candidates, .. }) = assistance.completion else {
        panic!("selected component name completion")
    };
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        ["callout", "notice"]
    );
    assert_eq!(
        assistance
            .hover
            .expect("exact dependency component")
            .name
            .as_str(),
        "notice"
    );
}

#[test]
fn landing_child_help_projects_the_effective_theme_and_native_section_rule() {
    let page = "format: gazette-landing/v1\ncontent:\n  id: content:landing\n  site: site:one\n  title: Landing\n  lifecycle: draft\n  structure: {kind: page, family: {kind: utility}}\nsections:\n  - component: callout\n    sections:\n      - component: notice\n      - body: An ordinary group.\n";
    let path = "content/page.landing.yaml";
    let analysis = authoring::analyze_with_package_baselines(
        input("workspace.toml", PACKAGE_WORKSPACE.as_bytes()),
        [input(path, page.as_bytes())],
        [
            package(
                PackageBaselineKey::Theme("theme:foundation".parse().unwrap()),
                FOUNDATION_THEME,
            ),
            package(
                PackageBaselineKey::Theme("theme:root".parse().unwrap()),
                ROOT_THEME,
            ),
            package(
                PackageBaselineKey::Plugin("commerce-plugin".parse().unwrap()),
                COMMERCE_PLUGIN,
            ),
        ],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("canonical package baselines");
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());
    let offset = page.find("callout").expect("component name") + 2;
    let help = analysis
        .component_assistance(&path.parse().unwrap(), page, offset, 16 << 10)
        .expect("bounded component assistance")
        .and_then(|help| help.hover)
        .and_then(|hover| hover.landing_children)
        .expect("landing child help");
    assert_eq!(
        help.named_components
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>(),
        ["callout", "notice"]
    );
    assert!(help.unnamed_section_requirement.contains("nonempty body"));
}

#[test]
fn invalid_workspace_does_not_invent_component_failures_in_its_dependent_pages() {
    let page = format!(
        "{}\n:::notice\nDependency-owned component.\n:::\n",
        document("content:one", "", "kind='page'\nfamily={kind='article'}")
    );
    let invalid = PACKAGE_WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    assert_ne!(invalid, PACKAGE_WORKSPACE);
    let analysis = authoring::analyze_with_package_baselines(
        input("workspace.toml", invalid.as_bytes()),
        [input("content/page.md", page.as_bytes())],
        [
            package(
                PackageBaselineKey::Theme("theme:foundation".parse().unwrap()),
                FOUNDATION_THEME,
            ),
            package(
                PackageBaselineKey::Theme("theme:root".parse().unwrap()),
                ROOT_THEME,
            ),
            package(
                PackageBaselineKey::Plugin("commerce-plugin".parse().unwrap()),
                COMMERCE_PLUGIN,
            ),
        ],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(!analysis.structurally_valid());
    assert!(
        analysis
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Workspace(_)))
    );
    assert!(
        !analysis
            .failures()
            .iter()
            .any(|failure| matches!(failure, Failure::Components { .. }))
    );
}

#[test]
fn package_identity_and_binding_are_single_owned_without_legacy_source_roles() {
    let root = PackageBaselineKey::Theme("theme:root".parse().unwrap());
    let analysis = authoring::analyze_with_package_baselines(
        input("workspace.toml", PACKAGE_WORKSPACE.as_bytes()),
        [],
        [package(root.clone(), FOUNDATION_THEME)],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("identity mismatch is phase-native evidence");
    assert_eq!(
        analysis.package_baselines()[&root],
        Sha256Digest::of(FOUNDATION_THEME)
    );
    assert!(analysis.failures().iter().any(|failure| matches!(
        failure,
        Failure::Package {
            package: PackageBaselineKey::Theme(package),
            source: Some(source),
            manifest: Some(manifest),
            error: PackageFailure::Binding(_),
        } if package.as_str() == "theme:root"
            && source.as_str() == "source:root"
            && manifest.as_str() == "root.toml"
    )));

    assert!(matches!(
        authoring::analyze_with_package_baselines(
            input("workspace.toml", PACKAGE_WORKSPACE.as_bytes()),
            [],
            [package(root.clone(), ROOT_THEME), package(root, ROOT_THEME),],
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        ),
        Err(Error::PackageSelection("duplicate package baseline"))
    ));
}

#[test]
fn page_package_ownership_is_resolved_once_from_exact_selected_bytes() {
    let page = document("content:guide", "", "kind='page'\nfamily={kind='article'}");
    let image = [0_u8, 159, 146, 150, 255];
    let analysis = authoring::analyze_with_manuals_and_package_baselines(
        input("workspace.toml", PACKAGE_WORKSPACE.as_bytes()),
        authoring::SelectedCorpus {
            asset_metadata: Default::default(),
            documents: vec![input("content/guide/index.md", page.as_bytes())],
            manuals: vec![],
            page_inputs: vec![input("content/guide/image.png", &image)],
        },
        [],
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("opaque retained page input is analyzed without UTF-8 coercion");
    assert!(analysis.structurally_valid(), "{:?}", analysis.failures());

    let owner: EntryPath = "content/guide/index.md".parse().unwrap();
    let ownership = analysis
        .page_package_ownership(&owner, Sha256Digest::of(page.as_bytes()))
        .expect("exact owner bytes")
        .expect("compiler-resolved package");
    assert_eq!(ownership.content.as_str(), "content:guide");
    assert!(matches!(
        ownership.role,
        authoring::PagePackageRole::Document
    ));
    assert_eq!(ownership.package.document(), &owner);
    assert_eq!(
        ownership.package.companions()[0].source_sha256(),
        Some(Sha256Digest::of(&image))
    );

    let companion: EntryPath = "content/guide/image.png".parse().unwrap();
    assert!(matches!(
        analysis
            .page_package_ownership(&companion, Sha256Digest::of(&image))
            .unwrap()
            .unwrap()
            .role,
        authoring::PagePackageRole::Companion { .. }
    ));
    assert!(
        analysis
            .page_package_ownership(&companion, Sha256Digest::of(b"changed"))
            .is_err()
    );
}

#[test]
fn structured_landings_do_not_invent_affine_editor_offsets_or_break_selected_set_analysis() {
    let source = "format: gazette-landing/v1\ncontent:\n  id: content:landing\n  site: site:one\n  title: Landing\n  lifecycle: draft\n  structure: {kind: page, family: {kind: utility}}\nsections:\n  - heading: {level: 2, text: 'नमस्ते'}\n    body: Text\n";
    let result = analyze(
        WORKSPACE,
        &[("index.landing.yaml", source.as_bytes())],
        Default::default(),
    )
    .unwrap();
    assert!(result.failures().is_empty(), "{:?}", result.failures());
    assert!(
        result
            .headings(&"index.landing.yaml".parse().unwrap(), source)
            .is_none(),
        "byte-edit assistance is not fabricated for normalized YAML"
    );
    let invalid = source.replace("format: gazette-landing/v1", "format: unsupported");
    let result = analyze(
        WORKSPACE,
        &[("index.landing.yaml", invalid.as_bytes())],
        Default::default(),
    )
    .unwrap();
    assert!(matches!(
        result.failures().first(),
        Some(Failure::Corpus(_))
    ));
}
