//! Owner-native public discussion configuration, separate from provider readiness.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::CapabilityId;

/// One complete discussion atom on a site or project, never inherited from a placement.
/// The capability selects implementation through the existing workspace composition.
/// These values are public configuration, not credentials or proof of activation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Discussion {
    /// Exact selected capability; no provider is guessed from the repository host.
    pub capability: CapabilityId,
    /// Opaque provider-side repository identity, distinct from its public address.
    pub repository_id: String,
    /// Opaque provider-side category identity; no display-label matching.
    pub category_id: String,
    /// Public presentation options. They never override the owner, repository,
    /// category or capability fields, and do not grant executable effects.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

impl Discussion {
    /// Check complete bounded public values; provider compatibility remains separate.
    pub fn is_valid(&self) -> bool {
        let identity = |value: &str| {
            !value.is_empty()
                && value.len() <= 256
                && !value
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control())
        };
        identity(&self.repository_id)
            && identity(&self.category_id)
            && self.options.len() <= 64
            && self.options.iter().all(|(key, value)| {
                !key.is_empty()
                    && key.len() <= 64
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                    && value.len() <= 4096
                    && !value.chars().any(char::is_control)
            })
            && self
                .options
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>()
                <= 16 * 1024
    }
}
