//! Exact ordered target selection for read-only observation, independent of signature expiry.

use super::{Error, Limits, Target, format, verify};
use gazette_core::source::Sha256Digest;

/// Structurally checked target set; not publisher approval or durable consumed intent.
/// No deserializer or public constructor bypasses the shared action validator.
pub struct TargetSet {
    targets: Vec<Target>,
    identity: Sha256Digest,
}

impl TargetSet {
    /// Complete exact ordered selection; no caller can mutate the checked fields.
    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// SHA-256 of `gazette.release-targets/1\0` followed by RFC 8785 target-array bytes.
    /// This binds selection, not mutable remote observations or an artifact's identity.
    pub fn identity(&self) -> Sha256Digest {
        self.identity
    }
}

/// Decode and check an entire exact target array before any native destination reads.
/// This requires no active signature because read-only recovery must survive expiry.
///
/// # Errors
/// Refuses malformed/over-limit fields, duplicates, unsupported actions or noncontiguous order.
pub fn parse_targets(bytes: &[u8], limits: Limits) -> Result<TargetSet, Error> {
    let (targets, value) = format::decode::<Vec<Target>>(bytes, limits)?;
    verify::check_targets(&targets, limits)?;
    let canonical = format::canonical(&value, limits)?;
    let mut identity = b"gazette.release-targets/1\0".to_vec();
    identity.extend_from_slice(&canonical);
    Ok(TargetSet {
        targets,
        identity: Sha256Digest::of(&identity),
    })
}

/// Check native typed targets through the same complete wire/semantic boundary.
///
/// # Errors
/// Refuses unsupported fields, order, duplicates, action spelling and work bounds.
pub fn check_targets(targets: &[Target], limits: Limits) -> Result<TargetSet, Error> {
    parse_targets(&format::encode(&targets, limits)?, limits)
}
