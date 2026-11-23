//! Explicit local Git-object preparation, separate from durable release admission.

use crate::source::{CommitId, RevisionLimits, TreeIdentity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Git authorship metadata, not a cryptographic signature or publisher role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitSignature {
    /// Exact name; normalization or ambient Git configuration is forbidden.
    pub name: String,
    /// Exact email, independent from authority or identity admission.
    pub email: String,
    /// Explicit nonnegative Unix seconds; never supplied by the adapter's clock.
    #[schemars(range(min = i64::MIN, max = i64::MAX))]
    pub seconds: i64,
    /// Explicit timezone offset in minutes, strictly within one day.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub offset_minutes: i32,
}

/// Complete deterministic commit recipe; no inferred parent, identity or time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitMetadata {
    /// Null means an explicitly selected root commit, not an unknown remote state.
    pub parent: Option<CommitId>,
    /// Exact Git author metadata.
    pub author: CommitSignature,
    /// Exact Git committer metadata.
    pub committer: CommitSignature,
    /// Complete UTF-8 message, retained byte-for-byte without cleanup.
    pub message: String,
}

impl CommitMetadata {
    /// Decode the complete bounded JSON object without defaults or positional forms.
    /// Duplicate decoded keys are refused at every depth; `parent: null` is explicit.
    /// This checks representation only. [`Self::validate`] and native verification
    /// retain ownership of recipe validity and object evidence.
    ///
    /// # Errors
    /// Refuses the byte ceiling, invalid JSON or an incomplete/non-native object shape.
    pub fn parse_json(bytes: &[u8], maximum: usize) -> Result<Self, CommitError> {
        if bytes.len() > maximum {
            return Err(CommitError::refused("metadata input exceeds byte ceiling"));
        }
        let original =
            crate::json::parse(bytes).map_err(|_| CommitError::refused("invalid metadata JSON"))?;
        let metadata = Self::deserialize(&original)
            .map_err(|_| CommitError::refused("invalid metadata JSON"))?;
        // The serialized owner defines required fields and nested object shape;
        // do not maintain a second transport-specific recipe declaration.
        if serde_json::to_value(&metadata).ok().as_ref() != Some(&original) {
            return Err(CommitError::refused(
                "metadata must contain the complete native object shape",
            ));
        }
        Ok(metadata)
    }

    /// Validate the shared exact recipe grammar without native access or identity inference.
    /// Provider-specific encoding, object existence and storage bounds remain adapter checks.
    ///
    /// # Errors
    /// Refuses missing/normalized/control-bearing names, invalid explicit times or an
    /// empty/NUL-bearing message before any native write is permitted.
    pub fn validate(&self) -> Result<(), CommitError> {
        if self.message.contains('\0') || self.message.is_empty() {
            return Err(CommitError::refused(
                "explicit nonempty NUL-free commit message is required",
            ));
        }
        for signature in [&self.author, &self.committer] {
            if signature.seconds < 0
                || !(-1439..=1439).contains(&signature.offset_minutes)
                || [&signature.name, &signature.email].into_iter().any(|text| {
                    text.is_empty() || text.trim() != text || text.chars().any(char::is_control)
                })
            {
                return Err(CommitError::refused(
                    "explicit commit signature metadata is invalid",
                ));
            }
        }
        Ok(())
    }
}

/// Explicit local object-write permission and bounded content/metadata access.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommitOptions {
    /// Permits objects in the exact selected local repository, never ref updates.
    pub allow_write_objects: bool,
    /// Complete content and decoded commit/tree metadata bounds.
    pub limits: RevisionLimits,
}

/// Complete local object read-back evidence, not a durable preparation receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CommitReport {
    /// Exact created or inspected commit; no branch or remote state is implied.
    pub commit: CommitId,
    /// Exact complete tree identity under the fixed non-executable blob profile.
    pub output: TreeIdentity,
    /// Explicit parent checked in the commit body.
    pub parent: Option<CommitId>,
    /// Complete inventory count, including nonempty directories.
    #[schemars(range(max = usize::MAX))]
    pub entries: usize,
    /// Total regular-file bytes in the committed tree.
    pub bytes: u64,
}

/// Conservative local object-write outcome; failures never authorize cleanup or retry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommitEffect {
    /// No object write was attempted.
    NotAttempted,
    /// An object write was attempted; unreferenced objects may remain.
    ObjectsMayRemain,
    /// Commit creation returned this OID, but complete read-back did not succeed.
    CommitCreated(CommitId),
}

/// Redacted technical failure with explicit local effect accounting.
#[derive(Debug)]
pub struct CommitError {
    /// Static diagnostic; never contains authored bytes, metadata or credentials.
    pub reason: &'static str,
    /// Known mutation boundary, independent from later successful receipt delivery.
    pub effect: CommitEffect,
}

impl CommitError {
    /// Refuse an operation before any local object write.
    pub fn refused(reason: &'static str) -> Self {
        Self {
            reason,
            effect: CommitEffect::NotAttempted,
        }
    }
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; local object effect {:?}; preserve and inspect before retrying",
            self.reason, self.effect
        )
    }
}

impl std::error::Error for CommitError {}
