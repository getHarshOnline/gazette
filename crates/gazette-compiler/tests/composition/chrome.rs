//! Copy inherits label by label while retaining each declaring source.

use super::*;
use gazette_compiler::workspace::{CompositionOriginQuery, ParsedWorkspace};

pub(super) const CHROME: &str = "[composition]\n\
        navigation=[{label='Alpha',destination='content:alpha'},{label='Beta',destination='content:beta'}]\n\
        [composition.chrome]\n\
        skip_to_content_label='Skip to content'\n\
        navigation_label='Primary navigation'\n\
        footer_label='Site footer'\n\
        [composition.chrome.shell]\n\
        close_label='Close'\n\
        [composition.chrome.shell.search]\n\
        trigger_label='Search'\n\
        title='Search this site'\n\
        field_label='Query'\n\
        loading_message='Loading'\n\
        empty_message='No results'\n\
        unavailable_message='Search unavailable'\n\
        no_script_destination='content:search'\n\
        [composition.chrome.shell.settings]\n\
        trigger_label='Settings'\n\
        title='Display settings'\n\
        color_mode_label='Color mode'\n\
        color_mode_system_label='System'\n\
        color_mode_light_label='Light'\n\
        color_mode_dark_label='Dark'\n\
        motion_label='Motion'\n\
        motion_system_label='System'\n\
        motion_reduce_label='Reduce'\n\
        motion_allow_label='Allow'\n";

#[test]
fn partial_site_copy_keeps_other_labels_and_reports_each_origin() {
    let source = [
        CHROME,
        "[composition.chrome.regions]\n'base:first'='First default'\n'base:second'='Second default'\n",
        OWNER,
        ALPHA,
        "[sites.composition.chrome]\nfooter_label='Local footer'\n[sites.composition.chrome.shell.settings]\ntitle='Local settings'\n[sites.composition.chrome.regions]\n'base:first'='Local first'\n",
        BETA,
    ].concat();
    let parsed = ParsedWorkspace::parse(&source, toml::Limits::default()).expect("partial copy");
    let composition = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    let value = serde_json::to_value(composition.chrome().unwrap().value()).unwrap();
    assert_eq!(value["footer_label"], "Local footer");
    assert_eq!(value["navigation_label"], "Primary navigation");
    assert_eq!(value["shell"]["settings"]["title"], "Local settings");
    assert_eq!(value["shell"]["settings"]["motion_label"], "Motion");
    assert_eq!(value["regions"]["base:first"], "Local first");
    assert_eq!(value["regions"]["base:second"], "Second default");
    for (path, spelling, scope) in [
        (
            vec!["footer_label"],
            "'Local footer'",
            SelectionScope::Site(key("site:alpha")),
        ),
        (
            vec!["navigation_label"],
            "'Primary navigation'",
            SelectionScope::Workspace,
        ),
        (
            vec!["shell", "settings", "title"],
            "'Local settings'",
            SelectionScope::Site(key("site:alpha")),
        ),
        (
            vec!["shell", "settings", "motion_label"],
            "'Motion'",
            SelectionScope::Workspace,
        ),
        (
            vec!["regions", "base:first"],
            "'Local first'",
            SelectionScope::Site(key("site:alpha")),
        ),
        (
            vec!["regions", "base:second"],
            "'Second default'",
            SelectionScope::Workspace,
        ),
    ] {
        let query = CompositionOriginQuery {
            site: key("site:alpha"),
            field: SelectionField::Chrome {},
            path: path
                .into_iter()
                .map(|key| toml::PathSegment::Key(key.into()))
                .collect(),
        };
        let origin = parsed.composition_origin(&query).expect("declaring label");
        assert_eq!(&source[origin.span.start..origin.span.end], spelling);
        assert_eq!(origin.selection.scope, scope);
    }
    assert!(
        parsed
            .composition_origin(&CompositionOriginQuery {
                site: key("site:alpha"),
                field: SelectionField::Chrome {},
                path: vec![],
            })
            .is_err(),
        "mixed-origin copy has no invented single source span"
    );
}

#[test]
fn explicit_empty_copy_is_invalid_instead_of_recovering_an_inherited_label() {
    let source = [
        CHROME,
        OWNER,
        ALPHA,
        "[sites.composition.chrome]\nfooter_label=''\n",
    ]
    .concat();
    assert!(workspace::compile(&source, toml::Limits::default()).is_err());
}

fn theme(copy: &str) -> gazette_compiler::theme_graph::EffectiveTheme {
    let source = format!(
        "package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={{page='page.html'}}\n{copy}"
    );
    let parsed =
        gazette_compiler::theme_package::ParsedManifest::parse(&source, toml::Limits::default())
            .unwrap();
    let graph = gazette_compiler::theme_graph::Graph::resolve(
        &theme_package("theme:base"),
        [parsed.manifest()],
    )
    .unwrap();
    gazette_compiler::theme_graph::EffectiveTheme::resolve(&graph).unwrap()
}

#[test]
fn theme_copy_fills_only_missing_labels_with_its_own_origins() {
    let theme = theme(
        "[chrome]\nskip_to_content_label='Theme skip'\nnavigation_label='Theme navigation'\nfooter_label='Theme footer'\n[chrome.regions]\n'base:first'='Theme first'\n'base:second'='Theme second'\n",
    );
    let compiled = compile(&[
        "[composition.chrome]\nnavigation_label='Workspace navigation'\n[composition.chrome.regions]\n'base:first'='Workspace first'\n",
        OWNER, ALPHA,
        "[sites.composition.chrome]\nfooter_label='Site footer'\n",
        BETA,
    ].concat());
    let selected = compiled
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    let merged = selected
        .with_theme_defaults(&theme, Default::default())
        .unwrap();
    let chrome = merged.chrome().unwrap();
    let value = chrome.value();
    assert_eq!(value.skip_to_content_label.as_deref(), Some("Theme skip"));
    assert_eq!(
        value.navigation_label.as_deref(),
        Some("Workspace navigation")
    );
    assert_eq!(value.footer_label.as_deref(), Some("Site footer"));
    assert_eq!(value.regions[&key("base:first")], "Workspace first");
    assert_eq!(value.regions[&key("base:second")], "Theme second");
    for (path, scope) in [
        (
            vec!["skip_to_content_label"],
            SelectionScope::Theme(theme_package("theme:base")),
        ),
        (vec!["navigation_label"], SelectionScope::Workspace),
        (
            vec!["footer_label"],
            SelectionScope::Site(key("site:alpha")),
        ),
        (
            vec!["regions", "base:second"],
            SelectionScope::Theme(theme_package("theme:base")),
        ),
    ] {
        let path: Vec<_> = path
            .into_iter()
            .map(|part| toml::PathSegment::Key(part.into()))
            .collect();
        assert_eq!(chrome.field_origin(&path).unwrap().scope, scope);
    }
    assert!(chrome.origin().is_none());
    assert!(
        selected
            .chrome()
            .unwrap()
            .value()
            .skip_to_content_label
            .is_none()
    );
    assert!(
        merged.capabilities().is_empty(),
        "copy does not select providers"
    );

    let bare = compile(&[OWNER, ALPHA].concat());
    let merged = bare
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap()
        .with_theme_defaults(&theme, Default::default())
        .unwrap();
    assert_eq!(
        merged.chrome().unwrap().origin().unwrap().scope,
        SelectionScope::Theme(theme_package("theme:base"))
    );
}

#[test]
fn disabled_search_keeps_its_explicit_selection_above_theme_copy() {
    let theme = theme("[chrome.shell.search]\ntrigger_label='Find'\ntitle='Find a page'\n");
    let compiled = compile(&[
        "[composition.chrome.shell.search]\nenabled=false\nno_script_destination='content:search'\n",
        OWNER, ALPHA, BETA,
        "[sites.composition.chrome.shell.search]\nenabled=true\n",
    ].concat());
    for (site, expected) in [("site:alpha", false), ("site:beta", true)] {
        let merged = compiled
            .workspace
            .composition_for(&key(site))
            .unwrap()
            .with_theme_defaults(&theme, Default::default())
            .unwrap();
        let search = merged
            .chrome()
            .unwrap()
            .value()
            .shell
            .search
            .as_ref()
            .unwrap();
        assert_eq!(search.enabled, Some(expected));
        assert_eq!(search.trigger_label.as_deref(), Some("Find"));
        assert_eq!(search.no_script_destination, Some(key("content:search")));
        assert!(merged.capabilities().is_empty());
    }
}

#[test]
fn combined_copy_obeys_region_and_expansion_limits() {
    let regions: String = (0..256).map(|n| format!("'base:r{n}'='Label'\n")).collect();
    let theme = theme(&format!("[chrome.regions]\n{regions}"));
    let compiled = compile(&[OWNER, ALPHA].concat());
    let bare = compiled
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    assert_eq!(
        bare.with_theme_defaults(&theme, Default::default())
            .unwrap()
            .chrome()
            .unwrap()
            .value()
            .regions
            .len(),
        256
    );
    for limits in [
        toml::Limits {
            nodes: 1,
            ..Default::default()
        },
        toml::Limits {
            source_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(bare.with_theme_defaults(&theme, limits).is_err());
    }
    let compiled = compile(
        &[
            OWNER,
            ALPHA,
            "[sites.composition.chrome.regions]\n'base:extra'='Extra'\n",
        ]
        .concat(),
    );
    assert!(
        compiled
            .workspace
            .composition_for(&key("site:alpha"))
            .unwrap()
            .with_theme_defaults(&theme, Default::default())
            .is_err()
    );

    let source = format!(
        "[composition.chrome.regions]\n{regions}{OWNER}{ALPHA}[sites.composition.chrome.regions]\n'base:extra'='Extra'\n"
    );
    assert!(workspace::compile(&source, Default::default()).is_err());
}
