//! Direct complete-set qualification reuses candidate, render and preparation-check owners.

use super::{
    DisclosurePolicy, PreparationQualification, PublicationRecipe, SiteRecipe, WorkspaceInput,
};
use crate::{authored, process, source, toml};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod report;
pub use report::QualificationReport;
use report::{Failure, Outcome, SiteResult};

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    workspace: WorkspaceInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selection: Option<authored::Selection>,
    disclosure: DisclosurePolicy,
    sites: Vec<Site>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    math_generation: Option<super::MathGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagram_generation: Option<super::DiagramGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::SocialCardGeneration")]
    social_card_generation: Option<super::SocialCardGeneration>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Site {
    render: SiteRecipe,
    qualification: PreparationQualification,
}

/// Exact operator request. Technical qualification never grants publisher authority.
/// No stage, journal, commit, listener or deployment destination is accepted here.
#[derive(Debug)]
pub struct QualificationRecipe {
    input: Input,
    publication: PublicationRecipe,
}

/// Native read and checker permissions remain independent of selected policy inputs.
#[derive(Clone, Copy, Debug)]
pub struct QualificationOptions {
    /// Permit acquisition of the exact configured source set.
    pub allow_read: bool,
    /// Separate permission and limits for unsandboxed policy checker processes.
    pub policy: process::Options,
    /// Workspace configuration decoding limits.
    pub syntax: toml::Limits,
    /// Complete-set source capture limits.
    pub acquisition: super::PublicationSetAcquisitionLimits,
    /// Per-site generated output limits.
    pub output: source::Limits,
    /// Shared semantic compilation and aggregate rendering limits.
    pub rendering: super::PublicationSetRenderLimits,
    /// Candidate review-request limits.
    pub review: super::ReviewLimits,
    /// Candidate response decoding limits.
    pub review_response: super::ReviewResponseLimits,
    /// Maximum complete candidate response bytes.
    pub review_response_bytes: usize,
    /// Supported-profile output disclosure inspection limits.
    pub disclosure: super::DisclosureProbeLimits,
    /// Per-site complete certificate/evidence encoding ceiling, separate from transport.
    pub evidence_bytes: usize,
}

impl Default for QualificationOptions {
    fn default() -> Self {
        Self {
            allow_read: false,
            policy: Default::default(),
            syntax: Default::default(),
            acquisition: Default::default(),
            output: Default::default(),
            rendering: Default::default(),
            review: Default::default(),
            review_response: Default::default(),
            review_response_bytes: 16 << 20,
            disclosure: Default::default(),
            evidence_bytes: 1 << 20,
        }
    }
}

impl QualificationRecipe {
    /// Native named-object contract for command and protocol discovery.
    pub fn input_schema() -> schemars::Schema {
        schemars::generate::SchemaSettings::draft2020_12()
            .for_serialize()
            .with(|settings| settings.inline_subschemas = true)
            .into_generator()
            .into_root_schema_for::<Input>()
    }

    /// Decode all sites and mandatory policy requirements before native acquisition.
    /// Omitted selection derives all non-draft content and its required assets;
    /// an explicit selection remains the exact owner block/override surface.
    ///
    /// # Errors
    /// Refuses duplicate keys, positional/unknown fields, null optional acquisition,
    /// missing candidate policies and empty/duplicate/oversized site sets.
    pub fn parse_json(bytes: &[u8], maximum: usize, sites: usize) -> Result<Self, &'static str> {
        if bytes.len() > maximum {
            return Err("qualification request exceeds input byte ceiling");
        }
        let original = crate::json::parse(bytes).map_err(|_| "invalid qualification JSON")?;
        let mut input: Input = serde_json::from_value(original.clone())
            .map_err(|_| "invalid qualification request shape")?;
        if serde_json::to_value(&input).ok().as_ref() != Some(&original) {
            return Err("qualification requires complete named native object shapes");
        }
        for site in &input.sites {
            site.qualification
                .validate()
                .map_err(|_| "invalid qualification requirements")?;
            if site.qualification.candidate_policy.is_none() {
                return Err("every qualification requires an explicit candidate policy");
            }
        }
        let publication = PublicationRecipe::new(
            input.sites.iter().map(|site| site.render.clone()).collect(),
            input.selection.clone(),
            None,
            sites,
        )?
        .with_math_generation(input.math_generation.take())?
        .with_diagram_generation(input.diagram_generation.take())?
        .with_social_card_generation(input.social_card_generation.take())?;
        Ok(Self { input, publication })
    }

    /// Acquire once, check the complete candidate, render, then qualify each exact output.
    /// Gazette creates no stage, database or Git state. Explicit configured checker and
    /// resource programs remain effectful and are not sandboxed by this operation.
    /// Every requested site survives failure; neither cancellation nor lost delivery is
    /// permission to repeat a checker. Inspect `is_complete`, never just the success subset.
    pub fn execute(&self, options: QualificationOptions) -> QualificationReport {
        let mut report = QualificationReport::new(self.publication.sites().cloned().collect());
        if let Err(error) = self.run(options, &mut report) {
            report.failure = Some(error);
        }
        report
    }

    fn run(
        &self,
        options: QualificationOptions,
        report: &mut QualificationReport,
    ) -> Result<(), Failure> {
        if !options.allow_read || !options.policy.allow_execute {
            return Err(Failure::before(
                "qualification_permission",
                "explicit source-read and policy-execution permissions required",
            ));
        }
        if options.evidence_bytes == 0 || options.review_response_bytes == 0 {
            return Err(Failure::before(
                "qualification_limits",
                "qualification evidence limits must be nonzero",
            ));
        }
        let policy_digest = self
            .input
            .disclosure
            .validate(options.disclosure)
            .map_err(|error| Failure::before("qualification_disclosure", error))?;
        for site in &self.input.sites {
            site.qualification
                .check_resource_limits(options.disclosure)
                .map_err(|error| Failure::before("qualification_resources", error))?;
        }
        let configuration = self
            .input
            .workspace
            .resolve(options.syntax)
            .map_err(|error| Failure::before(error.code(), error))?;
        let command = configuration
            .policy_command()
            .map_err(|error| Failure::before(error.code(), error))?
            .ok_or_else(|| {
                Failure::before("policy_unconfigured", "required checker is not configured")
            })?;
        let mut candidate_policies = BTreeMap::new();
        for site in &self.input.sites {
            let requirements = &site.qualification;
            if requirements.disclosure_policy_sha256 != policy_digest {
                return Err(Failure::before(
                    "qualification_disclosure",
                    "independent disclosure-policy digest differs",
                ));
            }
            requirements
                .output_policy
                .check_command(command)
                .map_err(|error| Failure::before(error.code(), error))?;
            let candidate = requirements.candidate_policy.as_ref().ok_or_else(|| {
                Failure::before("qualification_candidate", "candidate policy is required")
            })?;
            candidate
                .check_command(command)
                .map_err(|error| Failure::before(error.code(), error))?;
            candidate_policies.insert(site.render.site.clone(), candidate.clone());
        }
        let mut inputs = super::acquire_resolved_publications(
            configuration,
            &report.requested,
            options.acquisition,
        )
        .map_err(|error| Failure::before(error.code(), error))?;
        self.publication
            .acquire_documentation_catalogs(
                &mut inputs,
                options.acquisition.sources,
                options.output,
            )
            .map_err(|error| Failure::before(error.code(), error))?;
        for site in &self.input.sites {
            inputs
                .check_policy_source(Some(&site.qualification.output_policy))
                .map_err(|error| Failure::before(error.code(), error))?;
        }
        self.publication
            .acquire_math(&mut inputs, options.rendering.publication.corpus)
            .map_err(|error| Failure::Math(Box::new(error)))?;
        self.publication
            .acquire_diagrams(&mut inputs, options.rendering.publication.corpus)
            .map_err(|error| Failure::Diagram(Box::new(error)))?;
        let rendered = super::check_and_render_publications_with_generations(
            &inputs,
            self.publication.options(options.output),
            options.rendering,
            super::CandidateCheckOptions {
                requirements: Some(&candidate_policies),
                process: options.policy,
                review: options.review,
                bytes: options.review.bytes,
                response: options.review_response,
                response_bytes: options.review_response_bytes,
            },
            self.publication.social_card_generation(),
        )
        .map_err(|error| match error {
            super::PublicationSetError::Policy(error) => Failure::Candidate(error),
            error => Failure::before(error.code(), error),
        })?;
        report.candidate_checks = rendered.candidate_checks();
        // Complete render and identity preflight before the first output checker.
        // A known late failure must not execute a successful prefix unnecessarily.
        let mut invalid = false;
        for (index, site) in self.input.sites.iter().enumerate() {
            let failure = match rendered
                .sites()
                .get(&site.render.site)
                .expect("shared renderer retains every requested site")
            {
                Err(error) => Some(Failure::before(error.code(), error)),
                Ok(rendered) => site
                    .qualification
                    .check_render(rendered, options.evidence_bytes)
                    .err()
                    .map(|error| Failure::Check(Box::new(error))),
            };
            if let Some(failure) = failure {
                report.sites[index].outcome = Outcome::Failed {
                    failure,
                    output_policy: None,
                };
                invalid = true;
            }
        }
        if invalid {
            return Err(Failure::before(
                "qualification_render",
                "complete render/identity preflight failed; no output checker invoked",
            ));
        }
        for (index, site) in self.input.sites.iter().enumerate() {
            let result = rendered
                .sites()
                .get(&site.render.site)
                .expect("shared renderer retains every requested site");
            let outcome = match result {
                Err(error) => Outcome::Failed {
                    failure: Failure::before(error.code(), error),
                    output_policy: None,
                },
                Ok(rendered) => match super::check_preparation(
                    rendered,
                    &self.input.disclosure,
                    options.policy,
                    options.disclosure,
                    Some(&site.qualification.output_policy),
                ) {
                    Err(error) => Outcome::Failed {
                        failure: Failure::Check(Box::new(error)),
                        output_policy: None,
                    },
                    Ok(mut checks) => {
                        let certificate = checks
                            .qualify(&site.qualification, options.evidence_bytes)
                            .cloned();
                        let output_policy = checks.into_policy_evidence();
                        match certificate {
                            Ok(certificate) => Outcome::Qualified {
                                certificate: Box::new(certificate),
                                output_policy: output_policy.map(Box::new),
                            },
                            Err(error) => Outcome::Failed {
                                failure: Failure::Check(Box::new(error)),
                                output_policy: output_policy.map(Box::new),
                            },
                        }
                    }
                },
            };
            report.sites[index] = SiteResult {
                site: site.render.site.clone(),
                outcome,
            };
        }
        Ok(())
    }
}
