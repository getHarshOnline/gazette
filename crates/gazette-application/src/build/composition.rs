//! Effective composition source capture, keyed by source identity rather than fake placements.

use super::{SourceEvidence, binding};
use crate::{
    configuration::{self, ResolvedWorkspace},
    source::{AcquiredInput, AcquisitionMode, Batch, BatchError, PreparationLimits},
};
use gazette_compiler::{
    theme_package, toml,
    workspace::{self, Compilation, Key, PluginSelection, ResourceOwner, ResourceSelection},
};
use gazette_core::plugin::PluginPackageId;
use gazette_ports::source::{RevisionReader, SourceCapture};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    sync::Arc,
};

mod working_sources;

/// Exact selected native inputs and configuration for one consuming site.
///
/// Package and authored roles may share one capture. Authored sources may
/// include other sites needed for complete graph resolution. Payloads are retained
/// once per source key; package/provider compatibility,
/// compilation, policy admission and publication are not implied by capture.
#[derive(Debug)]
pub struct AcquiredInputs {
    pub(super) documentation_catalog: Option<Arc<super::DocumentationCatalog>>,
    pub(super) capabilities: Option<Arc<super::CapabilityCapture>>,
    pub(super) diagrams: Option<Arc<super::DiagramCapture>>,
    pub(super) math: Option<Arc<super::MathCapture>>,
    pub(super) manual_catalogs: BTreeMap<Key, Result<Vec<gazette_core::source::EntryPath>, String>>,
    shared: Arc<CapturedInputs>,
    site: Key,
    selected_themes: BTreeSet<theme_package::ThemePackageId>,
    selected_plugins: BTreeSet<PluginPackageId>,
}

/// One immutable acquisition shared by site views, never duplicated source payloads.
#[derive(Debug)]
pub(super) struct CapturedInputs {
    configuration: ResolvedWorkspace,
    sources: BTreeMap<Key, AcquiredInput>,
    selected: BTreeSet<Key>,
}

impl CapturedInputs {
    pub(super) fn source_roots(&self) -> Result<Vec<PathBuf>, CompositionError> {
        self.selected
            .iter()
            .map(|key| {
                let source = self
                    .compilation()
                    .workspace
                    .source(key)
                    .ok_or_else(|| CompositionError::MissingBinding(key.clone()))?;
                self.configuration
                    .source_root(source.location.root())
                    .map_err(|error| CompositionError::SourceRoot {
                        binding: key.clone(),
                        error,
                    })
            })
            .collect()
    }
    pub(super) fn compilation(&self) -> &Compilation {
        self.configuration.compilation()
    }
    pub(super) fn configuration_evidence(&self) -> &configuration::Evidence {
        self.configuration.evidence()
    }
    pub(super) fn sources(&self) -> &BTreeMap<Key, AcquiredInput> {
        &self.sources
    }
    pub(super) fn source_evidence(&self) -> BTreeMap<&Key, SourceEvidence<'_>> {
        self.sources
            .iter()
            .map(|(key, input)| (key, SourceEvidence::from(input)))
            .collect()
    }
}

impl AcquiredInputs {
    pub(super) fn from_documentation_sources(
        configuration: ResolvedWorkspace,
        site: Key,
        sources: BTreeMap<Key, AcquiredInput>,
        catalogs: BTreeMap<Key, Result<Vec<gazette_core::source::EntryPath>, String>>,
    ) -> Self {
        let selected = sources.keys().cloned().collect();
        let mut inputs = Self::from_capture(
            Arc::new(CapturedInputs {
                configuration,
                sources,
                selected,
            }),
            site,
            BTreeSet::new(),
            BTreeSet::new(),
        );
        inputs.manual_catalogs = catalogs;
        inputs
    }
    pub(super) fn resolve_root(
        &self,
        root: &workspace::SourceRoot,
    ) -> Result<PathBuf, configuration::SourceRootUnavailable> {
        self.shared.configuration.source_root(root)
    }
    /// Resolve this site's explicit output location against the retained native workspace.
    ///
    /// Omission stays absent. No process-directory fallback, filesystem observation,
    /// directory creation, generated-file ownership or permission to replace bytes is implied.
    /// The private native path must not be copied into public template or machine metadata.
    ///
    /// # Errors
    /// Refuses a relative destination when configuration was supplied as detached bytes.
    pub fn output_directory(
        &self,
    ) -> Result<Option<PathBuf>, configuration::SourceRootUnavailable> {
        self.compilation()
            .workspace
            .publication_for(&self.site)
            .and_then(|publication| publication.output.as_ref())
            .map(|output| self.resolve_root(&output.directory))
            .transpose()
    }

    /// Exact supplied or native document-set evidence behind this build graph.
    pub fn configuration_evidence(&self) -> &configuration::Evidence {
        self.shared.configuration_evidence()
    }
    /// Exact retained diagram results; no renderer invokes the native provider.
    pub fn diagram_capture(&self) -> Option<&super::DiagramCapture> {
        self.diagrams.as_deref()
    }
    /// Complete frozen capability inputs; Theme and Tera never receive acquisition handles.
    pub fn capability_capture(&self) -> Option<&super::CapabilityCapture> {
        self.capabilities.as_deref()
    }
    /// The same whole-workspace compilation that selected these native reads.
    pub fn compilation(&self) -> &Compilation {
        self.shared.compilation()
    }

    /// Retained configuration supplies the same bound commands before and after acquisition.
    pub fn configuration(&self) -> &ResolvedWorkspace {
        &self.shared.configuration
    }

    /// Complete native math acquisition retained separately from source facts and
    /// publication approval. Rendering reuses it without executing the typesetter.
    pub fn math_capture(&self) -> Option<&super::MathCapture> {
        self.math.as_deref()
    }

    /// Acquire selected math once before deterministic rendering. Replacing an
    /// existing capture requires a new input acquisition, not a silent rerun.
    ///
    /// # Errors
    /// Preserves complete-input, execution and response failures; never retries.
    pub fn acquire_math(
        &mut self,
        runner: &dyn gazette_ports::process::RetainedCaptureRunner,
        limits: gazette_compiler::corpus::Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        options: super::MathGenerationOptions,
    ) -> Result<(), super::MathGenerationError> {
        let capture = {
            let parsed = self
                .parse_publication(limits, markdown)
                .map_err(super::MathGenerationError::input)?;
            super::MathCapture::generate(&parsed, runner, options)?
        };
        self.math = capture.map(Arc::new);
        Ok(())
    }
    /// Exact consuming site, not inferred from native paths.
    pub fn site(&self) -> &Key {
        &self.site
    }
    /// Captures in source-key order, retaining distinct kind-specific evidence.
    pub fn sources(&self) -> &BTreeMap<Key, AcquiredInput> {
        &self.shared.sources
    }
    pub(super) fn selected_plugins(&self) -> &BTreeSet<PluginPackageId> {
        &self.selected_plugins
    }
    pub(super) fn selected_themes(&self) -> &BTreeSet<theme_package::ThemePackageId> {
        &self.selected_themes
    }
    pub(super) fn source_roots(&self) -> Result<Vec<PathBuf>, CompositionError> {
        let mut roots = self.shared.source_roots()?;
        if let Some(catalog) = self.documentation_catalog() {
            roots.extend(catalog.protected_roots().iter().cloned());
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }
    pub(super) fn from_capture(
        shared: Arc<CapturedInputs>,
        site: Key,
        selected_themes: BTreeSet<theme_package::ThemePackageId>,
        selected_plugins: BTreeSet<PluginPackageId>,
    ) -> Self {
        Self {
            manual_catalogs: BTreeMap::new(),
            documentation_catalog: None,
            capabilities: None,
            math: None,
            diagrams: None,
            shared,
            site,
            selected_themes,
            selected_plugins,
        }
    }
    pub(super) fn shares_capture(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
            && match (&self.diagrams, &other.diagrams) {
                (None, None) => true,
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
            && match (&self.math, &other.math) {
                (None, None) => true,
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
    }
    pub(super) fn source_evidence(&self) -> BTreeMap<&Key, SourceEvidence<'_>> {
        self.shared.source_evidence()
    }
    /// Operator-only source evidence; excludes payloads but retains configured private paths.
    pub fn inspect(&self) -> CompositionInspection<'_> {
        CompositionInspection {
            configuration: self.configuration_evidence(),
            compilation: self.compilation(),
            site: &self.site,
            sources: self.source_evidence(),
            math: self.math_capture(),
            diagrams: self.diagram_capture(),
        }
    }
}

/// Native evidence shared by the SDK and CLI, not a build or capability receipt.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CompositionInspection<'a> {
    /// Exact supplied or captured configuration inputs, including any machine binding.
    pub configuration: &'a configuration::Evidence,
    /// Retained native diagram input/output evidence, not provider dependency qualification.
    pub diagrams: Option<&'a super::DiagramCapture>,
    /// Private exact generation inputs/results; never public template or machine data.
    pub math: Option<&'a super::MathCapture>,
    /// Complete configuration, including unselected sites and selection origins.
    pub compilation: &'a Compilation,
    /// Explicit consumer.
    pub site: &'a Key,
    /// Exact selected source identities and verification, without synthetic output paths.
    pub sources: BTreeMap<&'a Key, SourceEvidence<'a>>,
}

/// Configuration or complete capture failed; no successfully acquired subset escapes.
#[derive(Debug)]
pub enum CompositionError {
    /// Direct-local capability capture failed after exact source acquisition.
    Capability(Box<super::CapabilityAcquisitionError>),
    /// An external fact source has no resolved capture; refusal precedes native reads.
    Facts(gazette_core::publication::facts::Unavailable),
    /// Complete compiler failure before native reads.
    Configuration(workspace::Error),
    /// No exact consumer site exists.
    UnknownSite(Key),
    /// A required resolved binding is absent; no path/name fallback.
    MissingBinding(Key),
    /// A resolved package reference unexpectedly lacks its capture binding.
    MissingPackage(String),
    /// The consumer has no explicit brand/publication binding.
    MissingPublication(Key),
    /// The workspace selects no authored or enabled project-documentation input.
    MissingContent,
    /// A workspace-relative root was selected from detached configuration bytes.
    SourceRoot {
        /// Exact source binding whose root could not be resolved.
        binding: Key,
        /// Missing native workspace-root observation.
        error: configuration::SourceRootUnavailable,
    },
    /// Unique selected source count exceeds the explicit preflight allowance.
    TooManySources {
        /// Caller-selected maximum.
        limit: usize,
    },
    /// Pin, confinement, native read or per-source bound failed.
    Source {
        /// Exact selected source key, independent of provider or Theme name.
        binding: Key,
        /// Original native failure.
        error: gazette_core::source::Error,
    },
    /// A reader returned more than the remaining complete-set allowance.
    Aggregate {
        /// Exact source whose inventory exceeded the allowance.
        binding: Key,
        /// Exhausted resource.
        resource: &'static str,
    },
}

impl CompositionError {
    /// Exact source binding implicated by a binding, root or native-capture failure.
    pub fn binding(&self) -> Option<&Key> {
        match self {
            Self::Capability(_) => None,
            Self::MissingBinding(binding)
            | Self::SourceRoot { binding, .. }
            | Self::Source { binding, .. }
            | Self::Aggregate { binding, .. } => Some(binding),
            _ => None,
        }
    }

    /// Stable failure class, preserving configuration/native classifications.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Capability(error) => error.code(),
            Self::Facts(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::UnknownSite(_) => "workspace_site",
            Self::MissingBinding(_) => "workspace_source_binding",
            Self::MissingPackage(_) => "workspace_package_binding",
            Self::MissingPublication(_) => "publication_binding",
            Self::MissingContent => "publication_content_binding",
            Self::SourceRoot { .. } => "workspace_source_root_unavailable",
            Self::TooManySources { .. } | Self::Aggregate { .. } => "composition_source_limit",
            Self::Source { error, .. } => error.code(),
        }
    }
}
impl fmt::Display for CompositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capability(error) => error.fmt(f),
            Self::Facts(error) => error.fmt(f),
            Self::Configuration(error) => error.fmt(f),
            Self::UnknownSite(site) => write!(f, "site {site} is not declared"),
            Self::MissingBinding(source) => write!(f, "resolved source {source} is absent"),
            Self::MissingPackage(package) => {
                write!(f, "resolved package {package} has no capture binding")
            }
            Self::MissingPublication(site) => {
                write!(f, "site {site} has no configured brand/publication binding")
            }
            Self::MissingContent => f.write_str("workspace has no configured authored or enabled project-documentation input; select a content source before publication acquisition"),
            Self::SourceRoot { binding, error } => {
                write!(f, "composition source {binding}: {error}")
            }
            Self::TooManySources { limit } => {
                write!(f, "selected composition source count exceeds {limit}")
            }
            Self::Source { binding, error } => write!(f, "composition source {binding}: {error}"),
            Self::Aggregate { binding, resource } => write!(
                f,
                "composition source {binding} exceeds complete-set {resource}"
            ),
        }
    }
}
impl std::error::Error for CompositionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Capability(error) => Some(error.as_ref()),
            Self::Facts(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::SourceRoot { error, .. } => Some(error),
            Self::Source { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Compile the whole workspace, then capture only the site's effective selected sources.
///
/// Source keys shared by Theme and several capabilities are captured once. Distinct
/// keys are not collapsed merely because paths or pins match. Disabled, shadowed
/// and other-site sources are not read. Every selected source must succeed;
/// optional policy cannot silently skip a failed capture. No staging or native
/// writes, provider execution, network access or compatibility claim occurs.
///
/// # Errors
/// Rejects configuration, unknown site, count, source-binding, native verification
/// and aggregate bounds before returning any partial acquired composition.
pub fn acquire_composition(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    source: &str,
    site: &Key,
    syntax: toml::Limits,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    let compilation =
        workspace::compile(source, syntax).map_err(CompositionError::Configuration)?;
    let configuration = ResolvedWorkspace::supplied(compilation);
    acquire_resolved_composition(reader, revisions, configuration, site, limits)
}

/// Capture one site's composition from an already resolved workspace, retaining
/// its exact configuration evidence and native root for relative source paths.
///
/// # Errors
/// Refuses missing bindings, incomplete captures and exceeded source bounds.
pub fn acquire_resolved_composition(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: ResolvedWorkspace,
    site: &Key,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    let packages = package_selection(&configuration.compilation().workspace, site)?;
    let selected = packages.sources.clone();
    capture(
        reader,
        revisions,
        configuration,
        site,
        selected.clone(),
        packages,
        limits,
    )
}

#[derive(Clone, Debug, Default)]
pub(super) struct PackageSelection {
    pub(super) sources: BTreeSet<Key>,
    pub(super) themes: BTreeSet<theme_package::ThemePackageId>,
    pub(super) plugins: BTreeSet<PluginPackageId>,
}

pub(super) fn package_selection(
    workspace: &workspace::Workspace,
    site: &Key,
) -> Result<PackageSelection, CompositionError> {
    let composition = workspace
        .composition_for(site)
        .ok_or_else(|| CompositionError::UnknownSite(site.clone()))?;
    let mut selected = PackageSelection::default();
    // Begin with only the selected root. Its retained manifest discloses the
    // dependency closure during bounded acquisition; unrelated site packages
    // never enter this site's source set merely because they share a workspace.
    if let Some(theme) = composition.theme() {
        let binding = workspace
            .theme_package(&theme.value().package)
            .ok_or_else(|| CompositionError::MissingPackage(theme.value().package.to_string()))?;
        selected.sources.extend(theme_sources(workspace, binding)?);
        selected.themes.insert(binding.package.clone());
    }
    for (package, selection) in composition.plugins() {
        if !matches!(selection.value(), PluginSelection::Enabled {}) {
            continue;
        }
        let binding = workspace
            .plugin_package(package)
            .ok_or_else(|| CompositionError::MissingPackage(package.to_string()))?;
        selected.sources.insert(binding.source.clone());
        selected.plugins.insert(package.clone());
    }
    for selection in composition.resources().values() {
        if let ResourceSelection::Enabled { resource, .. } = selection.value() {
            let binding = match &resource.owner {
                ResourceOwner::Theme(package) => workspace.theme_package(package),
                // Plugin resources require an independently enabled plugin-package
                // selection. That route already owns package capture; a resource
                // reference must never imply selection.
                ResourceOwner::Plugin(_) => continue,
            }
            .ok_or_else(|| {
                let package = match &resource.owner {
                    ResourceOwner::Theme(package) => package.to_string(),
                    ResourceOwner::Plugin(package) => package.to_string(),
                };
                CompositionError::MissingPackage(package)
            })?;
            selected.sources.extend(theme_sources(workspace, binding)?);
            if let ResourceOwner::Theme(package) = &resource.owner {
                selected.themes.insert(package.clone());
            }
        }
    }
    Ok(selected)
}

fn theme_sources(
    workspace: &workspace::Workspace,
    binding: &workspace::ThemePackageBinding,
) -> Result<BTreeSet<Key>, CompositionError> {
    let mut sources = BTreeSet::from([binding.source.clone()]);
    for mount in binding.resource_inputs.values() {
        let source = workspace
            .design_input(&mount.input)
            .ok_or_else(|| CompositionError::MissingPackage(binding.package.to_string()))?;
        sources.insert(source.id.clone());
    }
    Ok(sources)
}

pub(super) fn capture(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: ResolvedWorkspace,
    site: &Key,
    selected: BTreeSet<Key>,
    packages: PackageSelection,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    let mut presentations = BTreeMap::from([(site.clone(), packages)]);
    let capture = capture_partitioned(
        reader,
        revisions,
        configuration,
        &selected,
        &selected,
        &mut presentations,
        CapturePlan {
            limits,
            mode: AcquisitionMode::Strict,
            strict_presentations: true,
        },
    )?;
    let package_selection = presentations.remove(site).unwrap_or_default();
    let selected_plugins = package_selection.plugins;
    Ok(AcquiredInputs::from_capture(
        capture.inputs,
        site.clone(),
        package_selection.themes,
        selected_plugins,
    ))
}

/// Required sources fail the shared prerequisite. Remaining sources retain
/// independent failures for their consuming sites; this never makes them optional.
pub(super) struct CaptureOutcome {
    pub(super) inputs: Arc<CapturedInputs>,
    pub(super) failures: BTreeMap<Key, Arc<CompositionError>>,
}

#[derive(Clone, Copy)]
pub(super) struct CapturePlan {
    pub(super) limits: PreparationLimits,
    pub(super) mode: AcquisitionMode,
    pub(super) strict_presentations: bool,
}

pub(super) fn capture_partitioned(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: ResolvedWorkspace,
    selected: &BTreeSet<Key>,
    required: &BTreeSet<Key>,
    presentations: &mut BTreeMap<Key, PackageSelection>,
    plan: CapturePlan,
) -> Result<CaptureOutcome, CompositionError> {
    configuration
        .compilation()
        .workspace
        .require_resolved_facts()
        .map_err(CompositionError::Facts)?;
    if let Some(key) = required.difference(selected).next() {
        return Err(CompositionError::MissingBinding(key.clone()));
    }
    if selected.len() > plan.limits.sources {
        return Err(CompositionError::TooManySources {
            limit: plan.limits.sources,
        });
    }
    // Finish all key-to-coordinate preflight before the first native read.
    let mut requests = BTreeMap::new();
    for key in selected {
        let source = configuration
            .compilation()
            .workspace
            .source(key)
            .ok_or_else(|| CompositionError::MissingBinding(key.clone()))?;
        let selection = binding::selection(&configuration, &source.location).map_err(|error| {
            CompositionError::SourceRoot {
                binding: key.clone(),
                error,
            }
        })?;
        requests.insert(key.clone(), selection);
    }
    let mut batch = Batch::with_mode(plan.limits, plan.mode);
    let exclusive_themes =
        working_sources::exclusive_theme_sources(&configuration.compilation().workspace);
    let mut sources = BTreeMap::new();
    let mut failures = BTreeMap::new();
    // Shared content cannot lose its budget to site-only presentation work.
    // Each selected key still has exactly one attempt and one retained outcome.
    for key in required.iter().chain(selected.difference(required)) {
        let request = requests
            .get(key)
            .ok_or_else(|| CompositionError::MissingBinding(key.clone()))?;
        let input = (if exclusive_themes.contains(key) {
            batch.acquire_theme(reader, revisions, request)
        } else {
            batch.acquire(reader, revisions, request)
        })
        .map_err(|error| match error {
            BatchError::Native(error) => CompositionError::Source {
                binding: key.clone(),
                error,
            },
            BatchError::Aggregate(resource) => CompositionError::Aggregate {
                binding: key.clone(),
                resource,
            },
        });
        match input {
            Ok(input) => {
                sources.insert(key.clone(), input);
            }
            Err(error) if required.contains(key) => return Err(error),
            Err(error) => {
                failures.insert(key.clone(), Arc::new(error));
            }
        }
    }

    // Theme dependencies are package-authored facts, so their exact source keys
    // become knowable only after retaining and validating the parent manifest.
    // Traverse each site's declared graph independently while sharing one source
    // attempt and one aggregate budget across the operation.
    for selection in presentations.values_mut() {
        let mut pending = selection.themes.clone();
        let mut visited = BTreeSet::new();
        while let Some(package) = pending.pop_first() {
            if !visited.insert(package.clone()) {
                continue;
            }
            let Some(binding) = configuration
                .compilation()
                .workspace
                .theme_package(&package)
            else {
                // The Theme graph validator owns the precise missing-package
                // diagnostic after acquisition; never invent a source binding.
                continue;
            };
            selection.themes.insert(package.clone());
            for key in theme_sources(&configuration.compilation().workspace, binding)? {
                selection.sources.insert(key.clone());
                if !sources.contains_key(&key) && !failures.contains_key(&key) {
                    if sources.len() + failures.len() >= plan.limits.sources {
                        return Err(CompositionError::TooManySources {
                            limit: plan.limits.sources,
                        });
                    }
                    let source = configuration
                        .compilation()
                        .workspace
                        .source(&key)
                        .ok_or_else(|| CompositionError::MissingBinding(key.clone()))?;
                    let request =
                        binding::selection(&configuration, &source.location).map_err(|error| {
                            CompositionError::SourceRoot {
                                binding: key.clone(),
                                error,
                            }
                        })?;
                    let acquired = (if exclusive_themes.contains(&key) {
                        batch.acquire_theme(reader, revisions, &request)
                    } else {
                        batch.acquire(reader, revisions, &request)
                    })
                    .map_err(|error| match error {
                        BatchError::Native(error) => CompositionError::Source {
                            binding: key.clone(),
                            error,
                        },
                        BatchError::Aggregate(resource) => CompositionError::Aggregate {
                            binding: key.clone(),
                            resource,
                        },
                    });
                    match acquired {
                        Ok(input) => {
                            sources.insert(key.clone(), input);
                        }
                        Err(error) if plan.strict_presentations => return Err(error),
                        Err(error) => {
                            failures.insert(key.clone(), Arc::new(error));
                        }
                    }
                }
            }
            let Some(input) = sources.get(&binding.source) else {
                continue;
            };
            let Some(bytes) = input.file(&binding.manifest) else {
                continue;
            };
            let Ok(manifest) = std::str::from_utf8(bytes) else {
                continue;
            };
            let Ok(parsed) =
                theme_package::ParsedManifest::parse(manifest, toml::Limits::default())
            else {
                continue;
            };
            if !parsed.manifest().package.eq(&package) {
                continue;
            }
            pending.extend(
                parsed
                    .manifest()
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.package.clone()),
            );
        }
    }

    let selected = required
        .iter()
        .cloned()
        .chain(
            presentations
                .values()
                .flat_map(|selection| selection.sources.iter().cloned()),
        )
        .chain(selected.iter().cloned())
        .collect();
    Ok(CaptureOutcome {
        inputs: Arc::new(CapturedInputs {
            configuration,
            sources,
            selected,
        }),
        failures,
    })
}
