//! Find a workspace once for command-line, agent and editor clients.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use super::EntryPath;

/// The first successful source in the shared workspace lookup order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocationOrigin {
    /// A workspace file or folder named in this request.
    Request,
    /// The folder supplied by the client.
    Client,
    /// The folder where the process started.
    WorkingDirectory,
    /// The user's saved default.
    UserDefault,
}

/// The selected configuration path and the reason it was selected.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct WorkspaceLocation {
    /// Absolute folder containing the root configuration document.
    pub root: PathBuf,
    /// Root-relative configuration document, usually `gazette.toml`.
    pub workspace: EntryPath,
    /// Which lookup input supplied this workspace.
    pub found_by: LocationOrigin,
}

/// Host context, captured by the caller rather than read from global state.
pub struct WorkspaceLookup<'a> {
    /// Explicit file or folder. A missing selection fails instead of falling back.
    pub request: Option<&'a Path>,
    /// Client folder, searched up to its repository boundary.
    pub client: Option<&'a Path>,
    /// Process start folder; also the base for relative request paths.
    pub start: &'a Path,
    /// Optional saved default. Editors can omit it for an unrelated project.
    pub default: Option<&'a Path>,
}

/// A named lookup candidate could not be inspected or used.
#[derive(Debug)]
pub struct LocationError {
    /// The path at which lookup stopped.
    pub path: PathBuf,
    error: io::Error,
}

impl fmt::Display for LocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.error)
    }
}

impl std::error::Error for LocationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

fn error(path: &Path, error: io::Error) -> LocationError {
    LocationError {
        path: path.into(),
        error,
    }
}

fn absolute(path: &Path, start: &Path) -> PathBuf {
    if path.is_absolute() {
        path.into()
    } else {
        start.join(path)
    }
}

fn selected(path: &Path, found_by: LocationOrigin) -> Result<WorkspaceLocation, LocationError> {
    let metadata = fs::metadata(path).map_err(|e| error(path, e))?;
    let file = if metadata.is_dir() {
        path.join("gazette.toml")
    } else {
        path.into()
    };
    let metadata = fs::metadata(&file).map_err(|e| error(&file, e))?;
    if !metadata.is_file() {
        return Err(error(
            &file,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "workspace must be a regular file",
            ),
        ));
    }
    let root = file.parent().ok_or_else(|| {
        error(
            &file,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "workspace needs a containing folder",
            ),
        )
    })?;
    let workspace = file
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| {
            error(
                &file,
                io::Error::new(io::ErrorKind::InvalidInput, "invalid workspace filename"),
            )
        })?;
    Ok(WorkspaceLocation {
        root: root.canonicalize().map_err(|e| error(root, e))?,
        workspace,
        found_by,
    })
}

fn near(
    folder: &Path,
    found_by: LocationOrigin,
) -> Result<Option<WorkspaceLocation>, LocationError> {
    let folder = folder.canonicalize().map_err(|e| error(folder, e))?;
    if !folder.is_dir() {
        return Err(error(
            &folder,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "client and start paths must be folders",
            ),
        ));
    }
    for parent in folder.ancestors() {
        let file = parent.join("gazette.toml");
        match fs::metadata(&file) {
            Ok(_) => return selected(&file, found_by).map(Some),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(&file, e)),
        }
        // A client repository does not inherit another project's workspace.
        match fs::symlink_metadata(parent.join(".git")) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(parent, e)),
        }
    }
    Ok(None)
}

/// Look for an explicit request, client workspace, start workspace, then saved default.
/// This reads directory metadata only; loading and checking the selected configuration
/// remain separate operations. An unreadable or invalid selection is never skipped.
///
/// # Errors
/// Reports the path that failed. A valid search with no workspace returns `None`.
pub fn locate_workspace(
    input: WorkspaceLookup<'_>,
) -> Result<Option<WorkspaceLocation>, LocationError> {
    if !input.start.is_absolute() {
        return Err(error(
            input.start,
            io::Error::new(io::ErrorKind::InvalidInput, "start folder must be absolute"),
        ));
    }
    if let Some(path) = input.request {
        return selected(&absolute(path, input.start), LocationOrigin::Request).map(Some);
    }
    if let Some(path) = input.client
        && let Some(location) = near(&absolute(path, input.start), LocationOrigin::Client)?
    {
        return Ok(Some(location));
    }
    if let Some(location) = near(input.start, LocationOrigin::WorkingDirectory)? {
        return Ok(Some(location));
    }
    input
        .default
        .map(|path| selected(&absolute(path, input.start), LocationOrigin::UserDefault))
        .transpose()
}
