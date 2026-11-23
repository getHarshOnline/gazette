//! Current supplied workspace tokens share native analysis and selected-buffer state.

use gazette_sdk::{
    authoring::{
        self, Analysis, Input, Limits, NavigationError, PackageBaseline, PackageBaselineKey,
        session::{PackageSource, Session, Source},
    },
    source::{EntryPath, Precision, Sha256Digest},
    workspace::{DeclarationKind, ParsedWorkspace, WorkspaceInspection},
};

const THEME_BASE: &str =
    "package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={page='page.html'}\n";
const THEME_OVERRIDE: &str = "package='theme:override'\nnamespace='override'\ntemplates='templates'\nrender={page='page.html'}\n";
const PLUGIN: &str =
    "package='capabilities'\n[providers.search]\nimplements=['publication.search/v1']\n";

fn workspace() -> String {
    r#"# 😀 owner:é is not a reference
[composition]
theme={package='theme:base',palette='palette:night'}
[[principals]]
id="owner:\u00e9"
kind='person'
[[sites]]
id='site:one'
publisher='owner:é'
deployable=false
[[projects]]
id='project:one'
lifecycle='done'
visibility='private'
availability='unavailable'
[[sources]]
id='source:one'
location={kind='repository',root='/not-opened/navigation',commit='0000000000000000000000000000000000000000',subdirectory='docs'}
[[theme_packages]]
package='theme:base'
source='source:one'
manifest='theme.toml'
[[brands]]
id='brand:one'
name='owner:é'
[[brands.publications]]
site='site:one'
authored=[{source='source:one',target='authored'}]
[[taxonomies]]
id='taxonomy:one'
label='Subject'
terms=[{id='term:one',label='One'}]
[[placements]]
role = 'primary'
site='site:one'
project='project:one'
render=true
discoverable=false
documents={source='source:one',target='project-docs'}
"#.replace('\n', "\r\n")
}

fn path() -> EntryPath {
    "workspace.toml".parse().expect("workspace logical path")
}

fn package_source(id: &str, path: &str, package: PackageBaselineKey, text: &str) -> PackageSource {
    PackageSource {
        package,
        input: Source {
            id: id.into(),
            path: path.parse().expect("package logical path"),
            text: text.into(),
        },
    }
}

fn base_theme_source() -> PackageSource {
    package_source(
        "theme-base",
        "packages/theme-base.toml",
        PackageBaselineKey::Theme("theme:base".parse().expect("Theme package identity")),
        THEME_BASE,
    )
}

fn analyze(
    source: &str,
    documents: &[(&str, &str)],
    limits: Limits,
) -> Result<Analysis, authoring::Error> {
    authoring::analyze_with_package_baselines(
        Input {
            path: path(),
            bytes: source.as_bytes(),
        },
        documents.iter().map(|(path, source)| Input {
            path: path.parse().expect("selected document path"),
            bytes: source.as_bytes(),
        }),
        [PackageBaseline {
            package: PackageBaselineKey::Theme(
                "theme:base".parse().expect("Theme package identity"),
            ),
            bytes: THEME_BASE.as_bytes(),
        }],
        limits,
    )
}

#[test]
fn modular_workspace_session_resolves_cross_document_navigation_and_current_overlays() {
    let root_path: EntryPath = "gazette.toml".parse().expect("root path");
    let site_path: EntryPath = "sites/one.toml".parse().expect("site path");
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:publisher'\nkind='person'\n";
    let site = "[[sites]]\nid='site:one'\npublisher='owner:publisher'\ndeployable=false\n";
    let mut session = Session::new_with_workspace_documents(
        Source {
            id: "root".into(),
            path: root_path.clone(),
            text: root.into(),
        },
        [Source {
            id: "site".into(),
            path: site_path.clone(),
            text: site.into(),
        }],
        [],
        [],
        Default::default(),
    )
    .expect("modular selection");

    let reference_offset = site.find("owner:publisher").expect("publisher reference");
    let analysis = session.analysis().as_ref().expect("resolved workspace");
    let navigation = analysis
        .source_navigation(&site_path, site, reference_offset)
        .expect("current source")
        .expect("cross-document association");
    assert_eq!(navigation.path, &site_path);
    assert_eq!(navigation.definition.path, &root_path);
    assert_eq!(
        navigation.definition.location.span.slice(root),
        Some("'owner:publisher'")
    );
    assert_eq!(navigation.references.len(), 1);
    assert_eq!(navigation.references[0].path, &site_path);

    assert!(session.open("root", 1, "[broken\n".into()).expect("open"));
    assert_eq!(session.generation(), 2);
    assert!(matches!(
        session
            .analysis()
            .as_ref()
            .expect("current failed analysis remains queryable")
            .source_navigation(&site_path, site, reference_offset),
        Err(NavigationError::Unavailable)
    ));
    assert!(session.close("root").expect("close"));
    assert_eq!(session.generation(), 3);
    assert!(
        session
            .analysis()
            .as_ref()
            .expect("baseline restored")
            .source_navigation(&site_path, site, reference_offset)
            .expect("current source")
            .is_some()
    );
}

#[test]
fn workspace_identity_borrows_the_native_symbol_at_each_namespace_and_reference() {
    let source = workspace();
    let analysis = analyze(&source, &[], Default::default()).expect("current analysis");
    let symbols = analysis
        .workspace_symbols(&path(), &source)
        .expect("source")
        .expect("workspace");
    for symbol in symbols {
        let identity = analysis
            .workspace_identity(&path(), &source, symbol.location().span.start)
            .expect("current occurrence")
            .expect("declaration");
        assert!(
            std::ptr::eq(identity.symbol, symbol),
            "borrow the sole retained symbol"
        );
        assert_eq!(identity.occurrence, symbol.location());
        assert_eq!(identity.path, &path());
        assert_eq!(identity.source_sha256, Sha256Digest::of(source.as_bytes()));
    }
    let offset = source.find("publisher='owner:é'").expect("reference") + "publisher=".len();
    let identity = analysis
        .workspace_identity(&path(), &source, offset)
        .expect("lookup")
        .expect("reference");
    assert_eq!(
        identity.symbol.declaration().expect("Key declaration").kind,
        DeclarationKind::Principal
    );
    assert_eq!(identity.symbol.name(), "owner:é");
    assert_eq!(identity.occurrence.span.slice(&source), Some("'owner:é'"));
    assert_eq!(identity.occurrence.precision, Precision::Construct);
    assert_eq!(
        identity.symbol.location().span.slice(&source),
        Some("\"owner:\\u00e9\"")
    );
    for offset in [
        0,
        source.find("name='owner:é'").expect("name") + 7,
        source.len(),
    ] {
        assert!(
            analysis
                .workspace_identity(&path(), &source, offset)
                .expect("non-identity")
                .is_none()
        );
    }
}

#[test]
fn workspace_identity_refuses_wrong_preimages_and_tracks_unsaved_current_analysis() {
    let source = workspace();
    let mut state = Session::new_with_package_baselines(
        Source {
            id: "w".into(),
            path: path(),
            text: source.clone(),
        },
        [Source {
            id: "d".into(),
            path: "bad.md".parse().expect("path"),
            text: "+++\n[broken\n+++\n".into(),
        }],
        [base_theme_source()],
        Default::default(),
    )
    .expect("selection");
    let offset = source.find("publisher='owner:é'").expect("reference") + 12;
    let analysis = state.analysis().as_ref().expect("analysis");
    assert!(
        analysis
            .workspace_identity(&path(), &source, offset)
            .expect("unrelated corpus failure")
            .is_some()
    );
    for (path, text, position) in [
        (path(), format!("{source} "), offset),
        (
            "unknown.toml".parse().expect("path"),
            source.clone(),
            offset,
        ),
        (
            path(),
            source.clone(),
            source.find('😀').expect("emoji") + 1,
        ),
        (path(), source.clone(), source.len() + 1),
    ] {
        assert!(matches!(
            analysis.workspace_identity(&path, &text, position),
            Err(NavigationError::InvalidSource)
        ));
    }
    assert!(
        analysis
            .workspace_identity(&"bad.md".parse().expect("path"), "+++\n[broken\n+++\n", 0)
            .expect("selected non-workspace")
            .is_none()
    );
    let changed = source
        .replace("owner:é", "owner:two")
        .replace("owner:\\u00e9", "owner:two");
    assert!(state.open("w", 1, changed.clone()).expect("open"));
    let current = state.analysis().as_ref().expect("analysis");
    let moved = changed.find("publisher='owner:two'").expect("changed") + 12;
    assert_eq!(
        current
            .workspace_identity(&path(), &changed, moved)
            .expect("current")
            .expect("identity")
            .symbol
            .name(),
        "owner:two"
    );
    assert!(state.change("w", 2, "[broken\n".into()).expect("change"));
    assert!(matches!(
        state
            .analysis()
            .as_ref()
            .expect("current failure")
            .workspace_identity(&path(), "[broken\n", 0),
        Err(NavigationError::Unavailable)
    ));
    assert!(state.close("w").expect("restore baseline"));
    assert_eq!(
        state
            .analysis()
            .as_ref()
            .expect("restored")
            .workspace_identity(&path(), &source, offset)
            .expect("lookup")
            .expect("identity")
            .symbol
            .name(),
        "owner:é"
    );
}

#[test]
fn site_identity_explains_current_native_composition_without_reimplementing_inheritance() {
    let baseline = workspace().replace(
        "[[principals]]",
        "[composition.plugins]\n'capabilities'={state='enabled'}\n[composition.capabilities]\n'publication.search/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='search'},requirement='required'}\n[[theme_packages]]\npackage='theme:override'\nsource='source:one'\nmanifest='theme-override.toml'\n[[plugin_packages]]\npackage='capabilities'\nsource='source:one'\nmanifest='plugin.toml'\n[[principals]]",
    );
    let changed = baseline.replace(
        "deployable=false",
        "deployable=false\ncomposition={theme={package='theme:override'},capabilities={'publication.search/v1'={state='disabled'}}}",
    );
    let mut state = Session::new_with_package_baselines(
        Source {
            id: "w".into(),
            path: path(),
            text: baseline.clone(),
        },
        [Source {
            id: "bad".into(),
            path: "bad.md".parse().expect("path"),
            text: "+++\n[broken\n+++\n".into(),
        }],
        [
            base_theme_source(),
            package_source(
                "theme-override",
                "packages/theme-override.toml",
                PackageBaselineKey::Theme(
                    "theme:override".parse().expect("Theme package identity"),
                ),
                THEME_OVERRIDE,
            ),
            package_source(
                "plugin",
                "packages/plugin.toml",
                PackageBaselineKey::Plugin(
                    "capabilities".parse().expect("plugin package identity"),
                ),
                PLUGIN,
            ),
        ],
        Default::default(),
    )
    .expect("session");
    let inspect = |state: &Session, source: &str| {
        let offset = source.find("id='site:one'").expect("site") + 4;
        let analysis = state.analysis().as_ref().expect("current analysis");
        let identity = analysis
            .workspace_identity(&path(), source, offset)
            .expect("unrelated corpus failure preserves valid composition")
            .expect("site");
        let composition = identity
            .composition
            .expect("site owns a complete configured selection");
        let expected = gazette_sdk::workspace::compile(source, Default::default())
            .expect("native compilation");
        assert_eq!(
            composition,
            expected
                .workspace
                .composition_for(&"site:one".parse().expect("site"))
                .expect("native composition")
        );
        let site_reference = source
            .find("site='site:one'")
            .expect("publication reference")
            + 6;
        let reference = analysis
            .workspace_identity(&path(), source, site_reference)
            .expect("reference")
            .expect("site reference");
        assert!(std::ptr::eq(
            composition,
            reference.composition.expect("same site composition owner")
        ));
        assert_eq!(identity.source_sha256, Sha256Digest::of(source.as_bytes()));
        serde_json::to_value(composition).expect("native projection")
    };
    let inherited = inspect(&state, &baseline);
    assert_eq!(inherited["theme"]["origin"]["scope"], "workspace");
    assert_eq!(inherited["theme"]["value"]["palette"], "palette:night");
    assert_eq!(
        inherited["capabilities"]["publication.search/v1"]["value"]["state"],
        "enabled"
    );
    state
        .open("w", 1, changed.clone())
        .expect("current overlay");
    let overridden = inspect(&state, &changed);
    assert_eq!(
        overridden["theme"]["origin"]["scope"],
        serde_json::json!({"site":"site:one"})
    );
    assert_eq!(overridden["theme"]["value"]["package"], "theme:override");
    assert!(
        overridden["theme"]["value"]["palette"].is_null(),
        "whole atom does not retain old palette"
    );
    assert_eq!(
        overridden["capabilities"]["publication.search/v1"]["value"]["state"],
        "disabled"
    );
    assert_eq!(
        overridden["capabilities"]["publication.search/v1"]["origin"]["scope"],
        serde_json::json!({"site":"site:one"})
    );
    state
        .change("w", 2, "[broken\n".into())
        .expect("invalid overlay");
    assert!(matches!(
        state
            .analysis()
            .as_ref()
            .expect("current failures")
            .workspace_identity(&path(), "[broken\n", 0),
        Err(NavigationError::Unavailable)
    ));
    state.close("w").expect("baseline restoration");
    assert_eq!(inspect(&state, &baseline), inherited);
}

#[test]
fn native_six_namespace_symbols_and_references_preserve_exact_scalar_coordinates() {
    let source = workspace();
    let parsed =
        ParsedWorkspace::parse(&source, Default::default()).expect("native complete topology");
    let analysis = analyze(&source, &[], Default::default()).expect("SDK current analysis");
    assert!(analysis.structurally_valid());
    let symbols = analysis
        .workspace_symbols(&path(), &source)
        .expect("current symbols")
        .expect("workspace role");
    assert!(
        symbols
            .iter()
            .copied()
            .eq(parsed.navigation().symbols().iter())
    );
    assert_eq!(
        symbols
            .iter()
            .map(|v| v.declaration().expect("Key declaration").kind)
            .collect::<Vec<_>>(),
        [
            DeclarationKind::Principal,
            DeclarationKind::Site,
            DeclarationKind::Project,
            DeclarationKind::Source,
            DeclarationKind::Brand,
            DeclarationKind::Taxonomy,
        ]
    );
    assert_eq!(symbols[0].name(), "owner:é");
    assert_eq!(
        symbols[0].location().span.slice(&source),
        Some("\"owner:\\u00e9\"")
    );
    assert!(
        symbols
            .iter()
            .all(|v| v.location().precision == Precision::Construct)
    );
    let reference = source
        .find("publisher='owner:é'")
        .expect("native publisher field")
        + "publisher=".len();
    let nav = analysis
        .source_navigation(&path(), &source, reference)
        .expect("publisher query")
        .expect("declared reference");
    assert_eq!(nav.path, &path());
    assert_eq!(nav.source_sha256, Sha256Digest::of(source.as_bytes()));
    assert_eq!(nav.occurrence.span.slice(&source), Some("'owner:é'"));
    assert_eq!(nav.definition.location, symbols[0].location());
    assert_eq!(nav.definition.path, &path());
    assert_eq!(
        nav.definition.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(nav.references.len(), 1);
    assert_eq!(nav.references[0].location, nav.occurrence);
    assert_eq!(nav.references[0].path, &path());
    for symbol in symbols {
        let nav = analysis
            .source_navigation(&path(), &source, symbol.location().span.start)
            .expect("declaration query")
            .expect("native symbol");
        assert_eq!(nav.definition.location, symbol.location());
        assert_eq!(nav.occurrence, symbol.location());
        assert!(
            nav.references
                .windows(2)
                .all(|v| v[0].location.span.start < v[1].location.span.start)
        );
        for reference in &nav.references {
            let reverse = analysis
                .source_navigation(&path(), &source, reference.location.span.start)
                .expect("exact reference query")
                .expect("same-source association");
            assert_eq!(reverse.definition, nav.definition);
            assert_eq!(reverse.references, nav.references);
        }
        if symbol.declaration().expect("Key declaration").kind == DeclarationKind::Source {
            assert_eq!(nav.references.len(), 3);
        }
        if symbol.declaration().expect("Key declaration").kind == DeclarationKind::Site {
            assert_eq!(nav.references.len(), 2);
        }
    }
}

#[test]
fn nonoccurrences_and_stale_or_invalid_preimages_never_infer_identity() {
    let source = workspace();
    let analysis = analyze(&source, &[("page.md", "# Heading\n")], Default::default())
        .expect("current supplied sources");
    for offset in [
        0,
        source
            .find("name='owner:é'")
            .expect("arbitrary display name")
            + 6,
        source.find("theme:base").expect("workspace-declared theme"),
        source.find("palette:night").expect("palette"),
        source.find("term:one").expect("scoped taxonomy term"),
        source.len(),
    ] {
        assert_eq!(
            analysis
                .source_navigation(&path(), &source, offset)
                .expect("valid nonoccurrence"),
            None
        );
    }
    for offset in [
        source.find('😀').expect("non-BMP comment") + 1,
        source.len() + 1,
    ] {
        assert_eq!(
            analysis
                .source_navigation(&path(), &source, offset)
                .expect_err("invalid byte boundary"),
            NavigationError::InvalidSource
        );
    }
    let stale = source.replace("Subject", "Changed");
    assert_eq!(
        analysis
            .workspace_symbols(&path(), &stale)
            .expect_err("whole source binding"),
        NavigationError::InvalidSource
    );
    assert_eq!(
        analysis
            .source_navigation(&path(), &stale, 0)
            .expect_err("stale navigation"),
        NavigationError::InvalidSource
    );
    assert_eq!(
        analysis
            .workspace_symbols(&"unknown.toml".parse().expect("unknown path"), &source)
            .expect_err("unselected source"),
        NavigationError::InvalidSource
    );
    assert!(
        analysis
            .workspace_symbols(&"page.md".parse().expect("page path"), "# Heading\n")
            .expect("selected content role")
            .is_none()
    );
}

#[test]
fn workspace_and_content_phase_failures_do_not_hide_independent_current_facts() {
    let source = workspace();
    let broken_content = analyze(
        &source,
        &[("broken.md", "+++\n[content\n+++\n")],
        Default::default(),
    )
    .expect("native failure report");
    assert!(
        broken_content
            .failures()
            .iter()
            .any(|v| matches!(v, authoring::Failure::Corpus(_)))
    );
    assert_eq!(
        broken_content
            .workspace_symbols(&path(), &source)
            .expect("workspace survives content failure")
            .expect("workspace symbols")
            .len(),
        6
    );
    assert!(
        broken_content
            .source_navigation(
                &path(),
                &source,
                source.find("publisher=").expect("publisher") + 11
            )
            .expect("independent workspace navigation")
            .is_some()
    );
    let invalid = source.replace("publisher='owner:é'", "publisher='owner:missing'");
    let page = "# Heading\n\n[go](#heading)\n";
    let broken_workspace = analyze(&invalid, &[("page.md", page)], Default::default())
        .expect("current semantic failures");
    assert!(
        broken_workspace
            .failures()
            .iter()
            .any(|v| matches!(v, authoring::Failure::Workspace(_)))
    );
    let (compilation, native_navigation) = WorkspaceInspection::parse(&invalid, Default::default())
        .expect("complete current syntax")
        .into_parts();
    assert!(compilation.is_err(), "source facts do not admit meaning");
    let native_navigation = native_navigation.expect("independent current coordinates");
    let symbols = broken_workspace
        .workspace_symbols(&path(), &invalid)
        .expect("syntax survives semantic failure")
        .expect("selected workspace");
    assert!(
        symbols
            .iter()
            .copied()
            .eq(native_navigation.symbols().iter())
    );
    assert_eq!(symbols.len(), 6);
    assert_eq!(
        broken_workspace.inputs()["workspace.toml"],
        Sha256Digest::of(invalid.as_bytes())
    );
    assert_eq!(
        broken_workspace
            .workspace_symbols(&path(), &source)
            .expect_err("previous valid bytes are not the current syntax"),
        NavigationError::InvalidSource
    );
    assert_eq!(
        broken_workspace
            .source_navigation(
                &path(),
                &invalid,
                invalid.find("publisher=").expect("unresolved publisher") + "publisher=".len(),
            )
            .expect_err("syntax facts do not produce semantically resolved navigation"),
        NavigationError::Unavailable
    );
    let heading = broken_workspace
        .source_navigation(
            &"page.md".parse().expect("page"),
            page,
            page.find("[go]").expect("heading reference"),
        )
        .expect("independent native Markdown")
        .expect("heading target");
    assert_eq!(
        heading.definition.location.span.slice(page),
        Some("# Heading")
    );
}

#[test]
fn session_overlays_replace_navigation_stale_versions_do_nothing_and_close_restores_capture() {
    let baseline = workspace();
    let mut state = Session::new_with_package_baselines(
        Source {
            id: "workspace".into(),
            path: path(),
            text: baseline.clone(),
        },
        [],
        [base_theme_source()],
        Default::default(),
    )
    .expect("selected baseline session");
    assert!(
        state
            .open("workspace", 7, baseline.clone())
            .expect("initial overlay")
    );
    let invalid = baseline
        .replace("publisher='owner:é'", "publisher='owner:missing'")
        .replace("id='brand:one'", "id='brand:overlay'");
    assert!(
        state
            .change("workspace", 8, invalid.clone())
            .expect("accepted invalid current workspace")
    );
    let generation = state.generation();
    let current = state.analysis().as_ref().expect("native failure record");
    assert!(!current.structurally_valid());
    let symbols = current
        .workspace_symbols(&path(), &invalid)
        .expect("current independently valid syntax")
        .expect("workspace symbols");
    let (compilation, native_navigation) = WorkspaceInspection::parse(&invalid, Default::default())
        .expect("new overlay syntax")
        .into_parts();
    assert!(compilation.is_err());
    assert!(
        symbols.iter().copied().eq(native_navigation
            .expect("new overlay coordinates")
            .symbols()
            .iter())
    );
    let brand = symbols
        .iter()
        .find(|symbol| {
            symbol.declaration().expect("Key declaration").kind == DeclarationKind::Brand
        })
        .expect("new overlay brand");
    assert_eq!(brand.name(), "brand:overlay");
    assert_eq!(
        brand.location().span.slice(&invalid),
        Some("'brand:overlay'")
    );
    assert!(symbols.iter().all(|symbol| symbol.name() != "brand:one"));
    assert_eq!(
        current
            .workspace_symbols(&path(), &baseline)
            .expect_err("previous valid source cannot supply symbols"),
        NavigationError::InvalidSource
    );
    assert_eq!(
        current
            .source_navigation(&path(), &invalid, brand.location().span.start)
            .expect_err("current source symbols do not admit resolved navigation"),
        NavigationError::Unavailable
    );
    assert!(
        !state
            .change("workspace", 8, baseline.clone())
            .expect("same version ignored")
    );
    assert!(
        !state
            .change("workspace", 6, baseline.clone())
            .expect("older version ignored")
    );
    assert_eq!(state.generation(), generation);
    assert_eq!(state.files()["workspace"].text(), invalid);
    let replacement = baseline
        .replace("owner:\\u00e9", "owner:new")
        .replace("owner:é", "owner:new");
    assert!(
        state
            .change("workspace", 9, replacement.clone())
            .expect("new valid source")
    );
    let current = state.analysis().as_ref().expect("fresh valid analysis");
    assert_eq!(
        current
            .workspace_symbols(&path(), &replacement)
            .expect("replacement symbols")
            .expect("workspace")[0]
            .name(),
        "owner:new"
    );
    assert_eq!(
        current
            .workspace_symbols(&path(), &baseline)
            .expect_err("old bytes refused"),
        NavigationError::InvalidSource
    );
    assert!(state.close("workspace").expect("restore retained baseline"));
    let restored = state.analysis().as_ref().expect("restored analysis");
    assert_eq!(state.files()["workspace"].text(), baseline);
    assert_eq!(
        restored.inputs()["workspace.toml"],
        Sha256Digest::of(baseline.as_bytes())
    );
    assert_eq!(
        restored
            .workspace_symbols(&path(), &baseline)
            .expect("restored symbols")
            .expect("workspace")[0]
            .name(),
        "owner:é"
    );
    assert!(!state.close("workspace").expect("already closed no-op"));
}

#[test]
fn workspace_and_markdown_share_exact_complete_navigation_budgets() {
    let source = workspace();
    let parsed = ParsedWorkspace::parse(&source, Default::default())
        .expect("native complete workspace index");
    let targets: Vec<_> = parsed
        .navigation()
        .symbols()
        .iter()
        .filter_map(|symbol| {
            gazette_sdk::authored::AuthoredReferenceDeclaration::workspace(symbol, &path(), &source)
                .expect("source-bound native completion declaration")
        })
        .collect();
    let workspace_occurrences = parsed.navigation().occurrence_count() + targets.len();
    let workspace_bytes = parsed.navigation().key_bytes()
        + targets
            .iter()
            .map(|target| target.fact_bytes())
            .sum::<usize>();
    let page = "# Heading\n\n[go](#heading) one[^é] two[^é].\n\n[^é]: note\n";
    let native = gazette_sdk::markdown::parse(page, Default::default()).expect("native page facts");
    let page_occurrences = native.footnote_definitions().len()
        + native.footnote_references().len()
        + native
            .heading_references()
            .expect("heading references")
            .len()
        + native
            .heading_completion_slots()
            .expect("heading slots")
            .len()
        + native
            .symbolic_completion_slots()
            .expect("symbolic slots")
            .len();
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
    let exact = Limits {
        navigation_occurrences: workspace_occurrences + page_occurrences,
        navigation_bytes: workspace_bytes + page_bytes,
        ..Default::default()
    };
    let analysis =
        analyze(&source, &[("page.md", page)], exact).expect("exact combined source facts");
    assert!(
        analysis
            .workspace_symbols(&path(), &source)
            .expect("complete workspace")
            .is_some()
    );
    assert!(
        analysis
            .source_navigation(
                &"page.md".parse().expect("page"),
                page,
                page.find("[go]").expect("heading link")
            )
            .expect("complete Markdown navigation")
            .is_some()
    );
    for limits in [
        Limits {
            navigation_occurrences: exact.navigation_occurrences - 1,
            ..exact
        },
        Limits {
            navigation_bytes: exact.navigation_bytes - 1,
            ..exact
        },
    ] {
        assert!(
            matches!(
                analyze(&source, &[("page.md", page)], limits),
                Err(authoring::Error::Limit(_))
            ),
            "aggregate failure returns no partial analysis"
        );
    }
    assert!(
        analyze(
            &source,
            &[],
            Limits {
                navigation_occurrences: workspace_occurrences,
                navigation_bytes: workspace_bytes,
                ..Default::default()
            }
        )
        .is_ok()
    );
    for (limits, resource) in [
        (
            Limits {
                navigation_occurrences: workspace_occurrences - 1,
                ..Default::default()
            },
            "navigation occurrences",
        ),
        (
            Limits {
                navigation_bytes: workspace_bytes - 1,
                ..Default::default()
            },
            "navigation bytes",
        ),
    ] {
        assert!(
            matches!(analyze(&source, &[], limits), Err(authoring::Error::Limit(value)) if value == resource)
        );
    }
}
