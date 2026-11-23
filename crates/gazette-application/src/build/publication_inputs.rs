//! Join configured publication and package inputs without inventing placements.

use std::fmt;

use gazette_compiler::{
    corpus::{self, ParsedCorpus},
    toml,
    workspace::{self, Key},
};
use gazette_core::source::Content;
use gazette_ports::source::{RevisionReader, SourceCapture};

use super::{AcquiredInputs, CompositionError, DocumentError, ParsedPublication, composition};
use crate::source::PreparationLimits;

/// Capture selected package sources and the complete configured publication universe.
///
/// All authored and rendered project-document sources participate, including other sites, so
/// later graph resolution does not fabricate cross-site endpoints. Exact keys shared
/// across roles use one capture and one aggregate budget. Disabled document placements
/// are excluded by the workspace compiler's binding iterator. No writes or publication occur.
///
/// # Errors
/// Rejects complete configuration, absent publication/content universe, source-count,
/// native verification and aggregate limits without a successfully captured subset.
pub fn acquire_publication(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    source: &str,
    site: &Key,
    syntax: toml::Limits,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    let compilation =
        workspace::compile(source, syntax).map_err(CompositionError::Configuration)?;
    let configuration = crate::configuration::ResolvedWorkspace::supplied(compilation);
    acquire_resolved_publication(reader, revisions, configuration, site, limits)
}

/// Capture the publication universe from the same resolved workspace used by
/// native loading, preserving modular evidence and relative-root ownership.
///
/// # Errors
/// Refuses missing publication bindings, incomplete captures and source bounds.
pub fn acquire_resolved_publication(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    site: &Key,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, CompositionError> {
    let packages = package_selection(&configuration.compilation().workspace, site)?;
    let mut selected = packages.sources.clone();
    selected.extend(required_sources(&configuration.compilation().workspace));
    composition::capture(
        reader,
        revisions,
        configuration,
        site,
        selected,
        packages,
        limits,
    )
}

/// Shared publication prerequisites, independent of selected presentation manifests.
/// The workspace checker consumes one retained policy source, even across site views.
pub(super) fn required_sources(
    workspace: &workspace::Workspace,
) -> std::collections::BTreeSet<Key> {
    workspace
        .content_bindings()
        .chain(workspace.legal_sources())
        .map(|binding| binding.source_key().clone())
        .chain(
            workspace
                .policy_source()
                .map(|policy| policy.source.clone()),
        )
        .chain(
            workspace
                .math_typesetter()
                .map(|typesetter| typesetter.source.clone()),
        )
        .collect()
}

pub(super) fn package_selection(
    workspace: &workspace::Workspace,
    site: &Key,
) -> Result<composition::PackageSelection, CompositionError> {
    let packages = composition::package_selection(workspace, site)?;
    check_publication(workspace, site)?;
    Ok(packages)
}

// A documentation-only or mirror-only consumer needs no dummy authored folder.
// Its actual pages/redirects are resolved from this complete retained universe;
// an empty selected site still fails at the shared render selection boundary.
fn check_publication(workspace: &workspace::Workspace, site: &Key) -> Result<(), CompositionError> {
    if workspace.publication_for(site).is_none() {
        return Err(CompositionError::MissingPublication(site.clone()));
    }
    if workspace.content_bindings().next().is_none() && workspace.legal_sources().next().is_none() {
        return Err(CompositionError::MissingContent);
    }
    Ok(())
}

/// Publication mapping failed before a complete retained syntax corpus could be returned.
#[derive(Debug)]
pub enum PublicationInputError {
    /// A retained typesetting capture does not bind to this exact input set.
    Math(super::MathBindingError),
    /// The consumer or complete workspace content universe is not configured.
    Configuration(CompositionError),
    /// A configured content source was not captured.
    Binding(Key),
    /// Shared path, payload, syntax or aggregate mapping validation failed.
    Document(DocumentError),
}

impl PublicationInputError {
    pub(super) fn source_bytes_limit(&self) -> Option<&corpus::Error> {
        match self {
            Self::Document(DocumentError::Corpus(
                error @ corpus::Error::SourceBytesLimit { .. },
            )) => Some(error),
            _ => None,
        }
    }

    /// Stable class preserving shared document/corpus diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Math(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::Binding(_) => "publication_content_binding",
            Self::Document(error) => error.code(),
        }
    }
}
impl fmt::Display for PublicationInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Math(error) => error.fmt(f),
            Self::Configuration(error) => error.fmt(f),
            Self::Binding(key) => write!(f, "content binding {key} is absent or was not captured"),
            Self::Document(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PublicationInputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Math(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::Document(error) => Some(error),
            _ => None,
        }
    }
}

impl AcquiredInputs {
    /// Parse all configured publication captures, retaining assets and original source bytes.
    ///
    /// Logical paths are explicit binding prefixes plus literal captured entry paths,
    /// never URLs or inferred content identities. Configuration and the source map
    /// retain each native origin. Presentation-only acquisition can satisfy this
    /// operation only if every required content source was also captured by identity.
    /// This is syntax mapping, not content admission or project-doc lowering.
    ///
    /// # Errors
    /// Rejects absent captures, inconsistent payloads, mapping limits and complete-corpus
    /// failures without rereading native source. The workspace compiler has already
    /// rejected overlapping content prefixes before acquisition.
    pub fn parse_publication(
        &self,
        limits: corpus::Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<ParsedPublication<'_>, PublicationInputError> {
        let workspace = &self.compilation().workspace;
        check_publication(workspace, self.site()).map_err(PublicationInputError::Configuration)?;
        let mut inputs = Vec::new();
        let mut profiles = std::collections::BTreeMap::new();
        let mut legal_paths = std::collections::BTreeSet::new();
        let mut path_bytes = 0usize;
        for content in workspace
            .content_bindings()
            .chain(workspace.legal_sources())
        {
            let source = self
                .sources()
                .get(content.source_key())
                .ok_or_else(|| PublicationInputError::Binding(content.source_key().clone()))?;
            if let Some(binding) = content.legal_binding()
                && source.file(&binding.entry).is_none()
            {
                return Err(PublicationInputError::Document(
                    DocumentError::MissingPayload(binding.entry.clone()),
                ));
            }
            for entry in source.inventory().entries() {
                if matches!(entry.content, Content::Directory) {
                    continue;
                }
                if content
                    .legal_binding()
                    .is_some_and(|binding| binding.entry != entry.path)
                {
                    continue;
                }
                let path = content
                    .logical_path(&entry.path)
                    .map_err(|error| PublicationInputError::Document(DocumentError::Path(error)))?;
                // Several legal outputs share one retained document and parser result.
                // This deduplication applies only to explicit legal references, never
                // to conflicting authored-tree mappings.
                if content.legal_binding().is_some() && !legal_paths.insert(path.clone()) {
                    continue;
                }
                if inputs.len() >= limits.files {
                    return Err(mapping_limit("files"));
                }
                path_bytes = path_bytes
                    .checked_add(path.as_str().len())
                    .filter(|&used| used <= limits.path_bytes)
                    .ok_or_else(|| mapping_limit("path bytes"))?;
                let bytes = source.file(&entry.path).ok_or_else(|| {
                    PublicationInputError::Document(DocumentError::MissingPayload(
                        entry.path.clone(),
                    ))
                })?;
                profiles.insert(path.clone(), workspace.markdown_profile(&path));
                inputs.push(corpus::Input { path, bytes });
            }
        }
        let corpus = ParsedCorpus::parse_with_profiles(inputs, &profiles, limits, markdown)
            .map_err(|error| PublicationInputError::Document(DocumentError::Corpus(error)))?;
        let corpus = match self.math_capture() {
            Some(math) => math
                .bind(self, corpus)
                .map_err(PublicationInputError::Math)?,
            None => corpus,
        };
        Ok(ParsedPublication::new(self, corpus))
    }
}

fn mapping_limit(resource: &'static str) -> PublicationInputError {
    PublicationInputError::Document(DocumentError::Corpus(corpus::Error::Limit(resource)))
}
