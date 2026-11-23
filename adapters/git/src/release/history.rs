//! Bounded native reachable objects, never repository copying or checkout.

use super::{Objects, Result, SelectedRepository, custody};
use gazette_core::{
    release::{HistoryLimits, ParentSource},
    source::{CommitId, MetadataBudget},
};
use git2::{ObjectType, Oid, Repository};
use std::{collections::BTreeMap, fs, path::Path};

struct Object {
    id: Oid,
    kind: ObjectType,
    bytes: Vec<u8>,
}

/// Exact retained bytes sever the source dependency before destination effects.
pub(crate) struct Capture(Vec<Object>);

impl Capture {
    pub(crate) fn write(self, repository: &Repository) -> Result<()> {
        let odb = repository
            .odb()
            .map_err(|_| "destination object storage unavailable")?;
        for object in self.0 {
            let written = odb
                .write(object.kind, &object.bytes)
                .map_err(|_| "offline history object write failed")?;
            if written != object.id {
                return Err("offline history write changed object identity");
            }
        }
        Ok(())
    }
}

pub(crate) fn capture(
    source: &ParentSource,
    destination: &Path,
    parent: &CommitId,
    limits: HistoryLimits,
    storage_bytes: u64,
    budget: &mut MetadataBudget,
) -> Result<Capture> {
    let destination =
        fs::canonicalize(destination).map_err(|_| "deployment destination cannot be resolved")?;
    capture_at(source, &destination, parent, limits, storage_bytes, budget)
}

pub(super) fn capture_for_setup(
    source: &ParentSource,
    destination: &Path,
    parent: &CommitId,
    limits: HistoryLimits,
    storage_bytes: u64,
    budget: &mut MetadataBudget,
) -> Result<Capture> {
    let destination = super::setup::coordinate(destination)?;
    capture_at(source, &destination, parent, limits, storage_bytes, budget)
}

fn capture_at(
    source: &ParentSource,
    destination: &Path,
    parent: &CommitId,
    limits: HistoryLimits,
    storage_bytes: u64,
    budget: &mut MetadataBudget,
) -> Result<Capture> {
    let selected = match source {
        ParentSource::Bare { path } => SelectedRepository::open(path, budget)?,
        ParentSource::Worktree { path } => SelectedRepository::open_worktree(path, budget)?,
    };
    let source_root = selected
        .workdir
        .as_ref()
        .map_or(selected.root.as_path(), |(path, _)| path.as_path());
    if destination.starts_with(source_root)
        || source_root.starts_with(destination)
        || destination.starts_with(&selected.root)
        || selected.root.starts_with(destination)
    {
        return Err("parent object source and deployment destination overlap");
    }
    custody::inspect_storage(&selected.root, storage_bytes, budget)?;
    let mut retained = Vec::new();
    walk(&selected.repo, parent, limits, budget, |id, kind, bytes| {
        retained.push(Object {
            id,
            kind,
            bytes: bytes.to_vec(),
        });
        Ok(())
    })?;
    custody::inspect_storage(&selected.root, storage_bytes, budget)?;
    selected.recheck()?;
    Ok(Capture(retained))
}

pub(super) fn verify(
    repository: &Repository,
    parent: &CommitId,
    limits: HistoryLimits,
    budget: &mut MetadataBudget,
) -> Result<()> {
    walk(repository, parent, limits, budget, |_, _, _| Ok(()))
}

fn enqueue(
    id: Oid,
    kind: ObjectType,
    pending: &mut Vec<(Oid, ObjectType)>,
    seen: &mut BTreeMap<Oid, ObjectType>,
    limits: HistoryLimits,
    budget: &mut MetadataBudget,
) -> Result<()> {
    if let Some(expected) = seen.get(&id) {
        return if *expected == kind {
            Ok(())
        } else {
            Err("history object has conflicting referenced types")
        };
    }
    if seen.len() >= limits.objects {
        return Err("history object count ceiling exceeded");
    }
    budget
        .charge(128)
        .map_err(|_| "history traversal metadata ceiling exceeded")?;
    seen.insert(id, kind);
    pending.push((id, kind));
    Ok(())
}

fn walk(
    repository: &Repository,
    parent: &CommitId,
    limits: HistoryLimits,
    budget: &mut MetadataBudget,
    mut consume: impl FnMut(Oid, ObjectType, &[u8]) -> Result<()>,
) -> Result<()> {
    let id = Oid::from_str(parent.as_str()).map_err(|_| "invalid exact parent coordinate")?;
    let mut pending = Vec::new();
    let mut seen = BTreeMap::new();
    enqueue(
        id,
        ObjectType::Commit,
        &mut pending,
        &mut seen,
        limits,
        budget,
    )?;
    let objects =
        Objects::new(repository, budget).map_err(|_| "parent object storage unavailable")?;
    let mut remaining = limits.bytes;
    while let Some((id, kind)) = pending.pop() {
        let (bytes, actual_kind) = objects
            .odb
            .read_header(id)
            .map_err(|_| "history object is missing or corrupt")?;
        if actual_kind != kind {
            return Err("history object has the wrong referenced type");
        }
        let ceiling = limits
            .object_bytes
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        if bytes > ceiling {
            return Err("history object or total decoded byte ceiling exceeded");
        }
        remaining = remaining
            .checked_sub(bytes as u64)
            .ok_or("history total decoded byte ceiling exceeded")?;
        if matches!(kind, ObjectType::Commit | ObjectType::Tree) {
            objects
                .remaining_metadata
                .charge(bytes)
                .map_err(|_| "history decoded metadata ceiling exceeded")?;
        }
        let object = objects
            .checked(id, kind, ceiling, Path::new("<history>"))
            .map_err(|_| "history object bytes are missing, corrupt or over-limit")?;
        consume(id, kind, object.data())?;
        drop(object);
        match kind {
            ObjectType::Commit => {
                let commit = repository
                    .find_commit(id)
                    .map_err(|_| "history commit cannot be decoded")?;
                enqueue(
                    commit.tree_id(),
                    ObjectType::Tree,
                    &mut pending,
                    &mut seen,
                    limits,
                    objects.remaining_metadata,
                )?;
                for parent in commit.parent_ids() {
                    enqueue(
                        parent,
                        ObjectType::Commit,
                        &mut pending,
                        &mut seen,
                        limits,
                        objects.remaining_metadata,
                    )?;
                }
            }
            ObjectType::Tree => {
                let tree = repository
                    .find_tree(id)
                    .map_err(|_| "history tree cannot be decoded")?;
                for entry in &tree {
                    let kind = match entry.filemode_raw() {
                        0o040000 => ObjectType::Tree,
                        0o100644 | 0o100755 | 0o120000 => ObjectType::Blob,
                        0o160000 => {
                            return Err(
                                "history gitlinks require external repositories and are unsupported",
                            );
                        }
                        _ => return Err("history tree mode is unsupported"),
                    };
                    enqueue(
                        entry.id(),
                        kind,
                        &mut pending,
                        &mut seen,
                        limits,
                        objects.remaining_metadata,
                    )?;
                }
            }
            ObjectType::Blob => (),
            _ => return Err("history object kind is unsupported"),
        }
    }
    Ok(())
}
