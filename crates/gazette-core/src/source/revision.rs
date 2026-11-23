//! Literal Git coordinates and declared observation limits.

use super::Limits;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::str::FromStr;

/// Whether a bounded literal local branch name can be checked without ref expansion.
///
/// Acquisition and retained provenance share this spelling contract. A valid name
/// establishes neither a repository nor a commit; the native reader checks HEAD.
pub fn valid_branch_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !matches!(value, "@" | "HEAD")
        && !value.starts_with('-')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("@{")
        && !value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || "~^:?*[\\".contains(ch))
        && value
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

/// A full lowercase SHA-1 Git commit coordinate, never a moving ref or prefix.
///
/// Parsing checks the coordinate's spelling, not whether an object exists or is
/// a commit. Other Git object formats require explicit adapter support.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct CommitId(#[schemars(length(min = 40, max = 40), pattern("^[0-9a-f]{40}$"))] String);

/// A value is not the supported exact commit coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCommitId;

impl fmt::Display for InvalidCommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "use a full 40-character lowercase Git commit ID, not a branch, tag or abbreviation",
        )
    }
}

impl std::error::Error for InvalidCommitId {}

impl FromStr for CommitId {
    type Err = InvalidCommitId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 40
            || !value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(InvalidCommitId);
        }
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CommitId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl CommitId {
    /// The exact canonical coordinate supplied by the caller.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Limits for selected source content and the Git metadata needed to reach it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RevisionLimits {
    /// Source-tree entry, depth and file-byte ceilings.
    pub content: Limits,
    /// Combined decoded commit/tree bytes examined, including ancestor trees.
    #[schemars(range(max = usize::MAX))]
    pub metadata_bytes: usize,
}

impl Default for RevisionLimits {
    fn default() -> Self {
        Self {
            content: Limits::default(),
            // Bound logical metadata loaded to reach and enumerate a source.
            // This is not an OS memory limit on the native Git library.
            metadata_bytes: 16 << 20,
        }
    }
}
