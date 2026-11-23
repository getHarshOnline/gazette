//! Shared bounded local namespace checks for repository setup, not adoption policy.

use super::Result;
use gazette_core::{release::ParentSource, source::MetadataBudget};
use std::{
    fs::{self, File},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

pub(super) fn coordinate(path: &Path) -> Result<PathBuf> {
    ParentSource::Bare {
        path: path.to_owned(),
    }
    .validate()
    .map_err(|e| e.reason)?;
    let parent = path.parent().ok_or("repository root has no parent")?;
    let parent = fs::canonicalize(parent).map_err(|_| "repository parent must already exist")?;
    if !fs::symlink_metadata(&parent)
        .map_err(|_| "repository parent unavailable")?
        .is_dir()
    {
        return Err("repository parent is not a directory");
    }
    Ok(parent.join(path.file_name().ok_or("repository name is missing")?))
}

pub(super) fn separate(root: &Path, protected: &[PathBuf]) -> Result<()> {
    for selected in protected {
        let other = fs::canonicalize(selected)
            .or_else(|_| coordinate(selected).map_err(std::io::Error::other))
            .map_err(|_| "protected repository coordinate cannot be resolved")?;
        if root.starts_with(&other) || other.starts_with(root) {
            return Err("repository setup overlaps protected source, journal or output");
        }
    }
    Ok(())
}

pub(super) fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("repository setup requires an absent selected entry"),
    }
}

pub(super) struct Directory {
    pub(super) path: PathBuf,
    pub(super) identity: (u64, u64),
    held: File,
}
impl Directory {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "repository directory is missing")?;
        if !metadata.is_dir() {
            return Err("repository requires a real directory");
        }
        let held = File::open(path).map_err(|_| "repository directory cannot be held")?;
        let value = Self {
            path: path.to_owned(),
            identity: (metadata.dev(), metadata.ino()),
            held,
        };
        value.recheck()?;
        Ok(value)
    }
    pub(super) fn create(path: &Path) -> Result<Self> {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|_| "repository directory creation failed; preserve selected path")?;
        Self::open(path)
    }
    pub(super) fn recheck(&self) -> Result<()> {
        for metadata in [self.held.metadata(), fs::symlink_metadata(&self.path)] {
            let metadata = metadata.map_err(|_| "repository directory identity changed")?;
            if !metadata.is_dir() || (metadata.dev(), metadata.ino()) != self.identity {
                return Err("repository directory identity changed");
            }
        }
        Ok(())
    }
    pub(super) fn synchronize(&self) -> Result<()> {
        self.recheck()?;
        self.held
            .sync_all()
            .map_err(|_| "repository directory synchronization failed")?;
        File::open(self.path.parent().ok_or("repository parent missing")?)
            .and_then(|file| file.sync_all())
            .map_err(|_| "repository parent synchronization failed")?;
        self.recheck()
    }
}

pub(super) fn read(path: &Path, budget: &mut MetadataBudget) -> Result<Vec<u8>> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "repository metadata file unavailable")?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("repository metadata must be a private regular file");
    }
    let size = usize::try_from(metadata.len()).map_err(|_| "repository metadata too large")?;
    budget
        .charge(size)
        .map_err(|_| "repository metadata ceiling exceeded")?;
    let bytes = fs::read(path).map_err(|_| "repository metadata cannot be read")?;
    if bytes.len() != size {
        return Err("repository metadata changed during observation");
    }
    Ok(bytes)
}
