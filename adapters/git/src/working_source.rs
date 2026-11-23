//! Git owns ignore semantics; filesystem capture still owns confined file reads.

use std::{cell::Cell, collections::BTreeSet};

use gazette_core::source::RevisionLimits;
use gazette_ports::source::SourceFilter;

use super::*;

pub(super) fn filter(
    root: &Path,
    subdirectory: Option<&Subdirectory>,
    limits: RevisionLimits,
) -> Result<Option<Box<dyn SourceFilter>>, Error> {
    let metadata = std::fs::symlink_metadata(root)
        .map_err(|e| native_error("inspect working source root", root, e))?;
    if !metadata.is_dir() {
        return Err(Error::Unsupported(root.to_owned()));
    }
    match std::fs::symlink_metadata(root.join(".git")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(native_error("inspect Git entry", root, error)),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::Unsupported(root.join(".git")));
        }
        Ok(_) => {}
    }
    let repo = open_worktree(root)?;
    let index = repo
        .index()
        .map_err(|e| git_error("read tracked source paths", root, e))?;
    let mut remaining = MetadataBudget::new(limits.metadata_bytes);
    let prefix = subdirectory.map_or_else(String::new, |path| format!("{path}/"));
    let mut tracked = BTreeSet::new();
    for entry in index.iter() {
        remaining
            .charge(entry.path.len())
            .map_err(|_| exceeded(root, "tracked path bytes"))?;
        let path = std::str::from_utf8(&entry.path)
            .map_err(|_| invalid(root, "tracked path is not UTF-8"))?;
        if !path.starts_with(&prefix) {
            continue;
        }
        let mut path = path;
        loop {
            tracked.insert(path.to_owned());
            let Some((parent, _)) = path.rsplit_once('/') else {
                break;
            };
            if !parent.starts_with(&prefix) {
                break;
            }
            remaining
                .charge(parent.len())
                .map_err(|_| exceeded(root, "tracked path bytes"))?;
            path = parent;
        }
    }
    Ok(Some(Box::new(WorkingFilter {
        repo,
        root: root.to_owned(),
        prefix,
        tracked,
        path_bytes: Cell::new(remaining.remaining()),
    })))
}

struct WorkingFilter {
    repo: Repository,
    root: PathBuf,
    prefix: String,
    tracked: BTreeSet<String>,
    path_bytes: Cell<usize>,
}

impl SourceFilter for WorkingFilter {
    fn allows(&self, path: &EntryPath) -> Result<bool, Error> {
        let relative = format!("{}{path}", self.prefix);
        // Every nonempty path lookup charges metadata work. Captured-entry bounds
        // belong to the native inventory, not the repository index or repeat visits.
        self.path_bytes.set(
            self.path_bytes
                .get()
                .checked_sub(relative.len())
                .ok_or_else(|| exceeded(&self.root, "working path bytes"))?,
        );
        if relative == ".git" {
            return Ok(false);
        }
        // An ignored directory can contain a deliberately tracked input. Keep its
        // ancestors, then apply ignore rules separately to each untracked child.
        if self.tracked.contains(&relative) {
            return Ok(true);
        }
        self.repo
            .status_should_ignore(Path::new(&relative))
            .map(|ignored| !ignored)
            .map_err(|e| git_error("resolve working source ignore rules", &self.root, e))
    }
}

fn exceeded(root: &Path, resource: &'static str) -> Error {
    Error::Limit {
        path: root.to_owned(),
        resource,
    }
}
