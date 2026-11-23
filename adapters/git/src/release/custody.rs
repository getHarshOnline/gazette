//! Immutable local refs plus bounded native synchronization, not release authority.

use super::{Git, Result, SelectedRepository, inspect, signatures};
use gazette_core::{
    release::{
        CommitMetadata, CustodyEffect, CustodyError, CustodyOptions, CustodyReport,
        custody_reference,
    },
    source::{CommitId, MetadataBudget, RevisionLimits, Sha256Digest, TreeIdentity},
};
use gazette_ports::release::ReleaseCustody;
use git2::{ErrorCode, Oid, ReferenceType, Repository};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

impl ReleaseCustody for Git {
    fn retain_commit(
        &self,
        root: &Path,
        id: Sha256Digest,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        options: CustodyOptions,
    ) -> std::result::Result<CustodyReport, CustodyError> {
        if !options.allow_retain {
            return Err(CustodyError::refused(
                "explicit local custody permission is required",
            ));
        }
        let mut effect = CustodyEffect::NotAttempted;
        (|| {
            let mut budget = MetadataBudget::new(options.limits.metadata_bytes);
            signatures(metadata, &mut budget)?;
            let selected = SelectedRepository::open(root, &mut budget)?;
            let reference = custody_reference(id);
            let oid = Oid::from_str(commit.as_str()).map_err(|_| "invalid custody commit")?;
            let exists = reference_exists(&selected.repo, &reference, oid)?;
            let verified = inspect(
                &selected.repo,
                commit,
                output,
                metadata,
                options.limits,
                &mut budget,
            )?;
            // Refuse already excessive storage before the first possible ref write.
            snapshot(&selected.root, options.storage_bytes, &mut budget)?;
            selected.recheck()?;
            if !exists {
                check_creation(&selected.repo, &reference, &mut budget)?;
                effect = CustodyEffect::ReferenceMayRemain;
                selected
                    .repo
                    .reference(&reference, oid, false, "Gazette local object custody")
                    .map_err(
                        |_| "custody reference creation failed; never replace existing state",
                    )?;
            }
            require_reference(&selected.repo, &reference, oid)?;
            synchronize(&selected.root, options.storage_bytes, &mut budget)?;
            selected.recheck()?;
            let reopened = SelectedRepository::open(root, &mut budget)?;
            require_reference(&reopened.repo, &reference, oid)?;
            let observed = inspect(
                &reopened.repo,
                commit,
                output,
                metadata,
                options.limits,
                &mut budget,
            )?;
            if observed != verified {
                return Err("custody object evidence changed during retention");
            }
            require_reference(&reopened.repo, &reference, oid)?;
            reopened.recheck()?;
            selected.recheck()?;
            Ok(CustodyReport {
                id,
                commit: observed,
            })
        })()
        .map_err(|reason| CustodyError { reason, effect })
    }

    fn inspect_custody(
        &self,
        root: &Path,
        id: Sha256Digest,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: RevisionLimits,
    ) -> std::result::Result<CustodyReport, CustodyError> {
        (|| {
            let mut budget = MetadataBudget::new(limits.metadata_bytes);
            signatures(metadata, &mut budget)?;
            let selected = SelectedRepository::open(root, &mut budget)?;
            let reference = custody_reference(id);
            let oid = Oid::from_str(commit.as_str()).map_err(|_| "invalid custody commit")?;
            require_reference(&selected.repo, &reference, oid)?;
            let report = inspect(
                &selected.repo,
                commit,
                output,
                metadata,
                limits,
                &mut budget,
            )?;
            require_reference(&selected.repo, &reference, oid)?;
            selected.recheck()?;
            Ok(CustodyReport { id, commit: report })
        })()
        .map_err(CustodyError::refused)
    }
}

fn check_creation(repository: &Repository, name: &str, budget: &mut MetadataBudget) -> Result<()> {
    // Native reference creation obtains an ambient reflog signature even when
    // unused. Refuse every mode that could persist it; never rewrite config/logs.
    let configuration = repository
        .config()
        .map_err(|_| "custody configuration cannot be inspected")?;
    let logging = match configuration.get_bool("core.logallrefupdates") {
        Ok(value) => value,
        Err(error) if error.code() == ErrorCode::NotFound => false, // Verified bare repository.
        Err(_) => return Err("custody reflog mode is invalid or always; creation is refused"),
    };
    if logging
        && repository
            .reference_has_log(name)
            .map_err(|_| "custody reflog cannot be inspected")?
    {
        return Err("custody creation would update an existing reflog");
    }

    // A custody ref must not make an unborn HEAD/branch chain acquire a value.
    // Follow public native lookups, retaining bounded cycle evidence only.
    let mut current = "HEAD".to_owned();
    let mut seen = BTreeSet::new();
    loop {
        budget
            .charge(current.len().saturating_add(128))
            .map_err(|_| "custody HEAD chain metadata ceiling exceeded")?;
        if current == name {
            return Err("HEAD depends on the selected custody reference");
        }
        if !seen.insert(current.clone()) {
            return Err("custody HEAD chain is cyclic");
        }
        let reference = match repository.find_reference(&current) {
            Ok(reference) => reference,
            Err(error) if error.code() == ErrorCode::NotFound && current != "HEAD" => return Ok(()),
            Err(_) => return Err("custody HEAD chain cannot be inspected"),
        };
        match reference.kind() {
            Some(ReferenceType::Direct) => return Ok(()),
            Some(ReferenceType::Symbolic) => {
                let target = reference
                    .symbolic_target()
                    .map_err(|_| "custody HEAD target is not valid text")?
                    .ok_or("custody HEAD target is missing")?;
                budget
                    .charge(target.len())
                    .map_err(|_| "custody HEAD chain metadata ceiling exceeded")?;
                current = target.to_owned();
            }
            None => return Err("custody HEAD reference has an unsupported type"),
        }
    }
}

fn reference_exists(repository: &Repository, name: &str, expected: Oid) -> Result<bool> {
    match repository.find_reference(name) {
        Ok(reference)
            if reference.kind() == Some(ReferenceType::Direct)
                && reference.target() == Some(expected) =>
        {
            Ok(true)
        }
        Ok(_) => Err("custody reference is symbolic or binds a different commit"),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(false),
        Err(_) => Err("custody reference cannot be inspected"),
    }
}

fn require_reference(repository: &Repository, name: &str, expected: Oid) -> Result<()> {
    if reference_exists(repository, name, expected)? {
        Ok(())
    } else {
        Err("custody reference is missing")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    directory: bool,
    device: u64,
    inode: u64,
    bytes: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Stamp {
    fn from_metadata(metadata: &Metadata) -> Result<Self> {
        if !metadata.is_dir() && (!metadata.is_file() || metadata.nlink() != 1) {
            return Err("custody storage contains shared or special files");
        }
        Ok(Self {
            directory: metadata.is_dir(),
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
            mode: metadata.mode(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }

    fn read(path: &Path) -> Result<Self> {
        Self::from_metadata(
            &fs::symlink_metadata(path).map_err(|_| "custody storage cannot be inspected")?,
        )
    }
}

fn snapshot(
    root: &Path,
    maximum: u64,
    budget: &mut MetadataBudget,
) -> Result<BTreeMap<PathBuf, Stamp>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    let mut bytes = 0u64;
    while let Some(path) = pending.pop() {
        budget
            .charge(path.as_os_str().len().saturating_add(128))
            .map_err(|_| "custody namespace metadata ceiling exceeded")?;
        let stamp = Stamp::read(&path)?;
        if stamp.directory {
            for child in fs::read_dir(&path).map_err(|_| "custody directory cannot be inspected")? {
                let child = child.map_err(|_| "custody directory changed")?;
                // Bound queued paths before retaining them, including wide directories.
                budget
                    .charge(
                        child
                            .file_name()
                            .len()
                            .saturating_add(path.as_os_str().len())
                            .saturating_add(128),
                    )
                    .map_err(|_| "custody namespace metadata ceiling exceeded")?;
                pending.push(child.path());
            }
        } else {
            bytes = bytes
                .checked_add(stamp.bytes)
                .filter(|&bytes| bytes <= maximum)
                .ok_or("custody storage byte ceiling exceeded")?;
        }
        entries.insert(path, stamp);
    }
    Ok(entries)
}

pub(crate) fn inspect_storage(
    root: &Path,
    maximum: u64,
    budget: &mut MetadataBudget,
) -> Result<()> {
    snapshot(root, maximum, budget).map(|_| ())
}

pub(crate) fn synchronize(root: &Path, maximum: u64, budget: &mut MetadataBudget) -> Result<()> {
    let before = snapshot(root, maximum, budget)?;
    let parent = root.parent().ok_or("custody repository has no parent")?;
    let parent_stamp = Stamp::read(parent)?;
    // Data first; reverse lexical path order puts descendants before their ancestors.
    for (path, stamp) in before
        .iter()
        .filter(|(_, stamp)| !stamp.directory)
        .chain(before.iter().rev().filter(|(_, stamp)| stamp.directory))
    {
        synchronize_entry(path, stamp)?;
    }
    synchronize_entry(parent, &parent_stamp)?;
    if snapshot(root, maximum, budget)? != before {
        return Err("custody namespace changed during synchronization");
    }
    Ok(())
}

fn synchronize_entry(path: &Path, expected: &Stamp) -> Result<()> {
    if Stamp::read(path)? != *expected {
        return Err("custody entry identity changed");
    }
    let file =
        File::open(path).map_err(|_| "custody entry cannot be opened for synchronization")?;
    let opened = Stamp::from_metadata(
        &file
            .metadata()
            .map_err(|_| "custody opened entry cannot be inspected")?,
    )?;
    if opened != *expected || Stamp::read(path)? != *expected {
        return Err("custody opened entry does not match its native identity");
    }
    file.sync_all()
        .map_err(|_| "custody entry synchronization failed")?;
    if Stamp::read(path)? != *expected {
        return Err("custody entry changed during synchronization");
    }
    Ok(())
}
