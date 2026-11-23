//! Typed selection inheritance; discovery cannot replace a selected provider.

use super::{Diagnostic, Error, Key, PluginPackageBinding, Site, ThemePackageBinding};
use crate::theme_package::ThemePackageId;
use gazette_core::{
    plugin::PluginPackageId,
    publication::{
        CapabilityId,
        capability_runtime::{CapabilityAbsence, ProviderRef},
    },
    resource::ResourceId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod chrome;
mod invitations;
mod plugins;
mod reading_assistance;
mod resources;

pub use chrome::{ResolvedChrome, SearchControlCopy, SettingsControlCopy, ShellCopy, SiteChrome};
pub use invitations::InvitationSelection;
pub use reading_assistance::{ReadingAssistance, ReadingAssistanceProvider};

/// Workspace defaults or site declarations; omission inherits only at the site layer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    /// Whole Theme binding; site replacement never retains another Theme's palette.
    pub theme: Option<ThemeSelection>,
    /// Interface copy inherited label by label from workspace and admitted Theme defaults.
    pub chrome: Option<SiteChrome>,
    /// Exact content identities mapped to retained Theme entries. Omission inherits
    /// the complete workspace map; an explicit empty map clears it for this site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_templates: Option<BTreeMap<Key, gazette_core::source::EntryPath>>,
    /// Ordered navigation; omission inherits, while an explicit empty list clears it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_destinations"
    )]
    pub navigation: Option<Vec<NavigationEntry>>,
    /// Ordered footer destinations, independently inherited from navigation and chrome copy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_destinations"
    )]
    pub footer: Option<Vec<NavigationEntry>>,
    /// Page invitations selected independently from navigation and footer destinations.
    #[serde(default)]
    pub invitations: BTreeMap<Key, InvitationSelection>,
    /// Independently selected capability contracts; explicit disabled entries survive resolution.
    #[serde(default)]
    pub capabilities: BTreeMap<CapabilityId, CapabilitySelection>,
    /// Independently selected plugin packages and their site inheritance state.
    #[serde(default)]
    pub plugins: BTreeMap<PluginPackageId, PluginSelection>,
    /// Independently selected package resources and their site contribution behavior.
    #[serde(default)]
    pub resources: BTreeMap<ResourceId, ResourceSelection>,
}

/// One ordered site-navigation label bound to an existing Gazette content identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigationEntry {
    /// Optional Theme-owned semantic address, independent of visible wording and destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    /// Workspace-owned visible label; it never substitutes for destination title.
    pub label: String,
    /// Existing content identity resolved through the shared content graph and route table.
    pub destination: Key,
}

fn optional_destinations<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<Vec<NavigationEntry>>, D::Error> {
    crate::tables::many(decoder).map(Some)
}

/// One plugin-package selection, independent from its exported providers and resources.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PluginSelection {
    /// Block an inherited package selection for this site.
    Disabled {},
    /// Admit this exact captured package for its declared plugin contributions.
    Enabled {},
}

/// One resource binding, independent of capability-provider activation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ResourceSelection {
    /// Block an inherited resource; consumers may not fall through to its old binding.
    Disabled {},
    /// Select one package-owned resource, output and contribution behavior as an atom.
    Enabled {
        /// Exact package-owned logical resource; no provider role is invented.
        resource: ResourceRef,
        /// Canonical public file route; adapters never invent it.
        output: gazette_core::publication::routing::RoutePath,
        /// Whether the site emits this resource directly or only when another resource needs it.
        #[serde(rename = "use")]
        use_: ResourceUse,
    },
}

/// How one selected resource contributes to the effective site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceUse {
    /// Site-level resource emitted as part of the selected publication experience.
    Site,
    /// Dependency emitted only through the resource graph that reaches it.
    Dependency,
}

/// Selected root Theme package and optional palette, not capture coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeSelection {
    /// Root package of the effective Theme dependency graph.
    pub package: ThemePackageId,
    /// At most one Theme-owned named palette; membership requires Theme admission.
    pub palette: Option<Key>,
}

/// Selected failure policy; optional is not permission for an arbitrary substitute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Requirement {
    /// Unavailable or invalid implementation prevents successful capability admission.
    Required,
    /// Absence/degradation still requires explicit policy and recorded evidence.
    Optional,
}

/// Explicit selection, distinct from discovered availability or emitted artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CapabilitySelection {
    /// Suppress this selection; never fall through to an inherited provider.
    Disabled {},
    /// Select exactly this provider and failure policy.
    Enabled {
        /// Explicitly owned implementation, separate from the capability contract key.
        provider: ProviderRef,
        /// Whether successful admission requires this capability.
        requirement: Requirement,
        /// Exact absence permission; never suppresses source, compiler or budget failures.
        allow_absence: Option<CapabilityAbsence>,
    },
}

/// Package class owning one logical resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "package", rename_all = "kebab-case")]
pub enum ResourceOwner {
    /// Resource declared by a Theme package.
    Theme(ThemePackageId),
    /// Resource declared by a plugin package.
    Plugin(PluginPackageId),
}

/// Exact package-qualified resource selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    /// Package identity and class; never a provider or contribution scope.
    pub owner: ResourceOwner,
    /// Package-local Gazette resource identity.
    pub resource: ResourceId,
}

/// Winning declaration layer; no author, publisher or rights roles are inherited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionScope {
    /// Default declared by an admitted Theme package.
    Theme(ThemePackageId),
    /// Workspace default.
    Workspace,
    /// Exact site declaration overriding or extending defaults.
    Site(Key),
}

/// Atomic selection whose origin can be explained through retained syntax.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SelectionField {
    /// Whole Theme binding, including its palette selection.
    Theme {},
    /// Whole site-chrome copy, independent from ordered destinations.
    Chrome {},
    /// Complete map of exact content identities to Theme entries.
    PageTemplates {},
    /// Independently inherited ordered navigation destinations.
    Navigation {},
    /// Independently inherited ordered footer destinations.
    Footer {},
    /// One independently inherited page invitation.
    Invitation {
        /// Theme-owned semantic invitation key.
        invitation: Key,
    },
    /// One named capability binding.
    Capability {
        /// Exact selected capability contract.
        capability: CapabilityId,
    },
    /// One independently selected plugin package.
    Plugin {
        /// Exact workspace-declared plugin package identity.
        package: PluginPackageId,
    },
    /// One independently selected resource binding.
    Resource {
        /// Exact workspace-selected resource identity.
        resource: ResourceId,
    },
}

/// Logical origin of a resolved value, not a fabricated local field span.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct SelectionOrigin {
    /// Winning layer.
    pub scope: SelectionScope,
    /// Selected atom within that layer.
    pub field: SelectionField,
}

/// Immutable derived value with its winning declaration origin; never deserialized as authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Selected<T> {
    value: T,
    origin: SelectionOrigin,
}

impl<T> Selected<T> {
    /// Effective value, resolved once by the workspace compiler.
    pub fn value(&self) -> &T {
        &self.value
    }
    /// Winning declaration; retained syntax supplies exact coordinates separately.
    pub fn origin(&self) -> &SelectionOrigin {
        &self.origin
    }
}

/// Immutable per-site desired composition; not source verification, compatibility or readiness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ResolvedComposition {
    theme: Option<Selected<ThemeSelection>>,
    chrome: Option<ResolvedChrome>,
    page_templates: Option<Selected<BTreeMap<Key, gazette_core::source::EntryPath>>>,
    navigation: Option<Selected<Vec<NavigationEntry>>>,
    footer: Option<Selected<Vec<NavigationEntry>>>,
    invitations: BTreeMap<Key, Selected<InvitationSelection>>,
    capabilities: BTreeMap<CapabilityId, Selected<CapabilitySelection>>,
    plugins: BTreeMap<PluginPackageId, Selected<PluginSelection>>,
    resources: BTreeMap<ResourceId, Selected<ResourceSelection>>,
}

impl ResolvedComposition {
    /// Effective Theme or none; a build must require a verified selected Theme.
    pub fn theme(&self) -> Option<&Selected<ThemeSelection>> {
        self.theme.as_ref()
    }
    /// Effective workspace/site chrome with its winning declaration origin.
    pub fn chrome(&self) -> Option<&ResolvedChrome> {
        self.chrome.as_ref()
    }
    /// Effective exact page-template bindings and their winning declaration origin.
    pub fn page_templates(
        &self,
    ) -> Option<&Selected<BTreeMap<Key, gazette_core::source::EntryPath>>> {
        self.page_templates.as_ref()
    }
    /// Effective navigation; an explicit empty list retains its clearing origin.
    pub fn navigation(&self) -> Option<&Selected<Vec<NavigationEntry>>> {
        self.navigation.as_ref()
    }
    /// Effective footer destinations, independently selected from navigation and chrome.
    pub fn footer(&self) -> Option<&Selected<Vec<NavigationEntry>>> {
        self.footer.as_ref()
    }
    /// Page invitation policies, including explicit disabled selections.
    pub fn invitations(&self) -> &BTreeMap<Key, Selected<InvitationSelection>> {
        &self.invitations
    }
    /// All selected contracts, including explicit disabled values, in exact key order.
    pub fn capabilities(&self) -> &BTreeMap<CapabilityId, Selected<CapabilitySelection>> {
        &self.capabilities
    }
    /// Plugin-package selections, including explicit disabled bindings.
    pub fn plugins(&self) -> &BTreeMap<PluginPackageId, Selected<PluginSelection>> {
        &self.plugins
    }
    /// Resource selections, including explicit disabled bindings.
    pub fn resources(&self) -> &BTreeMap<ResourceId, Selected<ResourceSelection>> {
        &self.resources
    }
}

pub(super) fn validate(
    composition: &Composition,
    scope: SelectionScope,
    theme_packages: &BTreeMap<ThemePackageId, ThemePackageBinding>,
    plugin_packages: &BTreeMap<PluginPackageId, PluginPackageBinding>,
    errors: &mut Vec<Diagnostic>,
) {
    plugins::validate(composition, &scope, plugin_packages, errors);
    resources::validate(composition, &scope, theme_packages, plugin_packages, errors);
    let prefix = match &scope {
        SelectionScope::Theme(package) => format!("themes[{package:?}]"),
        SelectionScope::Workspace => "composition".into(),
        SelectionScope::Site(site) => format!("sites[{:?}].composition", site.as_str()),
    };
    if let Some(theme) = &composition.theme
        && !theme_packages.contains_key(&theme.package)
    {
        errors.push(Diagnostic {
            code: "workspace_theme_package_reference",
            field: format!("{prefix}.theme.package"),
            key: None,
        });
    }
    if let Some(chrome) = &composition.chrome {
        if let Some(assistance) = &chrome.reading_assistance {
            reading_assistance::validate(assistance, &prefix, errors);
        }
        for (field, value) in chrome.copy_fields() {
            if invalid_copy(value) {
                errors.push(Diagnostic {
                    code: "workspace_chrome_copy",
                    field: format!("{prefix}.chrome.{field}"),
                    key: None,
                });
            }
        }
        if chrome.regions.len() > 256 {
            errors.push(Diagnostic {
                code: "workspace_chrome_region_limit",
                field: format!("{prefix}.chrome.regions"),
                key: None,
            });
        }
        for (region, copy) in &chrome.regions {
            if invalid_copy(copy) {
                errors.push(Diagnostic {
                    code: "workspace_chrome_copy",
                    field: format!("{prefix}.chrome.regions[{:?}]", region.as_str()),
                    key: Some(region.clone()),
                });
            }
        }
    }
    for (field, entries) in [
        ("navigation", &composition.navigation),
        ("footer", &composition.footer),
    ] {
        if let Some(entries) = entries {
            if entries.len() > 256 {
                errors.push(Diagnostic {
                    code: "workspace_navigation_limit",
                    field: format!("{prefix}.{field}"),
                    key: None,
                });
            }
            let mut keys = std::collections::BTreeSet::new();
            for entry in entries {
                if let Some(key) = &entry.key
                    && (!key.as_str().contains(':') || !keys.insert(key))
                {
                    errors.push(Diagnostic {
                        code: "workspace_navigation_key",
                        field: format!("{prefix}.{field}.key"),
                        key: Some(key.clone()),
                    });
                }
                if invalid_copy(&entry.label) {
                    errors.push(Diagnostic {
                        code: "workspace_navigation_copy",
                        field: format!("{prefix}.{field}.label"),
                        key: Some(entry.destination.clone()),
                    });
                }
            }
        }
    }
    for (capability, selection) in &composition.capabilities {
        if let CapabilitySelection::Enabled {
            provider: ProviderRef::Plugin { package, .. },
            ..
        } = selection
            && !plugin_packages.contains_key(package)
        {
            errors.push(Diagnostic {
                code: "workspace_plugin_package_reference",
                field: format!(
                    "{prefix}.capabilities[{:?}].provider.package",
                    capability.as_str()
                ),
                key: None,
            });
        }
    }
    for (capability, selection) in &composition.capabilities {
        if matches!(
            selection,
            CapabilitySelection::Enabled {
                requirement: Requirement::Required,
                allow_absence: Some(_),
                ..
            }
        ) {
            errors.push(Diagnostic {
                code: "workspace_capability_absence",
                field: format!(
                    "{prefix}.capabilities[{:?}].allow_absence",
                    capability.as_str()
                ),
                key: None,
            });
        }
    }
}

pub(super) fn resolve(
    defaults: &Composition,
    sites: &BTreeMap<Key, Site>,
    limits: crate::toml::Limits,
) -> Result<BTreeMap<Key, ResolvedComposition>, Error> {
    let mut result = BTreeMap::new();
    let mut used = 0usize;
    let mut text = 0usize;
    for site in sites.values() {
        let local = &site.composition;
        let chosen_theme = local
            .theme
            .as_ref()
            .map(|v| (v, SelectionScope::Site(site.id.clone())))
            .or_else(|| {
                defaults
                    .theme
                    .as_ref()
                    .map(|v| (v, SelectionScope::Workspace))
            });
        let theme = chosen_theme
            .map(|(value, scope)| {
                charge(&mut used, 1, limits.nodes, "composition selections")?;
                for key in std::iter::once(site.id.as_str())
                    .chain(std::iter::once(value.package.as_str()))
                    .chain(value.palette.iter().map(Key::as_str))
                {
                    charge(
                        &mut text,
                        key.len(),
                        limits.source_bytes,
                        "composition text bytes",
                    )?;
                }
                Ok(Selected {
                    value: value.clone(),
                    origin: SelectionOrigin {
                        scope,
                        field: SelectionField::Theme {},
                    },
                })
            })
            .transpose()?;
        let chrome = chrome::resolve(
            defaults.chrome.as_ref(),
            local.chrome.as_ref(),
            &site.id,
            limits,
            &mut used,
            &mut text,
        )?;
        let page_templates = local
            .page_templates
            .as_ref()
            .map(|value| (value, SelectionScope::Site(site.id.clone())))
            .or_else(|| {
                defaults
                    .page_templates
                    .as_ref()
                    .map(|value| (value, SelectionScope::Workspace))
            })
            .map(|(value, scope)| {
                charge(
                    &mut used,
                    value.len().saturating_add(1),
                    limits.nodes,
                    "page template selections",
                )?;
                charge(
                    &mut text,
                    site.id.as_str().len(),
                    limits.source_bytes,
                    "composition text bytes",
                )?;
                for (content, template) in value {
                    for length in [content.as_str().len(), template.as_str().len()] {
                        charge(
                            &mut text,
                            length,
                            limits.source_bytes,
                            "composition text bytes",
                        )?;
                    }
                }
                Ok(Selected {
                    value: value.clone(),
                    origin: SelectionOrigin {
                        scope,
                        field: SelectionField::PageTemplates {},
                    },
                })
            })
            .transpose()?;
        let navigation = resolve_destinations(
            defaults.navigation.as_ref(),
            local.navigation.as_ref(),
            &site.id,
            SelectionField::Navigation {},
            limits,
            (&mut used, &mut text),
        )?;
        let footer = resolve_destinations(
            defaults.footer.as_ref(),
            local.footer.as_ref(),
            &site.id,
            SelectionField::Footer {},
            limits,
            (&mut used, &mut text),
        )?;
        let mut capabilities = BTreeMap::new();
        // Each layer is ordered; the final map is globally sorted. The site
        // atom replaces the whole default; there is no field-by-field provider merge.
        for (key, value) in defaults
            .capabilities
            .iter()
            .filter(|(key, _)| !local.capabilities.contains_key(*key))
            .map(|(k, v)| (k, (v, SelectionScope::Workspace)))
            .chain(
                local
                    .capabilities
                    .iter()
                    .map(|(k, v)| (k, (v, SelectionScope::Site(site.id.clone())))),
            )
        {
            charge(&mut used, 1, limits.nodes, "composition selections")?;
            let (value, scope) = value;
            for id in [site.id.as_str(), key.as_str()] {
                charge(
                    &mut text,
                    id.len(),
                    limits.source_bytes,
                    "composition text bytes",
                )?;
            }
            if let CapabilitySelection::Enabled { provider, .. } = value {
                let ids = match provider {
                    ProviderRef::Gazette { provider } => [Some(provider.as_str()), None],
                    ProviderRef::Plugin { package, provider } => {
                        [Some(package.as_str()), Some(provider.as_str())]
                    }
                };
                for id in ids.into_iter().flatten() {
                    charge(
                        &mut text,
                        id.len(),
                        limits.source_bytes,
                        "composition text bytes",
                    )?;
                }
            }
            capabilities.insert(
                key.clone(),
                Selected {
                    value: value.clone(),
                    origin: SelectionOrigin {
                        scope,
                        field: SelectionField::Capability {
                            capability: key.clone(),
                        },
                    },
                },
            );
        }
        let invitations = invitations::resolve(defaults, site, limits, &mut used, &mut text)?;
        let plugins = plugins::resolve(defaults, site, limits, &mut used, &mut text)?;
        let resources = resources::resolve(defaults, site, limits, &mut used, &mut text)?;
        result.insert(
            site.id.clone(),
            ResolvedComposition {
                theme,
                chrome,
                page_templates,
                invitations,
                navigation,
                footer,
                capabilities,
                plugins,
                resources,
            },
        );
    }
    Ok(result)
}

fn invalid_copy(value: &str) -> bool {
    value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
}

fn resolve_destinations(
    defaults: Option<&Vec<NavigationEntry>>,
    local: Option<&Vec<NavigationEntry>>,
    site: &Key,
    field: SelectionField,
    limits: crate::toml::Limits,
    budget: (&mut usize, &mut usize),
) -> Result<Option<Selected<Vec<NavigationEntry>>>, Error> {
    let (used, text) = budget;
    local
        .map(|value| (value, SelectionScope::Site(site.clone())))
        .or_else(|| defaults.map(|value| (value, SelectionScope::Workspace)))
        .map(|(value, scope)| {
            charge(used, 1, limits.nodes, "composition selections")?;
            charge(
                text,
                site.as_str().len(),
                limits.source_bytes,
                "composition text bytes",
            )?;
            for entry in value {
                charge(used, 1, limits.nodes, "composition selections")?;
                if let Some(key) = &entry.key {
                    charge(
                        text,
                        key.as_str().len(),
                        limits.source_bytes,
                        "composition text bytes",
                    )?;
                }
                for value in [entry.label.as_str(), entry.destination.as_str()] {
                    charge(
                        text,
                        value.len(),
                        limits.source_bytes,
                        "composition text bytes",
                    )?;
                }
            }
            Ok(Selected {
                value: value.clone(),
                origin: SelectionOrigin { scope, field },
            })
        })
        .transpose()
}

pub(super) fn validate_resolved(
    site: &Key,
    composition: &ResolvedComposition,
    errors: &mut Vec<Diagnostic>,
) {
    if composition
        .chrome()
        .is_some_and(|copy| copy.value().regions.len() > 256)
    {
        errors.push(Diagnostic {
            code: "workspace_chrome_region_limit",
            field: format!("sites[{:?}].composition.chrome.regions", site.as_str()),
            key: None,
        });
    }
    let selected = |package: &PluginPackageId| {
        matches!(
            composition.plugins.get(package).map(Selected::value),
            Some(PluginSelection::Enabled {})
        )
    };
    for (capability, selection) in &composition.capabilities {
        let CapabilitySelection::Enabled {
            provider: ProviderRef::Plugin { package, .. },
            ..
        } = selection.value()
        else {
            continue;
        };
        if !selected(package) {
            errors.push(Diagnostic {
                code: "workspace_plugin_package_selection",
                field: format!(
                    "sites[{:?}].composition.capabilities[{:?}].provider.package",
                    site.as_str(),
                    capability.as_str()
                ),
                key: None,
            });
        }
    }
    for (resource, selection) in &composition.resources {
        let ResourceSelection::Enabled {
            resource:
                ResourceRef {
                    owner: ResourceOwner::Plugin(package),
                    ..
                },
            ..
        } = selection.value()
        else {
            continue;
        };
        if !selected(package) {
            errors.push(Diagnostic {
                code: "workspace_plugin_package_selection",
                field: format!(
                    "sites[{:?}].composition.resources[{:?}].resource.owner",
                    site.as_str(),
                    resource.as_str()
                ),
                key: None,
            });
        }
    }
}

fn charge(
    used: &mut usize,
    amount: usize,
    maximum: usize,
    name: &'static str,
) -> Result<(), Error> {
    *used = used
        .checked_add(amount)
        .filter(|&n| n <= maximum)
        .ok_or(Error::Limit(name))?;
    Ok(())
}
