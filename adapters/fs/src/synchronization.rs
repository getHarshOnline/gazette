//! Existing private output flushing; no creation, repair or cleanup ownership.

use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use gazette_core::{
    release::{WorktreeOutputSyncOptions, WorktreeSyncError},
    source::{EntryPath, Error, Inventory, InventoryBuilder, Limits},
};
use gazette_ports::release::WorktreeOutputSync;
use rustix::fs::{self, Mode, OFlags};

use super::{
    FileSystem, ReadProfile, Selection, changed,
    destination::{DestinationRoots, open_metadata},
    io_error,
    stage::parent_and_name,
    visit,
};

pub(super) fn validate_entry(
    metadata: &Metadata,
    path: &Path,
    identity: (u64, u64),
    directory: bool,
) -> Result<(), Error> {
    let expected_mode = if directory { 0o700 } else { 0o600 };
    if (metadata.dev(), metadata.ino()) != identity
        || metadata.mode() & 0o7777 != expected_mode
        || (directory && !metadata.is_dir())
        || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
    {
        return Err(Error::Changed(path.to_owned()));
    }
    Ok(())
}

pub(super) fn synchronize_entry(
    file: &File,
    path: &Path,
    identity: (u64, u64),
    directory: bool,
) -> Result<(), Error> {
    let metadata = file
        .metadata()
        .map_err(|e| io_error("inspect synchronization handle", path, e))?;
    validate_entry(&metadata, path, identity, directory)?;
    file.sync_all()
        .map_err(|e| io_error("synchronize private entry", path, e))
}

impl WorktreeOutputSync for FileSystem {
    fn synchronize_worktree_output(
        &self,
        root: &Path,
        expected: &Inventory,
        protected_roots: &[PathBuf],
        options: WorktreeOutputSyncOptions,
    ) -> Result<Inventory, WorktreeSyncError> {
        if !options.allow_synchronize {
            return Err(WorktreeSyncError::refused(
                "output synchronization requires explicit permission",
            ));
        }
        let mut attempted = false;
        synchronize(root, expected, protected_roots, options.limits, &mut attempted).map_err(|_| WorktreeSyncError {
            reason: "output synchronization could not verify exact private bytes and stable native identities",
            attempted,
        })
    }
}

fn metadata(file: &File, path: &Path) -> Result<Metadata, Error> {
    file.metadata()
        .map_err(|e| io_error("inspect synchronization identity", path, e))
}

fn stable(file: &File, path: &Path, before: &Metadata) -> Result<(), Error> {
    let after = metadata(file, path)?;
    validate_entry(&after, path, (before.dev(), before.ino()), before.is_dir())?;
    if changed(before, &after) {
        return Err(Error::Changed(path.to_owned()));
    }
    Ok(())
}

fn observe(
    root: &File,
    limits: Limits,
) -> Result<(Inventory, BTreeMap<EntryPath, Metadata>), Error> {
    let mut inventory = InventoryBuilder::new(limits);
    let mut observed = BTreeMap::new();
    visit(
        root,
        "",
        &mut inventory,
        None,
        ReadProfile::Worktree,
        Some(&mut observed),
        Selection::Excluding(&[]),
    )?;
    Ok((inventory.finish(), observed))
}

fn open_entry(root: &File, path: &EntryPath, directory: bool) -> Result<File, Error> {
    let (parent, name) = parent_and_name(root, path)?;
    let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    if directory {
        flags |= OFlags::DIRECTORY;
    }
    fs::openat(&parent, name, flags, Mode::empty())
        .map(File::from)
        .map_err(|e| {
            io_error(
                "open output synchronization entry",
                Path::new(path.as_str()),
                e,
            )
        })
}

fn synchronize(
    root: &Path,
    expected: &Inventory,
    protected: &[PathBuf],
    limits: Limits,
    attempted: &mut bool,
) -> Result<Inventory, Error> {
    let roots =
        DestinationRoots::observe(root, protected).map_err(|_| Error::Changed(root.to_owned()))?;
    let root_file = roots.directory();
    let root_before = metadata(root_file, root)?;
    validate_entry(
        &root_before,
        root,
        (root_before.dev(), root_before.ino()),
        true,
    )?;
    let parent = File::from(
        fs::openat(
            root_file,
            "..",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| io_error("hold output parent", root, e))?,
    );
    let parent_before = metadata(&parent, root)?;
    validate_entry(
        &parent_before,
        root,
        (parent_before.dev(), parent_before.ino()),
        true,
    )?;
    let git = open_metadata(root_file)?;
    let git_before = metadata(&git, Path::new(".git"))?;
    let (before, nodes) = observe(root_file, limits)?;
    if &before != expected {
        return Err(Error::ContentMismatch(root.to_owned()));
    }
    roots.recheck()?;
    stable(root_file, root, &root_before)?;
    stable(&parent, root, &parent_before)?;
    // Reverse lexical inventory order places every descendant before its parent.
    for (path, original) in nodes.iter().rev() {
        roots.recheck()?;
        let path_ref = Path::new(path.as_str());
        let file = open_entry(root_file, path, original.is_dir())?;
        stable(&file, path_ref, original)?;
        *attempted = true;
        synchronize_entry(
            &file,
            path_ref,
            (original.dev(), original.ino()),
            original.is_dir(),
        )?;
        stable(&file, path_ref, original)?;
        stable(
            &open_entry(root_file, path, original.is_dir())?,
            path_ref,
            original,
        )?;
    }
    roots.recheck()?;
    stable(root_file, root, &root_before)?;
    *attempted = true;
    synchronize_entry(
        root_file,
        root,
        (root_before.dev(), root_before.ino()),
        true,
    )?;
    stable(&parent, root, &parent_before)?;
    synchronize_entry(
        &parent,
        root,
        (parent_before.dev(), parent_before.ino()),
        true,
    )?;
    let (after, current) = observe(root_file, limits)?;
    if &after != expected || current.len() != nodes.len() {
        return Err(Error::ContentMismatch(root.to_owned()));
    }
    for (path, original) in &nodes {
        let observed = current
            .get(path)
            .ok_or_else(|| Error::Changed(path.as_str().into()))?;
        if changed(original, observed) {
            return Err(Error::Changed(path.as_str().into()));
        }
    }
    let git_after = metadata(&open_metadata(root_file)?, Path::new(".git"))?;
    if changed(&git_before, &git_after) {
        return Err(Error::Changed(".git".into()));
    }
    stable(root_file, root, &root_before)?;
    stable(&parent, root, &parent_before)?;
    roots.recheck()?;
    Ok(after)
}
