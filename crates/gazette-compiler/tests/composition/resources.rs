//! Resource selections inherit independently from capability providers and retain exact origins.

use super::*;
use gazette_compiler::workspace::{
    CompositionOriginQuery, ParsedWorkspace, ResourceOwner, ResourceSelection, ResourceUse,
};

const RESOURCE: &str = "[composition.plugins]\n'capabilities'={state='enabled'}\n[composition.resources]\n'module:shared'={state='enabled',resource={owner={kind='plugin',package='capabilities'},resource='module:shared'},output='/assets/shared.js',use='dependency'}\n";

#[test]
fn resources_inherit_override_and_explain_the_exact_declaring_source() {
    let source = [
        RESOURCE,
        OWNER,
        ALPHA,
        "[sites.composition.resources]\n'module:shared'={state='enabled',resource={owner={kind='theme',package='theme:local'},resource='module:shared'},output='/assets/local.js',use='site'}\n",
        BETA,
        SOURCES,
    ]
    .concat();
    let parsed = ParsedWorkspace::parse(&source, toml::Limits::default())
        .expect("native resource selections");
    for (site, expected, expected_use, scope) in [
        (
            "site:alpha",
            "'/assets/local.js'",
            ResourceUse::Site,
            SelectionScope::Site(key("site:alpha")),
        ),
        (
            "site:beta",
            "'/assets/shared.js'",
            ResourceUse::Dependency,
            SelectionScope::Workspace,
        ),
    ] {
        let composition = parsed
            .compilation()
            .workspace
            .composition_for(&key(site))
            .expect("site");
        assert!(
            composition.capabilities().is_empty(),
            "resource selection cannot activate a capability"
        );
        assert_eq!(composition.resources().len(), 1);
        assert_eq!(
            composition.resources()[&resource("module:shared")]
                .origin()
                .scope,
            scope
        );
        let ResourceSelection::Enabled {
            resource: selected,
            output,
            use_,
        } = composition.resources()[&resource("module:shared")].value()
        else {
            panic!("resource remains enabled")
        };
        assert_eq!(output.as_str(), expected.trim_matches('\''));
        assert_eq!(*use_, expected_use);
        match site {
            "site:alpha" => assert_eq!(
                selected.owner,
                ResourceOwner::Theme(theme_package("theme:local"))
            ),
            "site:beta" => assert_eq!(
                selected.owner,
                ResourceOwner::Plugin(plugin_package("capabilities"))
            ),
            _ => unreachable!(),
        }
        assert_eq!(selected.resource, resource("module:shared"));
        let query = CompositionOriginQuery {
            site: key(site),
            field: SelectionField::Resource {
                resource: resource("module:shared"),
            },
            path: vec![gazette_compiler::toml::PathSegment::Key("output".into())],
        };
        let origin = parsed
            .composition_origin(&query)
            .expect("winning retained atom");
        assert_eq!(&source[origin.span.start..origin.span.end], expected);
        assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
        assert!(origin.path.iter().any(|segment| {
            segment == &gazette_compiler::toml::PathSegment::Key("resources".into())
        }));
    }
}

#[test]
fn explicit_disable_does_not_recover_the_workspace_resource_or_output() {
    let source = [
        RESOURCE,
        OWNER,
        ALPHA,
        "[sites.composition.resources]\n'module:shared'={state='disabled'}\n",
        BETA,
        SOURCES,
    ]
    .concat();
    let compiled = compile(&source);
    assert_eq!(
        compiled
            .workspace
            .composition_for(&key("site:alpha"))
            .expect("site")
            .resources()[&resource("module:shared")]
            .value(),
        &ResourceSelection::Disabled {}
    );
    assert!(matches!(
        compiled
            .workspace
            .composition_for(&key("site:beta"))
            .expect("site")
            .resources()[&resource("module:shared")]
            .value(),
        ResourceSelection::Enabled { .. }
    ));
    for missing in ["output", "use"] {
        let replacement = match missing {
            "output" => {
                "{state='enabled',resource={owner={kind='theme',package='theme:replacement'},resource='module:shared'},use='site'}"
            }
            _ => {
                "{state='enabled',resource={owner={kind='theme',package='theme:replacement'},resource='module:shared'},output='/local.js'}"
            }
        };
        let partial = source.replace("{state='disabled'}", replacement);
        assert!(
            workspace::compile(&partial, toml::Limits::default()).is_err(),
            "replacement cannot inherit the old {missing}"
        );
    }
}

#[test]
fn resource_package_references_outputs_and_use_are_strict_before_acquisition() {
    let source = [RESOURCE, OWNER, ALPHA, SOURCES].concat();
    for (changed, code) in [
        (
            source.replace(
                "kind='plugin',package='capabilities'",
                "kind='plugin',package='missing'",
            ),
            "workspace_resource_package_reference",
        ),
        (
            source.replace(
                "kind='plugin',package='capabilities'",
                "kind='theme',package='theme:missing'",
            ),
            "workspace_resource_package_reference",
        ),
        (
            source.replace("output='/assets/shared.js'", "output='/assets/'"),
            "workspace_resource_output",
        ),
    ] {
        assert!(
            invalid(&changed).iter().any(|error| error.code == code),
            "{code}"
        );
    }
    for changed in [
        source.replace("use='dependency'", "use='unknown'"),
        source.replace(",use='dependency'", ""),
        source.replace("[composition.resources]", "[composition.modules]"),
    ] {
        assert!(matches!(
            workspace::compile(&changed, toml::Limits::default()),
            Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
        ));
    }
}

#[test]
fn brand_identity_roles_require_an_enabled_site_resource_selection() {
    let brand = "[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:alpha'\nidentity_assets={site_icon='module:shared',link_preview='module:shared'}\n";
    let source = [RESOURCE, OWNER, ALPHA, brand, SOURCES]
        .concat()
        .replace("use='dependency'", "use='site'");
    let compiled = compile(&source);
    let publication = compiled
        .workspace
        .publication_for(&key("site:alpha"))
        .expect("brand publication");
    assert_eq!(
        publication.identity_assets.site_icon.as_ref(),
        Some(&resource("module:shared"))
    );
    assert_eq!(
        publication.identity_assets.link_preview.as_ref(),
        Some(&resource("module:shared"))
    );

    let missing = source.replace("site_icon='module:shared'", "site_icon='image:missing'");
    assert!(invalid(&missing).iter().any(|error| {
        error.code == "workspace_identity_asset_resource"
            && error.field.contains("identity_assets.site_icon")
    }));
}

#[test]
fn inherited_resource_fields_are_charged_to_the_derived_text_budget() {
    let output = format!("/assets/{}.js", "a".repeat(4096));
    let mut source = format!(
        "[composition.plugins]\n'capabilities'={{state='enabled'}}\n[composition.resources]\n\
         'module:graph'={{state='enabled',resource={{owner={{kind='plugin',package='capabilities'}},resource='module:graph'}},output='{output}',use='site'}}\n"
    );
    source.push_str(OWNER);
    source.push_str(SOURCES);
    for index in 0..8 {
        source.push_str(&format!(
            "[[sites]]\nid='site:item{index}'\npublisher='owner:one'\ndeployable=false\n"
        ));
    }
    let compiled = compile(&source);
    assert_eq!(compiled.workspace.sites().count(), 8);
    assert!(
        toml::ParsedToml::parse(
            &source,
            toml::Limits {
                source_bytes: source.len(),
                ..toml::Limits::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        workspace::compile(
            &source,
            toml::Limits {
                source_bytes: source.len(),
                ..toml::Limits::default()
            }
        ),
        Err(workspace::Error::Limit("composition text bytes"))
    ));
}

#[test]
fn resource_imports_use_logical_keys_and_reject_ambiguous_aliases() {
    use gazette_compiler::plugin::ParsedManifest;
    let manifest = "package='capabilities'\n[resources.'module:entry']\ninputs=['entry.ts']\nbuild={kind='browser-module',entry='entry.ts',imports={'@shared'='module:shared'}}\n";
    let parsed =
        ParsedManifest::parse(manifest, toml::Limits::default()).expect("logical resource import");
    let recipe = &parsed.manifest().resources[&resource("module:entry")].build;
    assert!(
        matches!(recipe, gazette_core::resource::Recipe::BrowserModule { imports, .. }
        if imports.get("@shared") == Some(&resource("module:shared")))
    );
    for alias in [
        "./shared.ts",
        "../shared",
        "/absolute",
        "http:source",
        "@shared//part",
        "@shared/../part",
        "bad%20key",
    ] {
        let changed = manifest.replace("@shared", alias);
        assert!(
            ParsedManifest::parse(&changed, toml::Limits::default()).is_err(),
            "{alias}"
        );
    }
}

#[test]
fn theme_defaults_are_below_workspace_site_and_explicit_disable_with_bounded_expansion() {
    let source = [
        RESOURCE,
        OWNER,
        ALPHA,
        "[sites.composition.resources]\n'module:shared'={state='disabled'}\n",
        BETA,
        SOURCES,
    ]
    .concat();
    let parsed = ParsedWorkspace::parse(&source, toml::Limits::default()).unwrap();
    let manifest = gazette_compiler::theme_package::ParsedManifest::parse(
        "package='theme:local'\nnamespace='local'\ntemplates='templates'\nrender={page='page.html'}\n\
[[resources]]\nid='module:shared'\ninputs=['main.js']\nbuild={kind='browser-module',entry='main.js'}\noutput='/theme.js'\n\
[[resources]]\nid='module:only-theme'\ninputs=['other.js']\nbuild={kind='browser-module',entry='other.js'}\noutput='/other.js'\nuse='dependency'\n",
        toml::Limits::default(),
    ).unwrap();
    let graph = gazette_compiler::theme_graph::Graph::resolve(
        &theme_package("theme:local"),
        [manifest.manifest()],
    )
    .unwrap();
    let theme = gazette_compiler::theme_graph::EffectiveTheme::resolve(&graph).unwrap();
    for site in ["site:alpha", "site:beta"] {
        let composition = parsed
            .compilation()
            .workspace
            .composition_for(&key(site))
            .unwrap();
        let merged = composition
            .with_theme_defaults(&theme, toml::Limits::default())
            .unwrap();
        assert_eq!(
            merged.resources()[&resource("module:shared")],
            composition.resources()[&resource("module:shared")]
        );
        let default = &merged.resources()[&resource("module:only-theme")];
        assert_eq!(
            default.origin().scope,
            SelectionScope::Theme(theme_package("theme:local"))
        );
        assert!(matches!(
            default.value(),
            ResourceSelection::Enabled {
                use_: ResourceUse::Dependency,
                ..
            }
        ));
        let limits = toml::Limits {
            nodes: 1,
            ..toml::Limits::default()
        };
        assert!(composition.with_theme_defaults(&theme, limits).is_err());
        assert_eq!(
            composition.resources().len(),
            1,
            "package admission does not mutate workspace-only declarations"
        );
    }
}
