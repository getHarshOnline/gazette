//! One owned operator recipe; interfaces share decoding and native adapter composition.

use super::{
    DisclosurePolicy, LocalPreparationOptions, LocalPreparationSetError, PublicationRecipe,
    PublicationSetAcquisitionLimits, PublicationSetRenderLimits, SitePreparationReceipt,
    SitePreparationRequest, SiteRecipe, WorkspaceInput,
};
use crate::{
    authored,
    release_commit::{CommitMetadata, ParentSource},
    source, store, toml,
};
use gazette_ports::destination::RootSeparation;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
};
mod delivery;
pub use delivery::LocalPreparationDelivery;
mod report;
pub use report::LocalPreparationRunReport;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    workspace: WorkspaceInput,
    journal: PathBuf,
    expected_header: store::JournalHeader,
    coordinator: Option<store::coordinator::LocalCoordinatorSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selection: Option<authored::Selection>,
    disclosure: DisclosurePolicy,
    candidate_check: Option<super::ReviewLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BTreeMap<crate::workspace::Key, super::CandidatePolicyRequirement>")]
    candidate_policies: Option<BTreeMap<crate::workspace::Key, super::CandidatePolicyRequirement>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::recipe::review_responses_schema")]
    review_responses: Option<BTreeMap<crate::workspace::Key, super::ReviewSubmission>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::MathGeneration")]
    math_generation: Option<super::MathGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::DiagramGeneration")]
    diagram_generation: Option<super::DiagramGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::SocialCardGeneration")]
    social_card_generation: Option<super::SocialCardGeneration>,
    sites: Vec<Site>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Site {
    render: SiteRecipe,
    stage_parent: PathBuf,
    stage_name: String,
    preparation_id: source::Sha256Digest,
    metadata: CommitMetadata,
    parent_source: Option<ParentSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_policy: Option<super::OutputPolicyRequirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    qualification: Option<super::PreparationQualification>,
}

/// An exact complete local request, not publisher approval or release authorization.
/// Private fields prevent mutation between decoding and the one native execution path.
#[derive(Debug)]
pub struct LocalPreparationRecipe {
    input: Input,
    publication: PublicationRecipe,
}

/// Independent source/read, rendering and native preparation bounds and permissions.
#[derive(Clone, Copy, Debug)]
pub struct LocalPreparationRunOptions {
    /// Explicit permission for configured local source acquisition; no network is inferred.
    pub allow_read: bool,
    /// Complete workspace syntax limits.
    pub syntax: toml::Limits,
    /// Exact selected-source and total acquisition limits.
    pub acquisition: PublicationSetAcquisitionLimits,
    /// Per-site generated tree ceiling supplied to the shared rendering recipe.
    pub output: source::Limits,
    /// Aggregate render and capability bounds.
    pub rendering: PublicationSetRenderLimits,
    /// Independent per-response transport and matched-record ceilings.
    pub review_response: super::ReviewResponseLimits,
    /// Aggregate original response bytes, independently bounded before source acquisition.
    pub review_response_bytes: usize,
    /// Existing independent staging, journal, process and Git permissions.
    pub preparation: LocalPreparationOptions,
}

impl Default for LocalPreparationRunOptions {
    fn default() -> Self {
        Self {
            allow_read: false,
            syntax: Default::default(),
            acquisition: Default::default(),
            output: Default::default(),
            rendering: Default::default(),
            review_response: Default::default(),
            review_response_bytes: 16 << 20,
            preparation: Default::default(),
        }
    }
}

/// Failures before preparation have no preparation effects; later failures retain native evidence.
#[derive(Debug)]
pub enum LocalPreparationRunError {
    /// Native diagram acquisition failed; original process/capture evidence survives.
    Diagram(Box<super::DiagramGenerationError>),
    /// Native typesetting failed before preparation; exact invocation custody survives.
    Math(Box<super::MathGenerationError>),
    /// The requested pre-render checker failed; no rendering/preparation ran.
    Candidate(Box<super::CandidateCheckFailure>),
    /// No partial site set is prepared; prior candidate effects remain visible.
    Render {
        /// Every requested site, including those that rendered successfully.
        requested: Vec<crate::workspace::Key>,
        /// Exact zero-exit invocations before rendering.
        candidate_checks: BTreeMap<crate::workspace::Key, super::PolicyInvocation>,
        /// Original site-local rendering failures, without a retained partial output tree.
        failures: BTreeMap<crate::workspace::Key, LocalPreparationRenderFailure>,
    },
    /// Invalid input or acquisition/render failure, before any preparation begins.
    Before {
        /// Native phase classification.
        code: &'static str,
        /// Diagnostic, not permission to retry an effectful preparation.
        message: String,
    },
    /// Complete requested set, completed prefix and later native uncertainty.
    Preparation {
        /// Actual candidate invocations survive later render/preparation failures.
        candidate_checks: BTreeMap<crate::workspace::Key, super::PolicyInvocation>,
        /// Original complete-set native preparation evidence.
        error: Box<LocalPreparationSetError>,
    },
}

/// Owned native rendering diagnostic used after the borrowed captured input is released.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LocalPreparationRenderFailure {
    /// Original application failure class.
    pub code: &'static str,
    /// Original private diagnostic; not public Website copy.
    pub message: String,
}

impl fmt::Display for LocalPreparationRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Math(error) => error.fmt(f),
            Self::Diagram(error) => error.fmt(f),
            Self::Before { code, message } => {
                write!(f, "{code}: {message}; preparation not started")
            }
            Self::Candidate(error) => error.fmt(f),
            Self::Render { failures, .. } => write!(
                f,
                "{} site renders failed; preserve candidate invocation evidence; preparation not started",
                failures.len()
            ),
            Self::Preparation { error, .. } => error.fmt(f),
        }
    }
}
impl std::error::Error for LocalPreparationRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Math(error) => Some(error.as_ref()),
            Self::Diagram(error) => Some(error.as_ref()),
            Self::Before { .. } => None,
            Self::Render { .. } => None,
            Self::Candidate(error) => Some(error.as_ref()),
            Self::Preparation { error, .. } => Some(error.as_ref()),
        }
    }
}

impl LocalPreparationRecipe {
    /// Discovery projection of the complete named input, including explicit nullable fields.
    /// The native decoder remains responsible for duplicate keys, bounds and semantic checks.
    pub fn input_schema() -> schemars::Schema {
        schemars::generate::SchemaSettings::draft2020_12()
            .for_serialize()
            .with(|settings| settings.inline_subschemas = true)
            .into_generator()
            .into_root_schema_for::<Input>()
    }

    /// Decode a bounded complete native JSON shape, rejecting duplicate keys at every depth.
    /// Omitted selection derives all non-draft content and its required assets;
    /// an explicit selection remains the exact owner block/override surface. Existing
    /// nullable options remain explicit; review responses may be omitted, but cannot
    /// be null. This is a local operation
    /// recipe, not a compatibility ABI or an authenticated publication-policy document.
    ///
    /// # Errors
    /// Rejects invalid shape, missing fields, oversized/duplicate sites, relative native paths
    /// duplicate preparation IDs and invalid commit/parent grammar without opening paths.
    pub fn parse_json(bytes: &[u8], maximum: usize, sites: usize) -> Result<Self, &'static str> {
        if bytes.len() > maximum {
            return Err("local preparation request exceeds input byte ceiling");
        }
        let original = crate::json::parse(bytes).map_err(|_| "invalid local preparation JSON")?;
        if !super::recipe::review_response_shape(&original) {
            return Err(
                "review responses require complete named submission, binding and policy objects",
            );
        }
        let mut input: Input = serde_json::from_value(original.clone())
            .map_err(|_| "invalid local preparation shape")?;
        if serde_json::to_value(&input).ok().as_ref() != Some(&original) {
            return Err("local preparation requires the complete named native object shape");
        }
        if input.review_responses.is_some() && input.candidate_check.is_none() {
            return Err("review responses require an explicitly selected candidate checker");
        }
        if let Some(policies) = &input.candidate_policies {
            if input.candidate_check.is_none() {
                return Err("candidate policies require an explicitly selected candidate checker");
            }
            if input.review_responses.is_some() {
                return Err("live candidate policies cannot mix with imported review responses");
            }
            if policies.keys().collect::<BTreeSet<_>>()
                != input.sites.iter().map(|site| &site.render.site).collect()
            {
                return Err("candidate policies require the exact complete requested site set");
            }
        }
        if !input.journal.is_absolute() || input.sites.iter().any(|s| !s.stage_parent.is_absolute())
        {
            return Err("journal and stage parents require explicit absolute paths");
        }
        if input.coordinator.as_ref().is_some_and(|selected| {
            !selected.anchor_root.is_absolute() || !selected.binding.journal_root.is_absolute()
        }) {
            return Err("coordinator anchor and journal binding require explicit absolute paths");
        }
        if input
            .sites
            .iter()
            .map(|site| *site.preparation_id.as_bytes())
            .collect::<BTreeSet<_>>()
            .len()
            != input.sites.len()
        {
            return Err("preparation IDs must be distinct across the complete set");
        }
        for site in &input.sites {
            site.metadata
                .validate()
                .map_err(|_| "invalid local commit recipe")?;
            gazette_core::release::validate_worktree_parent(
                site.metadata.parent.as_ref(),
                site.parent_source.as_ref(),
            )
            .map_err(|_| "parent pin requires exactly one valid explicit local history source")?;
        }
        let publication = PublicationRecipe::new(
            input.sites.iter().map(|s| s.render.clone()).collect(),
            input.selection.clone(),
            input.review_responses.take(),
            sites,
        )?
        .with_math_generation(input.math_generation.take())?
        .with_diagram_generation(input.diagram_generation.take())?
        .with_social_card_generation(input.social_card_generation.take())?;
        Ok(Self { input, publication })
    }

    /// Existing private journal root; useful for reconciliation after failed response delivery.
    pub fn journal(&self) -> &std::path::Path {
        &self.input.journal
    }

    /// Original expectation only, never a later writer observation or a replay capability.
    pub fn expected_header(&self) -> store::JournalHeader {
        self.input.expected_header
    }

    /// Exact original custody selection, retained for independent reconciliation after delivery loss.
    pub fn coordinator(&self) -> Option<&store::coordinator::LocalCoordinatorSelection> {
        self.input.coordinator.as_ref()
    }

    /// Complete requested site/ID coordinates in original order, never discovered from history.
    pub fn preparations(
        &self,
    ) -> impl Iterator<Item = (&crate::workspace::Key, source::Sha256Digest)> {
        self.input
            .sites
            .iter()
            .map(|s| (&s.render.site, s.preparation_id))
    }

    /// Validate every preparation recipe before acquisition, then capture once, render the
    /// complete explicit candidate and run existing local preparation.
    /// No source is reacquired between rendering and preparation. No store is initialized,
    /// migrated, signed or remotely published. A selected checker is independently effectful.
    ///
    /// # Errors
    /// Preserves pre-preparation diagnostics and complete later native failure evidence.
    /// Failed delivery or cancellation does not roll back or authorize repeating effects.
    pub fn execute(
        &self,
        options: LocalPreparationRunOptions,
    ) -> Result<Vec<SitePreparationReceipt>, LocalPreparationRunError> {
        let before = |code, error: &dyn fmt::Display| LocalPreparationRunError::Before {
            code,
            message: error.to_string(),
        };
        if !options.allow_read {
            return Err(before(
                "local_preparation_permission",
                &"explicit local source-read permission required",
            ));
        }
        self.publication
            .check_response_bounds(options.review_response, options.review_response_bytes)
            .map_err(|e| before("local_preparation_review_responses", &e))?;
        let mut configuration = None;
        if let Some(policies) = &self.input.candidate_policies {
            // Compile declarations only: missing or mismatched evaluator selection must
            // fail before source acquisition, journal access or process execution.
            let resolved = self
                .input
                .workspace
                .resolve(options.syntax)
                .map_err(|e| before(e.code(), &e))?;
            let command = resolved
                .policy_command()
                .map_err(|e| before(e.code(), &e))?
                .ok_or_else(|| {
                    before(
                        "policy_unconfigured",
                        &"required candidate checker is not configured",
                    )
                })?;
            for required in policies.values() {
                required
                    .check_command(command)
                    .map_err(|e| before(e.code(), &e))?;
            }
            configuration = Some(resolved);
        }
        let requests = self
            .input
            .sites
            .iter()
            .map(|site| SitePreparationRequest {
                site: &site.render.site,
                local: super::LocalPreparationRequest {
                    stage: super::PreparationRequest {
                        journal: &self.input.journal,
                        expected: self.input.expected_header,
                        parent: &site.stage_parent,
                        name: &site.stage_name,
                        id: site.preparation_id,
                    },
                    metadata: &site.metadata,
                    parent_source: site.parent_source.as_ref(),
                    disclosure: &self.input.disclosure,
                    output_policy: site.output_policy.as_ref(),
                    qualification: site.qualification.as_ref(),
                    coordinator: self.coordinator(),
                },
            })
            .collect::<Vec<_>>();
        for request in &requests {
            request
                .local
                .validate(options.preparation)
                .map_err(|e| before("local_preparation_preflight", &e))?;
        }
        let configuration = configuration
            .map_or_else(|| self.input.workspace.resolve(options.syntax), Ok)
            .map_err(|e| before(e.code(), &e))?;
        let selected = self.publication.sites().cloned().collect::<Vec<_>>();
        let mut inputs =
            super::acquire_resolved_publications(configuration, &selected, options.acquisition)
                .map_err(|e| before(e.code(), &e))?;
        let workspace = &inputs.compilation().workspace;
        gazette_application::build::ensure_deployable_sites(
            workspace,
            requests
                .iter()
                .map(|request| (request.site, workspace.destination(request.site).is_some())),
        )
        .map_err(|error| before(error.code(), &error))?;
        self.publication
            .acquire_documentation_catalogs(
                &mut inputs,
                options.acquisition.sources,
                options.output,
            )
            .map_err(|error| before(error.code(), &error))?;
        // Binding is already decidable from the acquired workspace. Refuse before any
        // optional candidate checker or resource compiler can execute.
        for request in &requests {
            let required_policy = request
                .local
                .effective_output_policy()
                .map_err(|e| before("preparation_qualification", &e))?;
            inputs
                .check_policy_source(required_policy)
                .map_err(|e| before(e.code(), &e))?;
            if let Some(required) = required_policy {
                let command = inputs
                    .configuration()
                    .policy_command()
                    .map_err(|e| before(e.code(), &e))?
                    .ok_or_else(|| {
                        before(
                            "policy_unconfigured",
                            &"required output checker is not configured",
                        )
                    })?;
                required
                    .check_command(command)
                    .map_err(|e| before(e.code(), &e))?;
            }
        }
        if let Some(selected) = self.coordinator() {
            let mut protected = inputs.source_roots().map_err(|e| before(e.code(), &e))?;
            protected.extend(
                requests
                    .iter()
                    .map(|request| request.local.stage.parent.to_owned()),
            );
            protected.extend(requests.iter().filter_map(|request| {
                request
                    .local
                    .parent_source
                    .map(|source| source.path().to_owned())
            }));
            gazette_application::store::coordinator::check_selection_roots(
                &gazette_adapter_fs::FileSystem,
                selected,
                &protected,
            )
            .map_err(|e| before("local_preparation_separation", &e))?;
        }
        if self.input.candidate_check.is_some() {
            // Even a read-only SQLite connection may maintain coordination files.
            // Protect the complete selected source union and every parent-history
            // source before opening the journal, not merely before its later writer.
            let mut protected = inputs.source_roots().map_err(|e| before(e.code(), &e))?;
            protected.extend(requests.iter().filter_map(|request| {
                request
                    .local
                    .parent_source
                    .map(|source| source.path().to_owned())
            }));
            gazette_adapter_fs::FileSystem
                .check_root_separation(&self.input.journal, &protected)
                .map_err(|e| before("local_preparation_separation", &e))?;
            // A read-only observation, not a reservation: later effect boundaries
            // still recheck the exact generation and immutable preparation IDs.
            let database = gazette_adapter_sqlite::Sqlite::journal_database(&self.input.journal);
            for request in &requests {
                let observed = store::inspect_preparation(
                    &database,
                    request.local.stage.id,
                    Some(&self.input.expected_header),
                    options.preparation.stage.writer.session.read,
                )
                .map_err(|e| before("local_preparation_history", &e))?;
                if observed.record.is_some() {
                    return Err(before(
                        "local_preparation_history",
                        &"preparation already exists; inspect and reconcile without rerunning candidate checks",
                    ));
                }
            }
        }
        self.publication
            .acquire_math(&mut inputs, options.rendering.publication.corpus)
            .map_err(|error| LocalPreparationRunError::Math(Box::new(error)))?;
        self.publication
            .acquire_diagrams(&mut inputs, options.rendering.publication.corpus)
            .map_err(|error| LocalPreparationRunError::Diagram(Box::new(error)))?;
        let mut render_options = self.publication.options(options.output);
        render_options.derive_selection = true;
        let rendered = match self.input.candidate_check {
            Some(review) => super::check_and_render_publications_with_generations(
                &inputs,
                render_options,
                options.rendering,
                super::CandidateCheckOptions {
                    process: options.preparation.policy,
                    review,
                    bytes: review.bytes,
                    response: options.review_response,
                    response_bytes: options.review_response_bytes,
                    requirements: self.input.candidate_policies.as_ref(),
                },
                self.publication.social_card_generation(),
            ),
            None => super::render_publications_with_generations(
                &inputs,
                render_options,
                options.rendering,
                self.publication.social_card_generation(),
            ),
        }
        .map_err(|error| match error {
            super::PublicationSetError::Policy(error) => LocalPreparationRunError::Candidate(error),
            error => before(error.code(), &error),
        })?;
        if !rendered.is_complete() {
            return Err(LocalPreparationRunError::Render {
                requested: self.publication.sites().cloned().collect(),
                candidate_checks: rendered.candidate_checks(),
                failures: rendered
                    .sites()
                    .iter()
                    .filter_map(|(site, result)| {
                        result.as_ref().err().map(|error| {
                            (
                                site.clone(),
                                LocalPreparationRenderFailure {
                                    code: error.code(),
                                    message: error.to_string(),
                                },
                            )
                        })
                    })
                    .collect(),
            });
        }
        super::prepare_local_set(&rendered, &requests, options.preparation).map_err(|error| {
            LocalPreparationRunError::Preparation {
                candidate_checks: rendered.candidate_checks(),
                error: Box::new(error),
            }
        })
    }
}
