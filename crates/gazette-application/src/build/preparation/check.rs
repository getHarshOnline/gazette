//! Retained checks and optional supported-profile qualification join the original preparation.

use std::path::Path;

use gazette_core::{
    process, raster,
    release::PublicationDestination,
    source::{CapturedSource, Sha256Digest},
    store,
};
use gazette_ports::{
    destination::RootSeparation,
    store::{JournalWriter, PreparationWriter, WriterConnector},
};
use serde::Serialize;
use std::path::PathBuf;

use super::{PreparationError, PreparationFailure};
use crate::build::{
    DisclosureInspectors, DisclosurePolicy, DisclosureProbe, DisclosureProbeLimits,
    OutputPolicyCheck, PublicationRender, serialization,
};

/// Existing retained preparation selected for an immutable check append.
#[derive(Clone, Copy, Debug)]
pub struct PreparationCheckRequest<'a> {
    /// Existing private journal root, separate from sources and the retained stage.
    pub journal: &'a Path,
    /// Independently retained expected journal generation.
    pub expected: store::JournalHeader,
    /// Exact original staging intent and native retention; never invented output identity.
    pub preparation: &'a store::PreparationRecord,
}

/// Permissions and resource bounds for check history, without stage or Git effects.
#[derive(Clone, Copy, Debug)]
pub struct PreparationCheckOptions {
    /// Explicit permission to append or replay this evidence.
    pub record: store::PreparationWriteOptions,
    /// Existing local writer acquisition policy.
    pub writer: store::WriterOptions,
    /// Maximum bytes for each complete private preparation/check evidence encoding.
    pub evidence_bytes: usize,
}

impl Default for PreparationCheckOptions {
    fn default() -> Self {
        Self {
            record: Default::default(),
            writer: Default::default(),
            evidence_bytes: 1 << 20,
        }
    }
}

#[derive(Serialize)]
struct Evidence<'a, 'r, 's> {
    format: &'static str,
    preparation_evidence_sha256: Sha256Digest,
    disclosure: &'a DisclosureProbe<'r>,
    policy: Policy<'a, 'r, 's>,
    #[serde(skip_serializing_if = "Option::is_none")]
    technical_certificate: Option<&'a super::TechnicalCertificate>,
}

#[derive(Serialize)]
#[serde(tag = "status", content = "evidence", rename_all = "kebab-case")]
enum Policy<'a, 'r, 's> {
    NotConfigured,
    Checked(&'a OutputPolicyCheck<'r, 's>),
}

/// One required scan and optional configured checker over the same retained publication.
/// Private construction prevents diagnostic probes or unrelated invocations being substituted.
#[derive(Debug)]
pub struct PreparationChecks<'r, 'a> {
    rendered: &'r PublicationRender<'a>,
    disclosure: DisclosureProbe<'r>,
    policy: Option<OutputPolicyCheck<'r, 'a>>,
    certificate: Option<super::TechnicalCertificate>,
}

impl<'a> PublicationRender<'a> {
    /// Scan required disclosure first, then pass that exact report and output to the checker.
    /// An absent checker remains explicit; neither result certifies or approves publication.
    /// No source acquisition, staging, journal or Git effect is performed.
    ///
    /// # Errors
    /// Refuses non-preparation output, incomplete scanning, findings and failed configured
    /// execution. A scan failure never invokes the checker; native process uncertainty survives.
    pub fn check_preparation(
        &self,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        metadata: &raster::InspectMetadata<'_>,
        policy: &DisclosurePolicy,
        execution: process::Options,
        limits: DisclosureProbeLimits,
        requirement: Option<&crate::build::OutputPolicyRequirement>,
    ) -> Result<PreparationChecks<'_, 'a>, PreparationError> {
        self.check_preparation_with_inspectors(
            runner,
            DisclosureInspectors {
                raster: Some(metadata),
                ..Default::default()
            },
            policy,
            execution,
            limits,
            requirement,
        )
    }

    /// Run preparation checks with the explicit inspectors for retained representations.
    /// Missing representation inspectors refuse complete disclosure evidence.
    ///
    /// # Errors
    /// Refuses non-preparation output, incomplete scanning, findings and failed configured
    /// execution. A scan failure never invokes the checker; native process uncertainty survives.
    pub fn check_preparation_with_inspectors(
        &self,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        inspectors: DisclosureInspectors<'_>,
        policy: &DisclosurePolicy,
        execution: process::Options,
        limits: DisclosureProbeLimits,
        requirement: Option<&crate::build::OutputPolicyRequirement>,
    ) -> Result<PreparationChecks<'_, 'a>, PreparationError> {
        self.ensure_preparation_mode()?;
        let before = PreparationError::before;
        let configured = self
            .inputs()
            .compilation()
            .workspace
            .policy_check()
            .is_some();
        if configured || requirement.is_some() {
            crate::build::policy::policy_input(self.inputs(), requirement.map(|r| &r.policy))
                .map_err(|error| before(PreparationFailure::Policy(error)))?;
            let command = crate::build::policy::command(self.inputs(), execution)
                .map_err(|error| before(PreparationFailure::Policy(error)))?;
            if let Some(required) = requirement {
                required
                    .check_command(command)
                    .map_err(|error| before(PreparationFailure::Policy(error)))?;
            }
        }
        let disclosure = self
            .site()
            .check_disclosure_policy_with_inspectors(policy, inspectors, limits)
            .map_err(|error| before(PreparationFailure::Disclosure(error)))?;
        let policy = if configured {
            Some(
                self.check_output_with_disclosure(
                    runner,
                    execution,
                    Some(&disclosure),
                    requirement,
                )
                .map_err(|error| before(PreparationFailure::Policy(error)))?,
            )
        } else {
            None
        };
        Ok(PreparationChecks {
            rendered: self,
            disclosure,
            policy,
            certificate: None,
        })
    }
}

impl<'r, 'a> PreparationChecks<'r, 'a> {
    /// Qualify the already completed required scan and matched policy response without
    /// another invocation or native effect. Repeated different expectations refuse;
    /// a failure leaves the existing checks and any prior certificate unchanged.
    ///
    /// # Errors
    /// Refuses mismatched independent identities, exit-only checks and bounded encoding failure.
    pub fn qualify(
        &mut self,
        requirements: &super::PreparationQualification,
        maximum: usize,
    ) -> Result<&super::TechnicalCertificate, PreparationError> {
        requirements.validate()?;
        requirements.check_render(self.rendered, maximum)?;
        let original = self.rendered.preparation_evidence(maximum)?;
        let disclosure =
            serialization::encode(&self.disclosure, maximum, "qualification disclosure")
                .map_err(|e| PreparationError::before(PreparationFailure::Evidence(e)))?;
        let policy = serialization::encode(
            &self
                .policy
                .as_ref()
                .map_or(Policy::NotConfigured, Policy::Checked),
            maximum,
            "qualification policy",
        )
        .map_err(|e| PreparationError::before(PreparationFailure::Evidence(e)))?;
        let certificate = super::TechnicalCertificate::derive(
            requirements,
            &original,
            self.rendered.site().output().inventory().identity(),
            &disclosure,
            &policy,
        )?;
        serialization::encode(&certificate, maximum, "technical certificate")
            .map_err(|e| PreparationError::before(PreparationFailure::Evidence(e)))?;
        if self
            .certificate
            .as_ref()
            .is_some_and(|previous| previous != &certificate)
        {
            return Err(PreparationError::before(PreparationFailure::Qualification));
        }
        Ok(self.certificate.insert(certificate))
    }

    /// Present only after the independent supported-profile qualification succeeded.
    pub fn certificate(&self) -> Option<&super::TechnicalCertificate> {
        self.certificate.as_ref()
    }

    /// Move the original output-check observation without another invocation.
    /// This transfers evidence only, not a certificate or publication permission.
    pub fn into_policy_evidence(self) -> Option<crate::build::OutputPolicyEvidence> {
        self.policy.map(OutputPolicyCheck::into_evidence)
    }

    /// Required report used by the optional checker and the durable evidence producer.
    pub fn disclosure(&self) -> &DisclosureProbe<'r> {
        &self.disclosure
    }

    /// Exact configured invocation, or honest absence without a synthetic passing result.
    pub fn policy(&self) -> Option<&OutputPolicyCheck<'r, 'a>> {
        self.policy.as_ref()
    }

    /// Append retained checks to an existing preparation without rescanning or re-executing.
    /// Original acquisition, configuration, selection and rendering evidence must match too.
    ///
    /// This records historical checks on immutable retained bytes, not current stage custody,
    /// checker authenticity, full certification or publication approval. An optional technical
    /// certificate retains only its explicitly bounded profile. Git preparation must still
    /// verify the current stage against that same output identity before native effects.
    /// No source bytes are reread, stage created, checker rerun or Git operation performed.
    ///
    /// # Errors
    /// Refuses mismatched input/output, missing permissions, unsafe journal placement
    /// and conflicting/retroactive history. Failed
    /// record delivery preserves journal coordinates and never authorizes an automatic retry.
    pub fn record_preparation_check<F, C>(
        &self,
        filesystem: &F,
        connector: &C,
        request: PreparationCheckRequest<'_>,
        options: PreparationCheckOptions,
    ) -> Result<store::PreparationReceipt, PreparationError>
    where
        F: RootSeparation,
        C: WriterConnector,
        C::Writer: PreparationWriter,
    {
        let before = PreparationError::before;
        options
            .record
            .validate()
            .map_err(|e| before(PreparationFailure::Journal(e)))?;
        options
            .writer
            .validate()
            .map_err(|e| before(PreparationFailure::Journal(e)))?;
        let rendered = self.rendered;
        let policy = self.policy.as_ref();
        if policy.is_some()
            != rendered
                .inputs()
                .compilation()
                .workspace
                .policy_check()
                .is_some()
            || policy.is_some_and(|policy| {
                !std::ptr::eq(policy.rendered(), rendered)
                    || policy.output() != rendered.site().output().inventory().identity()
            })
        {
            return Err(before(PreparationFailure::Mismatch));
        }
        rendered.ensure_preparation_mode()?;
        let original_evidence = rendered.preparation_evidence(options.evidence_bytes)?;
        let encoded = serialization::encode(
            &Evidence {
                format: "gazette.preparation-check/1",
                preparation_evidence_sha256: Sha256Digest::of(&original_evidence),
                disclosure: &self.disclosure,
                policy: policy.map_or(Policy::NotConfigured, Policy::Checked),
                technical_certificate: self.certificate.as_ref(),
            },
            options.evidence_bytes,
            "preparation check evidence",
        )
        .map_err(|e| before(PreparationFailure::Evidence(e)))?;
        record_retained_check(
            filesystem,
            connector,
            request,
            options,
            rendered.site().output(),
            rendered.protected_roots(),
            rendered
                .inputs()
                .compilation()
                .workspace
                .destination(rendered.inputs().site()),
            &original_evidence,
            &encoded,
        )
    }
}

/// Append one already-derived check to exact retained bytes through the single journal path.
/// Artifact owners must validate their semantic check before entering this mechanical join.
#[allow(clippy::too_many_arguments)]
pub(in crate::build) fn record_retained_check<F, C>(
    filesystem: &F,
    connector: &C,
    request: PreparationCheckRequest<'_>,
    options: PreparationCheckOptions,
    output: &CapturedSource,
    protected_roots: &[PathBuf],
    destination: Option<&PublicationDestination>,
    original_evidence: &[u8],
    check_evidence: &[u8],
) -> Result<store::PreparationReceipt, PreparationError>
where
    F: RootSeparation,
    C: WriterConnector,
    C::Writer: PreparationWriter,
{
    let before = PreparationError::before;
    options
        .record
        .validate()
        .map_err(|e| before(PreparationFailure::Journal(e)))?;
    options
        .writer
        .validate()
        .map_err(|e| before(PreparationFailure::Journal(e)))?;
    if original_evidence.len() > options.evidence_bytes
        || check_evidence.len() > options.evidence_bytes
    {
        return Err(before(PreparationFailure::Evidence(
            gazette_core::render::Error::Limit("preparation check evidence"),
        )));
    }
    let original = request.preparation;
    let intent = &original.intent;
    let retention = original
        .retention
        .as_ref()
        .ok_or_else(|| before(PreparationFailure::Mismatch))?;
    let inventory = output.inventory();
    if intent.output != inventory.identity()
        || intent.destination.as_ref() != destination
        || intent.entries != inventory.entries().len() as u64
        || intent.bytes != inventory.total_bytes()
        || intent.evidence.as_bytes() != original_evidence
        || intent.evidence_sha256 != Sha256Digest::of(original_evidence)
        || retention.intent_id != intent.id
        || retention.output != intent.output
        || retention.entries != intent.entries
        || retention.bytes != intent.bytes
    {
        return Err(before(PreparationFailure::Mismatch));
    }
    let evidence =
        std::str::from_utf8(check_evidence).map_err(|_| before(PreparationFailure::Mismatch))?;
    let input = store::PreparationCheckInput {
        intent,
        retention,
        evidence,
        evidence_sha256: Sha256Digest::of(check_evidence),
    };
    let path = intent.address.path();
    let mut protected = protected_roots.to_vec();
    protected.push(path.clone());
    filesystem
        .check_root_separation(request.journal, &protected)
        .map_err(|e| before(PreparationFailure::Separation(e)))?;
    let mut writer = connector
        .acquire_writer(request.journal, request.expected, options.writer)
        .map_err(|e| before(PreparationFailure::Writer(e)))?;
    let acquired = writer
        .inspect()
        .map_err(|e| PreparationError {
            cause: Box::new(PreparationFailure::Journal(e)),
            receipt: None,
            journal: None,
            writer_acquired: true,
            stage: Some(path.clone()),
        })?
        .header;
    let receipt = writer
        .record_preparation_check(input, options.record)
        .map_err(|e| PreparationError {
            cause: Box::new(PreparationFailure::Record(e)),
            receipt: None,
            journal: Some(acquired),
            writer_acquired: true,
            stage: Some(path.clone()),
        })?;
    let result = (|| {
        if receipt.record.intent != *intent
            || receipt.record.retention.as_ref() != Some(retention)
            || original.check.as_ref().is_some_and(|previous| {
                receipt.inserted || receipt.record.check.as_ref() != Some(previous)
            })
            || receipt.record.check.as_ref().is_none_or(|check| {
                check.intent_id != intent.id
                    || check.evidence != evidence
                    || check.evidence_sha256 != input.evidence_sha256
            })
        {
            return Err(PreparationFailure::Mismatch);
        }
        writer.close().map_err(PreparationFailure::Journal)
    })();
    match result {
        Ok(()) => Ok(receipt),
        Err(cause) => Err(PreparationError {
            cause: Box::new(cause),
            journal: Some(receipt.journal),
            receipt: Some(Box::new(receipt)),
            writer_acquired: true,
            stage: Some(path),
        }),
    }
}
