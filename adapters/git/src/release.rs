//! Shared release-object encoding and exact bare/independent repository verification.

pub(crate) mod acquisition;
mod checkout;
pub(crate) mod custody;
pub(crate) mod history;
mod setup;
mod worktree;

use crate::{Git, Objects, TreeProfile, visit};
use gazette_core::{
    release::{
        CommitEffect, CommitError, CommitMetadata, CommitOptions, CommitReport, CommitSignature,
    },
    source::{
        CommitId, Content, InventoryBuilder, MetadataBudget, RevisionLimits, TreeIdentity,
        TreeInput,
    },
};
use gazette_ports::release::ReleaseCommits;
use git2::{
    Index, IndexEntry, IndexTime, ObjectType, Oid, Repository, RepositoryOpenFlags, Signature, Time,
};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, &'static str>;

impl ReleaseCommits for Git {
    fn prepare_commit(
        &self,
        root: &Path,
        input: TreeInput<'_>,
        metadata: &CommitMetadata,
        options: CommitOptions,
    ) -> std::result::Result<CommitReport, CommitError> {
        if !options.allow_write_objects {
            return Err(CommitError::refused(
                "explicit local Git object-write permission is required",
            ));
        }
        let mut effect = CommitEffect::NotAttempted;
        let result = (|| {
            let mut budget = MetadataBudget::new(options.limits.metadata_bytes);
            validate_input(&input, options.limits, &mut budget)?;
            let (author, committer) = signatures(metadata, &mut budget)?;
            let selected = SelectedRepository::open(root, &mut budget)?;
            let repo = &selected.repo;
            parent(repo, metadata, &mut budget)?;
            let mut index = retained_index(&input)?;
            effect = CommitEffect::ObjectsMayRemain;
            let commit = write_objects(
                repo,
                &input,
                metadata,
                (&author, &committer),
                &mut index,
                &mut budget,
            )?;
            effect = CommitEffect::CommitCreated(commit.clone());
            selected.recheck()?;
            // Reopen the exact repository; never accept an in-memory index or
            // successful write return as independent complete object read-back.
            let read_back = SelectedRepository::open(root, &mut budget)?;
            let report = inspect(
                &read_back.repo,
                &commit,
                input.inventory().identity(),
                metadata,
                options.limits,
                &mut budget,
            )?;
            read_back.recheck()?;
            selected.recheck()?;
            Ok(report)
        })();
        result.map_err(|reason| CommitError { reason, effect })
    }

    fn verify_commit(
        &self,
        root: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: RevisionLimits,
    ) -> std::result::Result<CommitReport, CommitError> {
        (|| {
            let mut budget = MetadataBudget::new(limits.metadata_bytes);
            signatures(metadata, &mut budget)?;
            let selected = SelectedRepository::open(root, &mut budget)?;
            let report = inspect(
                &selected.repo,
                commit,
                output,
                metadata,
                limits,
                &mut budget,
            )?;
            selected.recheck()?;
            Ok(report)
        })()
        .map_err(CommitError::refused)
    }
}

fn retained_index(input: &TreeInput<'_>) -> Result<Index> {
    let mut index = Index::new().map_err(|_| "could not create isolated in-memory index")?;
    // One complete-set index owner; never inspect worktree attributes or filters.
    for entry in input.inventory().entries() {
        if let Content::File { bytes, .. } = entry.content {
            let data = input.file(&entry.path).ok_or("retained file is missing")?;
            let id = Oid::hash_object(ObjectType::Blob, data)
                .map_err(|_| "retained blob cannot be identified")?;
            index
                .add(&IndexEntry {
                    ctime: IndexTime::new(0, 0),
                    mtime: IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100644,
                    uid: 0,
                    gid: 0,
                    file_size: u32::try_from(bytes)
                        .map_err(|_| "file exceeds Git index size profile")?,
                    id,
                    flags: 0,
                    flags_extended: 0,
                    path: entry.path.as_str().as_bytes().to_vec(),
                })
                .map_err(|_| "in-memory index rejected the retained file")?;
        }
    }
    Ok(index)
}

fn write_objects(
    repo: &Repository,
    input: &TreeInput<'_>,
    metadata: &CommitMetadata,
    signatures: (&Signature<'_>, &Signature<'_>),
    index: &mut Index,
    budget: &mut MetadataBudget,
) -> Result<CommitId> {
    let parent = parent(repo, metadata, budget)?;
    for entry in input.inventory().entries() {
        if let Content::File { .. } = entry.content {
            repo.blob(input.file(&entry.path).ok_or("retained file is missing")?)
                .map_err(|_| "local blob write failed")?;
        }
    }
    let tree_id = index
        .write_tree_to(repo)
        .map_err(|_| "local tree write failed")?;
    let tree = repo
        .find_tree(tree_id)
        .map_err(|_| "prepared tree cannot be read")?;
    let parents: Vec<_> = parent.iter().collect();
    let body = commit_bytes(
        repo,
        signatures.0,
        signatures.1,
        &metadata.message,
        &tree,
        &parents,
        budget,
    )?;
    repo.odb()
        .and_then(|odb| odb.write(ObjectType::Commit, &body))
        .map_err(|_| "local commit write failed")?
        .to_string()
        .parse()
        .map_err(|_| "unsupported prepared commit coordinate")
}

fn validate_input(
    input: &TreeInput<'_>,
    limits: RevisionLimits,
    budget: &mut MetadataBudget,
) -> Result<()> {
    let mut bounds = InventoryBuilder::new(limits.content);
    let entries = input.inventory().entries();
    for entry in entries {
        bounds
            .insert(entry.path.clone(), entry.content)
            .map_err(|_| "output content ceiling or structure refused")?;
        if entry
            .path
            .as_str()
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
        {
            return Err("Git administrative names cannot be release output");
        }
        match entry.content {
            Content::Directory => {
                let prefix = format!("{}/", entry.path);
                let first =
                    entries.partition_point(|candidate| candidate.path.as_str() < prefix.as_str());
                if !entries
                    .get(first)
                    .is_some_and(|next| next.path.as_str().starts_with(&prefix))
                {
                    return Err("empty directories cannot be represented by a deployable Git tree");
                }
            }
            Content::File { bytes, .. } if bytes > u64::from(u32::MAX) => {
                return Err("file exceeds Git index size profile");
            }
            Content::File { .. } => (),
        }
        // Charge canonical tree-entry framing, including each path component.
        budget
            .charge(entry.path.as_str().len().saturating_add(32))
            .map_err(|_| "output metadata ceiling exceeded")?;
    }
    Ok(())
}

fn signatures(
    metadata: &CommitMetadata,
    budget: &mut MetadataBudget,
) -> Result<(Signature<'static>, Signature<'static>)> {
    metadata.validate().map_err(|error| error.reason)?;
    budget
        .charge(metadata.message.len())
        .map_err(|_| "commit metadata ceiling exceeded")?;
    Ok((
        signature(&metadata.author, budget)?,
        signature(&metadata.committer, budget)?,
    ))
}

fn signature(value: &CommitSignature, budget: &mut MetadataBudget) -> Result<Signature<'static>> {
    budget
        .charge(
            value
                .name
                .len()
                .saturating_add(value.email.len())
                .saturating_add(64),
        )
        .map_err(|_| "commit metadata ceiling exceeded")?;
    let signature = Signature::new(
        &value.name,
        &value.email,
        &Time::new(value.seconds, value.offset_minutes),
    )
    .map_err(|_| "Git rejected explicit signature metadata")?;
    if !signature_matches(&signature, value) {
        return Err("Git changed explicit signature metadata");
    }
    Ok(signature)
}

fn signature_matches(signature: &Signature<'_>, value: &CommitSignature) -> bool {
    signature.name_bytes() == value.name.as_bytes()
        && signature.email_bytes() == value.email.as_bytes()
        && signature.when().seconds() == value.seconds
        && signature.when().offset_minutes() == value.offset_minutes
}

fn commit_bytes(
    repo: &Repository,
    author: &Signature<'_>,
    committer: &Signature<'_>,
    message: &str,
    tree: &git2::Tree<'_>,
    parents: &[&git2::Commit<'_>],
    budget: &mut MetadataBudget,
) -> Result<git2::Buf> {
    let body = repo
        .commit_create_buffer(author, committer, message, tree, parents)
        .map_err(|_| "explicit commit encoding failed")?;
    budget
        .charge(body.len())
        .map_err(|_| "commit metadata ceiling exceeded")?;
    Ok(body)
}

fn parent<'a>(
    repo: &'a Repository,
    metadata: &CommitMetadata,
    budget: &mut MetadataBudget,
) -> Result<Option<git2::Commit<'a>>> {
    metadata
        .parent
        .as_ref()
        .map(|id| {
            let oid =
                Oid::from_str(id.as_str()).map_err(|_| "unsupported explicit parent coordinate")?;
            Objects::new(repo, budget)
                .map_err(|_| "local object database unavailable")?
                .metadata(oid, ObjectType::Commit, Path::new("<parent>"))
                .map_err(|_| "explicit parent is missing, corrupt, unsupported or over-limit")?;
            repo.find_commit(oid)
                .map_err(|_| "explicit parent is not a valid local commit")
        })
        .transpose()
}

pub(crate) fn inspect(
    repo: &Repository,
    id: &CommitId,
    expected: TreeIdentity,
    metadata: &CommitMetadata,
    limits: RevisionLimits,
    budget: &mut MetadataBudget,
) -> Result<CommitReport> {
    let parent = parent(repo, metadata, budget)?;
    let oid = Oid::from_str(id.as_str()).map_err(|_| "unsupported commit coordinate")?;
    let mut objects =
        Objects::new(repo, budget).map_err(|_| "local object database unavailable")?;
    let tree = objects
        .commit_tree(oid, Path::new("<commit>"))
        .map_err(|_| "commit or tree is missing, corrupt, unsupported or over-limit")?;
    let commit = repo
        .find_commit(oid)
        .map_err(|_| "commit cannot be decoded")?;
    let expected_parent = metadata.parent.as_ref().map(|id| id.as_str());
    let actual_parent = commit.parent_ids().next().map(|id| id.to_string());
    if commit.parent_count() != usize::from(metadata.parent.is_some())
        || actual_parent.as_deref() != expected_parent
        || !signature_matches(&commit.author(), &metadata.author)
        || !signature_matches(&commit.committer(), &metadata.committer)
        || commit.message_bytes() != metadata.message.as_bytes()
    {
        return Err("commit does not match the exact parent, author, committer or message");
    }
    let mut inventory = InventoryBuilder::new(limits.content);
    visit(
        &mut objects,
        &tree,
        "",
        &mut inventory,
        TreeProfile::Release,
    )
    .map_err(|_| "committed output is corrupt, unsupported or over-limit")?;
    let inventory = inventory.finish();
    if inventory.identity() != expected {
        return Err("commit tree differs from the exact retained output");
    }
    let parents: Vec<_> = parent.iter().collect();
    let encoded = commit_bytes(
        repo,
        &commit.author(),
        &commit.committer(),
        &metadata.message,
        &tree,
        &parents,
        budget,
    )?;
    if Oid::hash_object(ObjectType::Commit, &encoded)
        .map_err(|_| "commit encoding cannot be hashed")?
        != oid
    {
        return Err("commit encoding differs from the exact deterministic recipe");
    }
    Ok(CommitReport {
        commit: id.clone(),
        output: inventory.identity(),
        parent: metadata.parent.clone(),
        entries: inventory.entries().len(),
        bytes: inventory.total_bytes(),
    })
}

/// Qualify absence from the exact independent local ref database, not advertisements.
/// Reuses preparation's bounded namespace checks without loading objects or writing refs.
pub(super) fn ref_absent(root: &Path, reference: &str, maximum: usize) -> Result<bool> {
    let selected = SelectedRepository::open(root, &mut MetadataBudget::new(maximum))?;
    let absent = match selected.repo.find_reference(reference) {
        Ok(_) => false,
        Err(error) if error.code() == git2::ErrorCode::NotFound => true,
        Err(_) => return Err("native ref namespace could not be inspected"),
    };
    selected.recheck()?;
    Ok(absent)
}

pub(crate) struct SelectedRepository {
    pub(crate) repo: Repository,
    pub(crate) root: PathBuf,
    identity: (u64, u64),
    workdir: Option<(PathBuf, (u64, u64))>,
}

impl SelectedRepository {
    pub(crate) fn open(root: &Path, budget: &mut MetadataBudget) -> Result<Self> {
        let root: PathBuf = root.components().collect();
        let metadata =
            fs::symlink_metadata(&root).map_err(|_| "selected local repository is missing")?;
        if !metadata.is_dir() {
            return Err("selected repository must be a real directory");
        }
        let canonical =
            fs::canonicalize(&root).map_err(|_| "selected repository cannot be resolved")?;
        check_namespace(&canonical, budget)?;
        let repo = Repository::open_ext(
            &canonical,
            RepositoryOpenFlags::NO_SEARCH | RepositoryOpenFlags::BARE,
            &[] as &[&str],
        )
        .map_err(|_| "exact existing bare deployment repository required")?;
        if !repo.is_bare()
            || repo.workdir().is_some()
            || fs::canonicalize(repo.path()).ok().as_ref() != Some(&canonical)
            || fs::canonicalize(repo.commondir()).ok().as_ref() != Some(&canonical)
        {
            return Err("repository is not independent bare deployment storage");
        }
        Ok(Self {
            repo,
            root: canonical,
            identity: (metadata.dev(), metadata.ino()),
            workdir: None,
        })
    }

    pub(crate) fn recheck(&self) -> Result<()> {
        let current = fs::symlink_metadata(&self.root)
            .map_err(|_| "deployment repository identity changed")?;
        if !current.is_dir() || self.identity != (current.dev(), current.ino()) {
            return Err("deployment repository identity changed");
        }
        if let Some((path, identity)) = &self.workdir {
            let current =
                fs::symlink_metadata(path).map_err(|_| "deployment worktree identity changed")?;
            if !current.is_dir() || *identity != (current.dev(), current.ino()) {
                return Err("deployment worktree identity changed");
            }
        }
        Ok(())
    }
}

fn check_namespace(root: &Path, budget: &mut MetadataBudget) -> Result<()> {
    // Refuse shared object custody before libgit2 opens it. No fetching or alternate
    // traversal is admitted. This is a trusted-local namespace check, not exclusion
    // against a hostile same-user process replacing paths after inspection.
    for name in [
        "commondir",
        "shallow",
        "objects/info/alternates",
        "objects/info/http-alternates",
    ] {
        match fs::symlink_metadata(root.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => {
                return Err("shared, shallow or alternate deployment repositories are unsupported");
            }
        }
    }
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in
            fs::read_dir(directory).map_err(|_| "deployment namespace cannot be inspected")?
        {
            let entry = entry.map_err(|_| "deployment namespace cannot be inspected")?;
            budget
                .charge(entry.file_name().len().saturating_add(64))
                .map_err(|_| "deployment namespace metadata ceiling exceeded")?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|_| "deployment namespace changed")?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if !metadata.is_file() || metadata.nlink() != 1 {
                return Err("deployment namespace contains shared or special files");
            } else if !entry
                .path()
                .strip_prefix(root)
                .map_err(|_| "deployment namespace escaped")?
                .starts_with("objects")
            {
                budget
                    .charge(
                        usize::try_from(metadata.len())
                            .map_err(|_| "deployment metadata file is too large")?,
                    )
                    .map_err(|_| "deployment namespace metadata ceiling exceeded")?;
            }
        }
    }
    Ok(())
}
