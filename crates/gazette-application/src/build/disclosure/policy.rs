//! Instance-selected prohibitions; class completeness is not owner approval.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{publication::Key, source::Sha256Digest};
use serde::{Deserialize, Serialize};

use super::{DisclosureProbeError, DisclosureProbeLimits};
use crate::build::serialization::encode;

pub use gazette_compiler::disclosure::Pattern as DisclosurePattern;

/// Why the policy owner prohibits a match, not a classification inferred from output.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum DisclosureClass {
    /// Absolute local paths, including explicitly selected socket locations.
    LocalPath,
    /// Private hosts, addresses, networks or exact endpoint coordinates.
    PrivateEndpoint,
    /// Exact supplied credentials, secret material or private environment values.
    Secret,
    /// Engine/instance-selected registry, database, WAL or generated-artifact identifiers.
    GeneratedArtifact,
    /// Owner-selected test, fixture or unapproved-claim tokens.
    FixtureClaim,
    /// Internal identifiers and endpoint names not intended for publication.
    InternalIdentifier,
}

impl DisclosureClass {
    const REQUIRED: [Self; 6] = [
        Self::LocalPath,
        Self::PrivateEndpoint,
        Self::Secret,
        Self::GeneratedArtifact,
        Self::FixtureClaim,
        Self::InternalIdentifier,
    ];
}

/// A private prohibition and its explicit owner-selected purpose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DisclosureRule {
    /// Declared purpose; matching a class does not establish factual or policy authority.
    pub class: DisclosureClass,
    /// Bounded lexical predicate, applied to filenames and every supported decoded channel.
    pub pattern: DisclosurePattern,
}

/// Complete explicit rule selection for a policy check, with no guessed private defaults.
///
/// All six classes must be represented. This verifies coverage declarations, not their
/// adequacy: the instance owner must supply the actual prohibited values and scope.
/// There is no secret guessing, automatic code-example exemption or output rewriting.
/// Decoding supplies selections, not an authenticated policy document or approval.
/// Native validation must still precede effects; transport owns duplicate/shape checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DisclosurePolicy {
    /// Stable private rule identifiers. Ordered encoding makes insertion order irrelevant.
    pub rules: BTreeMap<Key, DisclosureRule>,
}

impl DisclosurePolicy {
    /// Validate complete selection and matcher syntax before any preparation effect.
    /// Returns the digest of the exact bounded, key-ordered policy encoding.
    ///
    /// # Errors
    /// Rejects missing classes, invalid predicates and rule/encoded-byte ceilings.
    pub fn validate(
        &self,
        limits: DisclosureProbeLimits,
    ) -> Result<Sha256Digest, DisclosureProbeError> {
        limits.validate()?;
        if self.rules.len() > limits.rules {
            return Err(DisclosureProbeError::Limit("rule count"));
        }
        let bytes = encode(self, limits.rules_bytes, "disclosure policy")
            .map_err(|_| DisclosureProbeError::Limit("rule bytes"))?;
        let mut classes = BTreeSet::new();
        for rule in self.rules.values() {
            rule.pattern
                .validate()
                .map_err(|_| DisclosureProbeError::Policy("invalid matcher"))?;
            classes.insert(rule.class);
        }
        if DisclosureClass::REQUIRED
            .iter()
            .any(|class| !classes.contains(class))
        {
            return Err(DisclosureProbeError::Policy("missing required class"));
        }
        Ok(Sha256Digest::of(&bytes))
    }
}

/// Algorithm/profile binding only. Engine bytes and policy authority remain separate gates.
pub(super) fn generic_rules_digest() -> Sha256Digest {
    super::technical_profile::generic_rules_digest()
}
