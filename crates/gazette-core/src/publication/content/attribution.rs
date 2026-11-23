//! Direct content roles, independent of site publishers and source provenance.

use serde::{Deserialize, Serialize};

use super::Key;

/// An explicitly declared relationship to this content object only.
/// A role never supplies another role, permission, consent or an inherited default.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum AttributionRole {
    /// Authorship of the specific object, not site ownership or affiliation.
    Author,
    /// Declared holder of rights in this object, not authorship or a licence grant.
    RightsHolder,
}

/// Direct reference to a separately declared principal in a specific content role.
/// The compiler binds the key; owner evidence must separately admit the claim.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    /// Exact principal key; names, contacts, repository owners and sites are not joins.
    pub principal: Key,
    /// Independent declared role; identical keys may explicitly hold different roles.
    pub role: AttributionRole,
}
