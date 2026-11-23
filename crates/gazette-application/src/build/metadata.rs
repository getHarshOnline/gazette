//! Admit captured Theme and plugin packages without native reads or role collapse.

use super::{AcquiredInputs, CompositionInspection};
use crate::source::AcquiredInput;
use gazette_compiler::{
    plugin, theme_graph,
    theme_package::{self, BrowserThemeColors, ThemePackageId},
    toml,
    workspace::{
        CapabilitySelection, Key, PluginPackageBinding, Requirement, ResourceOwner, ResourceRef,
        ResourceSelection, ResourceUse, ThemePackageBinding,
    },
};
use gazette_core::{
    plugin::{PluginPackageId, PluginPackageIdentity},
    publication::capability_runtime::{ProviderId, ProviderRef, ProviderRuntimeIdentity},
    resource::Recipe,
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use serde::Serialize;
use std::{collections::BTreeMap, fmt};

#[derive(Debug)]
/// A failure to admit captured Theme or plugin package metadata.
pub enum MetadataError {
    /// The selected site has no explicit Theme package binding.
    ThemeRequired,
    /// A package points at a workspace binding that is absent or inconsistent.
    Binding {
        /// The canonical identifier of the package or source with the bad binding.
        owner: String,
        /// The invariant that the binding failed.
        reason: &'static str,
    },
    /// A retained Theme manifest failed canonical parsing or validation.
    ThemeManifest {
        /// The workspace package identity whose manifest failed.
        package: ThemePackageId,
        /// The canonical Theme-manifest failure.
        error: theme_package::Error,
    },
    /// A retained plugin manifest failed canonical parsing or validation.
    PluginManifest {
        /// The workspace package identity whose manifest failed.
        package: PluginPackageId,
        /// The canonical plugin-manifest failure.
        error: plugin::Error,
    },
    /// The admitted Theme declarations do not form a valid effective graph.
    ThemeGraph(theme_graph::Error),
    /// Resolved Theme and workspace defaults exceed the compilation limits.
    Composition(gazette_compiler::workspace::Error),
    /// An explicit workspace selection is not exported by its selected package.
    Selection {
        /// The canonical identity of the selection owner.
        owner: String,
        /// The invariant that the selection failed.
        reason: &'static str,
    },
    /// A Gazette-owned provider was selected before its native catalog/runtime exists.
    ProviderUnavailable {
        /// Exact Gazette-local provider identity; no plugin owner is inferred.
        provider: ProviderId,
    },
    /// A declared package input is absent from the exact retained capture.
    Input {
        /// The workspace source binding used for the package.
        source: Key,
        /// The declared relative package input.
        path: EntryPath,
        /// The retained-input invariant that failed.
        reason: &'static str,
    },
    /// Stable package evidence could not be encoded.
    Evidence {
        /// The canonical package identity whose evidence failed.
        package: String,
    },
}

impl MetadataError {
    /// Return the stable machine-readable failure code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ThemeRequired => "theme_package_required",
            Self::Binding { .. } => "package_binding",
            Self::ThemeManifest { error, .. } => error.code(),
            Self::PluginManifest { error, .. } => error.code(),
            Self::ThemeGraph(error) => error.code(),
            Self::Composition(error) => error.code(),
            Self::Selection { .. } => "package_selection",
            Self::ProviderUnavailable { .. } => "provider_unavailable",
            Self::Input { .. } => "package_input",
            Self::Evidence { .. } => "package_evidence",
        }
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThemeRequired => {
                f.write_str("package check requires an explicit Theme; no fallback selected")
            }
            Self::Binding { owner, reason } => write!(f, "package binding {owner}: {reason}"),
            Self::ThemeManifest { package, error } => write!(f, "Theme package {package}: {error}"),
            Self::PluginManifest { package, error } => {
                write!(f, "plugin package {package}: {error}")
            }
            Self::ThemeGraph(error) => error.fmt(f),
            Self::Composition(error) => error.fmt(f),
            Self::Selection { owner, reason } => write!(f, "package selection {owner}: {reason}"),
            Self::ProviderUnavailable { provider } => write!(
                f,
                "Gazette provider {provider}: native provider catalog/runtime is unsupported or unconfigured"
            ),
            Self::Input {
                source,
                path,
                reason,
            } => write!(f, "package source {source}, input {path}: {reason}"),
            Self::Evidence { package } => write!(
                f,
                "package {package}: normalized binding evidence could not be encoded"
            ),
        }
    }
}

impl std::error::Error for MetadataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ThemeManifest { error, .. } => Some(error),
            Self::PluginManifest { error, .. } => Some(error),
            Self::ThemeGraph(error) => Some(error),
            Self::Composition(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct ThemePackage<'a> {
    binding: &'a ThemePackageBinding,
    source_binding_sha256: Sha256Digest,
    input_origins: BTreeMap<EntryPath, super::ResourceInputEvidence>,
    parsed: theme_package::ParsedManifest<'a>,
}

#[derive(Debug)]
struct PluginPackage<'a> {
    binding: &'a PluginPackageBinding,
    identity: PluginPackageIdentity,
    parsed: plugin::ParsedManifest<'a>,
}

/// Immutable admitted package graph over retained captures.
#[derive(Debug)]
pub struct CompositionMetadata<'a> {
    pub(super) acquired: &'a AcquiredInputs,
    root_theme: ThemePackageId,
    theme_order: Vec<ThemePackageId>,
    effective_theme: theme_graph::EffectiveTheme,
    composition: gazette_compiler::workspace::ResolvedComposition,
    browser_theme_colors: BrowserThemeColors,
    theme_packages: BTreeMap<ThemePackageId, ThemePackage<'a>>,
    plugin_packages: BTreeMap<PluginPackageId, PluginPackage<'a>>,
}

impl CompositionMetadata<'_> {
    /// Site settings after applying the admitted Theme's defaults beneath explicit choices.
    pub fn composition(&self) -> &gazette_compiler::workspace::ResolvedComposition {
        &self.composition
    }

    pub(super) fn theme_order(&self) -> &[ThemePackageId] {
        &self.theme_order
    }
    pub(super) fn effective_theme(&self) -> &theme_graph::EffectiveTheme {
        &self.effective_theme
    }
    pub(super) fn browser_theme_colors(&self) -> &BrowserThemeColors {
        &self.browser_theme_colors
    }

    pub(super) fn theme_graph(&self) -> theme_graph::Graph<'_> {
        theme_graph::Graph::resolve(
            &self.root_theme,
            self.theme_packages
                .values()
                .map(|package| package.parsed.manifest()),
        )
        .expect("package admission retains the same immutable Theme declarations")
    }

    pub(super) fn theme_manifest(
        &self,
        package: &ThemePackageId,
    ) -> Option<&theme_package::ParsedManifest<'_>> {
        self.theme_packages.get(package).map(|entry| &entry.parsed)
    }
    pub(super) fn theme_capture(&self, package: &ThemePackageId) -> Option<&AcquiredInput> {
        self.acquired
            .sources()
            .get(&self.theme_packages.get(package)?.binding.source)
    }
    pub(super) fn theme_binding(&self, package: &ThemePackageId) -> Option<&ThemePackageBinding> {
        Some(self.theme_packages.get(package)?.binding)
    }
    pub(super) fn plugin_capture(&self, package: &PluginPackageId) -> Option<&AcquiredInput> {
        self.acquired
            .sources()
            .get(&self.plugin_packages.get(package)?.binding.source)
    }
    pub(super) fn provider_runtime_identity(
        &self,
        provider: &ProviderRef,
    ) -> Option<ProviderRuntimeIdentity> {
        match provider {
            ProviderRef::Gazette { provider } => Some(ProviderRuntimeIdentity::Gazette {
                provider: provider.clone(),
            }),
            ProviderRef::Plugin { package, provider } => {
                let package = self.plugin_packages.get(package)?;
                Some(ProviderRuntimeIdentity::Plugin {
                    package: package.identity.clone(),
                    provider: provider.clone(),
                })
            }
        }
    }
    pub(super) fn resource(&self, resource: &ResourceRef) -> Option<AdmittedResource<'_>> {
        match &resource.owner {
            ResourceOwner::Theme(package) => {
                let entry = self.theme_packages.get(package)?;
                let declaration = entry
                    .parsed
                    .manifest()
                    .resources
                    .iter()
                    .find(|candidate| candidate.id == resource.resource)?;
                let capture = self.theme_capture(package)?;
                Some(AdmittedResource {
                    source: &entry.binding.source,
                    capture,
                    inputs: &declaration.inputs,
                    input_origins: Some(&entry.input_origins),
                    build: &declaration.build,
                    manifest_sha256: entry.parsed.source_sha256(),
                    source_binding_sha256: entry.source_binding_sha256,
                    tree: capture.inventory().identity(),
                })
            }
            ResourceOwner::Plugin(package) => {
                let entry = self.plugin_packages.get(package)?;
                let declaration = entry.parsed.manifest().resources.get(&resource.resource)?;
                let capture = self.plugin_capture(package)?;
                Some(AdmittedResource {
                    source: &entry.binding.source,
                    capture,
                    inputs: &declaration.inputs,
                    input_origins: None,
                    build: &declaration.build,
                    manifest_sha256: entry.identity.manifest,
                    source_binding_sha256: entry.identity.source,
                    tree: capture.inventory().identity(),
                })
            }
        }
    }

    /// Borrow a serializable evidence view without reopening package sources.
    ///
    /// # Panics
    ///
    /// Panics only if an admitted package's immutable retained capture has been
    /// removed after admission, which `CompositionMetadata` does not permit.
    pub fn inspect(&self) -> MetadataInspection<'_> {
        MetadataInspection {
            acquisition: self.acquired.inspect(),
            root_theme: &self.root_theme,
            composition: &self.composition,
            theme_order: &self.theme_order,
            theme_packages: self
                .theme_packages
                .iter()
                .map(|(package, entry)| {
                    let capture = self
                        .acquired
                        .sources()
                        .get(&entry.binding.source)
                        .expect("admitted package retains its capture");
                    (
                        package,
                        ThemePackageInspection {
                            source: &entry.binding.source,
                            manifest: &entry.binding.manifest,
                            source_binding_sha256: entry.source_binding_sha256,
                            manifest_sha256: entry.parsed.source_sha256(),
                            tree: capture.inventory().identity(),
                            input_origins: &entry.input_origins,
                            declaration: entry.parsed.manifest(),
                        },
                    )
                })
                .collect(),
            plugin_packages: self
                .plugin_packages
                .iter()
                .map(|(package, entry)| {
                    (
                        package,
                        PluginPackageInspection {
                            source: &entry.binding.source,
                            manifest: &entry.binding.manifest,
                            identity: &entry.identity,
                            declaration: entry.parsed.manifest(),
                        },
                    )
                })
                .collect(),
        }
    }
}

pub(super) struct AdmittedResource<'a> {
    pub source: &'a Key,
    pub capture: &'a AcquiredInput,
    pub inputs: &'a [EntryPath],
    pub input_origins: Option<&'a BTreeMap<EntryPath, super::ResourceInputEvidence>>,
    pub build: &'a Recipe,
    pub manifest_sha256: Sha256Digest,
    pub source_binding_sha256: Sha256Digest,
    pub tree: TreeIdentity,
}

#[derive(Serialize, schemars::JsonSchema)]
/// Serializable evidence for one admitted publication package graph.
pub struct MetadataInspection<'a> {
    /// Exact retained source-acquisition evidence shared by the composition.
    pub acquisition: CompositionInspection<'a>,
    /// Canonical identity of the selected root Theme package.
    pub root_theme: &'a ThemePackageId,
    /// Effective settings, including Theme defaults and each winning declaration layer.
    pub composition: &'a gazette_compiler::workspace::ResolvedComposition,
    /// Dependency-first order of Theme packages in the resolved graph.
    pub theme_order: &'a [ThemePackageId],
    /// Admitted Theme package evidence keyed by canonical package identity.
    pub theme_packages: BTreeMap<&'a ThemePackageId, ThemePackageInspection<'a>>,
    /// Admitted plugin package evidence keyed by canonical package identity.
    pub plugin_packages: BTreeMap<&'a PluginPackageId, PluginPackageInspection<'a>>,
}

#[derive(Serialize, schemars::JsonSchema)]
/// Exact identity and declaration evidence for an admitted Theme package.
pub struct ThemePackageInspection<'a> {
    /// Workspace source binding that supplied the retained package tree.
    pub source: &'a Key,
    /// Relative manifest path inside the retained source.
    pub manifest: &'a EntryPath,
    /// Digest of the normalized workspace source binding.
    pub source_binding_sha256: Sha256Digest,
    /// Digest of the exact retained Theme manifest bytes.
    pub manifest_sha256: Sha256Digest,
    /// Identity of the complete retained package source tree.
    pub tree: TreeIdentity,
    /// Every declared virtual input and its exact retained native origin.
    pub input_origins: &'a BTreeMap<EntryPath, super::ResourceInputEvidence>,
    /// Parsed canonical Theme package declaration.
    pub declaration: &'a theme_package::Manifest,
}

#[derive(Serialize, schemars::JsonSchema)]
/// Exact identity and declaration evidence for an admitted plugin package.
pub struct PluginPackageInspection<'a> {
    /// Workspace source binding that supplied the retained package tree.
    pub source: &'a Key,
    /// Relative manifest path inside the retained source.
    pub manifest: &'a EntryPath,
    /// Composite source, manifest, and retained-tree package identity.
    pub identity: &'a PluginPackageIdentity,
    /// Parsed canonical plugin package declaration.
    pub declaration: &'a plugin::Manifest,
}

impl AcquiredInputs {
    /// Admit selected package declarations from retained bytes without execution.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when a binding, retained manifest, Theme graph,
    /// selected provider or resource, or its immutable evidence is invalid.
    ///
    /// # Panics
    ///
    /// Panics only if a successfully resolved Theme graph omits its own root,
    /// which is an internal compiler invariant.
    pub fn check_metadata(
        &self,
        limits: toml::Limits,
    ) -> Result<CompositionMetadata<'_>, MetadataError> {
        let workspace = &self.compilation().workspace;
        let composition = workspace
            .composition_for(self.site())
            .ok_or(MetadataError::ThemeRequired)?;
        let selected_theme = composition
            .theme()
            .ok_or(MetadataError::ThemeRequired)?
            .value();
        let mut remaining = limits;
        let mut theme_packages = BTreeMap::new();
        for binding in workspace.theme_packages() {
            if !self.selected_themes().contains(&binding.package) {
                continue;
            }
            let (text, bytes) = retained_manifest(self, &binding.source, &binding.manifest)?;
            let parsed =
                theme_package::ParsedManifest::parse(text, remaining).map_err(|error| {
                    MetadataError::ThemeManifest {
                        package: binding.package.clone(),
                        error,
                    }
                })?;
            if parsed.manifest().package != binding.package {
                return Err(MetadataError::Selection {
                    owner: binding.package.to_string(),
                    reason: "Theme manifest identity does not match its workspace binding",
                });
            }
            charge(&mut remaining, bytes.len(), parsed.syntax().node_count());
            let source =
                workspace
                    .source(&binding.source)
                    .ok_or_else(|| MetadataError::Binding {
                        owner: binding.source.to_string(),
                        reason: "Theme package source binding is absent",
                    })?;
            let source_binding_sha256 =
                normalized_source_digest(source).map_err(|()| MetadataError::Evidence {
                    package: binding.package.to_string(),
                })?;
            let input_origins =
                super::resource_inputs::theme_inputs(self, binding, parsed.manifest())?;
            theme_packages.insert(
                binding.package.clone(),
                ThemePackage {
                    binding,
                    source_binding_sha256,
                    input_origins,
                    parsed,
                },
            );
        }
        let graph = theme_graph::Graph::resolve(
            &selected_theme.package,
            theme_packages
                .values()
                .map(|package| package.parsed.manifest()),
        )
        .map_err(MetadataError::ThemeGraph)?;
        let effective_theme =
            theme_graph::EffectiveTheme::resolve(&graph).map_err(MetadataError::ThemeGraph)?;
        let root = graph.root();
        let browser_theme_colors = match selected_theme.palette.as_ref() {
            Some(palette) => root
                .palettes
                .binary_search_by(|candidate| candidate.id.cmp(palette))
                .ok()
                .map(|index| root.palettes[index].browser_theme_colors.clone())
                .ok_or_else(|| MetadataError::Selection {
                    owner: root.package.to_string(),
                    reason: "selected palette is not declared by the root Theme package",
                })?,
            None => root.browser_theme_colors.clone().ok_or_else(|| {
                MetadataError::Selection {
                    owner: root.package.to_string(),
                    reason: "root Theme must resolve browser colors when no named palette is selected",
                }
            })?,
        };
        let theme_order = graph
            .packages()
            .iter()
            .map(|manifest| manifest.package.clone())
            .collect::<Vec<_>>();

        let mut plugin_packages = BTreeMap::new();
        for binding in workspace.plugin_packages() {
            if !self.selected_plugins().contains(&binding.package) {
                continue;
            }
            let (text, bytes) = retained_manifest(self, &binding.source, &binding.manifest)?;
            let parsed = plugin::ParsedManifest::parse(text, remaining).map_err(|error| {
                MetadataError::PluginManifest {
                    package: binding.package.clone(),
                    error,
                }
            })?;
            if parsed.manifest().package != binding.package {
                return Err(MetadataError::Selection {
                    owner: binding.package.to_string(),
                    reason: "plugin manifest identity does not match its workspace binding",
                });
            }
            charge(&mut remaining, bytes.len(), parsed.syntax().node_count());
            let source =
                workspace
                    .source(&binding.source)
                    .ok_or_else(|| MetadataError::Binding {
                        owner: binding.source.to_string(),
                        reason: "plugin package source binding is absent",
                    })?;
            let source_binding_sha256 =
                normalized_source_digest(source).map_err(|()| MetadataError::Evidence {
                    package: binding.package.to_string(),
                })?;
            validate_plugin_inputs(self, binding, parsed.manifest())?;
            let tree = self
                .sources()
                .get(&binding.source)
                .expect("retained manifest proved capture membership")
                .inventory()
                .identity()
                .sha256();
            let identity = parsed.identity(source_binding_sha256, tree);
            plugin_packages.insert(
                binding.package.clone(),
                PluginPackage {
                    binding,
                    identity,
                    parsed,
                },
            );
        }
        let composition = composition
            .with_theme_defaults(&effective_theme, limits)
            .map_err(MetadataError::Composition)?;
        validate_selections(
            &composition,
            &effective_theme,
            &plugin_packages,
            &theme_packages,
        )?;
        Ok(CompositionMetadata {
            acquired: self,
            root_theme: selected_theme.package.clone(),
            theme_order,
            effective_theme,
            composition,
            browser_theme_colors,
            theme_packages,
            plugin_packages,
        })
    }
}

fn retained_manifest<'a>(
    inputs: &'a AcquiredInputs,
    source: &Key,
    path: &EntryPath,
) -> Result<(&'a str, &'a [u8]), MetadataError> {
    let capture = inputs
        .sources()
        .get(source)
        .ok_or_else(|| MetadataError::Binding {
            owner: source.to_string(),
            reason: "selected package source was not captured",
        })?;
    let bytes = capture.file(path).ok_or_else(|| MetadataError::Input {
        source: source.clone(),
        path: path.clone(),
        reason: "package manifest must be a retained regular file",
    })?;
    let text = std::str::from_utf8(bytes).map_err(|_| MetadataError::Input {
        source: source.clone(),
        path: path.clone(),
        reason: "package manifest is not UTF-8",
    })?;
    Ok((text, bytes))
}

pub(super) fn normalized_source_digest(
    source: &gazette_compiler::workspace::Source,
) -> Result<Sha256Digest, ()> {
    serde_json::to_vec(source)
        .map(|bytes| Sha256Digest::of(&bytes))
        .map_err(|_| ())
}

fn charge(remaining: &mut toml::Limits, bytes: usize, nodes: usize) {
    remaining.source_bytes -= bytes;
    remaining.nodes -= nodes;
}

fn validate_plugin_inputs(
    inputs: &AcquiredInputs,
    binding: &PluginPackageBinding,
    manifest: &plugin::Manifest,
) -> Result<(), MetadataError> {
    let capture = inputs
        .sources()
        .get(&binding.source)
        .expect("retained manifest proved capture membership");
    let mut required = Vec::new();
    required.extend(manifest.configuration_schema.iter());
    required.extend(
        manifest
            .resources
            .values()
            .flat_map(|resource| &resource.inputs),
    );
    required.extend(
        manifest
            .extensions
            .values()
            .map(|extension| &extension.schema),
    );
    required.extend(
        manifest
            .components
            .values()
            .flat_map(|component| [&component.arguments, &component.model]),
    );
    for path in required {
        require_file(
            capture,
            &binding.source,
            path,
            "plugin input must be a retained regular file",
        )?;
    }
    Ok(())
}

fn require_file(
    capture: &AcquiredInput,
    source: &Key,
    path: &EntryPath,
    reason: &'static str,
) -> Result<(), MetadataError> {
    if capture.file(path).is_none() {
        return Err(MetadataError::Input {
            source: source.clone(),
            path: path.clone(),
            reason,
        });
    }
    Ok(())
}

fn validate_selections(
    composition: &gazette_compiler::workspace::ResolvedComposition,
    effective_theme: &theme_graph::EffectiveTheme,
    plugins: &BTreeMap<PluginPackageId, PluginPackage<'_>>,
    themes: &BTreeMap<ThemePackageId, ThemePackage<'_>>,
) -> Result<(), MetadataError> {
    for resource in effective_theme.resource_requirements().iter().chain(
        effective_theme
            .render()
            .social_card
            .iter()
            .flat_map(|recipe| &recipe.resource_requirements),
    ) {
        if !matches!(
            composition
                .resources()
                .get(resource)
                .map(|selection| selection.value()),
            Some(ResourceSelection::Enabled { .. })
        ) {
            return Err(MetadataError::Selection {
                owner: resource.to_string(),
                reason: "Theme requires an independently enabled logical resource selection",
            });
        }
    }
    for capability in effective_theme.required_capabilities() {
        if !matches!(
            composition
                .capabilities()
                .get(capability)
                .map(|selection| selection.value()),
            Some(CapabilitySelection::Enabled {
                requirement: Requirement::Required,
                ..
            })
        ) {
            return Err(MetadataError::Selection {
                owner: capability.to_string(),
                reason: "root Theme requires an enabled capability provider with required failure policy",
            });
        }
    }
    for (capability, selection) in composition.capabilities() {
        let CapabilitySelection::Enabled { provider, .. } = selection.value() else {
            continue;
        };
        let ProviderRef::Plugin { package, provider } = provider else {
            if !super::capabilities::gazette_provider_known(provider.provider()) {
                return Err(MetadataError::ProviderUnavailable {
                    provider: provider.provider().clone(),
                });
            }
            if !super::capabilities::gazette_provider_implements(provider.provider(), capability) {
                return Err(MetadataError::Selection {
                    owner: provider.provider().to_string(),
                    reason: "selected Gazette provider does not implement the selected capability",
                });
            }
            continue;
        };
        let package = plugins.get(package).ok_or_else(|| MetadataError::Binding {
            owner: package.to_string(),
            reason: "selected provider package was not admitted",
        })?;
        let declaration = package
            .parsed
            .manifest()
            .providers
            .get(provider)
            .ok_or_else(|| MetadataError::Selection {
                owner: provider.to_string(),
                reason: "selected provider is not exported by its plugin package",
            })?;
        if declaration.implements.binary_search(capability).is_err() {
            return Err(MetadataError::Selection {
                owner: provider.to_string(),
                reason: "selected provider does not implement the selected capability",
            });
        }
    }
    for (identity, selection) in composition.resources() {
        let ResourceSelection::Enabled { resource, use_, .. } = selection.value() else {
            continue;
        };
        let recipe = match &resource.owner {
            ResourceOwner::Theme(package) => themes
                .get(package)
                .and_then(|entry| {
                    entry
                        .parsed
                        .manifest()
                        .resources
                        .iter()
                        .find(|candidate| candidate.id == resource.resource)
                })
                .map(|resource| &resource.build),
            ResourceOwner::Plugin(package) => plugins
                .get(package)
                .and_then(|entry| entry.parsed.manifest().resources.get(&resource.resource))
                .map(|resource| &resource.build),
        }
        .ok_or_else(|| MetadataError::Selection {
            owner: resource.resource.to_string(),
            reason: "selected resource is not exported by its package",
        })?;
        if *use_ == ResourceUse::Dependency
            && !effective_theme
                .render()
                .social_card
                .as_ref()
                .is_some_and(|recipe| recipe.resource_requirements.contains(identity))
            && !matches!(recipe, gazette_core::resource::Recipe::BrowserModule { .. })
        {
            return Err(MetadataError::Selection {
                owner: resource.resource.to_string(),
                reason: "dependency resource must use a browser-module recipe",
            });
        }
    }
    Ok(())
}
