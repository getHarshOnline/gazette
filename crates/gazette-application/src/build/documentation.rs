//! Configured manuals and generated references share one retained target assembly boundary.

use super::{AcquiredInputs, CompositionError};
mod acquisition;
mod capture;
pub use acquisition::{DocumentationInputs, acquire_publication_documentation};
mod catalog;
mod discovery;
pub use discovery::{DocumentationCatalog, DocumentationCatalogEntry};
mod generation;
mod output;
mod preparation;
mod selection;
use crate::source::PreparationLimits;
use gazette_compiler::{
    toml,
    workspace::{
        self, Brand, DocumentationBuild, DocumentationInput, DocumentationNavigation,
        DocumentationRepository, Key,
    },
};
use gazette_core::{
    documentation::{
        self, Artifact, Format, ManualAsset, ManualPage, ManualSection, ManualSource,
        PlannedManualPage, PlannedManualSection, PlannedProject, PlannedProjectItem,
        PlannedReference, ProjectItem, ProjectModule, Reference, Request, Selection,
    },
    source::{self, CapturedSource, Content, InventoryBuilder},
    stage,
};
use gazette_ports::{
    documentation::DocumentationTarget,
    source::{RevisionReader, SourceCapture},
    stage::StageWriter,
};
pub use generation::{
    DocumentationGeneration, DocumentationGenerationCause, DocumentationGenerationError,
    DocumentationGenerationOptions, DocumentationJob,
};
pub(crate) use preparation::DocumentationTechnicalCertificate;
pub use preparation::{DocumentationPreparationCheck, DocumentationPreparationQualification};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
};

/// Acquire only the selected site's documentation inputs, once per exact source key.
///
/// Gazette publication content, Theme and other sites' source trees are not read. The whole
/// workspace is still resolved before native reads. This imports captured authored pages,
/// assets and generator artifacts; it does not invoke generators, discover repositories or
/// write a documentation target.
///
/// # Errors
/// Rejects invalid configuration, missing target selection, pin/confinement and complete-set limits.
pub fn acquire_documentation(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    markdown: &dyn gazette_ports::markdown::Markdown,
    source: &str,
    site: &Key,
    syntax: toml::Limits,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, DocumentationError> {
    let compilation =
        workspace::compile(source, syntax).map_err(CompositionError::Configuration)?;
    let configuration = crate::configuration::ResolvedWorkspace::supplied(compilation);
    acquire_resolved_documentation(reader, revisions, markdown, configuration, site, limits)
}

/// Acquire one documentation build from an already admitted native or supplied workspace.
/// Relative paths retain that workspace's selected native coordinate, never the process directory.
///
/// # Errors
/// Refuses missing documentation selections, native source failures or complete-set bounds.
pub fn acquire_resolved_documentation(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    markdown: &dyn gazette_ports::markdown::Markdown,
    configuration: crate::configuration::ResolvedWorkspace,
    site: &Key,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, DocumentationError> {
    capture::acquire(reader, revisions, markdown, configuration, site, limits)
}

/// Effect-free resolved documentation intent after workspace and target validation.
///
/// This is the application-owned inspection surface for CLI/MCP/editor projections.
/// It retains the exact resolved build contract instead of making interfaces
/// reconstruct manual/reference navigation or source provenance.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentationCheck {
    /// Exact selected site identity.
    pub site: Key,
    /// Exact brand identity owning the selected documentation repository.
    pub brand: Key,
    /// Brand-owned repository name.
    pub repository_name: String,
    /// Brand-owned local repository path; no native access is performed.
    pub repository_path: String,
    /// Complete resolved manual/reference plan in configured order.
    pub build: DocumentationBuild,
}

/// Resolve and target-preflight one documentation plan without source acquisition or effects.
///
/// # Errors
/// Rejects invalid workspace configuration, missing site bindings and target-incompatible plans.
pub fn check_documentation(
    source: &str,
    site: &Key,
    syntax: toml::Limits,
    target: &impl DocumentationTarget,
) -> Result<DocumentationCheck, DocumentationError> {
    let compilation =
        workspace::compile(source, syntax).map_err(CompositionError::Configuration)?;
    check_documentation_workspace(&compilation.workspace, site, target)
}

pub(crate) fn check_documentation_workspace(
    workspace: &workspace::Workspace,
    site: &Key,
    target: &impl DocumentationTarget,
) -> Result<DocumentationCheck, DocumentationError> {
    let (brand, repository, build) = selection(workspace, site)?;
    target
        .preflight(target_plan(brand, build))
        .map_err(DocumentationError::Target)?;
    Ok(DocumentationCheck {
        site: site.clone(),
        brand: brand.id.clone(),
        repository_name: repository.name.clone(),
        repository_path: repository.path.clone(),
        build: build.clone(),
    })
}

/// Resolve one documentation plan through the inward-owned target contract only.
///
/// This is the editor-safe path: it validates configured shape without selecting
/// a conversion/assembly adapter or performing native effects.
pub(crate) fn check_documentation_workspace_contract(
    workspace: &workspace::Workspace,
    site: &Key,
) -> Result<DocumentationCheck, DocumentationError> {
    let (brand, repository, build) = selection(workspace, site)?;
    target_plan(brand, build)
        .validate()
        .map_err(DocumentationError::Target)?;
    Ok(DocumentationCheck {
        site: site.clone(),
        brand: brand.id.clone(),
        repository_name: repository.name.clone(),
        repository_path: repository.path.clone(),
        build: build.clone(),
    })
}

fn selection<'a>(
    workspace: &'a workspace::Workspace,
    site: &Key,
) -> Result<
    (
        &'a Brand,
        &'a DocumentationRepository,
        &'a DocumentationBuild,
    ),
    DocumentationError,
> {
    if workspace.site(site).is_none() {
        return Err(CompositionError::UnknownSite(site.clone()).into());
    }
    let (brand, repository) = workspace
        .brands()
        .find_map(|brand| {
            brand
                .publications
                .iter()
                .find(|publication| &publication.site == site)
                .and_then(|publication| publication.documentation_repository.as_ref())
                .map(|repository| (brand, repository))
        })
        .ok_or_else(|| DocumentationError::Binding(site.clone()))?;
    let build = repository
        .build
        .as_ref()
        .ok_or_else(|| DocumentationError::Binding(site.clone()))?;
    Ok((brand, repository, build))
}

/// The owning publication's reservation, applied to its documentation host too.
/// A reservation without a resolvable terms address fails rather than leaving the
/// provider's default crawler policy in place.
fn usage_policy<'a>(
    workspace: &'a workspace::Workspace,
    site: &Key,
) -> Result<Option<gazette_core::publication::usage::UsagePolicy<'a>>, DocumentationError> {
    let Some(publication) = workspace.publication_for(site).filter(|publication| {
        publication
            .machine
            .terms
            .as_ref()
            .is_some_and(|terms| terms.tdm_reservation)
    }) else {
        return Ok(None);
    };
    let terms_url = publication.terms_url().ok_or_else(|| {
        DocumentationError::Target(documentation::Error::Invalid {
            context: site.to_string(),
            reason: "TDM reservation requires the publication's production public base URL",
        })
    })?;
    let crawlers = publication
        .machine
        .robots
        .as_ref()
        .filter(|robots| robots.enabled)
        .map(|robots| {
            robots
                .rules
                .iter()
                .map(gazette_compiler::workspace::CrawlerRule::group)
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(gazette_core::publication::usage::UsagePolicy {
        terms_url,
        crawlers,
    }))
}

fn publisher_metadata<'a>(
    workspace: &'a workspace::Workspace,
    site: &Key,
) -> Option<documentation::Publisher<'a>> {
    use gazette_core::publication::{PrincipalKind, PrincipalWebsite};
    let principal = workspace.principal(&workspace.site(site)?.publisher)?;
    if principal.kind != PrincipalKind::Organization {
        return None;
    }
    let url = match principal.website.as_ref()? {
        PrincipalWebsite::Site { site } => workspace.public_base_url(site)?.as_str(),
        PrincipalWebsite::External { url } => url.as_str(),
    };
    Some(documentation::Publisher {
        id: format!("{url}#{}", principal.kind.public_fragment()),
        name: principal.name.as_deref()?,
        legal_name: principal.legal_name.as_deref(),
        url,
        same_as: &principal.same_as,
    })
}

fn target_plan<'a>(brand: &'a Brand, build: &'a DocumentationBuild) -> documentation::Plan<'a> {
    documentation::Plan {
        name: &brand.name,
        target: &build.target,
        projects: build
            .modules
            .iter()
            .map(|module| PlannedProject {
                slug: &module.slug,
                title: &module.title,
                manual_assets: module.manual.as_ref().map_or_else(Vec::new, |manual| {
                    manual.assets.iter().map(|asset| &asset.path).collect()
                }),
                navigation: module
                    .navigation
                    .iter()
                    .map(|item| match item {
                        DocumentationNavigation::Manual { title, pages } => {
                            PlannedProjectItem::Manual(PlannedManualSection {
                                title,
                                pages: pages
                                    .iter()
                                    .map(|page| PlannedManualPage {
                                        title: &page.title,
                                        route: &page.route,
                                    })
                                    .collect(),
                            })
                        }
                        DocumentationNavigation::Reference {
                            slug,
                            title,
                            input,
                            pages,
                        } => {
                            let (format, selection) = match input.as_ref() {
                                DocumentationInput::Artifact {
                                    format, selection, ..
                                } => (*format, selection),
                                DocumentationInput::Generated { recipe, .. } => {
                                    (recipe.format, &recipe.output)
                                }
                            };
                            PlannedProjectItem::Reference(PlannedReference {
                                slug,
                                title,
                                format,
                                selection,
                                pages: pages.as_deref(),
                            })
                        }
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Exact imported artifact evidence, separate from a generator-execution or admission receipt.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentationArtifactEvidence<'a> {
    /// Native project identity; module labels do not replace it.
    pub project: &'a Key,
    /// Positionally selected module namespace.
    pub module: &'a str,
    /// Reference namespace within the module.
    pub reference: &'a str,
    /// Exact existing source binding.
    pub source: &'a Key,
    /// Exact configured file or directory selection.
    pub selection: &'a Selection,
    /// Selected native output format.
    pub format: Format,
    /// Retained file size/digest or directory kind, never a synthetic file checksum.
    pub content: Content,
    /// Complete retained input tree, including companions and nonpublic generation material.
    /// This identifies the target's available inputs, not a claim that every file was published.
    pub tree: source::TreeIdentity,
    /// Native generator observation when this artifact was generated in this operation.
    /// Imported artifacts never acquire execution evidence merely from their format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<&'a documentation::generator::Evidence>,
}

/// Kind of explicitly selected authored-manual input.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentationManualInputKind {
    /// One authored Markdown body selected for an explicit target route.
    Page,
    /// One file or directory selected as an authored-manual companion.
    Asset,
}

/// Exact authored-manual selection evidence, distinct from generated references.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentationManualEvidence<'a> {
    /// Authored page or explicit asset.
    pub kind: DocumentationManualInputKind,
    /// Exact existing source binding.
    pub source: &'a Key,
    /// Exact retained input selection.
    pub selection: &'a Selection,
    /// Explicit target route or asset path.
    pub output: &'a gazette_core::source::EntryPath,
    /// Retained file size/digest or directory kind.
    pub content: Content,
    /// Complete retained source tree identity.
    pub tree: source::TreeIdentity,
}

/// Complete local documentation output bound to its captured inputs and intended brand destination.
#[derive(Debug)]
pub struct DocumentationAssembly<'a> {
    preview_link_bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
    inputs: &'a AcquiredInputs,
    brand: &'a Key,
    repository: &'a DocumentationRepository,
    manual: Vec<DocumentationManualEvidence<'a>>,
    artifacts: Vec<DocumentationArtifactEvidence<'a>>,
    output: CapturedSource,
    catalog: Option<std::sync::Arc<DocumentationCatalog>>,
    protected: Vec<PathBuf>,
    generation_output_usage: (usize, u64),
}

impl DocumentationAssembly<'_> {
    /// Complete native generator output entries and bytes, including unselected companions.
    pub fn generation_output_usage(&self) -> (usize, u64) {
        self.generation_output_usage
    }

    /// Exact local hyperlink projection. Presence excludes durable/public output paths.
    pub fn preview_link_bases(
        &self,
    ) -> Option<&gazette_core::publication::routing::PreviewLinkBases> {
        self.preview_link_bases.as_ref()
    }
    /// Exact brand owning this independently configured documentation repository.
    pub fn brand(&self) -> &Key {
        self.brand
    }
    /// Original acquisition, including exact configuration and native revision evidence.
    pub fn inputs(&self) -> &AcquiredInputs {
        self.inputs
    }
    /// Intended configured repository; assembly does not create or replace this path.
    pub fn repository(&self) -> &DocumentationRepository {
        self.repository
    }
    /// Authored-manual selections in configured navigation/asset order.
    pub fn manual(&self) -> &[DocumentationManualEvidence<'_>] {
        &self.manual
    }
    /// Imported references in configured navigation order, not generator invocation proof.
    pub fn artifacts(&self) -> &[DocumentationArtifactEvidence<'_>] {
        &self.artifacts
    }
    /// Complete immutable local target; no publication admission or hosted build is implied.
    pub fn output(&self) -> &CapturedSource {
        &self.output
    }

    /// Write a fresh private stage, not the intended repository destination.
    /// The exact canonical path-byte ceiling is checked by the native stage owner before creation.
    ///
    /// # Errors
    /// Rejects source/output overlap, tighter limits, incomplete writes or read-back/cleanup failures.
    pub fn stage<S: StageWriter>(
        &self,
        writer: &S,
        parent: &Path,
        limits: source::Limits,
        directory_bytes: usize,
    ) -> Result<S::Stage, stage::Error> {
        let roots = self.protected_roots()?;
        writer.stage(
            stage::Location::FreshBounded {
                parent,
                directory_bytes,
            },
            &roots,
            self.output.as_input(),
            limits,
        )
    }

    fn protected_roots(&self) -> Result<Vec<PathBuf>, source::Error> {
        let mut roots =
            self.inputs
                .source_roots()
                .map_err(|error| source::Error::InvalidInventory {
                    path: error
                        .binding()
                        .map(|key| key.as_str().into())
                        .unwrap_or_else(|| "composition".into()),
                    reason: "captured source binding absent",
                })?;
        roots.extend(self.protected.iter().cloned());
        Ok(roots)
    }
}

impl AcquiredInputs {
    /// Assemble one complete manual/reference publication from retained captures only.
    ///
    /// No native reread, generator execution, authored-content rendering, database or network effect.
    /// The adapter receives borrowed configuration and artifacts, never native paths or workspaces.
    ///
    /// # Errors
    /// Rejects missing selection/payloads, target failures and complete output limits without a subset.
    pub fn assemble_documentation(
        &self,
        target: &(impl DocumentationTarget + ?Sized),
        limits: source::Limits,
    ) -> Result<DocumentationAssembly<'_>, DocumentationError> {
        self.assemble_documentation_inputs(target, limits, None, Vec::new())
    }

    fn assemble_documentation_inputs<'a>(
        &'a self,
        target: &(impl DocumentationTarget + ?Sized),
        limits: source::Limits,
        generated: Option<&'a BTreeMap<(usize, usize), documentation::generator::Generated>>,
        protected: Vec<PathBuf>,
    ) -> Result<DocumentationAssembly<'a>, DocumentationError> {
        let (brand, repository, build) = selection(&self.compilation().workspace, self.site())?;
        let mut manual_evidence = Vec::new();
        let mut projects = Vec::new();
        let mut artifacts = Vec::new();
        for (module_index, module) in build.modules.iter().enumerate() {
            let manual_tree = module
                .manual
                .as_ref()
                .map(|manual| {
                    self.sources()
                        .get(&manual.source)
                        .ok_or_else(|| DocumentationError::ManualInput {
                            source: manual.source.clone(),
                        })?
                        .as_input()
                        .map_err(documentation::Error::Tree)
                        .map_err(DocumentationError::Target)
                })
                .transpose()?;
            let mut navigation = Vec::new();
            for (item_index, item) in module.navigation.iter().enumerate() {
                let DocumentationNavigation::Reference {
                    slug,
                    title,
                    input,
                    pages,
                } = item
                else {
                    let DocumentationNavigation::Manual { title, pages } = item else {
                        unreachable!()
                    };
                    let configured = module
                        .manual
                        .as_ref()
                        .expect("workspace validation binds manual navigation to its source");
                    let tree = manual_tree
                        .as_ref()
                        .expect("manual configuration and retained tree project together");
                    let identity = tree.inventory().identity();
                    let mut resolved = Vec::new();
                    for page in pages {
                        let content = page.input.content(tree).map_err(|_| {
                            DocumentationError::ManualInput {
                                source: configured.source.clone(),
                            }
                        })?;
                        manual_evidence.push(DocumentationManualEvidence {
                            kind: DocumentationManualInputKind::Page,
                            source: &configured.source,
                            selection: &page.input,
                            output: &page.route,
                            content,
                            tree: identity,
                        });
                        resolved.push(ManualPage {
                            title: &page.title,
                            route: &page.route,
                            selection: &page.input,
                        });
                    }
                    navigation.push(ProjectItem::Manual(ManualSection {
                        title,
                        pages: resolved,
                    }));
                    continue;
                };
                let source = input.source();
                let (selected, format, tree, generation) = match input.as_ref() {
                    DocumentationInput::Artifact {
                        selection, format, ..
                    } => {
                        let input = self.sources().get(source).ok_or_else(|| {
                            DocumentationError::Input {
                                source: source.clone(),
                                selection: selection.clone(),
                            }
                        })?;
                        (
                            selection,
                            *format,
                            input
                                .as_input()
                                .map_err(documentation::Error::Tree)
                                .map_err(DocumentationError::Target)?,
                            None,
                        )
                    }
                    DocumentationInput::Generated { recipe, .. } => {
                        let result = generated
                            .and_then(|results| results.get(&(module_index, item_index)))
                            .ok_or_else(|| {
                                DocumentationError::GenerationRequired(source.clone())
                            })?;
                        (
                            &recipe.output,
                            recipe.format,
                            result.output.as_input(),
                            Some(&result.evidence),
                        )
                    }
                };
                let content = selected
                    .content(&tree)
                    .map_err(|_| DocumentationError::Input {
                        source: source.clone(),
                        selection: selected.clone(),
                    })?;
                artifacts.push(DocumentationArtifactEvidence {
                    project: &module.project,
                    module: &module.slug,
                    reference: slug,
                    source,
                    selection: selected,
                    format,
                    content,
                    tree: tree.inventory().identity(),
                    generation,
                });
                navigation.push(ProjectItem::Reference(Reference {
                    slug,
                    title,
                    pages: pages.as_deref(),
                    artifact: Artifact {
                        format,
                        tree,
                        selection: selected,
                    },
                }));
            }
            let manual = match (&module.manual, manual_tree) {
                (Some(configured), Some(tree)) => {
                    let identity = tree.inventory().identity();
                    let mut assets = Vec::new();
                    for asset in &configured.assets {
                        let content = asset.input.content(&tree).map_err(|_| {
                            DocumentationError::ManualInput {
                                source: configured.source.clone(),
                            }
                        })?;
                        manual_evidence.push(DocumentationManualEvidence {
                            kind: DocumentationManualInputKind::Asset,
                            source: &configured.source,
                            selection: &asset.input,
                            output: &asset.path,
                            content,
                            tree: identity,
                        });
                        assets.push(ManualAsset {
                            selection: &asset.input,
                            path: &asset.path,
                        });
                    }
                    Some(ManualSource {
                        source: configured.source.as_str(),
                        tree,
                        assets,
                    })
                }
                (None, None) => None,
                _ => unreachable!("manual configuration and retained tree project together"),
            };
            projects.push(ProjectModule {
                slug: &module.slug,
                title: &module.title,
                manual,
                navigation,
            });
        }
        let usage = usage_policy(&self.compilation().workspace, self.site())?;
        let assembled = target
            .assemble_with_catalog(
                Request {
                    name: &brand.name,
                    public_base_url: repository.public_base_url.as_ref(),
                    target: &build.target,
                    projects: &projects,
                    usage: usage.as_ref(),
                    metadata: documentation::SiteMetadata {
                        description: build.description.as_deref(),
                        publisher: publisher_metadata(&self.compilation().workspace, self.site()),
                    },
                },
                limits,
                documentation::CatalogLimits::default(),
            )
            .map_err(DocumentationError::Target)?;
        let output = assembled.output;
        let catalog = assembled
            .catalog
            .map(|catalog| {
                DocumentationCatalog::bind(self, catalog, output.inventory(), &protected)
                    .map(std::sync::Arc::new)
            })
            .transpose()?;
        // A provider cannot override the caller's complete output allowance.
        let mut checked = InventoryBuilder::new(limits);
        for entry in output.inventory().entries() {
            checked
                .insert(entry.path.clone(), entry.content)
                .map_err(documentation::Error::Tree)
                .map_err(DocumentationError::Target)?;
        }
        Ok(DocumentationAssembly {
            preview_link_bases: target.preview_link_bases().cloned(),
            inputs: self,
            brand: &brand.id,
            repository,
            manual: manual_evidence,
            artifacts,
            output,
            catalog,
            protected,
            generation_output_usage: generated.map_or((0, 0), |artifacts| {
                artifacts
                    .values()
                    .fold((0usize, 0u64), |(entries, bytes), artifact| {
                        (
                            entries.saturating_add(artifact.output.inventory().entries().len()),
                            bytes.saturating_add(artifact.output.inventory().total_bytes()),
                        )
                    })
            }),
        })
    }
}

/// Complete documentation preparation failed; no successful partial target is exposed.
#[derive(Debug)]
pub enum DocumentationError {
    /// Whole configuration or native acquisition failure.
    Composition(CompositionError),
    /// The site has no explicit documentation target selection.
    Binding(Key),
    /// An explicitly selected generator has not been run; imported bytes are not substituted.
    GenerationRequired(Key),
    /// A selected artifact is absent or has the wrong file/directory kind.
    Input {
        /// Exact source key.
        source: Key,
        /// Exact capture-relative selection.
        selection: Selection,
    },
    /// The configured authored-manual source or one of its selections is absent.
    ManualInput {
        /// Exact source key.
        source: Key,
    },
    /// Target-specific validation or output assembly failure.
    Target(documentation::Error),
}
impl DocumentationError {
    /// Stable interface class preserving native source and target failures.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Composition(error) => error.code(),
            Self::Binding(_) => "documentation_binding",
            Self::GenerationRequired(_) => "documentation_generation_required",
            Self::Input { .. } => "documentation_artifact",
            Self::ManualInput { .. } => "documentation_manual_input",
            Self::Target(error) => error.code(),
        }
    }
}
impl fmt::Display for DocumentationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Composition(error) => error.fmt(f),
            Self::Binding(site) => write!(
                f,
                "site {site} has no configured documentation build; select the brand-owned target"
            ),
            Self::GenerationRequired(source) => write!(
                f,
                "documentation source {source} requires explicit native generation before target assembly"
            ),
            Self::Input { source, selection } => write!(
                f,
                "documentation artifact {source}: {selection} is absent or has the wrong kind; check its exact binding"
            ),
            Self::ManualInput { source } => write!(
                f,
                "authored manual source {source} or one of its explicit selections is absent or has the wrong kind"
            ),
            Self::Target(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for DocumentationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Composition(error) => Some(error),
            Self::Target(error) => Some(error),
            _ => None,
        }
    }
}
impl From<CompositionError> for DocumentationError {
    fn from(error: CompositionError) -> Self {
        Self::Composition(error)
    }
}
