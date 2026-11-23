//! Whole-atom resource selection uses the same origin layers and budgets as capabilities.

use super::*;

pub(super) fn validate(
    composition: &Composition,
    scope: &SelectionScope,
    theme_packages: &BTreeMap<ThemePackageId, ThemePackageBinding>,
    plugin_packages: &BTreeMap<PluginPackageId, PluginPackageBinding>,
    errors: &mut Vec<Diagnostic>,
) {
    let prefix = match scope {
        SelectionScope::Theme(package) => format!("themes[{package:?}]"),
        SelectionScope::Workspace => "composition".into(),
        SelectionScope::Site(site) => format!("sites[{:?}].composition", site.as_str()),
    };
    for (key, selection) in &composition.resources {
        let ResourceSelection::Enabled {
            resource, output, ..
        } = selection
        else {
            continue;
        };
        let field = format!("{prefix}.resources[{:?}]", key.as_str());
        let package_exists = match &resource.owner {
            ResourceOwner::Theme(package) => theme_packages.contains_key(package),
            ResourceOwner::Plugin(package) => plugin_packages.contains_key(package),
        };
        if !package_exists {
            errors.push(Diagnostic {
                code: "workspace_resource_package_reference",
                field: format!("{field}.resource.owner"),
                key: None,
            });
        }
        if output.as_str().ends_with('/') {
            errors.push(Diagnostic {
                code: "workspace_resource_output",
                field: format!("{field}.output"),
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
) -> Result<BTreeMap<ResourceId, Selected<ResourceSelection>>, Error> {
    let local = &site.composition;
    let mut selected = BTreeMap::new();
    for (key, (value, scope)) in defaults
        .resources
        .iter()
        .filter(|(key, _)| !local.resources.contains_key(*key))
        .map(|(key, value)| (key, (value, SelectionScope::Workspace)))
        .chain(
            local
                .resources
                .iter()
                .map(|(key, value)| (key, (value, SelectionScope::Site(site.id.clone())))),
        )
    {
        charge(used, 1, limits.nodes, "composition selections")?;
        for key in [site.id.as_str(), key.as_str()] {
            charge(
                text,
                key.len(),
                limits.source_bytes,
                "composition text bytes",
            )?;
        }
        if let ResourceSelection::Enabled {
            resource, output, ..
        } = value
        {
            charge(
                text,
                output.as_str().len(),
                limits.source_bytes,
                "composition text bytes",
            )?;
            let package = match &resource.owner {
                ResourceOwner::Theme(package) => package.as_str(),
                ResourceOwner::Plugin(package) => package.as_str(),
            };
            for value in [package, resource.resource.as_str()] {
                charge(
                    text,
                    value.len(),
                    limits.source_bytes,
                    "composition text bytes",
                )?;
            }
        }
        selected.insert(
            key.clone(),
            Selected {
                value: value.clone(),
                origin: SelectionOrigin {
                    scope,
                    field: SelectionField::Resource {
                        resource: key.clone(),
                    },
                },
            },
        );
    }
    Ok(selected)
}

impl ResolvedComposition {
    /// Add defaults from the selected Theme without replacing workspace or site choices.
    /// Explicitly disabled resources continue to block inheritance.
    ///
    /// # Errors
    /// Rejects a combined selection exceeding the supplied node or text limits.
    pub fn with_theme_defaults(
        &self,
        theme: &crate::theme_graph::EffectiveTheme,
        limits: crate::toml::Limits,
    ) -> Result<Self, Error> {
        let mut nodes = 0;
        let mut text = 0;
        for (id, selected) in &self.resources {
            charge(&mut nodes, 1, limits.nodes, "composition selections")?;
            charge(
                &mut text,
                id.as_str().len(),
                limits.source_bytes,
                "composition text bytes",
            )?;
            if let ResourceSelection::Enabled {
                resource, output, ..
            } = selected.value()
            {
                let owner = match &resource.owner {
                    ResourceOwner::Theme(package) => package.as_str(),
                    ResourceOwner::Plugin(package) => package.as_str(),
                };
                for value in [owner, resource.resource.as_str(), output.as_str()] {
                    charge(
                        &mut text,
                        value.len(),
                        limits.source_bytes,
                        "composition text bytes",
                    )?;
                }
            }
        }
        for (id, default) in theme
            .resources()
            .iter()
            .filter(|(id, _)| !self.resources.contains_key(*id))
        {
            charge(&mut nodes, 1, limits.nodes, "composition selections")?;
            for value in [
                id.as_str(),
                default.package.as_str(),
                id.as_str(),
                default.output.as_str(),
            ] {
                charge(
                    &mut text,
                    value.len(),
                    limits.source_bytes,
                    "composition text bytes",
                )?;
            }
        }
        let chrome = chrome::inherit_theme(self.chrome.as_ref(), theme, limits)?;
        let mut resolved = self.clone();
        resolved.chrome = chrome;
        for (id, default) in theme.resources() {
            resolved
                .resources
                .entry(id.clone())
                .or_insert_with(|| Selected {
                    value: ResourceSelection::Enabled {
                        resource: ResourceRef {
                            owner: ResourceOwner::Theme(default.package.clone()),
                            resource: id.clone(),
                        },
                        output: default.output.clone(),
                        use_: default.use_,
                    },
                    origin: SelectionOrigin {
                        scope: SelectionScope::Theme(default.package.clone()),
                        field: SelectionField::Resource {
                            resource: id.clone(),
                        },
                    },
                });
        }
        Ok(resolved)
    }
}
