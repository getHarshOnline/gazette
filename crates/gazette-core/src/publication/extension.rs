//! Workspace-selected domain contracts; validation never grants publication authority.

use super::Key;
use serde::{Deserialize, Serialize};

/// One owner-selected data schema, independent of Theme components or rendering.
/// Its exact declaration belongs to the workspace input and compilation identity.
/// A valid schema constrains shape; it does not establish the truth, rights or
/// public admissibility of values which happen to satisfy it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSchema {
    /// Exact reference used by extension content; never inferred from its page title.
    pub id: Key,
    /// Explicit local JSON Schema. The compiler selects the supported dialect and
    /// rejects external acquisition before binding any content value.
    pub schema: serde_json::Value,
}
