//! Retained syntax and semantics shared by checking, review and local rendering.

use std::{collections::BTreeMap, fmt, sync::Arc};

use gazette_compiler::{authored, corpus};
use gazette_core::{
    publication::{Key, content},
    source::Sha256Digest,
};
use serde::Serialize;

use super::{AcquiredInputs, CompositionInspection};

/// Complete publication syntax bound to the acquisition that supplied its bytes.
/// The corpus borrows source payloads; construction cannot pair it with another workspace.
#[derive(Debug)]
pub struct ParsedPublication<'s> {
    inputs: &'s AcquiredInputs,
    corpus: corpus::ParsedCorpus<'s>,
}

impl<'s> ParsedPublication<'s> {
    pub(super) fn new(inputs: &'s AcquiredInputs, corpus: corpus::ParsedCorpus<'s>) -> Self {
        Self { inputs, corpus }
    }

    /// Exact retained acquisition, including the selected site and source revisions.
    pub fn inputs(&self) -> &'s AcquiredInputs {
        self.inputs
    }

    /// Original syntax/assets, without synthesized frontmatter or native rereads.
    pub fn corpus(&self) -> &corpus::ParsedCorpus<'s> {
        &self.corpus
    }

    /// Attach source-bound generated math before resolving the shared publication model.
    /// This is retained-input validation, not native execution or public admission.
    ///
    /// # Errors
    /// Rejects incomplete, extra or stale results without returning a partial publication.
    pub fn with_math(
        mut self,
        results: std::collections::BTreeMap<
            gazette_core::source::EntryPath,
            Vec<corpus::LocatedMathResult>,
        >,
    ) -> Result<Self, corpus::Error> {
        self.corpus = self.corpus.with_math(results)?;
        Ok(self)
    }

    /// Resolve the complete graph and validate every configured source/home binding once.
    ///
    /// Drafts, archived objects and foreign endpoints remain present for inspection.
    /// Success establishes structural validity, not rights, disclosure or public admission.
    /// Presentation metadata and resource compilation belong to later phases.
    ///
    /// # Errors
    /// Rejects semantic/route/reference failures and mismatched native source associations.
    /// No partially resolved publication or alternative configuration escapes.
    pub fn resolve(
        &self,
        limits: content::Limits,
    ) -> Result<ResolvedPublication<'_, 's>, PublicationResolutionError> {
        let authored = authored::resolve(self.inputs.compilation(), &self.corpus, limits)
            .map_err(PublicationResolutionError::Authored)?;
        authored
            .validate_source_bindings()
            .map_err(PublicationResolutionError::Source)?;
        Ok(ResolvedPublication {
            inputs: self.inputs,
            authored,
        })
    }
}

/// One structurally validated publication over the original syntax and acquisition.
///
/// This phase deliberately has no mutable graph access, policy verdict or public-output
/// capability. Inspection and local rendering borrow its single resolved model. A later
/// admission phase must evaluate exact uses, including nested references and assets;
/// merely filtering its top-level nodes cannot establish publication permission.
#[derive(Debug)]
pub struct ResolvedPublication<'a, 's> {
    inputs: &'s AcquiredInputs,
    authored: authored::ResolvedAuthored<'a, 's>,
}

impl<'a, 's> ResolvedPublication<'a, 's> {
    pub(super) fn with_preview_link_bases(
        mut self,
        bases: gazette_core::publication::routing::PreviewLinkBases,
    ) -> Self {
        self.authored = self.authored.with_preview_link_bases(bases);
        self
    }

    pub(super) fn with_preview_urls(
        mut self,
        urls: std::collections::BTreeMap<
            gazette_core::publication::Key,
            gazette_core::publication::routing::PublicBaseUrl,
        >,
    ) -> Result<Self, content::Error> {
        self.authored = self.authored.with_preview_urls(urls)?;
        Ok(self)
    }

    /// Consume this private owner into a closed non-draft candidate for local
    /// rendering. Selection is not field/rights/publisher-policy admission.
    /// A complete private review belongs before this transition; a candidate review
    /// belongs after it and binds the selection independently of equal projected values.
    ///
    /// # Errors
    /// Preserves compiler selection failures without a partial candidate.
    pub fn into_selected(
        self,
        selection: authored::Selection,
    ) -> Result<SelectedPublication<'a, 's>, authored::SelectionError> {
        let authored = self.authored.into_selected(&selection)?;
        Ok(SelectedPublication {
            publication: Self {
                inputs: self.inputs,
                authored,
            },
            selection: Arc::new(selection),
        })
    }

    /// Exact source/configuration bytes used by this resolution.
    pub fn inputs(&self) -> &'s AcquiredInputs {
        self.inputs
    }

    /// The existing compiler-owned graph, routes, syntax and provenance associations.
    pub fn authored(&self) -> &authored::ResolvedAuthored<'a, 's> {
        &self.authored
    }

    /// Private operator evidence, never a public graph or approval receipt.
    /// Includes the complete acquired configuration and source identities, not file bodies.
    pub fn inspect(&self) -> PublicationInspection<'_> {
        PublicationInspection {
            status: "resolved-unadmitted",
            acquisition: self.inputs.inspect(),
            graph: self.authored.content().graph(),
            page_packages: self.authored.page_packages(),
            authoring_sources: self
                .authored
                .corpus()
                .files()
                .iter()
                .filter_map(|(path, file)| {
                    file.source()?;
                    let workspace = &self.inputs.compilation().workspace;
                    let association = workspace
                        .content_source(path)
                        .or_else(|| workspace.legal_bindings(path).next())?;
                    Some(AuthoringSource {
                        source: association.source_key(),
                        entry: association.entry(path)?,
                        path,
                        source_sha256: file.source_sha256(),
                    })
                })
                .collect(),
            authoring_inputs: self
                .authored
                .authoring_asset_paths()
                .filter_map(|path| {
                    let file = self.authored.corpus().files().get(path)?;
                    let association = self.inputs.compilation().workspace.content_source(path)?;
                    Some(AuthoringSource {
                        source: association.source_key(),
                        entry: association.entry(path)?,
                        path,
                        source_sha256: file.source_sha256(),
                    })
                })
                .collect(),
        }
    }
}

/// One selected semantic owner, still requiring independently verified policy
/// evidence before public admission. This type offers no certification/release
/// capability and never evaluates instance rights or publisher decisions.
#[derive(Debug)]
pub struct SelectedPublication<'a, 's> {
    publication: ResolvedPublication<'a, 's>,
    selection: Arc<authored::Selection>,
}

impl<'a, 's> SelectedPublication<'a, 's> {
    /// Exact retained acquisition; no source capture is repeated after selection.
    pub fn inputs(&self) -> &'s AcquiredInputs {
        self.publication.inputs()
    }

    /// The sole selected graph, body associations and resource/asset selection.
    pub fn authored(&self) -> &authored::ResolvedAuthored<'a, 's> {
        self.publication.authored()
    }

    /// Complete private selection intent, including excluded identities/paths.
    pub fn selection(&self) -> &authored::Selection {
        &self.selection
    }

    /// Review the exact candidate with the same producers used before selection and during
    /// rendering. The request binds retained inputs and complete selection intent, including
    /// exclusions; its surfaces contain only candidate projections. Excluded program inputs
    /// are not executed. This is private review evidence, not policy admission or certification.
    ///
    /// # Errors
    /// Preserves projection failures and complete-review bounds, including selection encoding.
    pub fn review(
        &self,
        limits: super::ReviewLimits,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<super::PublicationReview, super::ReviewError> {
        self.publication
            .review_with_selection(Some(&self.selection), limits, raster, markdown)
    }

    pub(super) fn review_site(
        &self,
        site: &gazette_core::publication::Key,
        presentation: &AcquiredInputs,
        limits: super::ReviewLimits,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<super::PublicationReview, super::ReviewError> {
        self.publication.review_site(
            site,
            presentation,
            Some(&self.selection),
            limits,
            raster,
            markdown,
        )
    }

    pub(super) fn into_admitted(
        self,
        admissions: std::collections::BTreeMap<
            gazette_core::publication::Key,
            Arc<super::CandidateAdmission>,
        >,
        expected: impl Iterator<Item = gazette_core::publication::Key>,
    ) -> Result<super::AdmittedPublication<'a, 's>, super::AdmissionError> {
        super::AdmittedPublication::new(self, admissions, expected)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Shared rendering services and bounds"
    )]
    pub(super) fn render_site_view(
        &self,
        inputs: &'s AcquiredInputs,
        compiler: &impl gazette_ports::render::TemplateCompiler,
        resources: &impl gazette_ports::resource::ResourceCompiler,
        projection_inputs: super::page::ProjectionInputs<'_>,
        options: super::SiteRenderOptions<'_>,
        limits: super::PublicationOutputLimits,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<super::PublicationRender<'s>, super::PublicationRenderError> {
        self.publication.render_site_view(
            inputs,
            compiler,
            resources,
            projection_inputs,
            options,
            limits,
            host_files,
        )
    }

    /// Compile resources and every local output from the selected owner only.
    /// Excluded page programs are not invoked. Theme/workspace capabilities remain
    /// explicit selected configuration, not content-derived approval.
    ///
    /// # Errors
    /// Preserves the existing complete-site errors, including empty selected sites,
    /// output collisions and failures of required configured outputs.
    pub fn render_local(
        &self,
        compiler: &impl gazette_ports::render::TemplateCompiler,
        resources: &impl gazette_ports::resource::ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: super::SiteRenderOptions<'_>,
        limits: super::PublicationOutputLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<super::PublicationRender<'s>, super::PublicationRenderError> {
        self.render_local_with_host_files(
            compiler, resources, raster, options, limits, markdown, None,
        )
    }

    /// Render with an explicit pure hosting file provider.
    ///
    /// # Errors
    /// Preserves complete-output failures and rejects configured hosting without a provider.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit rendering services and bounds"
    )]
    pub fn render_local_with_host_files(
        &self,
        compiler: &impl gazette_ports::render::TemplateCompiler,
        resources: &impl gazette_ports::resource::ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: super::SiteRenderOptions<'_>,
        limits: super::PublicationOutputLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<super::PublicationRender<'s>, super::PublicationRenderError> {
        let mut rendered = self.publication.render_local_with_host_files(
            compiler, resources, raster, options, limits, markdown, host_files,
        )?;
        rendered.selection = Some(self.selection.clone());
        Ok(rendered)
    }
}

/// A required evaluator response matched every exact selected use and reported allow.
///
/// Construction is private to the candidate execution path. This retains the complete
/// original check rather than manufacturing a second receipt or policy representation.
/// It does not authenticate the evaluator, approve publication, certify output, authorize
/// release or establish that referenced evidence is true, current or sufficient.
#[derive(Debug)]
pub struct CandidateAdmission {
    check: Arc<super::PolicyCheck>,
}

impl CandidateAdmission {
    pub(super) fn derive(
        site: &Key,
        selected: &SelectedPublication<'_, '_>,
        check: Arc<super::PolicyCheck>,
        required: &super::CandidatePolicyRequirement,
        maximum: usize,
    ) -> Result<Self, AdmissionError> {
        if check.requirement() != Some(required)
            || check.invocation().command_sha256 != required.command_sha256
            || check.review().request().site() != site
        {
            return Err(AdmissionError::Binding("site, evaluator or requirement"));
        }
        let selection = super::serialization::encode(
            selected.selection(),
            maximum,
            "candidate admission selection",
        )
        .map_err(|_| AdmissionError::Binding("selection encoding"))?;
        if check.review().request().selection_sha256() != Some(Sha256Digest::of(&selection)) {
            return Err(AdmissionError::Binding("selected semantic owner"));
        }
        let source = check
            .policy_source()
            .ok_or(AdmissionError::Missing("retained policy source"))?;
        source
            .bytes()
            .map_err(|_| AdmissionError::Binding("retained policy source"))?;
        let retained = super::policy::policy_input(selected.inputs(), Some(&required.policy))
            .map_err(|_| AdmissionError::Binding("captured policy source"))?
            .ok_or(AdmissionError::Missing("captured policy source"))?;
        if &retained != source {
            return Err(AdmissionError::Binding("captured policy source"));
        }
        let response = check
            .response()
            .ok_or(AdmissionError::Missing("captured matched response"))?;
        if response.policy() != &required.policy
            || response.request_sha256() != check.review().sha256()
        {
            return Err(AdmissionError::Binding("matched response"));
        }
        if response
            .decisions()
            .iter()
            .any(|decision| decision.outcome != super::ReviewOutcome::Allow)
        {
            return Err(AdmissionError::Disposition);
        }
        Ok(Self { check })
    }

    /// The original complete candidate check, not a reconstructed admission receipt.
    pub fn check(&self) -> &super::PolicyCheck {
        &self.check
    }

    /// The independently selected requirement satisfied by the retained check.
    pub fn requirement(&self) -> Option<&super::CandidatePolicyRequirement> {
        self.check.requirement()
    }
}

/// One selected semantic owner with exact admissions for the complete requested site set.
/// Rendering borrows the selected owner directly; no graph, selection or policy copy exists.
#[derive(Debug)]
pub struct AdmittedPublication<'a, 's> {
    selected: SelectedPublication<'a, 's>,
    admissions: BTreeMap<Key, Arc<CandidateAdmission>>,
}

impl<'a, 's> AdmittedPublication<'a, 's> {
    pub(super) fn new(
        selected: SelectedPublication<'a, 's>,
        admissions: BTreeMap<Key, Arc<CandidateAdmission>>,
        expected: impl Iterator<Item = Key>,
    ) -> Result<Self, AdmissionError> {
        let expected = expected.collect::<Vec<_>>();
        if expected.is_empty()
            || admissions.len() != expected.len()
            || !expected.iter().eq(admissions.keys())
        {
            return Err(AdmissionError::Binding("complete admitted site set"));
        }
        Ok(Self {
            selected,
            admissions,
        })
    }

    pub(super) fn selected(&self) -> &SelectedPublication<'a, 's> {
        &self.selected
    }

    pub(super) fn admission(&self, site: &Key) -> Option<&Arc<CandidateAdmission>> {
        self.admissions.get(site)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Shared rendering services and bounds"
    )]
    pub(super) fn render_site_view(
        &self,
        inputs: &'s super::AcquiredInputs,
        compiler: &impl gazette_ports::render::TemplateCompiler,
        resources: &impl gazette_ports::resource::ResourceCompiler,
        projection_inputs: super::page::ProjectionInputs<'_>,
        options: super::SiteRenderOptions<'_>,
        limits: super::PublicationOutputLimits,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<super::PublicationRender<'s>, super::PublicationRenderError> {
        self.selected.render_site_view(
            inputs,
            compiler,
            resources,
            projection_inputs,
            options,
            limits,
            host_files,
        )
    }
}

/// A candidate check could not become typed admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    /// Required retained evidence is absent; exit-only and imported checks cannot admit.
    Missing(&'static str),
    /// Evidence does not bind the exact selected owner, site, policy, evaluator or response.
    Binding(&'static str),
    /// At least one exact use was not reported as allowed.
    Disposition,
}

impl AdmissionError {
    /// Stable diagnostic class without private policy or content values.
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing(_) => "candidate_admission_missing",
            Self::Binding(_) => "candidate_admission_binding",
            Self::Disposition => "candidate_admission_disposition",
        }
    }
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(item) => write!(f, "candidate admission requires {item}"),
            Self::Binding(item) => write!(f, "candidate admission mismatches {item}"),
            Self::Disposition => f.write_str("candidate admission requires all-allow decisions"),
        }
    }
}

impl std::error::Error for AdmissionError {}

#[derive(Debug)]
pub(super) enum CandidateEvidence {
    Checked(Arc<super::PolicyCheck>),
    Admitted(Arc<CandidateAdmission>),
}

impl CandidateEvidence {
    pub(super) fn check(&self) -> &super::PolicyCheck {
        match self {
            Self::Checked(check) => check,
            Self::Admitted(admission) => admission.check(),
        }
    }

    pub(super) fn admission(&self) -> Option<&CandidateAdmission> {
        match self {
            Self::Checked(_) => None,
            Self::Admitted(admission) => Some(admission),
        }
    }
}

/// Evidence that the same retained inputs passed structural checks only.
/// Native paths, unlisted/private values and drafts make this an operator-only report.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PublicationInspection<'a> {
    /// Fixed phase outcome; neither rendered nor admitted nor certified.
    pub status: &'static str,
    /// Complete configuration and exact captured source/pin evidence.
    pub acquisition: CompositionInspection<'a>,
    /// Borrowed complete structural graph, including independent lifecycle/role axes.
    pub graph: &'a content::Graph,
    /// Exact retained leaf-page package ownership, keyed by content identity.
    /// Each companion retains its source path, package-relative path, kind and digest.
    pub page_packages: &'a BTreeMap<Key, authored::PagePackage>,
    /// Complete retained text-source coordinates for editor selection, including
    /// legal inputs. These are private input paths, not public page routes.
    pub authoring_sources: Vec<AuthoringSource<'a>>,
    /// Opaque source inputs needed by current authoring semantics, separate from
    /// editable text and from the full media library selected for a build.
    pub authoring_inputs: Vec<AuthoringSource<'a>>,
}

/// One compiler-qualified text source, without its private source body.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AuthoringSource<'a> {
    /// Exact configured source owner.
    pub source: &'a Key,
    /// Literal entry within the captured source.
    pub entry: &'a str,
    /// Native compiler coordinate, preserving legal and authored roles.
    pub path: &'a gazette_core::source::EntryPath,
    /// Exact bytes the native corpus retained.
    pub source_sha256: Sha256Digest,
}

/// Structural checking stopped before presentation or policy evaluation.
#[derive(Debug)]
pub enum PublicationResolutionError {
    /// Original compiler failure, preserving its diagnostics and source coordinates.
    Authored(authored::Error),
    /// A document disagrees with its configured source, project or publication home.
    Source(authored::SourceBindingError),
}

impl PublicationResolutionError {
    /// Stable class shared by direct checking and rendering.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Authored(_) => "publication_authored",
            Self::Source(error) => error.code(),
        }
    }
}

impl fmt::Display for PublicationResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authored(error) => error.fmt(f),
            Self::Source(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PublicationResolutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Authored(error) => Some(error),
            Self::Source(error) => Some(error),
        }
    }
}
