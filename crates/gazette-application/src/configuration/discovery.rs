//! Discover configured working files without capturing assets or asserting revision identity.

use gazette_core::source::{Listing, ListingLimits, Subdirectory};
use gazette_ports::source::{RevisionReader, SourceCapture, SourceListing};

use super::*;

/// One configured source's current available paths, before logical corpus placement.
#[derive(Debug)]
pub struct DiscoveredSource {
    /// Configured source owner; publication and manual roles remain in the workspace.
    pub source: workspace::Key,
    /// Native root against which later confined reads must resolve.
    pub root: PathBuf,
    /// Optional confined subtree; listing paths are relative to this subtree.
    pub subdirectory: Option<Subdirectory>,
    /// Available working files only, without bytes or verified repository pins.
    pub listing: Listing,
}

/// A selected configured source could not be listed completely.
#[derive(Debug)]
pub enum DiscoveryError {
    /// The caller selected an absent source or a source without a machine binding.
    Unavailable(workspace::Key),
    /// One source's confined metadata listing failed.
    Source {
        /// Configured source whose discovery failed.
        source: workspace::Key,
        /// Underlying native or resource-limit failure.
        error: source::Error,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(source) => write!(f, "source {source} has no configured native root"),
            Self::Source { source, error } => write!(f, "source {source}: {error}"),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(_) => None,
            Self::Source { error, .. } => Some(error),
        }
    }
}

/// Discover the working sources used by publication pages, authored manuals and
/// package manifests in the selected workspace. Unused source declarations and
/// generated-reference inputs are not opened. Equal owners are listed once even
/// when several publications or roles use them.
///
/// The result contains availability only. Consumers retain the workspace's
/// separate logical placements and must capture text before analyzing it.
///
/// # Errors
/// Refuses an unavailable selected owner or an incomplete metadata listing.
pub fn discover_authoring_sources(
    reader: &(impl SourceListing + SourceCapture),
    revisions: &impl RevisionReader,
    loaded: &LoadedWorkspace,
    limits: ListingLimits,
) -> Result<Vec<DiscoveredSource>, DiscoveryError> {
    let workspace = &loaded.documents().compilation.workspace;
    let content = workspace
        .content_bindings()
        .chain(workspace.legal_sources())
        .map(|binding| binding.source_key());
    let manuals = workspace
        .brands()
        .flat_map(|brand| &brand.publications)
        .filter_map(|publication| publication.documentation_repository.as_ref())
        .filter_map(|repository| repository.build.as_ref())
        .flat_map(|build| &build.modules)
        .filter_map(|module| module.manual.as_ref())
        .map(|manual| &manual.source);
    let themes = workspace.theme_packages().map(|package| &package.source);
    let plugins = workspace.plugin_packages().map(|package| &package.source);
    discover_sources(
        reader,
        revisions,
        loaded,
        content.chain(manuals).chain(themes).chain(plugins),
        limits,
    )
}

/// List selected configured source owners under one aggregate metadata budget.
///
/// Callers choose owners from the loaded workspace's content, manual and package
/// bindings. Equal owners are listed once. This inspects current working paths;
/// it neither verifies commit/branch requirements nor supplies build evidence.
/// A later capture retains its independent bounds and revision checks.
///
/// # Errors
/// Refuses unknown/unbound source owners and incomplete or over-limit listings.
pub fn discover_sources<'a>(
    reader: &(impl SourceListing + SourceCapture),
    revisions: &impl RevisionReader,
    loaded: &LoadedWorkspace,
    sources: impl IntoIterator<Item = &'a workspace::Key>,
    mut limits: ListingLimits,
) -> Result<Vec<DiscoveredSource>, DiscoveryError> {
    use workspace::SourceLocation;
    let workspace = &loaded.documents().compilation.workspace;
    let mut discovered = BTreeMap::new();
    for key in sources {
        if discovered.contains_key(key) {
            continue;
        }
        let declaration = workspace
            .source(key)
            .ok_or_else(|| DiscoveryError::Unavailable(key.clone()))?;
        let root = loaded
            .source_roots()
            .get(key)
            .ok_or_else(|| DiscoveryError::Unavailable(key.clone()))?
            .clone();
        let subdirectory = match &declaration.location {
            SourceLocation::Filesystem { subdirectory, .. }
            | SourceLocation::WorkingRepository { subdirectory, .. } => subdirectory.clone(),
            SourceLocation::Repository { subdirectory, .. } => Some(subdirectory.clone()),
            SourceLocation::Submodule {
                submodule_path,
                subdirectory,
                ..
            } => Some(submodule_path.join(subdirectory)),
            SourceLocation::IncludedFilesystem { .. }
            | SourceLocation::FilteredFilesystem { .. } => None,
        };
        let listing = match &declaration.location {
            SourceLocation::WorkingRepository { .. } => {
                let filter = revisions.working_source_filter(
                    &root,
                    subdirectory.as_ref(),
                    source::RevisionLimits {
                        metadata_bytes: limits.path_bytes,
                        ..Default::default()
                    },
                );
                filter.and_then(|filter| {
                    let filter = filter.ok_or_else(|| source::Error::Unsupported(root.clone()))?;
                    reader.list_filtered(&root, subdirectory.as_ref(), filter.as_ref(), limits)
                })
            }
            SourceLocation::IncludedFilesystem { include, .. } => {
                reader.list_including(&root, include, limits)
            }
            SourceLocation::FilteredFilesystem { exclude, .. } => {
                reader.list_excluding(&root, exclude, limits)
            }
            _ => reader.list(&root, subdirectory.as_ref(), limits),
        }
        .map_err(|error| DiscoveryError::Source {
            source: key.clone(),
            error,
        })?;
        let path_bytes = listing
            .entries()
            .keys()
            .try_fold(0usize, |total, path| total.checked_add(path.as_str().len()));
        // Verify aggregate accounting even if a different adapter ignores its bounds.
        limits.entries = limits
            .entries
            .checked_sub(listing.entries().len())
            .ok_or_else(|| exceeded(key, "entry count"))?;
        limits.path_bytes = path_bytes
            .and_then(|bytes| limits.path_bytes.checked_sub(bytes))
            .ok_or_else(|| exceeded(key, "path bytes"))?;
        discovered.insert(
            key.clone(),
            DiscoveredSource {
                source: key.clone(),
                root,
                subdirectory,
                listing,
            },
        );
    }
    Ok(discovered.into_values().collect())
}

fn exceeded(source: &workspace::Key, resource: &'static str) -> DiscoveryError {
    DiscoveryError::Source {
        source: source.clone(),
        error: source::Error::Limit {
            path: PathBuf::new(),
            resource,
        },
    }
}
