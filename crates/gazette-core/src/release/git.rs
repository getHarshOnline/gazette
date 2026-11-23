//! Explicit native Git provider selection, independent of destinations and release authority.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One selected credential helper and its separate use permission.
///
/// The helper owns credential retrieval. Gazette never accepts, stores or forwards a secret.
/// Selection is inert for local transports and denied for HTTPS until `allow_credentials` is true.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitCredentialSelection {
    /// Absolute native helper executable, without arguments or shell syntax.
    #[schemars(regex(pattern = "^/[A-Za-z0-9/._-]+$"))]
    pub helper: PathBuf,
    /// Explicit permission for Git to invoke this helper for an HTTPS destination.
    #[serde(default)]
    pub allow_credentials: bool,
}

impl GitCredentialSelection {
    /// Validate only the explicit helper path, without opening it or granting permission.
    pub fn is_valid(&self) -> bool {
        self.helper.is_absolute()
            && self.helper.to_str().is_some_and(|helper| {
                helper.len() > 1
                    && helper.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-')
                    })
            })
    }
}

/// Fixed native Git reader/dispatcher selection shared by every release phase.
///
/// Omitted credentials mean anonymous transport. Ambient Git configuration, credential helpers,
/// askpass programs and process environment are never fallback sources.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitSelection {
    /// Explicit absolute Git executable.
    #[schemars(regex(pattern = "^/"))]
    pub program: PathBuf,
    /// Existing private empty isolation directory, held under trusted namespace control.
    #[schemars(regex(pattern = "^/"))]
    pub isolation: PathBuf,
    /// Optional independently selected HTTPS credential helper; never a credential value.
    #[serde(default)]
    pub credentials: Option<GitCredentialSelection>,
}

impl GitSelection {
    /// Whether all native paths have the literal profile required before any provider call.
    ///
    /// Credential helpers are deliberately restricted to an absolute shell-safe executable path:
    /// Git executes helper configuration through its helper command machinery, so arguments,
    /// whitespace and metacharacters are not accepted as part of this selection.
    pub fn is_valid(&self) -> bool {
        self.program.is_absolute()
            && self.isolation.is_absolute()
            && self
                .credentials
                .as_ref()
                .is_none_or(GitCredentialSelection::is_valid)
    }
}
