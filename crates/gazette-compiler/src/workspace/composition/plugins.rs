//! Plugin packages are selected as whole atoms, independently from their exports.

use super::*;

pub(super) fn validate(
    composition: &Composition,
    scope: &SelectionScope,
    plugin_packages: &BTreeMap<PluginPackageId, PluginPackageBinding>,
    errors: &mut Vec<Diagnostic>,
) {
    let prefix = match scope {
        SelectionScope::Theme(package) => format!("themes[{package:?}]"),
        SelectionScope::Workspace => "composition".into(),
        SelectionScope::Site(site) => format!("sites[{:?}].composition", site.as_str()),
    };
    for (package, selection) in &composition.plugins {
        if matches!(selection, PluginSelection::Enabled {})
            && !plugin_packages.contains_key(package)
        {
            errors.push(Diagnostic {
                code: "workspace_plugin_package_reference",
                field: format!("{prefix}.plugins[{:?}]", package.as_str()),
                key: None,
            });
        }
    }
}

pub(super) fn resolve(
    defaults: &Composition,
    site: &Site,
    limits: crate::toml::Limits,
    used: &mut usize,
    text: &mut usize,
) -> Result<BTreeMap<PluginPackageId, Selected<PluginSelection>>, Error> {
    let local = &site.composition;
    let mut selected = BTreeMap::new();
    for (package, (value, scope)) in defaults
        .plugins
        .iter()
        .filter(|(package, _)| !local.plugins.contains_key(*package))
        .map(|(package, value)| (package, (value, SelectionScope::Workspace)))
        .chain(
            local
                .plugins
                .iter()
                .map(|(package, value)| (package, (value, SelectionScope::Site(site.id.clone())))),
        )
    {
        charge(used, 1, limits.nodes, "composition selections")?;
        for value in [site.id.as_str(), package.as_str()] {
            charge(
                text,
                value.len(),
                limits.source_bytes,
                "composition text bytes",
            )?;
        }
        selected.insert(
            package.clone(),
            Selected {
                value: value.clone(),
                origin: SelectionOrigin {
                    scope,
                    field: SelectionField::Plugin {
                        package: package.clone(),
                    },
                },
            },
        );
    }
    Ok(selected)
}
