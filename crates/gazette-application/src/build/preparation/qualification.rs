//! Supported-profile technical qualification over existing immutable preparation evidence.

use std::collections::BTreeMap;

use gazette_core::source::{EntryPath, Sha256Digest, TreeIdentity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};

use super::{PreparationError, PreparationFailure};
use crate::build::{OutputPolicyRequirement, PublicationRender, serialization};

/// Independent expectations, never inferred from the check being qualified.
/// This binds repository-source provenance, the running Engine/build profile and
/// render/provider declarations, not a complete executable image, feature/toolchain
/// attestation, browser/serving behavior, policy authority or publisher attestation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparationQualification {
    /// Exact required disclosure scanner profile.
    pub scanner: String,
    /// Exact required recursive encoded-value profile.
    pub encoded_profile: String,
    /// Exact canonical/caseless filename inspection profile.
    pub filename_profile: String,
    /// Exact required generic decoding and matching procedure digest.
    pub generic_rules_sha256: Sha256Digest,
    /// Exact deterministic scanner-resource projection profile.
    pub resource_profile: String,
    /// SHA-256 of every exact parser, decoder, traversal and retained-work ceiling.
    pub resource_limits_sha256: Sha256Digest,
    /// Exact zero-removal profile selected before immutable disclosure scanning.
    pub exclusion_profile: String,
    /// Exact removed paths. The `0.1` technical profile admits only an empty list.
    pub removed: Vec<EntryPath>,
    /// Exact independently selected raster metadata providers used by retained coverage.
    /// Keys are the closed `png`, `jpeg` and `webp` names; absence requires no raster coverage.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raster_profiles: BTreeMap<String, String>,
    /// Exact required instance disclosure-policy projection digest.
    pub disclosure_policy_sha256: Sha256Digest,
    /// SHA-256 of the original compact repository-source provenance subtree.
    /// This binds observed repository bytes, never executable, feature or toolchain identity.
    pub repository_source_provenance_sha256: Sha256Digest,
    /// Independently selected running Gazette Engine version and Cargo build profile.
    pub runtime_build_profile: crate::build::RuntimeBuildProfile,
    /// SHA-256 of the original compact `rendering` JSON subtree, including provider evidence.
    pub rendering_sha256: Sha256Digest,
    /// Independently selected pinned checker and retained policy requirements.
    pub output_policy: OutputPolicyRequirement,
    /// Require checker-originated candidate decisions under this exact retained policy.
    /// Omission makes no candidate-policy claim; neither profile grants publisher approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "crate::build::CandidatePolicyRequirement")]
    pub candidate_policy: Option<crate::build::CandidatePolicyRequirement>,
}

impl PreparationQualification {
    /// Validate bounded profile names and the native policy-response allowance.
    ///
    /// # Errors
    /// Refuses malformed expectations without native reads or effects.
    pub fn validate(&self) -> Result<(), PreparationError> {
        if !crate::build::disclosure::technical_profile::coordinates_match(
            &self.scanner,
            &self.encoded_profile,
            &self.filename_profile,
            self.generic_rules_sha256,
        ) || self.resource_profile != crate::build::DisclosureProbeLimits::RESOURCE_PROFILE
            || !crate::build::disclosure::technical_profile::zero_exclusion_matches(
                &self.exclusion_profile,
                &self.removed,
            )
            || !crate::build::disclosure::technical_profile::valid_raster_requirements(
                &self.raster_profiles,
            )
            || !self.runtime_build_profile.is_valid()
            || self.runtime_build_profile != crate::build::runtime_build_profile()
            || self.output_policy.validate().is_err()
            || self
                .candidate_policy
                .as_ref()
                .is_some_and(|candidate| candidate.policy != self.output_policy.policy)
        {
            return Err(mismatch());
        }
        Ok(())
    }

    /// Match the chosen scan limits before acquiring sources or creating preparation effects.
    ///
    /// # Errors
    /// Refuses invalid limits or a digest different from the independent expectation.
    pub fn check_resource_limits(
        &self,
        limits: crate::build::DisclosureProbeLimits,
    ) -> Result<(), PreparationError> {
        self.validate()?;
        limits
            .validate()
            .map_err(|error| PreparationError::before(PreparationFailure::Disclosure(error)))?;
        if limits.resource_limits_sha256() != self.resource_limits_sha256 {
            return Err(mismatch());
        }
        Ok(())
    }

    /// Check independent input/render identities before any output-policy invocation.
    /// This is a binding preflight, not a disclosure check or technical certificate.
    ///
    /// # Errors
    /// Refuses unsuitable output, mismatched exact identities or evidence byte limits.
    pub fn check_render(
        &self,
        rendered: &PublicationRender<'_>,
        maximum: usize,
    ) -> Result<(), PreparationError> {
        self.validate()?;
        serialization::encode(self, maximum, "qualification requirements")
            .map_err(|error| PreparationError::before(PreparationFailure::Evidence(error)))?;
        if let Some(required) = &self.candidate_policy
            && rendered
                .candidate_admission()
                .and_then(crate::build::CandidateAdmission::requirement)
                != Some(required)
        {
            return Err(mismatch());
        }
        self.check_original(&rendered.preparation_evidence(maximum)?)
    }

    fn check_original(&self, original: &[u8]) -> Result<(), PreparationError> {
        let fields: BTreeMap<String, &RawValue> =
            serde_json::from_slice(original).map_err(|_| mismatch())?;
        for (key, expected) in [
            (
                "repository_source_provenance",
                self.repository_source_provenance_sha256,
            ),
            ("rendering", self.rendering_sha256),
        ] {
            let value = fields.get(key).ok_or_else(mismatch)?;
            if Sha256Digest::of(value.get().as_bytes()) != expected {
                return Err(mismatch());
            }
        }
        let runtime: crate::build::RuntimeBuildProfile = serde_json::from_str(
            fields
                .get("runtime_build_profile")
                .ok_or_else(mismatch)?
                .get(),
        )
        .map_err(|_| mismatch())?;
        if runtime != self.runtime_build_profile {
            return Err(mismatch());
        }
        if let Some(expected) = &self.candidate_policy
            && super::evidence::candidate_requirement(original)
                .map_err(|_| mismatch())?
                .as_ref()
                != Some(expected)
        {
            return Err(mismatch());
        }
        Ok(())
    }
}

/// A privately constructed supported-profile result, not whole-product certification,
/// public admission, a release grant or proof of deployment-server behavior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct TechnicalCertificate {
    profile: String,
    requirements: PreparationQualification,
    preparation_evidence_sha256: Sha256Digest,
    output: TreeIdentity,
    disclosure_sha256: Sha256Digest,
    policy_sha256: Sha256Digest,
}

impl TechnicalCertificate {
    /// Independently supplied expectations satisfied by the original observation.
    pub fn requirements(&self) -> &PreparationQualification {
        &self.requirements
    }

    /// Construct or rederive only after the owning required scan/policy checks succeed.
    pub(super) fn derive(
        requirements: &PreparationQualification,
        original: &[u8],
        output: TreeIdentity,
        disclosure: &[u8],
        policy: &[u8],
    ) -> Result<Self, PreparationError> {
        requirements.validate()?;
        requirements.check_original(original)?;
        let scan: Value = gazette_core::json::parse(disclosure).map_err(|_| mismatch())?;
        let checked: Value = gazette_core::json::parse(policy).map_err(|_| mismatch())?;
        let expected = [
            (
                "status",
                serde_json::json!("required-supported-profile-evidence"),
            ),
            ("scanner", serde_json::json!(requirements.scanner)),
            (
                "encoded_profile",
                serde_json::json!(requirements.encoded_profile),
            ),
            (
                "filename_profile",
                serde_json::json!(requirements.filename_profile),
            ),
            (
                "generic_rules_sha256",
                serde_json::json!(requirements.generic_rules_sha256),
            ),
            (
                "resource_profile",
                serde_json::json!(requirements.resource_profile),
            ),
            (
                "resource_limits_sha256",
                serde_json::json!(requirements.resource_limits_sha256),
            ),
            (
                "policy_sha256",
                serde_json::json!(requirements.disclosure_policy_sha256),
            ),
            ("output_identity", serde_json::json!(output)),
        ];
        if expected
            .iter()
            .any(|(key, value)| scan.get(key) != Some(value))
            || !crate::build::disclosure::technical_profile::report_matches(
                &scan,
                &requirements.raster_profiles,
                requirements.resource_limits_sha256,
            )
            || scan.get("findings") != Some(&serde_json::json!([]))
            || checked.get("status").and_then(Value::as_str) != Some("checked")
            || checked.pointer("/evidence/response/requirement")
                != Some(&serde_json::to_value(&requirements.output_policy).map_err(|_| mismatch())?)
        {
            return Err(mismatch());
        }
        Ok(Self {
            profile: crate::build::disclosure::technical_profile::PROFILE.into(),
            requirements: requirements.clone(),
            preparation_evidence_sha256: Sha256Digest::of(original),
            output,
            disclosure_sha256: Sha256Digest::of(disclosure),
            policy_sha256: Sha256Digest::of(policy),
        })
    }

    pub(super) fn inspect(
        value: &Value,
        original: &[u8],
        output: TreeIdentity,
        disclosure: &[u8],
        policy: &[u8],
        maximum: usize,
    ) -> Result<Self, PreparationError> {
        let requirements: PreparationQualification =
            serde_json::from_value(value.get("requirements").ok_or_else(mismatch)?.clone())
                .map_err(|_| mismatch())?;
        let derived = Self::derive(&requirements, original, output, disclosure, policy)?;
        if serde_json::to_value(&derived).map_err(|_| mismatch())? != *value {
            return Err(mismatch());
        }
        serialization::encode(&derived, maximum, "technical certificate")
            .map_err(|error| PreparationError::before(PreparationFailure::Evidence(error)))?;
        Ok(derived)
    }
}

fn mismatch() -> PreparationError {
    PreparationError::before(PreparationFailure::Qualification)
}
