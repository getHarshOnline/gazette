//! Complete selected TOML syntax inspection; not workspace semantic admission.

use std::fmt;
use std::path::Path;

use gazette_adapter_fs::FileSystem;
use gazette_core::source::{self, Subdirectory};

use crate::toml::{ParsedSet, SetError, SetInspection, SetLimits};

mod location;
mod preferences;
pub use location::{
    LocationError, LocationOrigin, WorkspaceLocation, WorkspaceLookup, locate_workspace,
};
pub use preferences::{
    default_workspace, locate_user_workspace, set_default_workspace, user_configuration_path,
};

pub use gazette_application::configuration::{
    CapturedWorkspace, DiscoveredSource, DiscoveryError, Error as WorkspaceLoadError,
    Limits as WorkspaceLoadLimits, LoadedWorkspace,
};
pub use gazette_core::source::{
    EntryPath, Limits as SourceLimits, ListedEntry, Listing, ListingLimits,
};

/// List the selected workspace's publication, manual and package source owners.
/// Unused roots stay unopened and assets contribute metadata, not retained bytes.
/// This does not capture compiler input or verify repository revisions.
///
/// # Errors
/// Refuses unavailable selected owners and incomplete or over-limit listings.
pub fn discover_authoring_sources(
    loaded: &LoadedWorkspace,
    limits: ListingLimits,
) -> Result<Vec<DiscoveredSource>, DiscoveryError> {
    gazette_application::configuration::discover_authoring_sources(
        &FileSystem,
        &gazette_adapter_git::Git,
        loaded,
        limits,
    )
}

/// Discover working paths for configured source owners without reading asset bytes.
/// Bounds apply across all selected owners; duplicate owners are listed once.
/// This is editor discovery, not captured content or verified build input.
///
/// # Errors
/// Refuses unknown/unbound owners, unsupported entries and metadata overflows.
pub fn discover_sources<'a>(
    loaded: &LoadedWorkspace,
    sources: impl IntoIterator<Item = &'a crate::workspace::Key>,
    limits: ListingLimits,
) -> Result<Vec<DiscoveredSource>, DiscoveryError> {
    gazette_application::configuration::discover_sources(
        &FileSystem,
        &gazette_adapter_git::Git,
        loaded,
        sources,
        limits,
    )
}

/// Independent native capture and parsed-set limits.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Entry/depth and byte bounds for confined native source capture.
    pub source: source::Limits,
    /// Whole-set and per-document syntax ceilings.
    pub syntax: SetLimits,
}

impl Default for Limits {
    fn default() -> Self {
        let syntax = SetLimits::default();
        Self {
            source: source::Limits {
                file_bytes: syntax.document.source_bytes as u64,
                total_bytes: syntax.source_bytes as u64,
                ..source::Limits::default()
            },
            syntax,
        }
    }
}

/// Native source or complete-set syntax failure, with no partial report.
#[derive(Debug)]
pub enum Error {
    /// The exact selected directory could not be captured safely and completely.
    Source(source::Error),
    /// At least one selected document or set invariant failed.
    Syntax(SetError),
}

impl Error {
    /// Stable original diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Source(error) => error.code(),
            Self::Syntax(error) => error.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(f),
            Self::Syntax(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Source(error) => error,
            Self::Syntax(error) => error,
        })
    }
}

/// Capture an explicit directory and parse every regular file as TOML, regardless of suffix.
///
/// Hidden files are included. Select a dedicated configuration tree, not a mixed
/// repository root. The shared filesystem adapter rejects links/special files and
/// detected changes; this unpinned capture is not an atomic filesystem snapshot.
/// There is no import discovery, reconciliation, database access or publication.
///
/// # Errors
///
/// Returns source or syntax failure without an older/partial successful set.
pub fn inspect_toml(
    root: &Path,
    subdirectory: Option<&Subdirectory>,
    limits: Limits,
) -> Result<SetInspection, Error> {
    let captured =
        gazette_application::source::capture(&FileSystem, root, subdirectory, limits.source)
            .map_err(Error::Source)?;
    let (_, files) = captured.into_parts();
    let parsed = ParsedSet::parse(files.iter(), limits.syntax).map_err(Error::Syntax)?;
    Ok(parsed.inspect())
}

/// Load one strict multi-document Gazette workspace from exact root-relative files.
///
/// The supplied root and workspace file are never inferred from the current directory.
/// No build, source-repository read, registry operation or publication effect occurs.
///
/// # Errors
///
/// Propagates exact native capture, syntax, module-set and workspace-resolution failures.
pub fn load_workspace(
    root: &Path,
    workspace: &gazette_core::source::EntryPath,
    limits: WorkspaceLoadLimits,
) -> Result<LoadedWorkspace, WorkspaceLoadError> {
    gazette_application::configuration::load(&FileSystem, root, workspace, limits)
}

/// Retain exact selected configuration bytes for editing before strict validation.
/// A root whose selection cannot be parsed stays available without guessed includes.
/// Valid selections retain their includes and separate machine binding even when
/// their text cannot form a valid workspace. Publication sources remain unopened.
///
/// # Errors
/// Refuses relative roots, missing or unsafe selected files, changing selectors
/// and exhausted capture limits. Syntax failures stay in the returned capture.
pub fn capture_workspace(
    root: &Path,
    workspace: &EntryPath,
    limits: WorkspaceLoadLimits,
) -> Result<CapturedWorkspace, WorkspaceLoadError> {
    gazette_application::configuration::capture(&FileSystem, root, workspace, limits)
}

/// Capture one explicit set of workspace-root-relative files without discovery.
///
/// This is the native counterpart to supplied editor buffers. Paths retain their
/// caller-selected roles outside this operation; no extension or directory scan
/// infers meaning.
///
/// # Errors
///
/// Rejects duplicate, unsafe, missing, changing or over-limit selections without
/// returning a partial capture.
pub fn capture_selected(
    root: &Path,
    paths: &[gazette_core::source::EntryPath],
    limits: gazette_core::source::Limits,
) -> Result<gazette_core::source::CapturedSource, gazette_core::source::Error> {
    gazette_application::source::capture_selected(&FileSystem, root, paths, limits)
}
