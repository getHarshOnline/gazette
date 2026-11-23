//! Rendered-output, staging and probing commands shared by CLI and MCP.

use super::{Command, Kernel};
use crate::command::publication;
use gazette_sdk::{build, source, toml, workspace};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

/// Stable rendered-output failure, including exact native stage residue details
/// when an effect may have started.
#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
    stage: Option<source::StageError>,
}

impl Failure {
    fn before(code: &'static str, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
            stage: None,
        }
    }

    fn stage(error: source::StageError) -> Self {
        Self {
            code: "source_staging",
            message: error.to_string(),
            stage: Some(error),
        }
    }

    /// Stable failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Redacted operator-facing diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Exact native stage failure when an effect may have started.
    #[must_use]
    pub const fn stage_error(&self) -> Option<&source::StageError> {
        self.stage.as_ref()
    }

    /// Recover the exact native partial-effect failure when staging started.
    ///
    /// # Errors
    /// Returns the unchanged pre-effect failure when staging never started.
    pub fn into_stage_error(self) -> Result<source::StageError, Self> {
        let Self {
            code,
            message,
            stage,
        } = self;
        match stage {
            Some(error) => Ok(error),
            None => Err(Self {
                code,
                message,
                stage: None,
            }),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Bound source composition shared by stage checking and retention.
pub struct StageInput {
    /// Permission for exact selected native source reads.
    pub allow_read: bool,
    /// Explicit workspace/site and placement identities.
    pub context: source::StageContext,
    /// Complete bound placement request set.
    pub placements: Vec<source::PlacementRequest>,
    /// Complete source acquisition ceilings.
    pub preparation: source::PreparationLimits,
    /// Explicit authored input root.
    pub authored: PathBuf,
    /// Existing private staging parent.
    pub parent: PathBuf,
    /// Authored-input and complete-composition ceilings.
    pub limits: source::Limits,
}

/// Successful write-free composition check.
pub struct CheckedStage {
    context: source::StageContext,
    inventory: source::Inventory,
}

impl CheckedStage {
    /// Explicit workspace/site context checked.
    #[must_use]
    pub const fn context(&self) -> &source::StageContext {
        &self.context
    }

    /// Complete resulting inventory.
    #[must_use]
    pub const fn inventory(&self) -> &source::Inventory {
        &self.inventory
    }
}

/// Write-free staging preflight command.
pub struct CheckStage(pub StageInput);

impl Command for CheckStage {
    type Outcome = CheckedStage;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let input = self.0;
        if !input.allow_read {
            return Err(Failure::before(
                "source_staging_permission",
                "explicit native-read permission required; no source acquisition started",
            ));
        }
        let prepared = source::prepare(input.placements, input.preparation)
            .map_err(|error| Failure::before("source_staging", error))?;
        let inventory = source::check_stage(
            &prepared,
            &input.context,
            &input.authored,
            &input.parent,
            input.limits,
        )
        .map_err(Failure::stage)?;
        Ok(CheckedStage {
            context: input.context,
            inventory,
        })
    }
}

/// Fresh source-stage request. A named location never falls back on collision.
pub struct CreateStage {
    /// Bound source composition.
    pub input: StageInput,
    /// Optional exact fresh child name.
    pub name: Option<String>,
    /// Optional canonical native directory byte ceiling.
    pub directory_bytes: Option<usize>,
}

/// Retained exact native stage and its explicit workspace/site binding.
pub struct RetainedStage {
    context: source::StageContext,
    retained: source::RetainedStage,
}

impl RetainedStage {
    /// Explicit workspace/site context retained.
    #[must_use]
    pub const fn context(&self) -> &source::StageContext {
        &self.context
    }

    /// Exact retained native stage.
    pub const fn retained(&self) -> &source::RetainedStage {
        &self.retained
    }

    /// Consume the outcome without reopening native paths.
    pub fn into_parts(self) -> (source::StageContext, source::RetainedStage) {
        (self.context, self.retained)
    }
}

impl Command for CreateStage {
    type Outcome = RetainedStage;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let input = self.input;
        if !input.allow_read {
            return Err(Failure::before(
                "source_staging_permission",
                "explicit native-read permission required; no source acquisition started",
            ));
        }
        if let Some(bytes) = self.directory_bytes
            && !(1..=1 << 20).contains(&bytes)
        {
            return Err(Failure::before(
                "stage_response_limit",
                "canonical stage directory byte ceiling must be in 1..=1048576",
            ));
        }
        let prepared = source::prepare(input.placements, input.preparation)
            .map_err(|error| Failure::before("source_staging", error))?;
        let location = match (self.name.as_deref(), self.directory_bytes) {
            (Some(name), Some(bytes)) => source::StageLocation::NamedBounded {
                parent: &input.parent,
                name,
                directory_bytes: bytes,
            },
            (None, Some(bytes)) => source::StageLocation::FreshBounded {
                parent: &input.parent,
                directory_bytes: bytes,
            },
            (Some(name), None) => source::StageLocation::Named {
                parent: &input.parent,
                name,
            },
            (None, None) => source::StageLocation::Fresh(&input.parent),
        };
        let retained = source::stage(
            &prepared,
            &input.context,
            &input.authored,
            location,
            input.limits,
        )
        .and_then(source::StagedTree::retain)
        .map_err(Failure::stage)?;
        Ok(RetainedStage {
            context: input.context,
            retained,
        })
    }
}

/// Independently pinned imported-output disclosure probe.
pub struct ProbeOutput {
    /// Explicit imported-output root.
    pub root: PathBuf,
    /// Independently retained exact tree identity.
    pub expected_tree: source::TreeIdentity,
    /// Native capture ceilings.
    pub source_limits: source::Limits,
    /// Complete private disclosure rules JSON.
    pub rules_json: String,
    /// Complete rules byte ceiling.
    pub max_rules_bytes: usize,
    /// Native scanner ceilings.
    pub scan_limits: build::DisclosureProbeLimits,
}

impl Command for ProbeOutput {
    type Outcome = Value;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        self.scan_limits
            .validate()
            .map_err(|error| Failure::before(error.code(), error))?;
        let rules = build::disclosure_input::output_rules(&self.rules_json, self.max_rules_bytes)
            .map_err(|error| Failure::before(error.code(), error))?;
        let output = source::capture_pinned(&self.root, self.expected_tree, self.source_limits)
            .map_err(|error| {
                Failure::before(
                    error.code(),
                    "retained tree capture or exact identity check failed; no report produced",
                )
            })?;
        let report = build::probe_disclosure(&output, &BTreeMap::new(), &rules, self.scan_limits)
            .map_err(|error| Failure::before(error.code(), error))?;
        serde_json::to_value(report)
            .map_err(|error| Failure::before("disclosure_output_response", error))
    }
}

/// One-site local render using explicit workspace source and Theme-owned entries.
pub struct RenderSite {
    /// Optional invocation corpus ceiling; the workspace supplies an omitted value.
    pub corpus_bytes: Option<usize>,
    /// Optional invocation math selection.
    pub math: Option<bool>,
    /// Explicit permission for the configured math provider.
    pub allow_execute: bool,
    /// Explicit local-operation acknowledgement.
    pub local: bool,
    /// Complete workspace TOML.
    pub source: String,
    /// Exact selected publication site.
    pub site: workspace::Key,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Source acquisition ceilings.
    pub acquisition: source::PreparationLimits,
    /// Owned output render selection.
    pub options: OwnedSiteRenderOptions,
    /// Optional private output retention parent.
    pub output_parent: Option<PathBuf>,
    /// Canonical retained-directory byte ceiling.
    pub directory_bytes: usize,
    /// Optional private disclosure-probe selection.
    pub disclosure: Option<(
        build::disclosure_input::WorkspaceInput,
        build::DisclosureProbeLimits,
    )>,
}

/// Owned one-site output selection; the selected Theme owns render entries.
pub struct OwnedSiteRenderOptions {
    /// Explicit presentation context; the application never reads a calendar clock.
    pub render_year: Option<gazette_sdk::authored::RenderYear>,
    /// Optional shared-model debug artifact; compilation never depends on this file.
    pub model_output: Option<source::EntryPath>,
    /// Optional graph-view output path.
    pub graph_output: Option<source::EntryPath>,
    /// Complete generated-output ceilings.
    pub limits: source::Limits,
}

/// Exact rendered report plus optional retained custody.
pub struct RenderSiteOutcome {
    report: Value,
    retained: Option<source::RetainedStage>,
}

impl RenderSiteOutcome {
    /// Complete owned operator report.
    #[must_use]
    pub fn report(&self) -> &Value {
        &self.report
    }

    /// Exact retained stage, when requested.
    #[must_use]
    pub const fn retained(&self) -> Option<&source::RetainedStage> {
        self.retained.as_ref()
    }

    /// Consume the owned report and optional retention custody.
    #[must_use]
    pub fn into_parts(self) -> (Value, Option<source::RetainedStage>) {
        (self.report, self.retained)
    }
}

/// Direct rendering keeps native documentation effects inside their owning callback.
pub enum DocumentationRenderSiteExecution<'a> {
    /// Rendering or output retention finished after all documentation catalogs were installed.
    Rendered {
        /// Original render/stage outcome, including typed native stage failures.
        outcome: Result<RenderSiteOutcome, Failure>,
        /// Every original completed documentation delivery.
        receipts: &'a [build::DocumentationReceipt],
    },
    /// Catalog acquisition or attachment failed with earlier deliveries retained.
    Refused {
        /// Original completed documentation deliveries.
        receipts: &'a [build::DocumentationReceipt],
        /// Original owning failure.
        error: build::DocumentationPublicationFailure,
    },
    /// Exceptional native custody is consumed before the documentation operation returns.
    Native {
        /// Exact configured documentation owner.
        site: &'a workspace::Key,
        /// Earlier completed documentation deliveries.
        receipts: &'a [build::DocumentationReceipt],
        /// Original native operation delivery, including borrowed Contract custody.
        execution: build::documentation_output::Execution<'a>,
    },
}

impl RenderSite {
    fn acquire(&self) -> Result<build::AcquiredPublicationSet, Failure> {
        if !self.local {
            return Err(Failure::before(
                "publication_mode",
                "--local is required; public admission is not implemented",
            ));
        }
        let inputs = build::acquire_publications(
            &self.source,
            std::slice::from_ref(&self.site),
            self.syntax,
            build::PublicationSetAcquisitionLimits {
                sites: 1,
                sources: self.acquisition,
            },
        )
        .map_err(|error| Failure::before(error.code(), error))?;
        Ok(inputs)
    }

    /// Execute explicitly selected documentation generation before direct rendering.
    /// Later rendering and retention failures reach the callback with every earlier receipt.
    ///
    /// # Errors
    /// Returns only selection or publication acquisition failures before documentation effects.
    pub fn with_documentation<T>(
        self,
        request: &build::DocumentationGenerationRequest,
        consume: impl FnOnce(DocumentationRenderSiteExecution<'_>) -> T,
    ) -> Result<T, Failure> {
        let mut request = request.clone();
        request.allow_execute &= self.allow_execute;
        request
            .validate()
            .map_err(|error| Failure::before("documentation_generation_selection", error))?;
        let mut inputs = self.acquire()?;
        Ok(build::generate_publication_documentation(
            &mut inputs,
            self.acquisition,
            self.options.limits,
            Some(&request),
            None,
            |execution| match execution {
                build::DocumentationPublicationExecution::Ready { inputs, receipts } => {
                    consume(DocumentationRenderSiteExecution::Rendered {
                        outcome: self.render_acquired(inputs),
                        receipts,
                    })
                }
                build::DocumentationPublicationExecution::Refused { receipts, error } => {
                    consume(DocumentationRenderSiteExecution::Refused { receipts, error })
                }
                build::DocumentationPublicationExecution::Native {
                    site,
                    receipts,
                    execution,
                } => consume(DocumentationRenderSiteExecution::Native {
                    site,
                    receipts,
                    execution,
                }),
            },
        ))
    }

    fn render_acquired(
        self,
        inputs: &mut build::AcquiredPublicationSet,
    ) -> Result<RenderSiteOutcome, Failure> {
        let workspace = &inputs.compilation().workspace;
        let corpus = build::CorpusLimits {
            source_bytes: self.corpus_bytes,
            ..Default::default()
        }
        .resolve(workspace);
        let math = build::MathGeneration::resolve(workspace, self.math, None, self.allow_execute);
        let social_cards =
            build::SocialCardGeneration::resolve(workspace, None, self.allow_execute);
        if let Some(math) = math {
            build::acquire_math(inputs, corpus, math.options())
                .map_err(|error| Failure::before(error.code(), error))?;
        }
        let output_limits = self.options.limits;
        let rendered_set = build::render_publications_with_generations(
            inputs,
            build::PublicationSetRenderOptions {
                derive_selection: false,
                preview_link_bases: None,
                sites: BTreeMap::from([(
                    self.site.clone(),
                    build::SiteRenderOptions {
                        render_year: self.options.render_year,
                        model_output: self.options.model_output.as_ref(),
                        graph: self.options.graph_output.as_ref().map(|output| {
                            build::GraphRenderOptions {
                                output,
                                limits: Default::default(),
                            }
                        }),
                        limits: output_limits,
                    },
                )]),
                selection: None,
                review_responses: None,
                preview_urls: None,
            },
            build::PublicationSetRenderLimits {
                publication: build::PublicationRenderLimits {
                    corpus,
                    ..Default::default()
                },
                ..Default::default()
            },
            social_cards.map(build::SocialCardGeneration::options),
        )
        .map_err(|error| Failure::before(error.code(), error))?;
        let rendered = rendered_set
            .sites()
            .get(&self.site)
            .expect("exact one-site render selection")
            .as_ref()
            .map_err(|error| Failure::before(error.code(), error))?;
        if let Some((request, limits)) = self.disclosure {
            let probe = build::probe_disclosure(
                rendered.site().output(),
                rendered.site().media_types(),
                &request.rules,
                limits,
            )
            .map_err(|error| Failure::before(error.code(), error))?;
            let report = serde_json::to_value(probe)
                .map_err(|error| Failure::before("publication_response", error))?;
            return Ok(RenderSiteOutcome {
                report,
                retained: None,
            });
        }
        let retained = match self.output_parent {
            Some(parent) => Some(
                build::stage_publication(rendered, &parent, output_limits, self.directory_bytes)
                    .and_then(source::StagedTree::retain)
                    .map_err(Failure::stage)?,
            ),
            None => None,
        };
        #[derive(Serialize)]
        struct Report<'a> {
            status: &'static str,
            inputs: build::CompositionInspection<'a>,
            build_selection: &'a workspace::BuildSelection,
            evidence: &'a build::SiteRenderEvidence,
            inventory: &'a source::Inventory,
            directory: Option<&'a std::path::Path>,
        }
        let report = serde_json::to_value(Report {
            status: if retained.is_some() {
                "local_retained"
            } else {
                "local_rendered"
            },
            inputs: inputs
                .sites()
                .get(&self.site)
                .expect("exact one-site acquisition")
                .as_ref()
                .map_err(|error| Failure::before(error.code(), error))?
                .inspect(),
            build_selection: inputs.build_selection(),
            evidence: rendered.site().evidence(),
            inventory: rendered.site().output().inventory(),
            directory: retained.as_ref().map(source::RetainedStage::path),
        })
        .map_err(|error| Failure::before("publication_response", error))?;
        Ok(RenderSiteOutcome { report, retained })
    }
}

impl Command for RenderSite {
    type Outcome = RenderSiteOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut inputs = self.acquire()?;
        build::acquire_documentation_catalogs(&mut inputs, self.acquisition, self.options.limits)
            .map_err(|error| Failure::before(error.code(), error))?;
        self.render_acquired(&mut inputs)
    }
}

/// Render one selected site and retain its output through the common publication owner.
pub struct StageRenderedSite {
    /// Shared publication acquisition/render request.
    pub publication: publication::Render,
    /// Existing private output parent.
    pub output_parent: PathBuf,
    /// Canonical retained-directory byte ceiling.
    pub directory_bytes: usize,
}

impl Command for StageRenderedSite {
    type Outcome = RenderSiteOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.output_parent.is_absolute() || !(1..=1 << 20).contains(&self.directory_bytes) {
            return Err(Failure::before(
                "stage_preflight",
                "absolute output parent and bounded directory bytes are required before rendering",
            ));
        }
        let parsed = self
            .publication
            .parse()
            .map_err(|error| Failure::before(error.code(), error))?;
        let parsed = parsed
            .resolve()
            .map_err(|error| Failure::before(error.code(), error))?;
        if parsed.recipe.sites().count() != 1 {
            return Err(Failure::before(
                "publication_site_selection",
                "exactly one resolved site is required; no content acquisition started",
            ));
        }
        let output_limits = parsed.output_limits();
        parsed
            .consume_rendered(|set| {
                let rendered = set
                    .sites()
                    .values()
                    .next()
                    .expect("one-site membership")
                    .as_ref()
                    .map_err(|error| Failure::before(error.code(), error))?;
                let retained = build::stage_publication(
                    rendered,
                    &self.output_parent,
                    output_limits,
                    self.directory_bytes,
                )
                .and_then(source::StagedTree::retain)
                .map_err(Failure::stage)?;
                #[derive(Serialize)]
                struct Report<'a> {
                    status: &'static str,
                    inputs: build::CompositionInspection<'a>,
                    evidence: &'a build::SiteRenderEvidence,
                    inventory: &'a source::Inventory,
                    directory: &'a std::path::Path,
                }
                let report = serde_json::to_value(Report {
                    status: "local_retained",
                    inputs: rendered.inputs().inspect(),
                    evidence: rendered.site().evidence(),
                    inventory: retained.inventory(),
                    directory: retained.path(),
                })
                .map_err(|error| Failure::before("publication_response", error))?;
                Ok(RenderSiteOutcome {
                    report,
                    retained: Some(retained),
                })
            })
            .map_err(|error| Failure::before(error.code(), error))?
    }
}

/// Render one selected site and probe its original in-memory output.
pub struct ProbeRenderedSite {
    /// Shared publication acquisition/render request.
    pub publication: publication::Render,
    /// Complete private disclosure rules JSON.
    pub rules_json: String,
    /// Complete rules byte ceiling.
    pub max_rules_bytes: usize,
    /// Native scanner ceilings.
    pub scan_limits: build::DisclosureProbeLimits,
}

impl Command for ProbeRenderedSite {
    type Outcome = Value;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        self.scan_limits
            .validate()
            .map_err(|error| Failure::before(error.code(), error))?;
        let rules = build::disclosure_input::output_rules(&self.rules_json, self.max_rules_bytes)
            .map_err(|error| Failure::before(error.code(), error))?;
        let parsed = self
            .publication
            .parse()
            .map_err(|error| Failure::before(error.code(), error))?;
        let parsed = parsed
            .resolve()
            .map_err(|error| Failure::before(error.code(), error))?;
        if parsed.recipe.sites().count() != 1 {
            return Err(Failure::before(
                "publication_site_selection",
                "exactly one resolved site is required; no content acquisition started",
            ));
        }
        parsed
            .consume_rendered(|set| {
                let rendered = set
                    .sites()
                    .values()
                    .next()
                    .expect("one-site membership")
                    .as_ref()
                    .map_err(|error| Failure::before(error.code(), error))?;
                let report = build::probe_disclosure(
                    rendered.site().output(),
                    rendered.site().media_types(),
                    &rules,
                    self.scan_limits,
                )
                .map_err(|error| Failure::before(error.code(), error))?;
                serde_json::to_value(report)
                    .map_err(|error| Failure::before("publication_response", error))
            })
            .map_err(|error| Failure::before(error.code(), error))?
    }
}
