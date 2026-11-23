//! Workspace-owned package capture bindings, separate from package-authored manifests.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt,
    str::FromStr,
};

use gazette_core::{plugin::PluginPackageId, source::EntryPath};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::Key;
use crate::theme_package::ThemePackageId;

/// A workspace-local shared resource input name, not a source, package or provider identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceInputName(String);

impl ResourceInputName {
    /// Exact bounded lowercase ASCII spelling, without normalization or inferred aliases.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ResourceInputName {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || value.len() > 64
            || !value.as_bytes()[0].is_ascii_lowercase()
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err("resource input name must match [a-z][a-z0-9_-]{0,63}");
        }
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for ResourceInputName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ResourceInputName {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        String::deserialize(decoder)?
            .parse()
            .map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for ResourceInputName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ResourceInputName".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": r"^[a-z][a-z0-9_-]{0,63}(?![\s\S])",
            "minLength": 1,
            "maxLength": 64,
            "description": "Workspace-local shared resource input name; not a source, package or provider identity."
        })
    }
}

/// One exact retained file supplying a package's logical resource input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceInputMount {
    /// Explicit shared input name resolved by this workspace, never an inferred token source.
    pub input: ResourceInputName,
    /// Literal file entry beneath that input's retained source capture.
    pub entry: EntryPath,
}

/// One exact Theme package capture and the manifest within that capture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemePackageBinding {
    /// Package identity expected from the retained manifest.
    pub package: ThemePackageId,
    /// Existing source capture; source declarations own capture coordinates only.
    pub source: Key,
    /// Literal manifest path beneath the selected capture.
    pub manifest: EntryPath,
    /// Exact logical resource paths supplied by explicit retained shared inputs.
    /// Targets are package-manifest input paths, not templates or native mount points.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(schema_with = "resource_inputs_schema")]
    pub resource_inputs: BTreeMap<EntryPath, ResourceInputMount>,
}

fn resource_inputs_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    // Schemars's generic map projection extracts a key pattern only. EntryPath
    // also owns component/control negations, so preserve its complete schema.
    schemars::json_schema!({
        "type": "object",
        "propertyNames": generator.subschema_for::<EntryPath>(),
        "additionalProperties": generator.subschema_for::<ResourceInputMount>()
    })
}

/// One exact plugin package capture and the manifest within that capture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageBinding {
    /// Package identity expected from the retained manifest.
    pub package: PluginPackageId,
    /// Existing source capture; source declarations own capture coordinates only.
    pub source: Key,
    /// Literal manifest path beneath the selected capture.
    pub manifest: EntryPath,
}

pub(super) fn index(
    themes: Vec<ThemePackageBinding>,
    plugins: Vec<PluginPackageBinding>,
    sources: &BTreeMap<Key, super::Source>,
    design_inputs: &BTreeMap<ResourceInputName, Key>,
    errors: &mut Vec<super::Diagnostic>,
) -> (
    BTreeMap<ThemePackageId, ThemePackageBinding>,
    BTreeMap<PluginPackageId, PluginPackageBinding>,
) {
    let mut theme_packages = BTreeMap::new();
    for binding in themes {
        if !sources.contains_key(&binding.source) {
            super::resolve::report(
                errors,
                "workspace_reference",
                "theme_packages.source",
                Some(&binding.source),
            );
        }
        for (target, mount) in &binding.resource_inputs {
            if !design_inputs.contains_key(&mount.input) {
                super::resolve::report(
                    errors,
                    "workspace_resource_input_reference",
                    &format!(
                        "theme_packages[{:?}].resource_inputs[{:?}].input",
                        binding.package.as_str(),
                        target.as_str()
                    ),
                    Some(binding.package.as_key()),
                );
            }
        }
        match theme_packages.entry(binding.package.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(binding);
            }
            Entry::Occupied(entry) => super::resolve::report(
                errors,
                "workspace_duplicate",
                "theme_packages",
                Some(entry.key().as_key()),
            ),
        }
    }

    let mut plugin_packages = BTreeMap::new();
    for binding in plugins {
        if !sources.contains_key(&binding.source) {
            super::resolve::report(
                errors,
                "workspace_reference",
                "plugin_packages.source",
                Some(&binding.source),
            );
        }
        match plugin_packages.entry(binding.package.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(binding);
            }
            Entry::Occupied(entry) => errors.push(super::Diagnostic {
                code: "workspace_duplicate",
                field: format!("plugin_packages[{:?}]", entry.key().as_str()),
                key: None,
            }),
        }
    }
    (theme_packages, plugin_packages)
}
