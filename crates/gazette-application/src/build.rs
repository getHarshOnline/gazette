//! Publication build orchestration over the pure compiler and narrow native ports.
//!
//! The compiler owns syntax trees and semantic resolution. This module connects
//! those results to bounded native phases; adapters never reconstruct workspace
//! meaning. Source acquisition feeds retained document syntax. Rendering,
//! persistence and certification are not implied by either phase.

mod binding;
mod capabilities;
mod capability_status;
mod components;
mod composition;
mod deployment;
mod diagrams;
mod disclosure;
mod documentation;
mod documentation_reading;
mod documents;
mod incremental;
mod islands;
mod listings;
mod math;
mod metadata;
mod mirrors;
mod output;
pub use listings::ListingRenderEvidence;
pub use output::{
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
mod page;
mod page_resources;
mod policy;
mod preparation;
mod presentation;
mod programs;
mod repository_source;
mod resource_cache;
mod resource_inputs;
pub use resource_inputs::ResourceInputEvidence;
mod resources;
pub use resource_cache::{ResourceCache, ResourceCacheLimits};
mod publication;
mod publication_inputs;
mod publication_model;
mod publication_set;
mod registry;
mod review;
mod search;
mod serialization;
mod site;
mod social_cards;
pub use social_cards::{
    SocialCardCaptureEvidence, SocialCardCaptureRequest, SocialCardGeneration,
    SocialCardGenerationError, SocialCardGenerationOptions, SocialCardProjection,
};
mod taxonomies;
pub use taxonomies::TaxonomyRenderEvidence;
mod sources;
mod templates;
pub mod theme_namespace;

/// Draft 2020-12 schema for the exact page context supplied to Theme templates.
#[must_use]
pub fn template_context_schema() -> schemars::Schema {
    page::template_context_schema()
}

pub use capabilities::{
    CapabilityAcquisitionError, CapabilityAcquisitionLimits, CapabilityCapture,
    FrozenCapabilityResult, FrozenProviderCapture,
};
pub use capability_status::{CapabilityEvidence, CapabilityOutcome, CapabilityProjection};
pub use composition::{
    AcquiredInputs, CompositionError, CompositionInspection, acquire_composition,
    acquire_resolved_composition,
};
pub use diagrams::{DiagramCapture, DiagramGenerationError, DiagramGenerationOptions};
pub use disclosure::{
    DisclosureClass, DisclosureCoverage, DisclosureDecoding, DisclosureEmbeddedCoverage,
    DisclosureFinding, DisclosureInspectors, DisclosurePattern, DisclosurePolicy, DisclosureProbe,
    DisclosureProbeError, DisclosureProbeLimits, DisclosureRepresentation, DisclosureRule,
    DisclosureStatus, check_disclosure, check_disclosure_policy, check_disclosure_policy_with_font,
    check_disclosure_policy_with_inspectors, check_disclosure_with_inspectors, probe_disclosure,
    probe_disclosure_with_inspectors,
};
pub(crate) use documentation::check_documentation_workspace_contract;
pub use documentation::{
    DocumentationArtifactEvidence, DocumentationAssembly, DocumentationCatalog,
    DocumentationCatalogEntry, DocumentationCheck, DocumentationError, DocumentationGeneration,
    DocumentationGenerationCause, DocumentationGenerationError, DocumentationGenerationOptions,
    DocumentationInputs, DocumentationJob, DocumentationManualEvidence,
    DocumentationManualInputKind, DocumentationPreparationCheck,
    DocumentationPreparationQualification, acquire_documentation,
    acquire_publication_documentation, acquire_resolved_documentation, check_documentation,
};
pub use documents::{DocumentError, DocumentInspection, ParsedSite};
pub use incremental::{
    BuildCacheAdapters, BuildCacheClean, BuildCacheError, BuildCacheKey, BuildCachePlan,
    BuildCachePlanError, BuildCacheReconcileOptions, BuildCacheRunError, BuildCacheSchedule,
    schedule_build_cache_site,
};
pub use islands::IslandEvidence;
pub use math::{
    MathBindingError, MathCapture, MathGenerationCause, MathGenerationError, MathGenerationOptions,
};
pub use metadata::{
    CompositionMetadata, MetadataError, MetadataInspection, PluginPackageInspection,
    ThemePackageInspection,
};
pub use mirrors::{MirrorModel, MirrorRenderEvidence, MirrorRoute};
pub use page::{PageRenderError, PageRenderEvidence, RenderedPage};
pub use page_resources::PageResourceEvidence;
pub use policy::{
    CandidateCheckFailure, CandidateCheckOptions, CandidatePolicyRequirement,
    MatchedOutputResponse, OutputPolicyCheck, OutputPolicyEvidence, OutputPolicyRequirement,
    OutputResponseError, PolicyCheck, PolicyCheckError, PolicyCheckStatus, PolicyInvocation,
};
pub use preparation::{
    DeployabilityError, LocalPreparationError, LocalPreparationFailure, LocalPreparationOptions,
    LocalPreparationRequest, LocalPreparationSetError, LocalPreparationSetFailure,
    PreparationCheckOptions, PreparationCheckRequest, PreparationChecks, PreparationError,
    PreparationFailure, PreparationOptions, PreparationQualification, PreparationRequest,
    SitePreparationReceipt, SitePreparationRequest, TechnicalCertificate, ensure_deployable_sites,
};
pub(crate) use preparation::{PreparationAdmissionEvidence, inspect_preparation_evidence};
pub use publication::{
    PublicationOutputLimits, PublicationRender, PublicationRenderError, PublicationRenderLimits,
};
pub use publication_inputs::{
    PublicationInputError, acquire_publication, acquire_resolved_publication,
};
pub use publication_model::{
    AdmissionError, AdmittedPublication, AuthoringSource, CandidateAdmission, ParsedPublication,
    PublicationInspection, PublicationResolutionError, ResolvedPublication, SelectedPublication,
};
pub use publication_set::{
    AcquiredPublicationSet, PublicationSetAcquisitionLimits, PublicationSetError,
    PublicationSetInspection, PublicationSetRenderLimits, PublicationSetRenderOptions,
    PublicationSetSiteInspection, PublicationSetStatus, PublicationSiteError,
    PublicationSourceFailure, RenderedPublicationSet, acquire_configured_publications,
    acquire_development_publications, acquire_development_resolved_publications,
    acquire_development_selected_publications, acquire_publications, acquire_resolved_publications,
    acquire_selected_publications,
};
pub use registry::{
    RegistryError, RegistryLimits, RegistryProjection, RegistryReconcileError,
    RegistryReconcileOptions, RegistryStatus,
};
pub use repository_source::{
    RepositorySourceProvenance, RuntimeBuildProfile, repository_source_provenance,
    runtime_build_profile, runtime_version_coordinate,
};
pub use resources::{
    BrowserModule, BrowserModules, ImportMap, ResourceEvidence, ResourceProjection,
    ThemeResourceProjection,
};
pub use review::{
    MatchedReviewResponse, PublicationReview, ReviewDecision, ReviewError, ReviewLimits,
    ReviewOutcome, ReviewPolicy, ReviewRequest, ReviewResponseBinding, ReviewResponseError,
    ReviewResponseLimits, ReviewResponseStatus, ReviewSubmission, ReviewSurface, ReviewTarget,
    ReviewUse, ReviewUses,
};
pub use site::{
    GraphRenderEvidence, GraphRenderOptions, RenderedSite, SitePageEvidence, SiteRenderError,
    SiteRenderEvidence, SiteRenderOptions,
};
pub use sources::{
    AcquiredSite, Error, Inspection, PlacementInspection, SourceEvidence, acquire_resolved_site,
    acquire_site,
};
pub use templates::{CompiledTheme, TemplateCompilation, TemplateError};
