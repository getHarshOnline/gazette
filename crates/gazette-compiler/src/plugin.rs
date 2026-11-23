//! Captured plugin-package declarations; parsing grants neither execution nor admission.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::{
    markdown::ComponentName,
    plugin::{PluginPackageId, PluginPackageIdentity},
    publication::{CapabilityId, CoreCapabilityCatalog, capability_runtime::ProviderId},
    resource::{Kind as ResourceKind, Recipe, ResourceId},
    source::{EntryPath, Sha256Digest},
};
use serde::{Deserialize, Serialize};

use crate::toml::{self, ParsedToml};

/// Strict package manifest; it describes exports but contains no executable loader contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Stable human-authored package name. Digests, not a version string, complete identity.
    pub package: PluginPackageId,
    /// Optional package-configuration contract stored inside the captured package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_schema: Option<EntryPath>,
    /// Provider implementations exported by this package.
    #[serde(default)]
    pub providers: BTreeMap<ProviderId, ProviderDeclaration>,
    /// Logical resources exported by this package.
    #[serde(default)]
    pub resources: BTreeMap<ResourceId, ResourceDeclaration>,
    /// Package-owned capability extensions and their local schemas.
    #[serde(default)]
    pub extensions: BTreeMap<CapabilityId, ExtensionDeclaration>,
    /// Plugin component contracts; Theme presentation remains a separate package graph.
    #[serde(default)]
    pub components: BTreeMap<ComponentName, ComponentDeclaration>,
}

/// One concrete provider role; invocation mechanics belong to later runtime adapters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderDeclaration {
    /// Capability contracts this provider promises to supply.
    pub implements: Vec<CapabilityId>,
}

/// One package-local resource compiled through Gazette's bounded retained-input contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceDeclaration {
    /// Complete declared source closure inside the captured plugin tree.
    pub inputs: Vec<EntryPath>,
    /// Existing typed resource recipe; no generic executor or dynamic loader.
    pub build: Recipe,
}

/// One namespaced model-extension contract; the schema is a local captured file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtensionDeclaration {
    /// Literal JSON Schema path inside the captured plugin tree.
    pub schema: EntryPath,
}

/// One plugin component's local argument/model contracts and optional browser module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentDeclaration {
    /// Captured-local JSON Schema for authored component arguments.
    pub arguments: EntryPath,
    /// Captured-local JSON Schema for the admitted model projection consumed by the component.
    pub model: EntryPath,
    /// Optional declared browser-module resource; omission means no client enhancement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<ResourceId>,
}

/// Syntax or complete-declaration rejection; no partial package is returned.
#[derive(Debug)]
pub enum Error {
    /// Shared bounded TOML parser/deserializer failure.
    Syntax(toml::Error),
    /// A cross-reference or package invariant failed.
    Declaration {
        /// Exact declaration that owns the invalid relationship.
        owner: String,
        /// Stable reason suitable for interface projection.
        reason: &'static str,
    },
}

impl Error {
    /// Stable failure class shared by interfaces.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Syntax(error) => error.code(),
            Self::Declaration { .. } => "plugin_manifest_invalid",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::Declaration { owner, reason } => write!(formatter, "plugin {owner}: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::Declaration { .. } => None,
        }
    }
}

/// Retained native TOML plus one completely validated package declaration.
#[derive(Debug)]
pub struct ParsedManifest<'s> {
    syntax: ParsedToml<'s>,
    manifest: Manifest,
}

impl<'s> ParsedManifest<'s> {
    /// Parse and validate all exports against Gazette's closed core capability catalog.
    ///
    /// The catalog is Gazette contract knowledge, not a provider selection. Package-owned
    /// extensions must use the package namespace and be implemented by an exported provider.
    ///
    /// # Errors
    ///
    /// Rejects bounded TOML syntax/schema failures, unknown fields, invalid identifiers,
    /// nonlocal paths and any provider, resource, component or extension reference that
    /// does not close over the complete declaration and Engine-owned core catalog.
    pub fn parse(source: &'s str, limits: toml::Limits) -> Result<Self, Error> {
        let syntax = ParsedToml::parse(source, limits).map_err(Error::Syntax)?;
        let mut manifest: Manifest = syntax.deserialize().map_err(Error::Syntax)?;
        validate(&mut manifest)?;
        Ok(Self { syntax, manifest })
    }

    /// Validated declaration; it is not a loaded provider or an admitted result.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Shared retained syntax and source coordinates for diagnostics/editor consumers.
    pub fn syntax(&self) -> &ParsedToml<'s> {
        &self.syntax
    }

    /// Complete immutable identity for this captured package.
    pub fn identity(&self, source: Sha256Digest, tree: Sha256Digest) -> PluginPackageIdentity {
        PluginPackageIdentity {
            package: self.manifest.package.clone(),
            source,
            manifest: Sha256Digest::of(self.syntax.source().as_bytes()),
            tree,
        }
    }
}

fn validate(manifest: &mut Manifest) -> Result<(), Error> {
    if manifest.providers.is_empty()
        && manifest.resources.is_empty()
        && manifest.extensions.is_empty()
        && manifest.components.is_empty()
    {
        return declaration(
            &manifest.package,
            "package must export a provider, resource, extension or component",
        );
    }

    let extension_prefix = format!("{}.", manifest.package);
    for capability in manifest.extensions.keys() {
        let contract = capability
            .as_str()
            .split_once('/')
            .expect("validated capability")
            .0;
        if !contract.starts_with(&extension_prefix) {
            return declaration(
                capability,
                "extension capability must use the package namespace",
            );
        }
    }

    let mut implemented = BTreeSet::new();
    for (provider, provider_declaration) in &mut manifest.providers {
        if provider_declaration.implements.is_empty() {
            return declaration(provider, "provider must implement at least one capability");
        }
        unique(
            provider,
            &mut provider_declaration.implements,
            "provider repeats a capability",
        )?;
        for capability in &provider_declaration.implements {
            if !CoreCapabilityCatalog::contains(capability)
                && !manifest.extensions.contains_key(capability)
            {
                return declaration(provider, "provider implements an undeclared capability");
            }
            implemented.insert(capability.clone());
        }
    }

    for (resource, resource_declaration) in &mut manifest.resources {
        if resource_declaration.inputs.is_empty() {
            return declaration(resource, "resource must declare at least one input");
        }
        unique(
            resource,
            &mut resource_declaration.inputs,
            "resource repeats an input",
        )?;
        if resource_declaration.build.validate_imports().is_err() {
            return declaration(resource, "resource declares invalid module import bindings");
        }
        if resource_declaration
            .inputs
            .binary_search(resource_declaration.build.entry())
            .is_err()
        {
            return declaration(resource, "resource build entry must belong to its inputs");
        }
    }

    for (component, component_declaration) in &manifest.components {
        let Some(module) = &component_declaration.module else {
            continue;
        };
        let Some(resource) = manifest.resources.get(module) else {
            return declaration(
                component,
                "component references an undeclared module resource",
            );
        };
        if resource.build.kind() != ResourceKind::BrowserModule {
            return declaration(
                component,
                "component module must be a browser-module resource",
            );
        }
    }

    if let Some(capability) = manifest
        .extensions
        .keys()
        .find(|capability| !implemented.contains(*capability))
    {
        return declaration(
            capability,
            "extension capability has no exported provider implementation",
        );
    }
    Ok(())
}

fn unique<T: Ord>(
    owner: impl fmt::Display,
    values: &mut [T],
    reason: &'static str,
) -> Result<(), Error> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        declaration(owner, reason)
    } else {
        Ok(())
    }
}

fn declaration<T>(owner: impl fmt::Display, reason: &'static str) -> Result<T, Error> {
    Err(Error::Declaration {
        owner: owner.to_string(),
        reason,
    })
}
