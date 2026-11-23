//! Selected baselines and full-text overlays shared by editor transports, without I/O.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use crate::{authoring, source::EntryPath, workspace::Key};

mod completion;
pub use completion::{PreviousReferenceCandidate, PreviousReferenceCompletion};
mod observations;
pub use observations::{AssetObservation, AssetObservationSource};
mod origin;
#[cfg(feature = "native-configuration")]
mod reselection;

#[cfg(feature = "native-configuration")]
pub mod native;
mod snapshot;
pub use origin::{Origin, OriginError, OriginQuery, OriginValue};
pub use snapshot::{Snapshot, SnapshotSource, SourceRole};

/// One explicitly selected source. The client identifier is opaque, never opened.
pub struct Source {
    /// Client identifier; URI syntax and protocol validation belong to the caller.
    pub id: String,
    /// Exact logical compiler source path, independent of the client identifier.
    pub path: EntryPath,
    /// Complete selected UTF-8 baseline, not a filename to read.
    pub text: String,
}

/// One authored-manual baseline bound to the exact configured capture source.
pub struct ManualSource {
    /// Exact configured workspace source key owning this manual tree.
    pub source: Key,
    /// Capture-relative logical path, client identity and complete current text.
    pub input: Source,
}

/// One explicitly selected opaque retained input used only by native page-package analysis.
pub struct PageInputSource {
    /// Client identifier; never opened or interpreted as authority.
    pub id: String,
    /// Exact logical compiler source path.
    pub path: EntryPath,
    /// Complete retained bytes, including non-UTF-8 assets.
    pub bytes: Vec<u8>,
}

/// One explicitly keyed Theme or plugin package baseline.
pub struct PackageSource {
    /// Exact typed package identity. Workspace bindings alone supply source and manifest paths.
    pub package: authoring::PackageBaselineKey,
    /// Opaque client identity, logical path and complete current bytes.
    pub input: Source,
}

/// Independent retained-state and shared analysis ceilings.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Complete selection count, including the workspace.
    pub documents: usize,
    /// Aggregate selected client-identifier bytes.
    pub identifier_bytes: usize,
    /// Aggregate baseline, overlay and optional completion-source UTF-8 bytes.
    pub source_bytes: usize,
    /// Native compiler and complete-result bounds.
    pub analysis: authoring::Limits,
}

impl Default for Limits {
    fn default() -> Self {
        let analysis = authoring::Limits::default();
        Self {
            // Selected native roots transfer locators, not a single bulk-text frame.
            // Retain finite corpus-scale membership while bounding bytes independently.
            documents: analysis.corpus.files,
            identifier_bytes: analysis.corpus.path_bytes,
            // A complete valid corpus must fit before overlays can be analyzed.
            // Callers can still select a smaller explicit retention allowance.
            source_bytes: analysis.corpus.source_bytes,
            analysis,
        }
    }
}

/// Rejected selection or update, without echoing identifiers or source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The supplied selection cannot have one unambiguous source owner.
    Selection(&'static str),
    /// Retained state or generation would exceed the declared ceiling.
    Limit(&'static str),
}

impl Error {
    /// Stable transport-independent diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Selection(_) => "authoring_session_selection",
            Self::Limit(_) => "authoring_session_limit",
        }
    }

    /// Bounded native reason, never rejected payload text.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Selection(reason) | Self::Limit(reason) => reason,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(reason) => write!(formatter, "authoring selection: {reason}"),
            Self::Limit(resource) => write!(formatter, "authoring session exceeds {resource}"),
        }
    }
}

impl std::error::Error for Error {}

/// Current text and version for one immutable selected path and baseline.
pub struct Document {
    path: EntryPath,
    analysis_path: EntryPath,
    baseline: String,
    overlay: Option<Overlay>,
}

/// Immutable opaque retained input; editor overlays cannot change package membership.
pub struct PageInput {
    path: EntryPath,
    bytes: Vec<u8>,
}

impl PageInput {
    /// Exact selected logical path.
    pub fn path(&self) -> &EntryPath {
        &self.path
    }

    /// Digest of the complete retained bytes.
    pub fn source_sha256(&self) -> gazette_core::source::Sha256Digest {
        gazette_core::source::Sha256Digest::of(&self.bytes)
    }
}

struct Overlay {
    version: i32,
    text: String,
}

impl Document {
    /// Exact selected logical identity; an overlay cannot change its role or path.
    pub fn path(&self) -> &EntryPath {
        &self.path
    }

    /// Internal compiler path. Manual sources are qualified by configured source;
    /// all transport-visible paths remain capture-relative through [`Self::path`].
    pub fn analysis_path(&self) -> &EntryPath {
        &self.analysis_path
    }

    /// Current complete text, or the selected baseline when no overlay is open.
    pub fn text(&self) -> &str {
        self.overlay
            .as_ref()
            .map_or(&self.baseline, |value| &value.text)
    }

    /// Last accepted open-overlay version; baseline sources have no editor version.
    pub fn version(&self) -> Option<i32> {
        self.overlay.as_ref().map(|value| value.version)
    }
}

/// Synchronous selected-source state and its sole current analysis result.
///
/// No source discovery, URI reads, background workers or publication effects occur.
/// Clients must bind asynchronous delivery to the generation they observed.
pub struct Session {
    files: BTreeMap<String, Document>,
    page_inputs: BTreeMap<String, PageInput>,
    asset_observations: BTreeMap<String, AssetObservation>,
    analysis: Result<authoring::Analysis, authoring::Error>,
    completion_basis: Option<completion::Basis>,
    generation: u64,
    workspace: String,
    workspace_documents: Option<BTreeSet<String>>,
    manuals: BTreeMap<String, Key>,
    package_baselines: BTreeMap<String, authoring::PackageBaselineKey>,
    retained_bytes: usize,
    limits: Limits,
}

impl Session {
    /// Retain one explicit workspace baseline and a complete selected document set.
    /// Invalid source semantics remain the current analysis, never a selection fallback.
    ///
    /// # Errors
    /// Refuses duplicate paths/identifiers and exhausted retained-selection bounds.
    pub fn new(
        workspace: Source,
        documents: impl IntoIterator<Item = Source>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_with_package_baselines(workspace, documents, [], limits)
    }

    /// Retain workspace, authored documents and explicitly keyed package baselines.
    /// Workspace bindings remain the sole source/manifest owner. Invalid current semantics
    /// remain analysis evidence; role ambiguity is rejected first.
    ///
    /// # Errors
    ///
    /// Refuses duplicate roles, paths or identifiers and exhausted selected-source bounds.
    pub fn new_with_package_baselines(
        workspace: Source,
        documents: impl IntoIterator<Item = Source>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_selected(
            workspace,
            None,
            documents,
            [],
            [],
            package_baselines,
            limits,
        )
    }

    /// Retain publication documents and distinct authored-manual baselines.
    /// Manuals participate in shared Markdown analysis but never authored-content
    /// resolution or Theme component binding.
    ///
    /// # Errors
    ///
    /// Refuses duplicate roles, paths or identifiers and exhausted selected-source bounds.
    pub fn new_with_manuals_and_package_baselines(
        workspace: Source,
        documents: impl IntoIterator<Item = Source>,
        manuals: impl IntoIterator<Item = ManualSource>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_with_manuals_page_inputs_and_package_baselines(
            workspace,
            documents,
            manuals,
            [],
            package_baselines,
            limits,
        )
    }

    /// Retain publication documents, authored manuals and exact opaque page inputs.
    /// Opaque inputs participate only in the compiler's page-package resolver and
    /// cannot be opened as text overlays.
    ///
    /// # Errors
    /// Refuses overlapping roles, duplicate identities or exhausted bounds.
    pub fn new_with_manuals_page_inputs_and_package_baselines(
        workspace: Source,
        documents: impl IntoIterator<Item = Source>,
        manuals: impl IntoIterator<Item = ManualSource>,
        page_inputs: impl IntoIterator<Item = PageInputSource>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_selected(
            workspace,
            None,
            documents,
            manuals,
            page_inputs,
            package_baselines,
            limits,
        )
    }

    /// Retain one explicit modular workspace root, its exact non-recursive included
    /// documents, authored documents and package baselines. The compiler remains
    /// the sole include-set and configuration-graph owner on every analysis generation.
    ///
    /// # Errors
    ///
    /// Refuses duplicate roles, paths or identifiers and exhausted selected-source bounds.
    pub fn new_with_workspace_documents(
        workspace: Source,
        workspace_documents: impl IntoIterator<Item = Source>,
        documents: impl IntoIterator<Item = Source>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_selected(
            workspace,
            Some(workspace_documents.into_iter().collect()),
            documents,
            [],
            [],
            package_baselines,
            limits,
        )
    }

    /// Retain a modular workspace, publication documents, distinct authored manuals,
    /// and package baselines without discovering any source.
    ///
    /// # Errors
    ///
    /// Refuses duplicate roles, paths or identifiers and exhausted selected-source bounds.
    pub fn new_with_workspace_documents_and_manuals(
        workspace: Source,
        workspace_documents: impl IntoIterator<Item = Source>,
        documents: impl IntoIterator<Item = Source>,
        manuals: impl IntoIterator<Item = ManualSource>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_with_workspace_documents_manuals_page_inputs(
            workspace,
            workspace_documents,
            documents,
            manuals,
            [],
            package_baselines,
            limits,
        )
    }

    /// Retain a modular workspace, publication documents, manuals and exact opaque
    /// page-package inputs without source discovery.
    ///
    /// # Errors
    /// Refuses overlapping roles, duplicate identities or exhausted bounds.
    pub fn new_with_workspace_documents_manuals_page_inputs(
        workspace: Source,
        workspace_documents: impl IntoIterator<Item = Source>,
        documents: impl IntoIterator<Item = Source>,
        manuals: impl IntoIterator<Item = ManualSource>,
        page_inputs: impl IntoIterator<Item = PageInputSource>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::new_selected(
            workspace,
            Some(workspace_documents.into_iter().collect()),
            documents,
            manuals,
            page_inputs,
            package_baselines,
            limits,
        )
    }

    fn new_selected(
        workspace: Source,
        workspace_documents: Option<Vec<Source>>,
        documents: impl IntoIterator<Item = Source>,
        manuals: impl IntoIterator<Item = ManualSource>,
        page_inputs: impl IntoIterator<Item = PageInputSource>,
        package_baselines: impl IntoIterator<Item = PackageSource>,
        limits: Limits,
    ) -> Result<Self, Error> {
        if workspace.id.len() > limits.identifier_bytes {
            return Err(Error::Limit("selected identifier bytes"));
        }
        let workspace_id = workspace.id.clone();
        let mut workspace_document_ids = workspace_documents.as_ref().map(|_| {
            let mut ids = BTreeSet::new();
            ids.insert(workspace_id.clone());
            ids
        });
        let mut package_roles = BTreeMap::new();
        let mut manual_ids = BTreeMap::new();
        let mut package_keys = BTreeSet::new();
        let mut package_sources = Vec::new();
        for baseline in package_baselines {
            if !package_keys.insert(baseline.package.clone()) {
                return Err(Error::Selection("duplicate package baseline"));
            }
            if package_roles
                .insert(baseline.input.id.clone(), baseline.package)
                .is_some()
            {
                return Err(Error::Selection("duplicate package identifier"));
            }
            package_sources.push(baseline.input);
        }
        let mut paths = BTreeSet::new();
        let mut files = BTreeMap::new();
        let mut retained_page_inputs = BTreeMap::new();
        let mut retained_bytes = 0usize;
        let mut identifier_bytes = 0usize;
        let mut path_bytes = 0usize;
        for source in std::iter::once(workspace)
            .chain(workspace_documents.into_iter().flatten().inspect(|source| {
                if let Some(ids) = &mut workspace_document_ids {
                    ids.insert(source.id.clone());
                }
            }))
            .chain(documents)
            .chain(package_sources)
        {
            path_bytes = path_bytes
                .checked_add(source.path.as_str().len())
                .filter(|bytes| *bytes <= limits.analysis.corpus.path_bytes)
                .ok_or(Error::Limit("selected path bytes"))?;
            if !paths.insert(source.path.clone()) || files.contains_key(&source.id) {
                return Err(Error::Selection("duplicate source path or identifier"));
            }
            retained_bytes = retained_bytes
                .checked_add(source.text.len())
                .filter(|bytes| *bytes <= limits.source_bytes)
                .ok_or(Error::Limit("selected source bytes"))?;
            identifier_bytes = identifier_bytes
                .checked_add(source.id.len())
                .filter(|bytes| *bytes <= limits.identifier_bytes)
                .ok_or(Error::Limit("selected identifier bytes"))?;
            if files.len() >= limits.documents {
                return Err(Error::Limit("selected documents"));
            }
            files.insert(
                source.id,
                Document {
                    analysis_path: source.path.clone(),
                    path: source.path,
                    baseline: source.text,
                    overlay: None,
                },
            );
        }
        let mut manual_bindings = BTreeSet::new();
        for manual in manuals {
            let binding = authoring::ManualBinding {
                source: manual.source.clone(),
                path: manual.input.path.clone(),
            };
            if !manual_bindings.insert(binding.clone())
                || files.contains_key(&manual.input.id)
                || manual_ids
                    .insert(manual.input.id.clone(), manual.source)
                    .is_some()
            {
                return Err(Error::Selection(
                    "duplicate manual source/path binding or identifier",
                ));
            }
            let analysis_path = binding
                .analysis_path()
                .map_err(|_| Error::Selection("invalid configured manual identity"))?;
            path_bytes = path_bytes
                .checked_add(binding.source.as_str().len())
                .and_then(|value| value.checked_add(binding.path.as_str().len()))
                .filter(|bytes| *bytes <= limits.analysis.corpus.path_bytes)
                .ok_or(Error::Limit("selected path bytes"))?;
            if !paths.insert(analysis_path.clone()) {
                return Err(Error::Selection("duplicate qualified source path"));
            }
            retained_bytes = retained_bytes
                .checked_add(manual.input.text.len())
                .filter(|bytes| *bytes <= limits.source_bytes)
                .ok_or(Error::Limit("selected source bytes"))?;
            identifier_bytes = identifier_bytes
                .checked_add(manual.input.id.len())
                .filter(|bytes| *bytes <= limits.identifier_bytes)
                .ok_or(Error::Limit("selected identifier bytes"))?;
            if files.len() >= limits.documents {
                return Err(Error::Limit("selected documents"));
            }
            files.insert(
                manual.input.id,
                Document {
                    path: manual.input.path,
                    analysis_path,
                    baseline: manual.input.text,
                    overlay: None,
                },
            );
        }
        for source in page_inputs {
            path_bytes = path_bytes
                .checked_add(source.path.as_str().len())
                .filter(|bytes| *bytes <= limits.analysis.corpus.path_bytes)
                .ok_or(Error::Limit("selected path bytes"))?;
            if !paths.insert(source.path.clone())
                || files.contains_key(&source.id)
                || retained_page_inputs.contains_key(&source.id)
            {
                return Err(Error::Selection("duplicate source path or identifier"));
            }
            retained_bytes = retained_bytes
                .checked_add(source.bytes.len())
                .filter(|bytes| *bytes <= limits.source_bytes)
                .ok_or(Error::Limit("selected source bytes"))?;
            identifier_bytes = identifier_bytes
                .checked_add(source.id.len())
                .filter(|bytes| *bytes <= limits.identifier_bytes)
                .ok_or(Error::Limit("selected identifier bytes"))?;
            if files.len() + retained_page_inputs.len() >= limits.documents {
                return Err(Error::Limit("selected documents"));
            }
            retained_page_inputs.insert(
                source.id,
                PageInput {
                    path: source.path,
                    bytes: source.bytes,
                },
            );
        }
        let analysis = analyze(
            AnalysisInputs {
                workspace: &workspace_id,
                workspace_documents: workspace_document_ids.as_ref(),
                manuals: &manual_ids,
                page_inputs: &retained_page_inputs,
                asset_observations: &BTreeMap::new(),
                package_baselines: &package_roles,
                files: &files,
            },
            limits.analysis,
        );
        Ok(Self {
            files,
            page_inputs: retained_page_inputs,
            asset_observations: BTreeMap::new(),
            analysis,
            completion_basis: None,
            generation: 1,
            workspace: workspace_id,
            workspace_documents: workspace_document_ids,
            manuals: manual_ids,
            package_baselines: package_roles,
            retained_bytes,
            limits,
        })
    }

    /// Borrow the exact selected set without permitting caller mutation.
    pub fn files(&self) -> &BTreeMap<String, Document> {
        &self.files
    }

    /// Client identity of the selected root configuration document.
    pub fn workspace_id(&self) -> &str {
        &self.workspace
    }

    /// Exact opaque retained page-package inputs, never editor-open documents.
    pub fn page_inputs(&self) -> &BTreeMap<String, PageInput> {
        &self.page_inputs
    }

    /// Return the typed package role for one exact selected identifier.
    ///
    /// The selected document path remains editor-local input identity; workspace bindings
    /// remain the sole owners of package source and manifest coordinates.
    pub fn package_baseline(&self, id: &str) -> Option<&authoring::PackageBaselineKey> {
        self.package_baselines.get(id)
    }

    /// Whether one exact selected identifier is an authored-manual baseline.
    pub fn is_manual(&self, id: &str) -> bool {
        self.manuals.contains_key(id)
    }

    /// Resolve one selected document or opaque page input to the compiler-owned package.
    ///
    /// # Errors
    /// Returns the native analysis error or rejects an unknown selected identifier.
    pub fn page_package(
        &self,
        id: &str,
    ) -> Result<Option<authoring::PagePackageOwnership<'_>>, authoring::NavigationError> {
        let analysis = self
            .analysis
            .as_ref()
            .map_err(|_| authoring::NavigationError::Unavailable)?;
        // Presentation/package baselines are selected editor documents, but they are
        // not publication-corpus inputs and therefore cannot own page packages.
        if self.package_baselines.contains_key(id) {
            return Ok(None);
        }
        // Authored manuals are explicitly selected as a distinct role and never
        // participate in Theme page-package ownership or entrypoint inference.
        if self.manuals.contains_key(id) {
            return Ok(None);
        }
        if let Some(document) = self.files.get(id) {
            return analysis.page_package_ownership(
                document.path(),
                gazette_core::source::Sha256Digest::of(document.text().as_bytes()),
            );
        }
        if let Some(input) = self.page_inputs.get(id) {
            return analysis.page_package_ownership(input.path(), input.source_sha256());
        }
        if let Some(asset) = self.asset_observations.get(id) {
            return analysis.observed_page_package_ownership(asset.path(), asset.observed_bytes());
        }
        Err(authoring::NavigationError::InvalidSource)
    }

    /// Resolve one authored resource scalar to its exact retained page input.
    ///
    /// # Errors
    /// Returns the native current-analysis failure or rejects an unknown selected
    /// document. No URI is opened and compiled outputs remain inspection evidence.
    pub fn page_resource_navigation(
        &self,
        id: &str,
        offset: usize,
    ) -> Result<Option<authoring::PageResourceNavigation<'_>>, authoring::NavigationError> {
        if id == self.workspace
            || self
                .workspace_documents
                .as_ref()
                .is_some_and(|documents| documents.contains(id))
            || self.manuals.contains_key(id)
            || self.package_baselines.contains_key(id)
        {
            return Ok(None);
        }
        let document = self
            .files
            .get(id)
            .ok_or(authoring::NavigationError::InvalidSource)?;
        self.analysis
            .as_ref()
            .map_err(|_| authoring::NavigationError::Unavailable)?
            .page_resource_navigation(document.path(), document.text(), offset)
    }

    /// Borrow current native analysis, including failure. Completion history is separate.
    pub fn analysis(&self) -> &Result<authoring::Analysis, authoring::Error> {
        &self.analysis
    }

    /// Monotonic identity within this session, advanced only by accepted source changes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Open a selected baseline as a full-text overlay. Unselected/already-open is a no-op.
    ///
    /// # Errors
    /// Refuses retained-byte or generation exhaustion without changing the session.
    pub fn open(&mut self, id: &str, version: i32, text: String) -> Result<bool, Error> {
        if self
            .files
            .get(id)
            .is_none_or(|document| document.overlay.is_some())
        {
            return Ok(false);
        }
        self.replace(id, version, text)?;
        Ok(true)
    }

    /// Replace one open overlay. Unselected/unopened or non-increasing versions are no-ops.
    ///
    /// # Errors
    /// Refuses retained-byte or generation exhaustion without changing the session.
    pub fn change(&mut self, id: &str, version: i32, text: String) -> Result<bool, Error> {
        if self
            .files
            .get(id)
            .and_then(Document::version)
            .is_none_or(|old| version <= old)
        {
            return Ok(false);
        }
        self.replace(id, version, text)?;
        Ok(true)
    }

    /// Retain an explicitly acknowledged save as the new selected baseline.
    /// No filesystem read is performed, and the text must equal the current overlay.
    ///
    /// # Errors
    /// Refuses stale text, retained-byte or generation exhaustion before any change.
    pub fn save(&mut self, id: &str, text: &str) -> Result<bool, Error> {
        let Some(document) = self.files.get(id) else {
            return Ok(false);
        };
        let Some(overlay) = &document.overlay else {
            return Ok(false);
        };
        if overlay.text != text {
            return Err(Error::Selection("save does not match current overlay"));
        }
        if document.baseline == text {
            self.completion_basis = None;
            return Ok(false);
        }
        let generation = self.next_generation()?;
        let bytes = (self.retained_bytes - document.baseline.len())
            .checked_add(text.len())
            .filter(|bytes| *bytes <= self.limits.source_bytes)
            .ok_or(Error::Limit("retained source bytes"))?;
        self.files
            .get_mut(id)
            .ok_or(Error::Selection("unselected source"))?
            .baseline = text.to_owned();
        self.retained_bytes = bytes;
        self.refresh(generation);
        Ok(true)
    }

    /// Close one overlay and reanalyze the selected baseline, never current native files.
    ///
    /// # Errors
    /// Refuses generation exhaustion before removing the overlay.
    pub fn close(&mut self, id: &str) -> Result<bool, Error> {
        if self
            .files
            .get(id)
            .is_none_or(|document| document.overlay.is_none())
        {
            return Ok(false);
        }
        let generation = self.next_generation()?;
        let document = self
            .files
            .get_mut(id)
            .ok_or(Error::Selection("unselected source"))?;
        let overlay = document
            .overlay
            .take()
            .ok_or(Error::Selection("source is not open"))?;
        self.retained_bytes -= overlay.text.len();
        self.refresh(generation);
        Ok(true)
    }

    fn replace(&mut self, id: &str, version: i32, text: String) -> Result<(), Error> {
        let generation = self.next_generation()?;
        let document = self
            .files
            .get_mut(id)
            .ok_or(Error::Selection("unselected source"))?;
        let previous = document
            .overlay
            .as_ref()
            .map_or(0, |value| value.text.len());
        let bytes = (self.retained_bytes - previous)
            .checked_add(text.len())
            .filter(|bytes| *bytes <= self.limits.source_bytes)
            .ok_or(Error::Limit("retained source bytes"))?;
        let previous_source = self.analysis.as_ref().ok().and_then(|analysis| {
            analysis
                .workspace_symbols(document.analysis_path(), document.text())
                .ok()
                .flatten()
                .filter(|_| bytes.saturating_add(document.text().len()) <= self.limits.source_bytes)
                .map(|_| document.text().to_owned())
        });
        document.overlay = Some(Overlay { version, text });
        self.retained_bytes = bytes;
        let previous_generation = self.generation;
        let previous_analysis =
            std::mem::replace(&mut self.analysis, Err(authoring::Error::InvalidSource));
        let previous_basis = self.completion_basis.take();
        self.refresh(generation);
        self.retain_completion_basis(
            id,
            previous_source,
            previous_analysis,
            previous_basis,
            previous_generation,
        );
        Ok(())
    }

    fn next_generation(&self) -> Result<u64, Error> {
        self.generation
            .checked_add(1)
            .ok_or(Error::Limit("analysis generation"))
    }

    fn refresh(&mut self, generation: u64) {
        self.completion_basis = None;
        self.generation = generation;
        self.analysis = analyze(
            AnalysisInputs {
                workspace: &self.workspace,
                workspace_documents: self.workspace_documents.as_ref(),
                manuals: &self.manuals,
                page_inputs: &self.page_inputs,
                asset_observations: &self.asset_observations,
                package_baselines: &self.package_baselines,
                files: &self.files,
            },
            self.limits.analysis,
        );
    }
}

struct AnalysisInputs<'a> {
    workspace: &'a str,
    workspace_documents: Option<&'a BTreeSet<String>>,
    manuals: &'a BTreeMap<String, Key>,
    page_inputs: &'a BTreeMap<String, PageInput>,
    asset_observations: &'a BTreeMap<String, AssetObservation>,
    package_baselines: &'a BTreeMap<String, authoring::PackageBaselineKey>,
    files: &'a BTreeMap<String, Document>,
}

fn analyze(
    inputs: AnalysisInputs<'_>,
    limits: authoring::Limits,
) -> Result<authoring::Analysis, authoring::Error> {
    let AnalysisInputs {
        workspace,
        workspace_documents,
        manuals,
        page_inputs,
        asset_observations,
        package_baselines,
        files,
    } = inputs;
    let workspace = files
        .get_key_value(workspace)
        .ok_or(authoring::Error::InvalidSource)?;
    let authored = || {
        files
            .iter()
            .filter(|(id, _)| {
                workspace_documents.map_or(*id != workspace.0, |members| !members.contains(*id))
                    && !package_baselines.contains_key(*id)
                    && !manuals.contains_key(*id)
            })
            .map(|(_, document)| input(document))
    };
    let manual_inputs = || {
        manuals.iter().filter_map(|(id, source)| {
            files.get(id).map(|document| authoring::ManualBaseline {
                source: source.clone(),
                path: document.path.clone(),
                bytes: document.text().as_bytes(),
            })
        })
    };
    let retained_page_inputs = || {
        page_inputs.values().map(|input| authoring::Input {
            path: input.path.clone(),
            bytes: &input.bytes,
        })
    };
    let packages = || {
        package_baselines.iter().filter_map(|(id, package)| {
            files.get(id).map(|document| authoring::PackageBaseline {
                package: package.clone(),
                bytes: document.text().as_bytes(),
            })
        })
    };
    let corpus = gazette_application::authoring::SelectedCorpus {
        documents: authored().collect(),
        manuals: manual_inputs().collect(),
        page_inputs: retained_page_inputs().collect(),
        asset_metadata: asset_observations
            .values()
            .map(|asset| (asset.path().clone(), asset.observed_bytes()))
            .collect(),
    };
    match workspace_documents {
        None => gazette_application::authoring::analyze_with_manuals_and_package_baselines(
            input(workspace.1),
            corpus,
            packages(),
            limits,
            &gazette_adapter_markdown::Comrak,
        ),
        Some(members) => {
            gazette_application::authoring::analyze_workspace_documents_with_manuals_and_package_baselines(
                workspace.1.path.clone(),
                members
                    .iter()
                    .filter_map(|id| files.get(id).map(input))
                    .collect(),
                corpus,
                packages(),
                limits,
                &gazette_adapter_markdown::Comrak,
            )
        }
    }
}

fn input(document: &Document) -> authoring::Input<'_> {
    authoring::Input {
        path: document.path.clone(),
        bytes: document.text().as_bytes(),
    }
}
