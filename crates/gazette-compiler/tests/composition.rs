//! Explicit workspace/site composition through the native TOML compiler.

use gazette_compiler::{
    theme_package::ThemePackageId,
    toml,
    workspace::{
        self, CapabilitySelection, Requirement, SelectionField, SelectionOrigin, SelectionScope,
        ThemeSelection,
    },
};
use gazette_core::{
    plugin::PluginPackageId,
    publication::{
        CapabilityId, Key,
        capability_runtime::{CapabilityAbsence, ProviderId, ProviderRef},
    },
    resource::ResourceId,
    source::Sha256Digest,
};

#[path = "composition/chrome.rs"]
mod chrome;
#[path = "composition/navigation.rs"]
mod navigation;
#[path = "composition/plugins.rs"]
mod plugins;
#[path = "composition/reading_assistance.rs"]
mod reading_assistance;
#[path = "composition/resources.rs"]
mod resources;

const OWNER: &str = "[[principals]]\nid='owner:one'\nkind='person'\n";
const ALPHA: &str = "[[sites]]\nid='site:alpha'\npublisher='owner:one'\ndeployable=false\n";
const BETA: &str = "[[sites]]\nid='site:beta'\npublisher='owner:one'\ndeployable=true\n";
const SOURCES: &str = "[[sources]]\nid='source:theme'\n\
    location={kind='repository',root='/not-opened/theme',commit='0000000000000000000000000000000000000000',subdirectory='theme'}\n\
    [[sources]]\nid='source:provider'\n\
    location={kind='repository',root='/not-opened/provider',commit='1111111111111111111111111111111111111111',subdirectory='provider'}\n\
    [[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='theme.toml'\n\
    [[theme_packages]]\npackage='theme:replacement'\nsource='source:provider'\nmanifest='theme.toml'\n\
    [[theme_packages]]\npackage='theme:local'\nsource='source:theme'\nmanifest='theme.toml'\n\
    [[plugin_packages]]\npackage='capabilities'\nsource='source:provider'\nmanifest='plugin.toml'\n";
const DEFAULTS: &str = "[composition]\n\
    theme={package='theme:base',palette='palette:night'}\n\
    [composition.plugins]\n\
    'capabilities'={state='enabled'}\n\
    [composition.capabilities]\n\
    'site.search/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='search'},requirement='required'}\n\
    'site.graph/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='graph'},requirement='optional'}\n\
    'site.feeds/v1'={state='disabled'}\n";

fn key(value: &str) -> Key {
    value.parse().expect("fixture exact key")
}

fn capability(value: &str) -> CapabilityId {
    value.parse().expect("fixture capability")
}

fn theme_package(value: &str) -> ThemePackageId {
    value.parse().expect("fixture Theme package")
}

fn plugin_package(value: &str) -> PluginPackageId {
    value.parse().expect("fixture plugin package")
}

fn provider(value: &str) -> ProviderId {
    value.parse().expect("fixture provider")
}

fn resource(value: &str) -> ResourceId {
    value.parse().expect("fixture resource")
}

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, toml::Limits::default()).expect("complete composition TOML")
}

fn origin(scope: SelectionScope, field: SelectionField) -> SelectionOrigin {
    SelectionOrigin { scope, field }
}

fn invalid(source: &str) -> Vec<workspace::Diagnostic> {
    match workspace::compile(source, toml::Limits::default())
        .expect_err("invalid complete composition")
    {
        workspace::Error::Invalid(errors) => errors,
        other => panic!("expected semantic reference/scope failures, got {other:?}"),
    }
}

#[test]
fn workspace_defaults_resolve_for_every_site_and_preserve_selection_origins() {
    let source = [DEFAULTS, OWNER, ALPHA, BETA, SOURCES].concat();
    let compiled = compile(&source);
    assert_eq!(compiled.source_sha256, Sha256Digest::of(source.as_bytes()));
    for site in ["site:alpha", "site:beta"] {
        let composition = compiled
            .workspace
            .composition_for(&key(site))
            .expect("every declared site");
        let theme = composition.theme().expect("inherited theme");
        assert_eq!(
            theme.value(),
            &ThemeSelection {
                package: theme_package("theme:base"),
                palette: Some(key("palette:night"))
            }
        );
        assert_eq!(
            theme.origin(),
            &origin(SelectionScope::Workspace, SelectionField::Theme {})
        );
        assert_eq!(composition.capabilities().len(), 3);
        let search = &composition.capabilities()[&capability("site.search/v1")];
        assert_eq!(
            search.value(),
            &CapabilitySelection::Enabled {
                provider: ProviderRef::Plugin {
                    package: plugin_package("capabilities"),
                    provider: provider("search")
                },
                requirement: Requirement::Required,
                allow_absence: None,
            }
        );
        let graph = &composition.capabilities()[&capability("site.graph/v1")];
        assert_eq!(
            graph.value(),
            &CapabilitySelection::Enabled {
                provider: ProviderRef::Plugin {
                    package: plugin_package("capabilities"),
                    provider: provider("graph")
                },
                requirement: Requirement::Optional,
                allow_absence: None,
            }
        );
        for (capability, selected) in composition.capabilities() {
            assert_eq!(
                selected.origin(),
                &origin(
                    SelectionScope::Workspace,
                    SelectionField::Capability {
                        capability: capability.clone()
                    }
                )
            );
        }
        assert_eq!(
            composition.capabilities()[&capability("site.feeds/v1")].value(),
            &CapabilitySelection::Disabled {}
        );
    }
    assert!(
        compiled
            .workspace
            .composition_for(&key("site:missing"))
            .is_none()
    );
    assert!(
        compiled
            .workspace
            .composition_for(&key("owner:one"))
            .is_none()
    );
    assert!(
        !compiled
            .workspace
            .site(&key("site:alpha"))
            .expect("nondeployable still checked")
            .deployable
    );
}

#[test]
fn site_chrome_copy_inherits_independently_from_ordered_navigation() {
    let chrome = chrome::CHROME;
    let compiled = compile(&[chrome, OWNER, ALPHA, BETA].concat());
    for site in ["site:alpha", "site:beta"] {
        let selected = compiled
            .workspace
            .composition_for(&key(site))
            .and_then(|composition| composition.chrome())
            .expect("inherited chrome");
        assert_eq!(
            selected.origin(),
            Some(&origin(
                SelectionScope::Workspace,
                SelectionField::Chrome {}
            ))
        );
        assert_eq!(
            compiled
                .workspace
                .composition_for(&key(site))
                .unwrap()
                .navigation()
                .unwrap()
                .value()
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Beta"]
        );
        assert_eq!(
            selected
                .value()
                .shell
                .search
                .as_ref()
                .and_then(|search| search.no_script_destination.as_ref())
                .map(Key::as_str),
            Some("content:search")
        );
    }
}

#[test]
fn site_override_replaces_whole_bindings_and_clears_omitted_palette() {
    let overrides = "[sites.composition]\n\
        theme={package='theme:replacement'}\n\
        [sites.composition.capabilities]\n\
        'site.search/v1'={state='disabled'}\n\
        'site.graph/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='local'},requirement='required'}\n\
        'site.feeds/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='feeds'},requirement='optional'}\n";
    let compiled = compile(&[DEFAULTS, OWNER, ALPHA, overrides, BETA, SOURCES].concat());
    let alpha = compiled
        .workspace
        .composition_for(&key("site:alpha"))
        .expect("overridden site");
    let theme = alpha.theme().expect("replacement theme");
    assert_eq!(
        theme.value(),
        &ThemeSelection {
            package: theme_package("theme:replacement"),
            palette: None
        }
    );
    assert_eq!(
        theme.origin(),
        &origin(
            SelectionScope::Site(key("site:alpha")),
            SelectionField::Theme {}
        )
    );
    assert_eq!(
        alpha.capabilities()[&capability("site.search/v1")].value(),
        &CapabilitySelection::Disabled {}
    );
    assert_eq!(
        alpha.capabilities()[&capability("site.graph/v1")].value(),
        &CapabilitySelection::Enabled {
            provider: ProviderRef::Plugin {
                package: plugin_package("capabilities"),
                provider: provider("local")
            },
            requirement: Requirement::Required,
            allow_absence: None,
        }
    );
    assert_eq!(
        alpha.capabilities()[&capability("site.feeds/v1")].value(),
        &CapabilitySelection::Enabled {
            provider: ProviderRef::Plugin {
                package: plugin_package("capabilities"),
                provider: provider("feeds")
            },
            requirement: Requirement::Optional,
            allow_absence: None,
        }
    );
    for (capability, selected) in alpha.capabilities() {
        assert_eq!(
            selected.origin(),
            &origin(
                SelectionScope::Site(key("site:alpha")),
                SelectionField::Capability {
                    capability: capability.clone()
                }
            )
        );
    }
    let beta = compiled
        .workspace
        .composition_for(&key("site:beta"))
        .expect("independent inherited site");
    assert_eq!(
        beta.theme().expect("default unchanged").value().palette,
        Some(key("palette:night"))
    );
    assert!(matches!(
        beta.capabilities()[&capability("site.search/v1")].value(),
        CapabilitySelection::Enabled {
            requirement: Requirement::Required,
            ..
        }
    ));
}

#[test]
fn absent_site_fields_inherit_but_explicit_disabled_is_not_absence() {
    let override_one = "[sites.composition.capabilities]\n'site.graph/v1'={state='disabled'}\n";
    let compiled = compile(&[DEFAULTS, OWNER, ALPHA, override_one, SOURCES].concat());
    let selected = compiled
        .workspace
        .composition_for(&key("site:alpha"))
        .expect("single-site workspace");
    assert_eq!(
        selected
            .theme()
            .expect("missing theme inherits")
            .origin()
            .scope,
        SelectionScope::Workspace
    );
    assert_eq!(selected.capabilities().len(), 3);
    assert_eq!(
        selected.capabilities()[&capability("site.graph/v1")].value(),
        &CapabilitySelection::Disabled {}
    );
    assert_eq!(
        selected.capabilities()[&capability("site.graph/v1")]
            .origin()
            .scope,
        SelectionScope::Site(key("site:alpha"))
    );
    assert_eq!(
        selected.capabilities()[&capability("site.search/v1")]
            .origin()
            .scope,
        SelectionScope::Workspace
    );
    let empty = compile(&[OWNER, ALPHA].concat());
    let empty = empty
        .workspace
        .composition_for(&key("site:alpha"))
        .expect("known site even with no selection");
    assert!(empty.theme().is_none());
    assert!(empty.capabilities().is_empty());
}

#[test]
fn capability_entries_are_not_partially_merged_with_workspace_defaults() {
    for partial in [
        "'site.search/v1'={state='enabled',requirement='optional'}\n",
        "'site.search/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='replacement'}}\n",
        "'site.search/v1'={state='enabled',provider={kind='plugin',provider='replacement'},requirement='optional'}\n",
        "'site.search/v1'={state='disabled',requirement='required'}\n",
    ] {
        let source = [
            DEFAULTS,
            OWNER,
            ALPHA,
            "[sites.composition.capabilities]\n",
            partial,
            SOURCES,
        ]
        .concat();
        assert!(
            matches!(
                workspace::compile(&source, toml::Limits::default()),
                Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
            ),
            "incomplete or stray fields cannot be filled by an inherited entry: {partial}"
        );
    }
    for theme in ["{}", "{palette='palette:day'}"] {
        let source = [
            DEFAULTS,
            OWNER,
            ALPHA,
            &format!("[sites.composition]\ntheme={theme}\n"),
            SOURCES,
        ]
        .concat();
        assert!(
            matches!(
                workspace::compile(&source, toml::Limits::default()),
                Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
            ),
            "theme replacement must provide its own complete binding, not fill from defaults: {theme}"
        );
    }
}

#[test]
fn package_references_are_checked_in_overridden_defaults_and_nondeployable_sites() {
    let defaults = "[composition]\n\
        theme={package='theme:invalid-default'}\n\
        [composition.capabilities]\n\
        'site.graph/v1'={state='enabled',provider={kind='plugin',package='missing-default-plugin',provider='graph'},requirement='optional'}\n";
    let overrides = "[sites.composition]\n\
        theme={package='theme:local'}\n\
        [sites.composition.capabilities]\n\
        'site.graph/v1'={state='disabled'}\n\
        'site.local/v1'={state='enabled',provider={kind='plugin',package='missing-local-plugin',provider='local'},requirement='required'}\n";
    let errors = invalid(&[defaults, OWNER, ALPHA, overrides, SOURCES].concat());
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_theme_package_reference")
    );
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.code == "workspace_plugin_package_reference")
            .count(),
        2
    );
    assert_eq!(errors.len(), 3);
    assert!(
        errors.windows(2).all(|pair| pair[0] < pair[1]),
        "errors are sorted and unique"
    );
}

#[test]
fn plugin_provider_selection_does_not_depend_on_theme_selection() {
    let defaults = "[composition.plugins]\n'capabilities'={state='enabled'}\n[composition.capabilities]\n'site.search/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='search'},requirement='optional'}\n";
    let valid = compile(&[defaults, OWNER, ALPHA, BETA, SOURCES].concat());
    for site in ["site:alpha", "site:beta"] {
        let composition = valid.workspace.composition_for(&key(site)).expect("site");
        assert!(composition.theme().is_none());
        assert!(matches!(
            composition.capabilities()[&capability("site.search/v1")].value(),
            CapabilitySelection::Enabled { .. }
        ));
    }
    let disabled = "[sites.composition.capabilities]\n'site.search/v1'={state='disabled'}\n";
    let valid = compile(&[defaults, OWNER, ALPHA, BETA, disabled, SOURCES].concat());
    assert_eq!(
        valid
            .workspace
            .composition_for(&key("site:beta"))
            .expect("disabled override")
            .capabilities()[&capability("site.search/v1")]
            .value(),
        &CapabilitySelection::Disabled {}
    );
}

#[test]
fn provider_owner_is_explicit_and_only_plugin_owners_require_a_package() {
    let gazette = "[composition.capabilities]\n\
        'site.search/v1'={state='enabled',provider={kind='gazette',provider='search'},requirement='required'}\n";
    let compiled = compile(&[gazette, OWNER, ALPHA].concat());
    assert_eq!(
        compiled
            .workspace
            .composition_for(&key("site:alpha"))
            .expect("declared site")
            .capabilities()[&capability("site.search/v1")]
            .value(),
        &CapabilitySelection::Enabled {
            provider: ProviderRef::Gazette {
                provider: provider("search")
            },
            requirement: Requirement::Required,
            allow_absence: None,
        }
    );

    let plugin = "[composition.capabilities]\n\
        'site.search/v1'={state='enabled',provider={kind='plugin',package='missing',provider='search'},requirement='required'}\n";
    assert!(
        invalid(&[plugin, OWNER, ALPHA].concat())
            .iter()
            .any(|error| error.code == "workspace_plugin_package_reference")
    );

    for provider in [
        "{provider='search'}",
        "{kind='gazette',package='capabilities',provider='search'}",
        "{kind='plugin',provider='search'}",
        "{kind='unknown',provider='search'}",
    ] {
        let source = format!(
            "[composition.capabilities]\n'site.search/v1'={{state='enabled',provider={provider},requirement='required'}}\n{OWNER}{ALPHA}"
        );
        assert!(
            matches!(
                workspace::compile(&source, toml::Limits::default()),
                Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
            ),
            "provider ownership requires exactly one admitted tagged shape: {provider}"
        );
    }
}

#[test]
fn provider_reference_schema_exposes_both_explicit_owner_shapes() {
    let schema = serde_json::to_string(&schemars::schema_for!(ProviderRef))
        .expect("ProviderRef JSON Schema");
    for contract in ["kind", "gazette", "plugin", "package", "provider"] {
        assert!(
            schema.contains(contract),
            "schema must expose {contract}: {schema}"
        );
    }
    assert!(
        !schema.contains("CapabilityProviderRef"),
        "the unpublished plugin-only shape has no compatibility schema"
    );
}

#[test]
fn provider_identity_cannot_serialize_contribution_scope_or_source() {
    for provider in [
        "{kind='plugin',package='capabilities',provider='local',scope='site'}",
        "{kind='plugin',package='capabilities',provider='local',source='source:provider'}",
    ] {
        let source = format!(
            "[composition.capabilities]\n'site.graph/v1'={{state='enabled',provider={provider},requirement='required'}}\n{OWNER}{ALPHA}{SOURCES}"
        );
        assert!(matches!(
            workspace::compile(&source, toml::Limits::default()),
            Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
        ));
    }
}

#[test]
fn strict_schema_rejects_unknowns_typos_and_provider_field_leakage() {
    for root in [
        "[composition]\npallete='palette:typo'\n",
        "[composition]\ntheme={package='theme:base',unknown=true}\n",
        "[composition.capabilities]\n'site.graph/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='graph',source='source:theme'},requirement='required'}\n",
        "[composition.capabilities]\n'site.graph/v1'={state='disable'}\n",
        "[composition.capabilities]\n'site.graph/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='graph'},requirement='prefer'}\n",
    ] {
        let source = [root, OWNER, ALPHA, SOURCES].concat();
        assert!(
            matches!(
                workspace::compile(&source, toml::Limits::default()),
                Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
            ),
            "strict composition profile: {root}"
        );
    }
}

#[test]
fn package_catalogs_reject_duplicate_identities_unknown_sources_and_legacy_source_manifests() {
    let additions = "[[theme_packages]]\npackage='theme:base'\nsource='source:missing-theme'\nmanifest='duplicate.toml'\n\
        [[plugin_packages]]\npackage='capabilities'\nsource='source:missing-plugin'\nmanifest='duplicate.toml'\n";
    let errors = invalid(&[OWNER, ALPHA, SOURCES, additions].concat());
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.code == "workspace_duplicate")
            .count(),
        2
    );
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.code == "workspace_reference")
            .count(),
        2
    );

    let legacy = "[[sources]]\nid='source:legacy'\npresentation_manifest='theme.toml'\n\
        location={kind='filesystem',root='.'}\n";
    assert!(matches!(
        workspace::compile(&[OWNER, ALPHA, legacy].concat(), toml::Limits::default()),
        Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
    ));
}

#[test]
fn declaration_order_changes_source_identity_not_resolved_composition() {
    let first = [DEFAULTS, OWNER, ALPHA, BETA, SOURCES].concat();
    let second = [DEFAULTS, SOURCES, OWNER, BETA, ALPHA].concat();
    let first = compile(&first);
    let second = compile(&second);
    assert_ne!(first.source_sha256, second.source_sha256);
    assert_eq!(first.workspace, second.workspace);
    assert_eq!(
        serde_json::to_value(&first.workspace).expect("shared workspace projection"),
        serde_json::to_value(&second.workspace).expect("reordered workspace projection")
    );
}

#[test]
fn inherited_composition_fanout_respects_independent_derived_resource_limits() {
    let mut source = String::from("[composition.capabilities]\n");
    for index in 0..30 {
        source.push_str(&format!("'site.item{index}/v1'={{state='disabled'}}\n"));
    }
    source.push_str(OWNER);
    for index in 0..30 {
        source.push_str(&format!(
            "[[sites]]\nid='site:item{index}'\npublisher='owner:one'\ndeployable=false\n"
        ));
    }
    let parsed = toml::ParsedToml::parse(&source, toml::Limits::default())
        .expect("bounded declaration syntax");
    let syntax_nodes = parsed.node_count();
    assert!(
        syntax_nodes < 900,
        "fixture must expand beyond its input syntax size"
    );
    let compiled = compile(&source);
    let selection_count: usize = compiled
        .workspace
        .sites()
        .map(|site| {
            compiled
                .workspace
                .composition_for(&site.id)
                .expect("every site")
                .capabilities()
                .len()
        })
        .sum();
    assert_eq!(
        selection_count, 900,
        "disabled selections are retained, not discarded to evade budgets"
    );
    assert!(matches!(
        workspace::compile(
            &source,
            toml::Limits {
                nodes: syntax_nodes,
                ..toml::Limits::default()
            }
        ),
        Err(workspace::Error::Limit("composition selections"))
    ));
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
fn duplicate_site_bodies_cannot_select_order_dependent_composition_diagnostics() {
    let bad = format!("{ALPHA}[sites.composition]\ntheme={{package='theme:missing'}}\n");
    let first = invalid(&[OWNER, ALPHA, &bad, SOURCES].concat());
    let second = invalid(&[OWNER, &bad, ALPHA, SOURCES].concat());
    assert_eq!(first, second);
    assert_eq!(
        first,
        vec![workspace::Diagnostic {
            code: "workspace_duplicate",
            field: "sites".into(),
            key: Some(key("site:alpha")),
        }]
    );
}

#[test]
fn exact_absence_permission_inherits_only_with_the_whole_capability_atom() {
    let defaults = "[composition.plugins]\n'capabilities'={state='enabled'}\n[composition.capabilities]\n'site.graph/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='graph'},requirement='optional',allow_absence='no-native-acquisition'}\n";
    for explicit in [false, true] {
        let permission = if explicit {
            ",allow_absence='no-native-acquisition'"
        } else {
            ""
        };
        let replacement = format!(
            "[sites.composition.capabilities]\n'site.graph/v1'={{state='enabled',provider={{kind='plugin',package='capabilities',provider='local'}},requirement='optional'{permission}}}\n"
        );
        let compiled = compile(&[defaults, OWNER, ALPHA, &replacement, BETA, SOURCES].concat());
        for (site, permitted) in [("site:alpha", explicit), ("site:beta", true)] {
            let selected = &compiled
                .workspace
                .composition_for(&key(site))
                .expect("declared site")
                .capabilities()[&capability("site.graph/v1")];
            let CapabilitySelection::Enabled { allow_absence, .. } = selected.value() else {
                panic!("enabled fixture atom")
            };
            assert_eq!(
                *allow_absence,
                permitted.then_some(CapabilityAbsence::NoNativeAcquisition)
            );
        }
    }
}

#[test]
fn absence_permission_is_strict_and_never_turns_a_required_capability_optional() {
    let document = |atom: &str| {
        format!(
            "[composition.plugins]\n'capabilities'={{state='enabled'}}\n[composition.capabilities]\n'site.graph/v1'={{{atom}}}\n{OWNER}{ALPHA}{SOURCES}"
        )
    };
    let atom = "state='enabled',provider={kind='plugin',package='capabilities',provider='graph'},requirement='optional',allow_absence='no-native-acquisition'";
    compile(&document(atom));
    let errors = invalid(&document(
        &atom.replace("requirement='optional'", "requirement='required'"),
    ));
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_capability_absence"),
        "{errors:?}"
    );
    for value in [
        "'no-artifact'",
        "'any-error'",
        "true",
        "['no-native-acquisition']",
        "{no-native-acquisition={}}",
    ] {
        let source = document(&atom.replace("'no-native-acquisition'", value));
        assert!(
            workspace::compile(&source, Default::default()).is_err(),
            "invalid absence representation {value}"
        );
    }
    assert!(
        workspace::compile(
            &document("state='disabled',allow_absence='no-native-acquisition'"),
            Default::default()
        )
        .is_err(),
        "disabled cannot carry enabled fallback policy"
    );
    let legacy_artifact = "state='enabled',provider={kind='plugin',package='capabilities',provider='graph'},requirement='optional',artifact={resource='module:graph',output='/graph.js'}";
    assert!(matches!(
        workspace::compile(&document(legacy_artifact), Default::default()),
        Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
    ));
}

#[test]
fn page_template_maps_inherit_atomically_and_keep_configuration_origins() {
    let source = format!(
        "{OWNER}\n[composition.page_templates]\n'content:one'='base/one.html'\n'content:two'='base/two.html'\n{ALPHA}\n{BETA}\n[sites.composition.page_templates]\n'content:replacement'='base/replacement.html'\n"
    );
    let compiled = compile(&source);
    let alpha = compiled
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap()
        .page_templates()
        .unwrap();
    assert_eq!(alpha.value().len(), 2);
    assert_eq!(alpha.value()[&key("content:one")].as_str(), "base/one.html");
    assert_eq!(
        alpha.origin(),
        &origin(SelectionScope::Workspace, SelectionField::PageTemplates {})
    );
    let beta = compiled
        .workspace
        .composition_for(&key("site:beta"))
        .unwrap()
        .page_templates()
        .unwrap();
    assert_eq!(
        beta.value().len(),
        1,
        "site map replaces rather than merges defaults"
    );
    assert_eq!(
        beta.origin(),
        &origin(
            SelectionScope::Site(key("site:beta")),
            SelectionField::PageTemplates {}
        )
    );
    let cleared = compile(&format!(
        "{OWNER}\n[composition.page_templates]\n'content:one'='base/one.html'\n{ALPHA}\n[sites.composition]\npage_templates={{}}\n"
    ));
    assert!(
        cleared
            .workspace
            .composition_for(&key("site:alpha"))
            .unwrap()
            .page_templates()
            .unwrap()
            .value()
            .is_empty()
    );
}

#[test]
fn page_template_bindings_reject_non_relative_entries() {
    for entry in ["../outside.html", "/absolute.html", ""] {
        let source =
            format!("{OWNER}{ALPHA}[sites.composition.page_templates]\n'content:one'='{entry}'\n");
        assert!(workspace::compile(&source, toml::Limits::default()).is_err());
    }
}

#[test]
fn page_template_origin_points_to_the_composing_field() {
    let source = format!(
        "{OWNER}{ALPHA}[sites.composition.page_templates]\n'content:one'='base/one.html'\n"
    );
    let parsed = workspace::ParsedWorkspace::parse(&source, toml::Limits::default()).unwrap();
    let origin = parsed
        .composition_origin(&workspace::CompositionOriginQuery {
            site: key("site:alpha"),
            field: SelectionField::PageTemplates {},
            path: vec![toml::PathSegment::Key("content:one".into())],
        })
        .unwrap();
    let span = origin.span;
    assert_eq!(&source[span.start..span.end], "'base/one.html'");
}

#[test]
fn short_theme_origins_keep_the_authored_coordinates_in_each_document_form() {
    for theme in [
        "'theme:base'",
        "{package='theme:base',palette='palette:night'}",
    ] {
        let source = format!("{OWNER}{SOURCES}{ALPHA}theme={theme}\n");
        let query = workspace::CompositionOriginQuery {
            site: key("site:alpha"),
            field: SelectionField::Theme {},
            path: vec![toml::PathSegment::Key("package".into())],
        };
        let parsed = workspace::ParsedWorkspace::parse(&source, Default::default()).unwrap();
        let origin = parsed.composition_origin(&query).unwrap();
        assert_eq!(&source[origin.span.start..origin.span.end], "'theme:base'");
        assert!(
            !origin
                .path
                .contains(&toml::PathSegment::Key("composition".into()))
        );

        let root = "gazette.toml".parse().unwrap();
        let sites = "sites.toml".parse().unwrap();
        let parsed = workspace::ParsedWorkspaceDocuments::parse(
            &root,
            [
                (&root, b"includes=['sites.toml']\n".as_slice()),
                (&sites, source.as_bytes()),
            ],
            Default::default(),
        )
        .unwrap();
        let origin = parsed.composition_origin(&query).unwrap();
        assert_eq!(origin.document, sites);
        assert_eq!(&source[origin.span.start..origin.span.end], "'theme:base'");
        let palette = workspace::CompositionOriginQuery {
            path: vec![toml::PathSegment::Key("palette".into())],
            ..query
        };
        if theme.starts_with('{') {
            let origin = parsed.composition_origin(&palette).unwrap();
            assert_eq!(
                &source[origin.span.start..origin.span.end],
                "'palette:night'"
            );
        } else {
            assert!(parsed.composition_origin(&palette).is_err());
        }
    }
}

#[test]
fn invitation_selection_inherits_as_a_whole_and_retains_its_origin() {
    let source = [
        "[composition.invitations.'theme:services']\nstate='enabled'\ndestination='content:services'\narticles=true\npages={'content:about'=true}\n",
        OWNER, ALPHA,
        "[sites.composition.invitations.'theme:services']\nstate='disabled'\n",
        BETA,
    ].concat();
    let parsed = workspace::ParsedWorkspace::parse(&source, toml::Limits::default()).unwrap();
    let alpha = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    assert!(matches!(
        alpha.invitations()[&key("theme:services")].value(),
        workspace::InvitationSelection::Disabled {}
    ));
    let beta = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:beta"))
        .unwrap();
    assert!(matches!(
        beta.invitations()[&key("theme:services")].value(),
        workspace::InvitationSelection::Enabled { articles: true, .. }
    ));
    let origin = parsed
        .composition_origin(&workspace::CompositionOriginQuery {
            site: key("site:alpha"),
            field: SelectionField::Invitation {
                invitation: key("theme:services"),
            },
            path: vec![toml::PathSegment::Key("state".into())],
        })
        .unwrap();
    assert_eq!(&source[origin.span.start..origin.span.end], "'disabled'");
    assert_eq!(
        origin.selection.scope,
        SelectionScope::Site(key("site:alpha"))
    );
}
