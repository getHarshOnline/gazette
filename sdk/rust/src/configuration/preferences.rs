//! User preferences shared by local command-line and protocol hosts.

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Map, Value};

/// Resolve request and local folders before reading the user's optional default.
///
/// # Errors
/// Reports an invalid selection or user configuration without skipping either.
pub fn locate_user_workspace(
    request: Option<&Path>,
    client: Option<&Path>,
    start: &Path,
) -> io::Result<Option<super::WorkspaceLocation>> {
    let local = super::locate_workspace(super::WorkspaceLookup {
        request,
        client,
        start,
        default: None,
    })
    .map_err(io::Error::other)?;
    if local.is_some() {
        return Ok(local);
    }
    let default = default_workspace(&user_configuration_path()?)?;
    super::locate_workspace(super::WorkspaceLookup {
        request,
        client,
        start,
        default: default.as_deref(),
    })
    .map_err(io::Error::other)
}

/// Locate Gazette's user configuration without reading or creating it.
///
/// # Errors
/// Returns an error when neither an absolute XDG configuration folder nor HOME exists.
pub fn user_configuration_path() -> io::Result<PathBuf> {
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "set HOME or XDG_CONFIG_HOME to locate Gazette's configuration",
            )
        })?;
    Ok(directory.join("gazette/config.json"))
}

fn read(path: &Path) -> io::Result<Map<String, Value>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Gazette user configuration exceeds 64 KiB",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Read the saved default; an absent file or setting means there is no default.
///
/// # Errors
/// Rejects malformed configuration and non-absolute saved paths.
pub fn default_workspace(path: &Path) -> io::Result<Option<PathBuf>> {
    read(path)?
        .get("workspace")
        .map(|value| {
            let path = value
                .as_str()
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "workspace must be an absolute path",
                    )
                })?;
            Ok(path)
        })
        .transpose()
}

/// Save a selected workspace, preserving other user preferences.
/// The caller chooses the workspace; finding a local project never sets a default.
///
/// # Errors
/// Reports invalid paths, malformed existing preferences and write failures.
pub fn set_default_workspace(path: &Path, workspace: &Path) -> io::Result<()> {
    if !workspace.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace must be an absolute path",
        ));
    }
    // Keep a user's dotfile link in place when replacing the complete JSON file.
    let path = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => path.canonicalize()?,
        Ok(_) => path.to_owned(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_owned(),
        Err(error) => return Err(error),
    };
    let mut settings = read(&path)?;
    settings.insert("workspace".into(), serde_json::to_value(workspace)?);
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration needs a parent folder",
        )
    })?;
    let mut bytes = serde_json::to_vec_pretty(&settings)?;
    bytes.push(b'\n');
    if bytes.len() > 65_536 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Gazette user configuration exceeds 64 KiB",
        ));
    }
    fs::create_dir_all(parent)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (temporary, mut file) = loop {
        let temporary = parent.join(format!(
            ".gazette-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::File::create_new(&temporary) {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        if let Ok(metadata) = fs::metadata(&path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
