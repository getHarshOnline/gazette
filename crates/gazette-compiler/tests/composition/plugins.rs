//! Plugin activation inherits independently from package catalog bindings and exports.

use super::*;
use gazette_compiler::workspace::{CompositionOriginQuery, ParsedWorkspace, PluginSelection};

const PLUGINS: &str = "[composition.plugins]\n\
    'component-plugin'={state='enabled'}\n\
    'extension-plugin'={state='enabled'}\n";

const PLUGIN_BINDINGS: &str = "[[plugin_packages]]\n\
    package='component-plugin'\nsource='source:provider'\nmanifest='component.toml'\n\
    [[plugin_packages]]\npackage='extension-plugin'\nsource='source:provider'\nmanifest='extension.toml'\n\
    [[plugin_packages]]\npackage='unselected-plugin'\nsource='source:provider'\nmanifest='unselected.toml'\n";

#[test]
fn plugin_packages_inherit_override_and_explain_the_exact_activation() {
    let source = [
        "[composition]\ntheme={package='theme:base'}\n",
        PLUGINS,
        OWNER,
        ALPHA,
        "[sites.composition.plugins]\n'extension-plugin'={state='disabled'}\n",
        BETA,
        SOURCES,
        PLUGIN_BINDINGS,
    ]
    .concat();
    let parsed = ParsedWorkspace::parse(&source, toml::Limits::default())
        .expect("native plugin-package selections");

    let alpha = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:alpha"))
        .expect("alpha");
    assert_eq!(alpha.plugins().len(), 2);
    assert_eq!(
        alpha.plugins()[&plugin_package("component-plugin")].value(),
        &PluginSelection::Enabled {}
    );
    assert_eq!(
        alpha.plugins()[&plugin_package("component-plugin")]
            .origin()
            .scope,
        SelectionScope::Workspace
    );
    assert_eq!(
        alpha.plugins()[&plugin_package("extension-plugin")].value(),
        &PluginSelection::Disabled {}
    );
    assert_eq!(
        alpha.plugins()[&plugin_package("extension-plugin")]
            .origin()
            .scope,
        SelectionScope::Site(key("site:alpha"))
    );
    assert!(
        !alpha
            .plugins()
            .contains_key(&plugin_package("unselected-plugin")),
        "catalog membership is not activation"
    );

    let beta = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:beta"))
        .expect("beta");
    assert!(matches!(
        beta.plugins()[&plugin_package("extension-plugin")].value(),
        PluginSelection::Enabled {}
    ));

    let query = CompositionOriginQuery {
        site: key("site:alpha"),
        field: SelectionField::Plugin {
            package: plugin_package("extension-plugin"),
        },
        path: vec![gazette_compiler::toml::PathSegment::Key("state".into())],
    };
    let origin = parsed
        .composition_origin(&query)
        .expect("site-declared plugin activation origin");
    assert_eq!(&source[origin.span.start..origin.span.end], "'disabled'");
    assert!(
        origin.path.iter().any(|segment| {
            segment == &gazette_compiler::toml::PathSegment::Key("plugins".into())
        })
    );
}

#[test]
fn enabled_plugin_requires_a_catalog_binding_and_exports_do_not_imply_activation() {
    let missing = [
        "[composition]\ntheme={package='theme:base'}\n[composition.plugins]\n'missing-plugin'={state='enabled'}\n",
        OWNER,
        ALPHA,
        SOURCES,
    ]
    .concat();
    assert!(invalid(&missing).iter().any(|diagnostic| {
        diagnostic.code == "workspace_plugin_package_reference"
            && diagnostic.field == "composition.plugins[\"missing-plugin\"]"
    }));

    let inferred = [DEFAULTS, OWNER, ALPHA, SOURCES].concat().replace(
        "[composition.plugins]\n'capabilities'={state='enabled'}\n",
        "",
    );
    assert!(invalid(&inferred).iter().any(|diagnostic| {
        diagnostic.code == "workspace_plugin_package_selection"
            && diagnostic.field.contains("capabilities")
    }));

    let old_shape = [
        "[composition]\ntheme={package='theme:base'}\n[composition.plugin_packages]\ncapabilities={state='enabled'}\n",
        OWNER,
        ALPHA,
        SOURCES,
    ]
    .concat();
    assert!(workspace::compile(&old_shape, toml::Limits::default()).is_err());
}
