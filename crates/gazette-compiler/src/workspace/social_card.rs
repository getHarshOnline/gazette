//! Explicit native program selections for the engine-owned social-card capture helper.

use gazette_core::process::PinnedExecutable;
use serde::{Deserialize, Serialize};

/// Pinned interpreter and browser; helper bytes, arguments and environment are engine-owned.
/// These pins do not attest fonts, shared libraries or other ambient runtime inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SocialCardRenderer {
    /// Absolute Node executable with mandatory exact byte length and SHA-256.
    #[serde(deserialize_with = "crate::tables::one")]
    pub node: PinnedExecutable,
    /// Absolute browser executable with mandatory exact byte length and SHA-256.
    #[serde(deserialize_with = "crate::tables::one")]
    pub browser: PinnedExecutable,
    /// Capture JPEG at this quality when selected. Otherwise capture PNG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 100))]
    pub jpeg_quality: Option<u8>,
}
