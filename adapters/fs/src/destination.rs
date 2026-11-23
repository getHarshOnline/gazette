//! Native root separation and bounded observation through held directory handles.

use std::collections::BTreeSet;
use std::fs::File;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Component, Path, PathBuf};

use gazette_core::destination::{
    Error, OutputAdministrationBinding, OutputBinding, OutputIdentity, OutputObservation,
};
use gazette_core::source::{Error as SourceError, Inventory, Limits};
use gazette_ports::destination::{DestinationReader, OutputReader};
use gazette_ports::release::WorktreeReader;
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};

use super::{
    FileSystem, ReadProfile, changed, io_error, open_directory, read_directory,
    read_directory_profile,
};

impl WorktreeReader for FileSystem {
    fn read_worktree(
        &self,
        root: &Path,
        protected_roots: &[PathBuf],
        limits: Limits,
    ) -> Result<Inventory, Error> {
        let roots = DestinationRoots::observe(root, protected_roots)?;
        let metadata_directory = open_metadata(roots.directory())?;
        let before = metadata_directory
            .metadata()
            .map_err(|e| io_error("inspect worktree metadata directory", Path::new(".git"), e))?;
        let inventory =
            read_directory_profile(roots.directory(), limits, None, ReadProfile::Worktree)?;
        let reopened = open_metadata(roots.directory())?
            .metadata()
            .map_err(|e| io_error("recheck worktree metadata directory", Path::new(".git"), e))?;
        if changed(&before, &reopened) {
            return Err(SourceError::Changed(PathBuf::from(".git")).into());
        }
        roots.recheck()?;
        Ok(inventory)
    }
}

pub(super) fn open_metadata(root: &File) -> Result<File, SourceError> {
    fs::openat(
        root,
        ".git",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| io_error("open independent worktree metadata", Path::new(".git"), e))
}

impl DestinationReader for FileSystem {
    fn read_destination(
        &self,
        root: &Path,
        protected_roots: &[PathBuf],
        limits: Limits,
    ) -> Result<Inventory, Error> {
        let roots = DestinationRoots::observe(root, protected_roots)?;
        let inventory = read_directory(roots.directory(), limits, None)?;
        roots.recheck()?;
        Ok(inventory)
    }
}

impl OutputReader for FileSystem {
    fn read_output(
        &self,
        root: &Path,
        protected_roots: &[PathBuf],
        limits: Limits,
    ) -> Result<OutputObservation, Error> {
        let roots = DestinationRoots::observe_output(root, protected_roots)?;
        let administration = OutputAdministration::observe(roots.directory())?;
        let binding = roots.binding(&administration)?;
        let inventory =
            read_directory_profile(roots.directory(), limits, None, ReadProfile::Output)?;
        administration.recheck(roots.directory())?;
        roots.recheck()?;
        Ok(OutputObservation::new(inventory, binding))
    }
}

/// The administrative entry is protected by identity, not interpreted as a repository.
pub(super) struct OutputAdministration(Option<(File, std::fs::Metadata)>);

impl OutputAdministration {
    fn binding(&self) -> Option<OutputAdministrationBinding> {
        self.0
            .as_ref()
            .map(|(_, metadata)| administration_binding(metadata))
    }

    pub(super) fn observe(root: &File) -> Result<Self, SourceError> {
        let declared = match fs::statat(root, ".git", AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => metadata,
            Err(rustix::io::Errno::NOENT) => return Ok(Self(None)),
            Err(error) => {
                return Err(io_error(
                    "inspect output administration",
                    Path::new(".git"),
                    error,
                ));
            }
        };
        let kind = FileType::from_raw_mode(declared.st_mode);
        let directory = match kind {
            FileType::Directory => OFlags::DIRECTORY,
            FileType::RegularFile => OFlags::empty(),
            _ => return Err(SourceError::Unsupported(".git".into())),
        };
        let file = File::from(
            fs::openat(
                root,
                ".git",
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC | directory,
                Mode::empty(),
            )
            .map_err(|error| io_error("hold output administration", Path::new(".git"), error))?,
        );
        let metadata = file
            .metadata()
            .map_err(|error| io_error("inspect held administration", Path::new(".git"), error))?;
        if metadata.dev() != declared.st_dev as u64 || metadata.ino() != declared.st_ino as u64 {
            return Err(SourceError::Changed(".git".into()));
        }
        Ok(Self(Some((file, metadata))))
    }

    pub(super) fn recheck(&self, root: &File) -> Result<(), SourceError> {
        let current = Self::observe(root)?;
        match (&self.0, &current.0) {
            (None, None) => Ok(()),
            (Some((held, before)), Some((_, after))) => {
                let held_after = held.metadata().map_err(|error| {
                    io_error("recheck held administration", Path::new(".git"), error)
                })?;
                if administration_binding(before) != administration_binding(after)
                    || administration_binding(before) != administration_binding(&held_after)
                    || (before.is_file()
                        && (changed(before, after) || changed(before, &held_after)))
                {
                    Err(SourceError::Changed(".git".into()))
                } else {
                    Ok(())
                }
            }
            _ => Err(SourceError::Changed(".git".into())),
        }
    }
}

fn administration_binding(metadata: &std::fs::Metadata) -> OutputAdministrationBinding {
    let identity = OutputIdentity::new(metadata.dev(), metadata.ino());
    if metadata.is_dir() {
        OutputAdministrationBinding::Directory(identity)
    } else {
        OutputAdministrationBinding::File(identity)
    }
}

pub(super) struct DestinationRoots {
    selected: PathBuf,
    target: Root,
    sources: Vec<(PathBuf, Root)>,
}

impl DestinationRoots {
    pub(super) fn binding(
        &self,
        administration: &OutputAdministration,
    ) -> Result<OutputBinding, Error> {
        OutputBinding::new(
            self.target.canonical.clone(),
            OutputIdentity::new(self.target.identity.0, self.target.identity.1),
            administration.binding(),
        )
    }

    /// Output alone may not select the named Git administration namespace.
    /// Source capture and worktree operations retain their independent contracts.
    pub(super) fn observe_output(root: &Path, protected: &[PathBuf]) -> Result<Self, Error> {
        check_output_namespace(root)?;
        let observed = Self::observe(root, protected)?;
        // An ordinary spelling can enter `.git` through an ancestor alias.
        // Root observation has already bound this canonical path to the held inode.
        check_output_namespace(observed.canonical())?;
        Ok(observed)
    }

    pub(super) fn observe(root: &Path, protected: &[PathBuf]) -> Result<Self, Error> {
        let target = Root::observe(root)?;
        let mut sources = Vec::with_capacity(protected.len());
        for path in protected {
            let source = Root::observe(path)?;
            if source.ancestors.contains(&target.identity)
                || target.ancestors.contains(&source.identity)
            {
                return Err(Error::SourceOverlap {
                    source: path.clone(),
                    destination: root.to_owned(),
                });
            }
            sources.push((path.clone(), source));
        }
        Ok(Self {
            selected: root.to_owned(),
            target,
            sources,
        })
    }

    pub(super) fn directory(&self) -> &File {
        &self.target.directory
    }
    pub(super) fn canonical(&self) -> &Path {
        &self.target.canonical
    }
    pub(super) fn recheck_target(&self) -> Result<(), SourceError> {
        self.target.recheck(&self.selected)
    }
    pub(super) fn recheck(&self) -> Result<(), SourceError> {
        self.recheck_target()?;
        for (path, root) in &self.sources {
            root.recheck(path)?;
        }
        Ok(())
    }
}

fn check_output_namespace(path: &Path) -> Result<(), SourceError> {
    if path.components().any(|component| {
        matches!(component, Component::Normal(name) if name.as_bytes().eq_ignore_ascii_case(b".git"))
    }) {
        return Err(SourceError::InvalidInventory {
            path: path.to_owned(),
            reason: "configured output must not be inside the Git administrative namespace",
        });
    }
    Ok(())
}

type Identity = (u64, u64);

struct Root {
    directory: File,
    identity: Identity,
    canonical: PathBuf,
    ancestors: BTreeSet<Identity>,
}

impl Root {
    fn observe(path: &Path) -> Result<Self, SourceError> {
        let directory = open_directory(path)?;
        let identity = identity(&directory, path)?;
        let canonical = path
            .canonicalize()
            .map_err(|e| io_error("resolve root", path, e))?;
        if identity != self::identity(&open_directory(&canonical)?, path)? {
            return Err(SourceError::Changed(path.to_owned()));
        }
        let ancestors = ancestry(&directory, &canonical)?;
        Ok(Self {
            directory,
            identity,
            canonical,
            ancestors,
        })
    }

    fn recheck(&self, path: &Path) -> Result<(), SourceError> {
        let current = Self::observe(path)?;
        if self.identity != current.identity
            || self.canonical != current.canonical
            || self.ancestors != current.ancestors
        {
            return Err(SourceError::Changed(path.to_owned()));
        }
        Ok(())
    }
}

fn identity(directory: &File, path: &Path) -> Result<Identity, SourceError> {
    let metadata = directory
        .metadata()
        .map_err(|e| io_error("inspect root identity", path, e))?;
    Ok((metadata.dev(), metadata.ino()))
}

fn ancestry(directory: &File, canonical: &Path) -> Result<BTreeSet<Identity>, SourceError> {
    let mut current = directory
        .try_clone()
        .map_err(|e| io_error("hold directory", canonical, e))?;
    let mut ancestors = BTreeSet::new();
    // A stable canonical absolute path bounds its native ancestry walk. A
    // moving hierarchy must fail, not keep a read-only check looping forever.
    for _ in 0..=canonical.components().count() {
        let child = identity(&current, canonical)?;
        if !ancestors.insert(child) {
            return Err(SourceError::Changed(canonical.to_owned()));
        }
        let parent = File::from(
            fs::openat(
                &current,
                "..",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| io_error("inspect root ancestry", canonical, e))?,
        );
        if identity(&parent, canonical)? == child {
            return Ok(ancestors);
        }
        current = parent;
    }
    Err(SourceError::Changed(canonical.to_owned()))
}

#[cfg(test)]
#[path = "../tests/internal/destination.rs"]
mod tests;
