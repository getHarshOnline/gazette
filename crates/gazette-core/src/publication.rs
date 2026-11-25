//! Publication vocabulary. Declared state is neither evidence nor permission.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

mod catalog;
pub use catalog::{
    Principal, PrincipalKind, PrincipalWebsite, Project, ProjectAvailability, ProjectOrigin,
    ProjectPlacementRole, ProjectSource, PublicationRole, PublicationRoleBinding, ResearchPaper,
    Taxonomy, TaxonomyTerm,
};
mod capability;
pub use capability::{CapabilityId, CoreCapabilityCatalog, InvalidCapabilityId};
pub mod browser;
pub mod capability_runtime;
mod project_resources;
pub use project_resources::ProjectResources;
mod project_kind;
pub use project_kind::ProjectKind;
mod project_license;
pub use project_license::{InvalidProjectLicense, ProjectLicense};
pub mod content;
mod discussion;
pub use discussion::Discussion;
mod extension;
pub use extension::ExtensionSchema;
pub mod facts;
mod language;
pub use language::{InvalidLanguageTag, LanguageTag};
pub mod registry;
pub mod routing;
pub mod usage;

/// Content state is independent of project continuity, visibility and availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContentLifecycle {
    /// Authored work not selected as published content.
    Draft,
    /// Owner-declared published state, still subject to admission gates.
    Published,
    /// Retained historical content; not a retired-project inference.
    Archived,
}

/// An opaque namespaced owner key, preserved exactly rather than derived from a label.
///
/// The unreleased grammar requires nonempty namespace and local parts separated
/// by `:` and excludes whitespace/control characters. It does not prove identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Key(String);

impl schemars::JsonSchema for Key {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PublicationKey".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // JSON Schema's portable Unicode ranges mirror Rust's White_Space and
        // Cc checks. Keep this projection beside its native validator, not in an
        // interface-specific copy of the identity grammar.
        schemars::json_schema!({
            "type": "string",
            "description": "Exact opaque namespace:local owner key; not proof of identity.",
            "pattern": "^[^:]+:.+",
            "not": {"pattern": r"[\u0000-\u0020\u007f-\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]"}
        })
    }
}

/// A key lacks its namespace/local part or contains whitespace/control characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidKey;

impl fmt::Display for InvalidKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a namespaced key with nonempty parts and no whitespace or controls")
    }
}

impl std::error::Error for InvalidKey {}

impl Key {
    /// The exact declared key; no case folding or contact-based normalization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Key {
    type Error = InvalidKey;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (namespace, local) = value.split_once(':').ok_or(InvalidKey)?;
        if namespace.is_empty()
            || local.is_empty()
            || value
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err(InvalidKey);
        }
        Ok(Self(value))
    }
}

impl FromStr for Key {
    type Err = InvalidKey;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Project continuity, independent of visibility and commercial availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectLifecycle {
    /// Intended work that has not entered development.
    Planned,
    /// Active work.
    Active,
    /// Explicitly paused work.
    Paused,
    /// Work that has been completed.
    Done,
    /// An ended project or relationship.
    Closed,
}

/// Owner-declared visibility; not a substitute for output admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    /// Publicly visible when applicable gates pass.
    Public,
    /// Not listed in discovery surfaces.
    Unlisted,
    /// Not public.
    Private,
}

/// Availability is not inferred from project lifecycle or visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    /// Availability is not applicable to this object.
    NotApplicable,
    /// Not available.
    Unavailable,
    /// Available only through an explicit gate.
    Gated,
    /// Declared available; operational evidence is separately required.
    Available,
}
