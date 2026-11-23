//! A workspace-selected policy file travels with the exact captured publication inputs.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_compiler::workspace::{Key, PolicySource, Workspace};
use gazette_core::source::{Sha256Digest, TreeIdentity};
use serde::{Deserialize, Serialize};

use super::{OutputPolicyRequirement, PolicyCheckError};
use crate::{
    build::{AcquiredInputs, AcquiredPublicationSet, ReviewPolicy},
    source::AcquiredInput,
};
use std::collections::BTreeMap;

// Policy documents are control input, not an unbounded second publication corpus.
const DOCUMENT_BYTES: usize = 1 << 20;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyDocument {
    pub binding: PolicySource,
    pub tree: TreeIdentity,
    pub sha256: Sha256Digest,
    document_base64: String,
}

impl PolicyDocument {
    /// Recheck retained history without acquiring a current file or trusting a claimed hash.
    pub(crate) fn bytes(&self) -> Result<Vec<u8>, PolicyCheckError> {
        if self.document_base64.len() > DOCUMENT_BYTES.div_ceil(3) * 4 {
            return Err(PolicyCheckError::PolicySource(
                "policy document exceeds 1 MiB",
            ));
        }
        let bytes = STANDARD
            .decode(&self.document_base64)
            .map_err(|_| PolicyCheckError::PolicySource("invalid retained policy encoding"))?;
        check_bytes(&bytes)?;
        if Sha256Digest::of(&bytes) != self.sha256
            || STANDARD.encode(&bytes) != self.document_base64
        {
            return Err(PolicyCheckError::PolicySource(
                "retained policy digest mismatch",
            ));
        }
        Ok(bytes)
    }
}

fn check_bytes(bytes: &[u8]) -> Result<(), PolicyCheckError> {
    if bytes.is_empty() || bytes.len() > DOCUMENT_BYTES {
        return Err(PolicyCheckError::PolicySource(
            "policy document must contain between one byte and 1 MiB",
        ));
    }
    Ok(())
}

/// Pure resolution: never reads native files, interprets policy rules or accepts authority.
/// An independently required response also requires the actual policy document.
pub(crate) fn policy_input(
    inputs: &AcquiredInputs,
    requirement: Option<&ReviewPolicy>,
) -> Result<Option<PolicyDocument>, PolicyCheckError> {
    resolve(
        &inputs.compilation().workspace,
        inputs.sources(),
        requirement,
    )
}

impl AcquiredPublicationSet {
    /// Check the selected retained policy file before candidate checks or resource execution.
    /// No native input is reopened and no policy meaning or authority is inferred.
    ///
    /// # Errors
    /// Refuses missing required policy input, invalid file bounds and independent hash mismatch.
    pub fn check_policy_source(
        &self,
        requirement: Option<&OutputPolicyRequirement>,
    ) -> Result<(), PolicyCheckError> {
        resolve(
            &self.compilation().workspace,
            self.sources(),
            requirement.map(|r| &r.policy),
        )
        .map(|_| ())
    }
}

fn resolve(
    workspace: &Workspace,
    sources: &BTreeMap<Key, AcquiredInput>,
    requirement: Option<&ReviewPolicy>,
) -> Result<Option<PolicyDocument>, PolicyCheckError> {
    let Some(binding) = workspace.policy_source() else {
        return if requirement.is_some() {
            Err(PolicyCheckError::PolicySource(
                "required policy source is not configured",
            ))
        } else {
            Ok(None)
        };
    };
    let source = sources
        .get(&binding.source)
        .ok_or(PolicyCheckError::PolicySource(
            "configured policy source was not captured",
        ))?;
    let bytes = source
        .file(&binding.entry)
        .ok_or(PolicyCheckError::PolicySource(
            "configured policy entry is not a retained regular file",
        ))?;
    check_bytes(bytes)?;
    let sha256 = Sha256Digest::of(bytes);
    if requirement.is_some_and(|required| required.source_sha256 != sha256) {
        return Err(PolicyCheckError::PolicySource(
            "independent policy digest mismatch",
        ));
    }
    Ok(Some(PolicyDocument {
        binding: binding.clone(),
        tree: source.inventory().identity(),
        sha256,
        document_base64: STANDARD.encode(bytes),
    }))
}
