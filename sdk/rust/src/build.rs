//! Build operations use one application pipeline with private adapter composition.

mod check;
mod diagrams;
pub use check::{CheckDiagnostic, WorkspaceCheck, check_workspace};
pub mod disclosure_input;
mod documentation_catalog;
pub(crate) mod documentation_generation;
pub use documentation_catalog::acquire_documentation_catalogs;
pub use documentation_generation::{
    DocumentationDelivery, DocumentationGenerationRequest, DocumentationPublicationExecution,
    DocumentationPublicationFailure, DocumentationReceipt, generate_publication_documentation,
};
pub mod documentation_output;
mod math;
pub(crate) mod social_cards;
pub use gazette_application::build::{
    SocialCardCaptureEvidence, SocialCardCaptureRequest, SocialCardGenerationError,
    SocialCardGenerationOptions, SocialCardProjection,
};
pub use social_cards::{
    SocialCardGeneration, check_and_render_publications_with_generations,
    render_publications_with_generations,
};
mod settings;
pub use settings::{CorpusLimits, LocalRenderLimits};
pub mod registry_output;
pub(crate) mod resources;
pub use diagrams::{DiagramGeneration, acquire_diagrams};
pub use gazette_application::build::{
    BuildCacheAdapters, BuildCacheClean, BuildCacheError, BuildCacheKey, BuildCachePlan,
    BuildCachePlanError, BuildCacheReconcileOptions, BuildCacheRunError, BuildCacheSchedule,
};

pub use gazette_application::build::{
    DiagramCapture, DiagramGenerationError, DiagramGenerationOptions,
};
pub use gazette_application::build::{MathGenerationError, MathGenerationOptions};
pub use gazette_core::store::CachedBuildSite;
pub use math::{MathGeneration, acquire_math};

mod execution_evidence;
mod qualification;
mod recipe;
pub use qualification::{QualificationOptions, QualificationRecipe, QualificationReport};
pub use recipe::{GraphRecipe, PublicationRecipe, SiteRecipe, WorkspaceInput};
mod selected;
pub use selected::{
    SelectedAcquisitionError, WorkspaceSource, acquire_selected_composition,
    acquire_selected_publication, acquire_selected_site,
};
mod publication;
pub use gazette_application::build::{
    MaterializationError, MaterializationErrorInspection, MaterializationEvidenceInspection,
    MaterializationInspection, MaterializationOptions, MaterializationPlan,
    MaterializationPlanCauseInspection, MaterializationResult, MaterializationSetPlan,
    MaterializationSetPlanError, MaterializationSetPlanFailure, MaterializationSetPlanInspection,
    MaterializationSetWriteError, MaterializationSetWriteInspection,
    MaterializationStoreInspection, MaterializedOutput, MaterializedOutputInspection, OutputChange,
    OutputDestinationInspection, OutputPlan, OutputPlanCauseInspection, OutputPlanError,
    OutputSetPlan, OutputSetPlanError, OutputSetPlanFailure, OutputSetWriteError,
    OutputSourceInspection,
};
pub use gazette_core::destination::OutputWriteError;
pub use publication::DocumentationRenderExecution;
pub use publication::{
    LocalPublicationRecipe, PublicationAddressing, ResolvedLocalPublicationRecipe,
};

/// Observe the configured local destination and bind an exact write-free plan.
/// No configuration fallback, staging, certification or publication occurs.
///
/// # Errors
/// Refuses unresolved/unsafe destinations, changed prior output and unowned collisions.
pub fn plan_output<'a>(
    rendered: &'a PublicationRender<'_>,
    previous: Option<&MaterializedOutput>,
    limits: gazette_core::source::Limits,
) -> Result<OutputPlan<'a>, OutputPlanError> {
    rendered.plan_output(&gazette_adapter_fs::FileSystem, previous, limits)
}

/// Consume one exact local output plan and verify complete filesystem read-back.
/// The caller must exclusively control the local namespace and retain the prior
/// generated evidence independently. This is not durable journal ownership,
/// certified preparation, a release effect or automatic interruption recovery.
///
/// # Errors
/// Refuses missing write permission or changed state; preserves partial effects
/// and exact residue without cleanup or automatic retry on later failure.
pub fn materialize_output(
    plan: OutputPlan<'_>,
    allow_write: bool,
) -> Result<MaterializedOutput, OutputWriteError> {
    plan.materialize(&gazette_adapter_fs::FileSystem, allow_write)
}

/// Plan the complete configured destination set before any native output write.
/// Limits cover all observed and proposed entries/bytes, not a per-site allowance.
///
/// # Errors
/// Refuses incomplete membership, destination overlap, unsafe roots or any failed site.
pub fn plan_outputs<'a>(
    rendered: &'a RenderedPublicationSet<'_>,
    previous: &std::collections::BTreeMap<gazette_core::publication::Key, MaterializedOutput>,
    limits: gazette_core::source::Limits,
) -> Result<OutputSetPlan<'a>, OutputSetPlanError> {
    rendered.plan_outputs(&gazette_adapter_fs::FileSystem, previous, limits)
}

/// Consume every original configured plan in deterministic site-key order.
/// Success proves complete local read-back only; no release or approval occurs.
///
/// # Errors
/// Stops on a failed site, preserving full membership, successful receipts and partial effects.
pub fn materialize_outputs(
    plan: OutputSetPlan<'_>,
    allow_write: bool,
) -> Result<
    std::collections::BTreeMap<gazette_core::publication::Key, MaterializedOutput>,
    OutputSetWriteError,
> {
    plan.materialize(&gazette_adapter_fs::FileSystem, allow_write)
}
mod preparation;
pub use preparation::{
    LocalPreparationDelivery, LocalPreparationRecipe, LocalPreparationRenderFailure,
    LocalPreparationRunError, LocalPreparationRunOptions, LocalPreparationRunReport,
};

pub use gazette_application::build::{
    DeployabilityError, LocalPreparationError, LocalPreparationFailure, LocalPreparationOptions,
    LocalPreparationRequest, LocalPreparationSetError, LocalPreparationSetFailure,
    PreparationCheckOptions, PreparationCheckRequest, PreparationChecks, PreparationError,
    PreparationFailure, PreparationOptions, PreparationQualification, PreparationRequest,
    RepositorySourceProvenance, RuntimeBuildProfile, SitePreparationReceipt,
    SitePreparationRequest, TechnicalCertificate, repository_source_provenance,
    runtime_build_profile, runtime_version_coordinate,
};

/// Retain, check and prepare one exact local publication without reacquiring or rendering inputs.
/// Requires an explicit complete non-draft selection; unfiltered local output is refused.
/// The application owns the sequence and failure history; this facade selects native adapters.
/// Success is checked local preparation, not certification or publication permission.
///
/// # Errors
/// Preserves the prior completed receipt and any later process, journal, stage or Git uncertainty.
pub fn prepare_local(
    rendered: &PublicationRender<'_>,
    request: LocalPreparationRequest<'_>,
    options: LocalPreparationOptions,
) -> Result<gazette_core::store::PreparationReceipt, LocalPreparationError> {
    rendered.prepare_local(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        &crate::store::coordinator::Connector {
            selection: request.coordinator,
            allow_anchor_write: options.allow_anchor_write,
        },
        &gazette_adapter_process::Runner,
        disclosure_inspectors(),
        request,
        options,
    )
}

/// Prepare one complete selected local render set without re-acquisition or publication.
/// Every request shares the same initial journal header; actual native receipts advance it.
/// The application owns preflight, ordering and preservation of partial native outcomes.
///
/// # Errors
/// Preserves complete membership, the completed prefix and any failing-site uncertainty.
pub fn prepare_local_set(
    rendered: &RenderedPublicationSet<'_>,
    requests: &[SitePreparationRequest<'_>],
    options: LocalPreparationOptions,
) -> Result<Vec<SitePreparationReceipt>, LocalPreparationSetError> {
    rendered.prepare_local(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        &crate::store::coordinator::Connector {
            selection: requests
                .first()
                .and_then(|request| request.local.coordinator),
            allow_anchor_write: options.allow_anchor_write,
        },
        &gazette_adapter_process::Runner,
        disclosure_inspectors(),
        requests,
        options,
    )
}

/// Run required disclosure before the optional configured output checker over retained bytes.
/// This creates no stage, journal or Git state and grants no publication approval.
///
/// # Errors
/// Refuses unsuitable output, incomplete disclosure and failed configured execution.
pub fn check_preparation<'r, 'a>(
    rendered: &'r PublicationRender<'a>,
    disclosure: &DisclosurePolicy,
    execution: gazette_core::process::Options,
    limits: DisclosureProbeLimits,
    requirement: Option<&OutputPolicyRequirement>,
) -> Result<PreparationChecks<'r, 'a>, PreparationError> {
    rendered.check_preparation_with_inspectors(
        &gazette_adapter_process::Runner,
        disclosure_inspectors(),
        disclosure,
        execution,
        limits,
        requirement,
    )
}

/// Record retained checks against the original staged preparation without repeating them.
/// This adds no stage, process, Git or publication effect and grants no certification.
///
/// # Errors
/// Preserves exact binding, permission, writer and immutable-record failures.
pub fn record_preparation_check(
    checked: &PreparationChecks<'_, '_>,
    request: PreparationCheckRequest<'_>,
    options: PreparationCheckOptions,
) -> Result<gazette_core::store::PreparationReceipt, PreparationError> {
    checked.record_preparation_check(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_sqlite::Sqlite,
        request,
        options,
    )
}

/// Persist exact intent before staging already-rendered bytes, then record native retention.
/// Requires an explicit complete non-draft selection before journal or filesystem effects.
/// This is local staging preparation, not a complete prepared release or publication approval.
///
/// # Errors
/// Preserves journal history and native residue on failure; existing intent requires reconciliation.
pub fn prepare_stage(
    rendered: &PublicationRender<'_>,
    request: PreparationRequest<'_>,
    options: PreparationOptions,
) -> Result<gazette_core::store::PreparationReceipt, PreparationError> {
    rendered.prepare_stage(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_sqlite::Sqlite,
        request,
        options,
    )
}

/// Parse acquired publication bytes with the same adapter as direct Markdown inspection.
///
/// # Errors
/// Preserves retained-source binding and complete corpus failures.
pub fn parse_publication(
    inputs: &AcquiredInputs,
    limits: crate::corpus::Limits,
) -> Result<ParsedPublication<'_>, PublicationInputError> {
    inputs.parse_publication(limits, &gazette_adapter_markdown::Comrak)
}

/// Parse acquired project documentation without reopening sources.
///
/// # Errors
/// Preserves corpus and retained payload failures.
pub fn parse_documents(
    inputs: &AcquiredSite,
    limits: crate::corpus::Limits,
) -> Result<ParsedSite<'_>, DocumentError> {
    inputs.parse_documents(limits, &gazette_adapter_markdown::Comrak)
}

/// Project a private review through the same Markdown and raster adapters as rendering.
///
/// # Errors
/// Preserves projection and complete-review bounds; no publication permission is granted.
pub fn review(
    publication: &ResolvedPublication<'_, '_>,
    limits: ReviewLimits,
) -> Result<PublicationReview, ReviewError> {
    publication.review(
        limits,
        &crate::raster::inspect,
        &gazette_adapter_markdown::Comrak,
    )
}

/// Review a selected candidate using the same native adapters as rendering.
/// The immutable request binds full selection intent without granting publication permission.
///
/// # Errors
/// Preserves projection and complete-review bounds without a partial request.
pub fn review_selected(
    publication: &SelectedPublication<'_, '_>,
    limits: ReviewLimits,
) -> Result<PublicationReview, ReviewError> {
    publication.review(
        limits,
        &crate::raster::inspect,
        &gazette_adapter_markdown::Comrak,
    )
}

pub use gazette_application::build::{
    AdmissionError, AdmittedPublication, CandidateAdmission, CandidateCheckFailure,
    CandidateCheckOptions, CandidatePolicyRequirement, MatchedOutputResponse, OutputPolicyCheck,
    OutputPolicyEvidence, OutputPolicyRequirement, OutputResponseError, PolicyCheck,
    PolicyCheckError, PolicyInvocation,
};

/// Invoke the configured workspace checker over exact retained candidate-review bytes.
/// Execution requires explicit permission; a zero exit is not final-output or publisher approval.
///
/// # Errors
/// Preserves missing configuration, review limits, process failures and nonzero exit evidence.
pub fn check_policy(
    publication: &SelectedPublication<'_, '_>,
    options: crate::process::Options,
    limits: ReviewLimits,
) -> Result<PolicyCheck, PolicyCheckError> {
    publication.check_policy(
        &gazette_adapter_process::Runner,
        options,
        limits,
        &crate::raster::inspect,
        &gazette_adapter_markdown::Comrak,
    )
}

/// Run the workspace checker against the exact complete retained output and private provenance.
/// No native source reread, rerender, stage, certification or release occurs.
///
/// # Errors
/// Refuses missing configuration/permission, incomplete bounded encoding and failed native checks.
pub fn check_output_policy<'r, 'a>(
    rendered: &'r PublicationRender<'a>,
    options: crate::process::Options,
    requirement: Option<&OutputPolicyRequirement>,
) -> Result<OutputPolicyCheck<'r, 'a>, PolicyCheckError> {
    rendered.check_output_policy(&gazette_adapter_process::Runner, options, requirement)
}

pub use gazette_application::build::{
    DocumentationArtifactEvidence, DocumentationAssembly, DocumentationCatalog,
    DocumentationCatalogEntry, DocumentationError, DocumentationGeneration,
    DocumentationGenerationCause, DocumentationGenerationError, DocumentationGenerationOptions,
    DocumentationJob, DocumentationManualEvidence, DocumentationManualInputKind,
    DocumentationPreparationCheck, DocumentationPreparationQualification, RegistryError,
    RegistryLimits, RegistryProjection, RegistryReconcileError, RegistryReconcileOptions,
};

/// Reconcile all registry-owned rows from a retained publication, never site-filtered output.
/// Native source roots remain protected and no renderer or source is executed again.
///
/// # Errors
/// Preserves permission/preparation, root separation, writer, registry and close evidence.
/// Failed effect read-back never authorizes an automatic retry or generation reset.
pub fn reconcile_registry(
    publication: &ResolvedPublication<'_, '_>,
    root: &std::path::Path,
    expected: crate::store::JournalHeader,
    options: RegistryReconcileOptions,
) -> Result<crate::store::RegistryReceipt, RegistryReconcileError> {
    publication.reconcile_registry(
        &gazette_adapter_sqlite::Sqlite,
        &gazette_adapter_fs::FileSystem,
        root,
        expected,
        options,
    )
}

/// Choose exact warm output or an explicit clean build from the private `SQLite` projection.
///
/// A warm artifact is local derived output only. It is never certification, publication
/// admission or release evidence, and a dependency mismatch never falls back to stale bytes.
///
/// # Errors
/// Refuses unsupported/corrupt storage, exceeded bounds and expected-header mismatch.
pub fn schedule_build_cache_site(
    database: &std::path::Path,
    expected: Option<&crate::store::JournalHeader>,
    key: &BuildCacheKey,
    options: crate::store::ReadOptions,
) -> Result<BuildCacheSchedule, crate::store::Error> {
    gazette_application::build::schedule_build_cache_site(
        &gazette_adapter_sqlite::Sqlite,
        database,
        expected,
        key,
        options,
    )
}

/// Derive exact cache keys from retained acquisition and the latest pre-render phase.
///
/// This resolves selected semantics and compiles Theme templates, but does not compile
/// page resources or render output. A complete warm set can therefore bypass rendering.
///
/// # Errors
/// Refuses incomplete acquisition, invalid semantics, Theme or adapter evidence.
pub fn plan_build_cache(
    inputs: &AcquiredPublicationSet,
    options: &PublicationSetRenderOptions<'_>,
    limits: PublicationSetRenderLimits,
) -> Result<BuildCachePlan, BuildCachePlanError> {
    inputs.plan_build_cache(
        BuildCacheAdapters {
            templates: &gazette_adapter_tera::Compiler,
            resources: &resources::Compiler,
            raster: &crate::raster::inspect,
            raster_profile: crate::raster::PROFILE,
            markdown: &gazette_adapter_markdown::Comrak,
        },
        options,
        limits,
    )
}

/// Revalidate an exact plan and perform the cache-eligible clean render.
///
/// # Errors
/// Refuses changed retained evidence, adapters, options, limits or selected semantics
/// before rendering, then preserves complete-set render failures.
pub fn render_build_cache_clean<'a>(
    inputs: &'a AcquiredPublicationSet,
    plan: BuildCachePlan,
    options: PublicationSetRenderOptions<'_>,
    limits: PublicationSetRenderLimits,
) -> Result<BuildCacheClean<'a>, BuildCacheRunError> {
    plan.render_clean(
        inputs,
        BuildCacheAdapters {
            templates: &gazette_adapter_tera::Compiler,
            resources: &resources::Compiler,
            raster: &crate::raster::inspect,
            raster_profile: crate::raster::PROFILE,
            markdown: &gazette_adapter_markdown::Comrak,
        },
        options,
        limits,
    )
}

/// Replace the complete rebuildable site cache from a clean successful render set.
///
/// # Errors
/// Refuses incomplete dependency coverage, unsafe roots and uncertain native effects.
pub fn reconcile_build_cache(
    clean: &BuildCacheClean<'_>,
    root: &std::path::Path,
    expected: crate::store::JournalHeader,
    options: BuildCacheReconcileOptions,
) -> Result<crate::store::BuildCacheReceipt, BuildCacheError> {
    clean.reconcile_build_cache(
        &gazette_adapter_sqlite::Sqlite,
        &gazette_adapter_fs::FileSystem,
        root,
        expected,
        options,
    )
}

/// Resolve one site's brand-owned documentation destination and complete
/// manual/reference navigation through the selected target preflight only.
///
/// This parses only the supplied workspace bytes. It does not acquire project
/// sources, invoke generators, assemble output, stage files or contact a host.
///
/// # Errors
/// Rejects invalid workspace topology, absent site bindings and target-incompatible
/// documentation intent without returning a partial plan.
pub fn check_documentation(
    source: &str,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
) -> Result<DocumentationCheck, DocumentationError> {
    gazette_application::build::check_documentation(
        source,
        site,
        syntax,
        &gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak),
    )
}

/// Acquire one site's explicitly selected manual and reference sources without native writes.
///
/// # Errors
/// Rejects whole-workspace errors, missing selections, native pin/confinement failures and limits.
pub fn acquire_documentation(
    source: &str,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredInputs, DocumentationError> {
    gazette_application::build::acquire_documentation(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        &gazette_adapter_markdown::Comrak,
        source,
        site,
        syntax,
        limits,
    )
}

/// Acquire documentation sources through one already loaded exact native workspace.
/// Configuration and relative destination coordinates retain that selected owner.
/// No unrelated sources, Theme packages, generators or output directories are discovered.
///
/// # Errors
/// Refuses missing selections, unsafe native inputs, revision mismatches and aggregate bounds.
pub fn acquire_documentation_workspace(
    workspace: crate::configuration::LoadedWorkspace,
    site: &crate::workspace::Key,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredInputs, DocumentationError> {
    gazette_application::build::acquire_resolved_documentation(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        &gazette_adapter_markdown::Comrak,
        workspace.into_resolved(),
        site,
        limits,
    )
}

/// Observe and reconcile the configured documentation repository without staging or generators.
///
/// # Errors
/// Refuses unresolved coordinates, unsafe roots, changed prior ownership and output bounds.
pub fn plan_documentation_output<'a>(
    assembly: &'a DocumentationAssembly<'_>,
    previous: Option<&MaterializedOutput>,
    limits: crate::source::Limits,
) -> Result<OutputPlan<'a>, OutputPlanError> {
    assembly.plan_output(&gazette_adapter_fs::FileSystem, previous, limits)
}

/// Assemble the configured documentation target from exact retained inputs; no generator or hosted effect.
///
/// # Errors
/// Rejects missing artifacts, unsupported target inputs and complete-output limits.
pub fn assemble_documentation(
    inputs: &AcquiredInputs,
    limits: crate::source::Limits,
) -> Result<DocumentationAssembly<'_>, DocumentationError> {
    inputs.assemble_documentation(
        &gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak),
        limits,
    )
}

/// Run configured native generators against retained inputs, without publication effects.
///
/// # Errors
/// Preserves complete prior jobs and native staging/process residue without automatic retry.
pub fn generate_documentation<'a>(
    inputs: &'a AcquiredInputs,
    parent: &std::path::Path,
    options: DocumentationGenerationOptions,
) -> Result<DocumentationGeneration<'a>, DocumentationGenerationError> {
    generate_documentation_with_protected(inputs, parent, options, &[])
}

/// Use the same native generation owner with every publication/configuration root protected at writes.
///
/// # Errors
/// Preserves original generation failures and retained stage evidence.
pub fn generate_documentation_with_protected<'a>(
    inputs: &'a AcquiredInputs,
    parent: &std::path::Path,
    options: DocumentationGenerationOptions,
    protected_roots: &[std::path::PathBuf],
) -> Result<DocumentationGeneration<'a>, DocumentationGenerationError> {
    inputs.generate_documentation_with_protected(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_documentation::NativeGenerator::new(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_process::Runner,
        ),
        &gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak),
        parent,
        options,
        protected_roots,
    )
}

/// Assemble generated and imported references through the same target adapter, without execution.
///
/// # Errors
/// Rejects incompatible artifacts or complete target limits without accepting a partial publication.
pub fn assemble_generated_documentation<'a>(
    generated: &'a DocumentationGeneration<'_>,
    limits: crate::source::Limits,
) -> Result<DocumentationAssembly<'a>, DocumentationError> {
    generated.assemble_documentation(
        &gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak),
        limits,
    )
}

/// Stage a complete local documentation tree while protecting every captured native source.
///
/// # Errors
/// Rejects overlap, canonical path-byte overflow and write/read-back/cleanup failures;
/// does not replace the configured repository.
pub fn stage_documentation(
    assembled: &DocumentationAssembly<'_>,
    parent: &std::path::Path,
    limits: crate::source::Limits,
    directory_bytes: usize,
) -> Result<crate::source::StagedTree, crate::source::StageError> {
    assembled
        .stage(
            &gazette_adapter_fs::FileSystem,
            parent,
            limits,
            directory_bytes,
        )
        .map(crate::source::StagedTree::from_native)
}

/// Durably prepare exact retained documentation build inputs, without hosted-output certification.
/// The selected stage and journal are explicit. Only the generated repository's own optional
/// destination is retained; the authored site's delivery destination is never inherited.
/// Generic journal inspection and worktree preparation consume a separately checked receipt.
///
/// # Errors
/// Preserves original journal coordinates, history and possible stage residue; replay never writes
/// a replacement. No sources, generators, disclosure checkers or remote providers are invoked.
pub fn prepare_documentation(
    assembled: &DocumentationAssembly<'_>,
    request: PreparationRequest<'_>,
    coordinator: Option<&crate::store::coordinator::LocalCoordinatorSelection>,
    allow_anchor_write: bool,
    options: PreparationOptions,
) -> Result<gazette_core::store::PreparationReceipt, PreparationError> {
    assembled.prepare_stage(
        &gazette_adapter_fs::FileSystem,
        &crate::store::coordinator::Connector {
            selection: coordinator,
            allow_anchor_write,
        },
        request,
        options,
    )
}

/// Require complete documentation-output disclosure scanning without rerunning target conversion.
/// Success is retained mechanical evidence, not Mintlify hosting proof or publication approval.
///
/// # Errors
/// Refuses incomplete policy, unsupported formats, findings and bounded scanner failures.
pub fn check_documentation_preparation<'r, 'a>(
    assembled: &'r DocumentationAssembly<'a>,
    policy: &DisclosurePolicy,
    limits: DisclosureProbeLimits,
) -> Result<DocumentationPreparationCheck<'r, 'a>, PreparationError> {
    assembled.check_preparation_with_inspectors(policy, disclosure_inspectors(), limits)
}

/// Append a completed documentation-output disclosure check through the shared journal owner.
/// No source, generator, target conversion, stage, Git or network effect is repeated.
///
/// # Errors
/// Preserves exact journal and stage evidence on writer or record failure.
pub fn record_documentation_preparation_check(
    checked: &DocumentationPreparationCheck<'_, '_>,
    request: PreparationCheckRequest<'_>,
    coordinator: Option<&crate::store::coordinator::LocalCoordinatorSelection>,
    allow_anchor_write: bool,
    options: PreparationCheckOptions,
) -> Result<gazette_core::store::PreparationReceipt, PreparationError> {
    checked.record(
        &gazette_adapter_fs::FileSystem,
        &crate::store::coordinator::Connector {
            selection: coordinator,
            allow_anchor_write,
        },
        request,
        options,
    )
}

pub use gazette_application::build::{
    AcquiredInputs, AcquiredPublicationSet, AcquiredSite, BrowserModule, BrowserModules,
    CapabilityEvidence, CapabilityOutcome, CapabilityProjection, CompiledTheme, CompositionError,
    CompositionInspection, CompositionMetadata, DisclosureClass, DisclosureCoverage,
    DisclosureDecoding, DisclosureEmbeddedCoverage, DisclosureFinding, DisclosurePolicy,
    DisclosureProbe, DisclosureProbeError, DisclosureProbeLimits, DisclosureRepresentation,
    DisclosureRule, DocumentError, DocumentInspection, DocumentationCheck, Error,
    GraphRenderEvidence, GraphRenderOptions, ImportMap, Inspection, MatchedReviewResponse,
    MetadataError, MetadataInspection, PageRenderError, PageRenderEvidence, ParsedPublication,
    ParsedSite, PluginPackageInspection, PublicationInputError, PublicationInspection,
    PublicationOutputLimits, PublicationRender, PublicationRenderError, PublicationRenderLimits,
    PublicationResolutionError, PublicationReview, PublicationSetAcquisitionLimits,
    PublicationSetError, PublicationSetInspection, PublicationSetRenderLimits,
    PublicationSetRenderOptions, PublicationSetSiteInspection, PublicationSiteError,
    PublicationSourceFailure, RenderedPage, RenderedPublicationSet, RenderedSite,
    ResolvedPublication, ResourceEvidence, ResourceInputEvidence, ResourceProjection,
    ReviewDecision, ReviewError, ReviewLimits, ReviewOutcome, ReviewPolicy, ReviewRequest,
    ReviewResponseBinding, ReviewResponseError, ReviewResponseLimits, ReviewSubmission,
    ReviewSurface, ReviewTarget, ReviewUse, ReviewUses, SelectedPublication, SitePageEvidence,
    SiteRenderError, SiteRenderEvidence, SiteRenderOptions, TemplateCompilation, TemplateError,
    ThemePackageInspection,
};

pub use gazette_compiler::disclosure::Pattern as DisclosurePattern;

/// Native decoder composition shared by every output qualification path.
/// The application owns traversal and evidence, not these concrete adapters.
pub(crate) fn disclosure_inspectors() -> gazette_application::build::DisclosureInspectors<'static> {
    gazette_application::build::DisclosureInspectors {
        raster: Some(&crate::raster::metadata),
        font: Some(&crate::font::inspect),
        markdown: Some(&gazette_adapter_markdown::Comrak),
    }
}

/// Probe retained bytes with the same native decoders used by required qualification.
/// The result is diagnostic evidence, never certification or publication permission.
///
/// # Errors
/// Preserves malformed syntax, missing coverage and exhausted bounds.
pub fn probe_disclosure<'a>(
    output: &'a gazette_core::source::CapturedSource,
    media_types: &std::collections::BTreeMap<
        gazette_core::source::EntryPath,
        gazette_core::render::MediaType,
    >,
    rules: &std::collections::BTreeMap<crate::workspace::Key, String>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    gazette_application::build::probe_disclosure_with_inspectors(
        output,
        media_types,
        rules,
        disclosure_inspectors(),
        limits,
    )
}

/// Require supported retained-output decoding and no selected literal findings.
///
/// # Errors
/// Refuses unsupported coverage, findings, malformed syntax and exhausted bounds.
pub fn check_disclosure<'a>(
    output: &'a gazette_core::source::CapturedSource,
    media_types: &std::collections::BTreeMap<
        gazette_core::source::EntryPath,
        gazette_core::render::MediaType,
    >,
    rules: &std::collections::BTreeMap<crate::workspace::Key, String>,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    gazette_application::build::check_disclosure_with_inspectors(
        output,
        media_types,
        rules,
        disclosure_inspectors(),
        limits,
    )
}

/// Check retained output with the SDK-selected native decoders.
/// The application owns the single required scanner and its exact-byte policy evidence.
///
/// # Errors
/// Refuses unsupported coverage, mismatched evidence, findings and exhausted bounds.
pub fn check_disclosure_policy<'a>(
    output: &'a gazette_core::source::CapturedSource,
    media_types: &std::collections::BTreeMap<
        gazette_core::source::EntryPath,
        gazette_core::render::MediaType,
    >,
    policy: &DisclosurePolicy,
    limits: DisclosureProbeLimits,
) -> Result<DisclosureProbe<'a>, DisclosureProbeError> {
    gazette_application::build::check_disclosure_policy_with_inspectors(
        output,
        media_types,
        policy,
        disclosure_inspectors(),
        limits,
    )
}

/// Capture one exact local site set under one shared configuration/source budget.
///
/// # Errors
/// Rejects invalid selection, configuration and shared content acquisition failure.
/// Presentation-only failures remain in the exact site's outcome; no stale fallback occurs.
pub fn acquire_publications(
    source: &str,
    sites: &[crate::workspace::Key],
    syntax: crate::toml::Limits,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let mut inputs = gazette_application::build::acquire_publications(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        source,
        sites,
        syntax,
        limits,
    )?;
    inputs.acquire_direct_local_capabilities(syntax, Default::default())?;
    Ok(inputs)
}

/// Acquire exactly the workspace-owned default site set without an invocation override.
///
/// # Errors
/// Refuses absent defaults, unknown sites, native source failures and bounded capture failures.
/// Read permission remains the caller's responsibility, as for explicit acquisition.
pub fn acquire_configured_publications(
    source: &str,
    syntax: crate::toml::Limits,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let configuration = gazette_application::configuration::resolve(source, syntax)
        .map_err(PublicationSetError::Configuration)?;
    let mut inputs = gazette_application::build::acquire_configured_publications(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        configuration,
        limits,
    )?;
    inputs.acquire_direct_local_capabilities(syntax, Default::default())?;
    Ok(inputs)
}

pub(crate) fn acquire_resolved_publications(
    configuration: gazette_application::configuration::ResolvedWorkspace,
    sites: &[crate::workspace::Key],
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let mut inputs = gazette_application::build::acquire_resolved_publications(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        configuration,
        sites,
        limits,
    )?;
    inputs.acquire_direct_local_capabilities(Default::default(), Default::default())?;
    Ok(inputs)
}

pub(crate) fn acquire_selected_publications(
    configuration: gazette_application::configuration::ResolvedWorkspace,
    selection: &crate::workspace::BuildSelection,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let mut inputs = gazette_application::build::acquire_selected_publications(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        configuration,
        selection,
        limits,
    )?;
    inputs.acquire_direct_local_capabilities(Default::default(), Default::default())?;
    Ok(inputs)
}

/// Render all requested sites from one retained semantic model, optionally selecting
/// one closed candidate before all site outputs. No staging or native effects.
///
/// # Errors
/// Rejects mismatched site options, candidate closure or shared syntax/semantic prerequisites. Site-local
/// failures remain explicit values in the result; inspect `is_complete` before claiming
/// the whole local operation succeeded. This is not release settlement or certification.
pub fn render_publications<'a>(
    inputs: &'a AcquiredPublicationSet,
    options: PublicationSetRenderOptions<'_>,
    limits: PublicationSetRenderLimits,
) -> Result<RenderedPublicationSet<'a>, PublicationSetError> {
    let resources =
        gazette_application::build::ResourceCache::new(resources::Compiler, Default::default());
    inputs.render_local_with_host_files(
        &gazette_adapter_tera::Compiler,
        &resources,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        Some(&gazette_integration_github::HostFiles),
    )
}

/// Run explicitly selected candidate checks before rendering the same retained site set.
/// No source is reacquired; zero-exit checks remain private, unverified invocation evidence.
///
/// # Errors
/// Preserves the completed checker prefix and failing invocation; no implicit retry.
pub fn check_and_render_publications<'a>(
    inputs: &'a AcquiredPublicationSet,
    options: PublicationSetRenderOptions<'_>,
    limits: PublicationSetRenderLimits,
    policy: CandidateCheckOptions<'_>,
) -> Result<RenderedPublicationSet<'a>, PublicationSetError> {
    let resources =
        gazette_application::build::ResourceCache::new(resources::Compiler, Default::default());
    inputs.check_and_render_local_with_host_files(
        &gazette_adapter_tera::Compiler,
        &resources,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        &gazette_adapter_process::Runner,
        policy,
        Some(&gazette_integration_github::HostFiles),
    )
}

/// Render complete local publication output from retained inputs; no native reads or writes.
///
/// # Errors
/// Rejects unsupported requested inputs, invalid semantics, template failures and output limits.
pub fn render_publication<'a>(
    inputs: &'a AcquiredInputs,
    options: SiteRenderOptions<'_>,
    limits: PublicationRenderLimits,
) -> Result<PublicationRender<'a>, PublicationRenderError> {
    inputs.render_publication_with_host_files(
        &gazette_adapter_tera::Compiler,
        &resources::Compiler,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        Some(&gazette_integration_github::HostFiles),
    )
}

/// Render an already checked publication without parsing or resolving its content again.
/// Uses the same adapter composition as the full parse/resolve/render convenience path.
///
/// # Errors
/// Propagates presentation, resource, rendering and complete-output failures.
/// Success is local output only, not admission, certification or release permission.
pub fn render_resolved_publication<'s>(
    publication: &ResolvedPublication<'_, 's>,
    options: SiteRenderOptions<'_>,
    limits: PublicationOutputLimits,
) -> Result<PublicationRender<'s>, PublicationRenderError> {
    publication.render_local_with_host_files(
        &gazette_adapter_tera::Compiler,
        &resources::Compiler,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        Some(&gazette_integration_github::HostFiles),
    )
}

/// Render a closed candidate through the same native adapters and shared model.
/// Its complete private selection is retained beside source/output evidence;
/// neither this operation nor the candidate establishes publication permission.
///
/// # Errors
/// Preserves complete-site and resource failures without returning partial output.
pub fn render_selected_publication<'s>(
    publication: &SelectedPublication<'_, 's>,
    options: SiteRenderOptions<'_>,
    limits: PublicationOutputLimits,
) -> Result<PublicationRender<'s>, PublicationRenderError> {
    publication.render_local_with_host_files(
        &gazette_adapter_tera::Compiler,
        &resources::Compiler,
        &crate::raster::inspect,
        options,
        limits,
        &gazette_adapter_markdown::Comrak,
        Some(&gazette_integration_github::HostFiles),
    )
}

/// Stage complete local output while protecting all consumed native roots.
///
/// # Errors
/// Rejects source/output overlap and canonical directory-byte overflow before creation;
/// preserves native write, read-back and cleanup failures.
pub fn stage_publication(
    rendered: &PublicationRender<'_>,
    parent: &std::path::Path,
    limits: crate::source::Limits,
    directory_bytes: usize,
) -> Result<crate::source::StagedTree, crate::source::StageError> {
    rendered
        .stage(
            &gazette_adapter_fs::FileSystem,
            parent,
            limits,
            directory_bytes,
        )
        .map(crate::source::StagedTree::from_native)
}

/// Compile the metadata-selected retained template subtree through the rendering adapter.
///
/// # Errors
/// Rejects input bounds, template errors or mismatched compiler identity without native effects.
pub fn compile_templates(
    metadata: &CompositionMetadata<'_>,
    limits: crate::render::Limits,
) -> Result<CompiledTheme, TemplateError> {
    metadata.compile_templates(&gazette_adapter_tera::Compiler, limits)
}

/// Capture one site's effective pinned Theme and capability sources without writes.
///
/// # Errors
/// Propagates configuration and complete-set source failures; no fallback or partial result.
pub fn acquire_composition(
    source: &str,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    gazette_application::build::acquire_composition(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        source,
        site,
        syntax,
        limits,
    )
}

/// Capture presentation, authored and enabled project-document sources once per exact key.
///
/// # Errors
/// Rejects missing bindings, invalid configuration, native verification and whole-set limits.
pub fn acquire_publication(
    source: &str,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    let mut inputs = gazette_application::build::acquire_publication(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        source,
        site,
        syntax,
        limits,
    )?;
    inputs
        .acquire_direct_local_capabilities(syntax, Default::default())
        .map_err(|error| CompositionError::Capability(Box::new(error)))?;
    Ok(inputs)
}

/// Compile a whole workspace and retain one explicitly selected site's pinned documentation.
///
/// No native state is modified. This is source acquisition, not a rendered build.
///
/// # Errors
/// Propagates whole-configuration and selected-source failures without partial results.
pub fn acquire_site(
    source: &str,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredSite, Error> {
    gazette_application::build::acquire_site(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        source,
        site,
        syntax,
        limits,
    )
}
