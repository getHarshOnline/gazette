//! Technical preparation of retained documentation build inputs, not hosted output.

use super::DocumentationAssembly;
use crate::build::{
    CompositionInspection, DisclosureInspectors, DisclosurePolicy, DisclosureProbe,
    DisclosureProbeLimits, DocumentationArtifactEvidence, PreparationCheckOptions,
    PreparationCheckRequest, PreparationError, PreparationFailure, PreparationOptions,
    PreparationRequest, RepositorySourceProvenance,
    preparation::{RetainedPreparation, record_retained_check},
    repository_source_provenance, serialization,
};
use gazette_compiler::workspace::{DocumentationRepository, Key};
use gazette_core::{
    raster,
    render::MediaType,
    source::{Content, EntryPath, Sha256Digest, TreeIdentity},
    store,
};
use gazette_ports::{
    stage::StagePlanner,
    store::{PreparationWriter, WriterConnector},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

#[derive(Serialize)]
struct Evidence<'a> {
    format: &'static str,
    artifact_class: &'static str,
    repository_source_provenance: RepositorySourceProvenance,
    inputs: CompositionInspection<'a>,
    brand: &'a Key,
    repository: &'a DocumentationRepository,
    artifacts: &'a [DocumentationArtifactEvidence<'a>],
    output: TreeIdentity,
}

#[derive(Serialize)]
struct CheckEvidence<'a> {
    format: &'static str,
    preparation_evidence_sha256: Sha256Digest,
    disclosure: &'a DisclosureProbe<'a>,
    policy: CheckPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    technical_certificate: Option<&'a DocumentationTechnicalCertificate>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum CheckPolicy {
    NotConfigured,
}

/// Independent exact expectations for generated-reference technical admission.
///
/// These coordinates qualify retained generation and assembly mechanics only. They are
/// neither an authored Website policy nor evidence that a hosting provider accepted or served
/// the repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationPreparationQualification {
    /// Exact compact `inputs` subtree, including configuration and pinned source evidence.
    pub inputs_sha256: Sha256Digest,
    /// Exact compact brand-owned documentation repository declaration.
    pub repository_sha256: Sha256Digest,
    /// Exact compact ordered artifact evidence, including native generator observations.
    pub artifacts_sha256: Sha256Digest,
    /// Exact assembled generated-documentation repository.
    pub output: TreeIdentity,
    /// Required disclosure scanner profile.
    pub scanner: String,
    /// Required recursive encoded-value profile.
    pub encoded_profile: String,
    /// Required canonical/caseless filename inspection profile.
    pub filename_profile: String,
    /// Required generic decoding and matching procedure.
    pub generic_rules_sha256: Sha256Digest,
    /// Exact deterministic scanner-resource projection profile.
    pub resource_profile: String,
    /// SHA-256 of every exact parser, decoder, traversal and retained-work ceiling.
    pub resource_limits_sha256: Sha256Digest,
    /// Required instance disclosure-policy projection.
    pub disclosure_policy_sha256: Sha256Digest,
}

impl DocumentationPreparationQualification {
    pub(crate) fn validate(&self) -> Result<(), PreparationError> {
        if [&self.scanner, &self.encoded_profile, &self.filename_profile]
            .into_iter()
            .any(|value| {
                value.is_empty()
                    || value.len() > 256
                    || !value.bytes().all(|byte| byte.is_ascii_graphic())
            })
            || !crate::build::disclosure::technical_profile::coordinates_match(
                &self.scanner,
                &self.encoded_profile,
                &self.filename_profile,
                self.generic_rules_sha256,
            )
            || self.resource_profile != DisclosureProbeLimits::RESOURCE_PROFILE
        {
            return Err(PreparationError::before(PreparationFailure::Qualification));
        }
        Ok(())
    }
}

/// Privately constructed proof that one exact generated-reference profile matched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub(crate) struct DocumentationTechnicalCertificate {
    profile: &'static str,
    requirements: DocumentationPreparationQualification,
    preparation_evidence_sha256: Sha256Digest,
    disclosure_sha256: Sha256Digest,
}

impl DocumentationTechnicalCertificate {
    pub(crate) fn requirements(&self) -> &DocumentationPreparationQualification {
        &self.requirements
    }

    fn derive(
        requirements: &DocumentationPreparationQualification,
        original: &[u8],
        disclosure: &[u8],
    ) -> Result<Self, PreparationError> {
        requirements.validate()?;
        let fields: BTreeMap<String, &RawValue> =
            serde_json::from_slice(original).map_err(|_| qualification_failure())?;
        if raw_text(fields.get("artifact_class"))? != "documentation-build-input"
            || raw_digest(fields.get("inputs"))? != requirements.inputs_sha256
            || raw_digest(fields.get("repository"))? != requirements.repository_sha256
            || raw_digest(fields.get("artifacts"))? != requirements.artifacts_sha256
        {
            return Err(qualification_failure());
        }
        let value: Value =
            gazette_core::json::parse(original).map_err(|_| qualification_failure())?;
        let output: TreeIdentity = serde_json::from_value(
            value
                .get("output")
                .ok_or_else(qualification_failure)?
                .clone(),
        )
        .map_err(|_| qualification_failure())?;
        if output != requirements.output {
            return Err(qualification_failure());
        }
        let scan: Value =
            gazette_core::json::parse(disclosure).map_err(|_| qualification_failure())?;
        let expected = [
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
            ("output_identity", serde_json::json!(requirements.output)),
        ];
        if expected
            .iter()
            .any(|(key, expected)| scan.get(key) != Some(expected))
            || !crate::build::disclosure::technical_profile::resource_report_matches(
                &scan,
                requirements.resource_limits_sha256,
            )
            || scan.get("findings") != Some(&serde_json::json!([]))
        {
            return Err(qualification_failure());
        }
        Ok(Self {
            profile: "gazette-generated-documentation/1",
            requirements: requirements.clone(),
            preparation_evidence_sha256: Sha256Digest::of(original),
            disclosure_sha256: Sha256Digest::of(disclosure),
        })
    }

    pub(crate) fn inspect(
        value: &Value,
        original: &[u8],
        disclosure: &[u8],
        maximum: usize,
    ) -> Result<Self, PreparationError> {
        let requirements: DocumentationPreparationQualification = serde_json::from_value(
            value
                .get("requirements")
                .ok_or_else(qualification_failure)?
                .clone(),
        )
        .map_err(|_| qualification_failure())?;
        let derived = Self::derive(&requirements, original, disclosure)?;
        if serde_json::to_value(&derived).map_err(|_| qualification_failure())? != *value {
            return Err(qualification_failure());
        }
        serialization::encode(&derived, maximum, "documentation technical certificate")
            .map_err(|error| PreparationError::before(PreparationFailure::Evidence(error)))?;
        Ok(derived)
    }
}

fn raw_text(value: Option<&&RawValue>) -> Result<String, PreparationError> {
    serde_json::from_str(value.ok_or_else(qualification_failure)?.get())
        .map_err(|_| qualification_failure())
}

fn raw_digest(value: Option<&&RawValue>) -> Result<Sha256Digest, PreparationError> {
    Ok(Sha256Digest::of(
        value.ok_or_else(qualification_failure)?.get().as_bytes(),
    ))
}

fn qualification_failure() -> PreparationError {
    PreparationError::before(PreparationFailure::Qualification)
}

/// Required lexical disclosure evidence over one exact generated-reference output.
///
/// This is neither Mintlify hosting proof nor authored-publication certification. It exists so
/// the generic retained-preparation/release machinery can distinguish checked generated bytes
/// from a merely assembled repository without inventing another release system.
#[derive(Debug)]
pub struct DocumentationPreparationCheck<'r, 'a> {
    assembly: &'r DocumentationAssembly<'a>,
    disclosure: DisclosureProbe<'r>,
    certificate: Option<DocumentationTechnicalCertificate>,
}

fn media_types(output: &gazette_core::source::CapturedSource) -> BTreeMap<EntryPath, MediaType> {
    output
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            if !matches!(entry.content, Content::File { .. }) {
                return None;
            }
            let suffix = entry
                .path
                .as_str()
                .rsplit_once('.')
                .map_or("", |(_, suffix)| suffix)
                .to_ascii_lowercase();
            let media = match (entry.path.as_str(), suffix.as_str()) {
                (".mintignore", _) => MediaType::Text,
                (_, "md") => MediaType::Markdown,
                // Keep MDX distinct from Markdown. The suffix dispatcher admits only
                // generated literal carriers; general MDX still refuses coverage.
                (_, "mdx") => return None,
                (_, "yaml" | "yml" | "txt" | "toml") => MediaType::Text,
                (_, "json" | "map") => MediaType::Json,
                (_, "jsonld") => MediaType::JsonLd,
                (_, "html" | "htm") => MediaType::Html,
                (_, "xml") => MediaType::Xml,
                (_, "svg") => MediaType::Svg,
                (_, "css") => MediaType::Css,
                (_, "js" | "mjs" | "cjs" | "ts" | "mts" | "cts") => MediaType::JavaScript,
                (_, "png") => MediaType::Png,
                (_, "jpg" | "jpeg") => MediaType::Jpeg,
                (_, "woff2") => MediaType::Woff2,
                _ => return None,
            };
            Some((entry.path.clone(), media))
        })
        .collect()
}

#[cfg(test)]
#[path = "../../../tests/internal/documentation_preparation.rs"]
mod tests;

impl<'a> DocumentationAssembly<'a> {
    fn preparation_evidence(&self, maximum: usize) -> Result<Vec<u8>, PreparationError> {
        let before = PreparationError::before;
        if self.preview_link_bases().is_some() {
            return Err(before(PreparationFailure::Preview));
        }
        let (brand, _, _) =
            super::selection(&self.inputs.compilation().workspace, self.inputs.site())
                .map_err(|_| before(PreparationFailure::Mismatch))?;
        serialization::encode(
            &Evidence {
                format: "gazette.documentation-preparation/1",
                artifact_class: "documentation-build-input",
                repository_source_provenance: repository_source_provenance(),
                inputs: self.inputs.inspect(),
                brand: &brand.id,
                repository: self.repository,
                artifacts: &self.artifacts,
                output: self.output.inventory().identity(),
            },
            maximum,
            "documentation preparation evidence bytes",
        )
        .map_err(|error| before(PreparationFailure::Evidence(error)))
    }

    /// Apply the same required disclosure scanner to the complete generated repository.
    /// Mintlify page/configuration formats are declared here only for scanner dispatch; target
    /// conversion and compatibility validation remain owned by the Mintlify adapter.
    ///
    /// # Errors
    /// Refuses incomplete policy, unsupported retained formats, findings and exhausted bounds.
    pub fn check_preparation<'r>(
        &'r self,
        policy: &DisclosurePolicy,
        metadata: &raster::InspectMetadata<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DocumentationPreparationCheck<'r, 'a>, PreparationError> {
        self.check_preparation_with_inspectors(
            policy,
            DisclosureInspectors {
                raster: Some(metadata),
                ..Default::default()
            },
            limits,
        )
    }

    /// Apply required disclosure with explicit representation inspectors.
    /// Missing inspectors and unsupported MDX refuse complete evidence.
    ///
    /// # Errors
    /// Refuses incomplete policy, unsupported retained formats, findings and exhausted bounds.
    pub fn check_preparation_with_inspectors<'r>(
        &'r self,
        policy: &DisclosurePolicy,
        inspectors: DisclosureInspectors<'_>,
        limits: DisclosureProbeLimits,
    ) -> Result<DocumentationPreparationCheck<'r, 'a>, PreparationError> {
        if self.preview_link_bases().is_some() {
            return Err(PreparationError::before(PreparationFailure::Preview));
        }
        let disclosure = crate::build::check_disclosure_policy_with_inspectors(
            &self.output,
            &media_types(&self.output),
            policy,
            inspectors,
            limits,
        )
        .map_err(|error| PreparationError::before(PreparationFailure::Disclosure(error)))?;
        Ok(DocumentationPreparationCheck {
            assembly: self,
            disclosure,
            certificate: None,
        })
    }

    /// Durably retain this complete documentation repository in an explicitly selected local stage.
    ///
    /// Records exact configuration, source revisions, generator observations and target bytes
    /// through the same staging transaction used by authored publications. No native input is
    /// reacquired, and the configured repository path is not automatically used as the stage.
    /// The intent retains only this generated repository's explicit delivery coordinates; it
    /// never inherits the authored site's destination. Existing generic journal worktree and
    /// release mechanics can consume a separately checked receipt without another release system.
    ///
    /// # Errors
    /// Refuses missing permissions, exceeded bounds, source overlap and replay. Preserves exact
    /// journal and possible native residue on a later failure; never recreates an existing intent.
    pub fn prepare_stage<S, C>(
        &self,
        stage: &S,
        store: &C,
        request: PreparationRequest<'_>,
        options: PreparationOptions,
    ) -> Result<store::PreparationReceipt, PreparationError>
    where
        S: StagePlanner,
        C: WriterConnector,
        C::Writer: PreparationWriter,
    {
        let before = PreparationError::before;
        crate::build::ensure_deployable_sites(
            &self.inputs.compilation().workspace,
            std::iter::once((self.inputs.site(), self.repository.destination.is_some())),
        )
        .map_err(|error| before(PreparationFailure::Deployability(error)))?;
        if self
            .inputs
            .sources()
            .values()
            .any(crate::source::AcquiredInput::is_development)
        {
            return Err(before(PreparationFailure::Preview));
        }
        let evidence = self.preparation_evidence(options.evidence_bytes)?;
        let roots = self
            .protected_roots()
            .map_err(|error| before(PreparationFailure::Stage(error.into())))?;
        RetainedPreparation {
            output: &self.output,
            protected_roots: &roots,
            destination: self.repository.destination.as_ref(),
            evidence: String::from_utf8(evidence)
                .map_err(|_| before(PreparationFailure::Mismatch))?,
        }
        .prepare_stage(stage, store, request, options)
    }
}

impl DocumentationPreparationCheck<'_, '_> {
    /// Required scanner evidence over the exact assembly; status remains partial evidence.
    pub fn disclosure(&self) -> &DisclosureProbe<'_> {
        &self.disclosure
    }

    /// Qualify the retained source/generator/assembly and disclosure evidence against an
    /// independently supplied generated-documentation profile.
    ///
    /// # Errors
    /// Refuses any changed evidence or profile coordinate without writing history.
    pub fn qualify(
        &mut self,
        requirements: &DocumentationPreparationQualification,
        maximum: usize,
    ) -> Result<(), PreparationError> {
        let original = self.assembly.preparation_evidence(maximum)?;
        let disclosure = serialization::encode(
            &self.disclosure,
            maximum,
            "documentation qualification disclosure",
        )
        .map_err(|error| PreparationError::before(PreparationFailure::Evidence(error)))?;
        self.certificate = Some(DocumentationTechnicalCertificate::derive(
            requirements,
            &original,
            &disclosure,
        )?);
        Ok(())
    }

    /// Append this exact generated-repository check to its retained preparation.
    ///
    /// The application rederives original preparation evidence from the same assembly and
    /// compares destination, output and retention before the shared journal append. No source,
    /// generator, stage, Git or network operation is repeated.
    ///
    /// # Errors
    /// Refuses mismatched history, bounds, root overlap and unavailable writer permission.
    pub fn record<F, C>(
        &self,
        filesystem: &F,
        connector: &C,
        request: PreparationCheckRequest<'_>,
        options: PreparationCheckOptions,
    ) -> Result<store::PreparationReceipt, PreparationError>
    where
        F: gazette_ports::destination::RootSeparation,
        C: WriterConnector,
        C::Writer: PreparationWriter,
    {
        let original = self.assembly.preparation_evidence(options.evidence_bytes)?;
        let encoded = serialization::encode(
            &CheckEvidence {
                format: "gazette.documentation-preparation-check/1",
                preparation_evidence_sha256: Sha256Digest::of(&original),
                disclosure: &self.disclosure,
                policy: CheckPolicy::NotConfigured,
                technical_certificate: self.certificate.as_ref(),
            },
            options.evidence_bytes,
            "documentation preparation check evidence bytes",
        )
        .map_err(|error| PreparationError::before(PreparationFailure::Evidence(error)))?;
        let roots = self
            .assembly
            .protected_roots()
            .map_err(|error| PreparationError::before(PreparationFailure::Stage(error.into())))?;
        record_retained_check(
            filesystem,
            connector,
            request,
            options,
            &self.assembly.output,
            &roots,
            self.assembly.repository.destination.as_ref(),
            &original,
            &encoded,
        )
    }
}
