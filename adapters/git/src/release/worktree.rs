//! Independent repository metadata over retained output; never checkout or copy it.

use super::{
    Git, Result, SelectedRepository, check_namespace, custody, history, inspect, retained_index,
    signatures, validate_input, write_objects,
};
use gazette_core::{
    release::{
        CommitMetadata, ParentSource, WorktreeEffect, WorktreeError, WorktreeLimits,
        WorktreeOptions, WorktreeReport,
    },
    source::{CommitId, MetadataBudget, TreeIdentity, TreeInput},
};
use gazette_ports::release::{ReleaseWorktrees, WorktreeSync};
use git2::{
    Config, Index, ObjectFormat, Oid, ReferenceType, Repository, RepositoryInitOptions,
    RepositoryOpenFlags,
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

// A generated private profile, not a Git configuration parser. Exact preflight
// prevents include/includeIf from being followed before a later rejection.
const CONFIGURATION: &[u8] = b"[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = false\n";

impl ReleaseWorktrees for Git {
    fn prepare_worktree(
        &self,
        root: &Path,
        input: TreeInput<'_>,
        metadata: &CommitMetadata,
        parent_source: Option<&ParentSource>,
        options: WorktreeOptions,
    ) -> std::result::Result<WorktreeReport, WorktreeError> {
        if !options.allow_initialize {
            return Err(WorktreeError::refused(
                "explicit deployment repository initialization permission is required",
            ));
        }
        let mut effect = WorktreeEffect::NotAttempted;
        (|| {
            let mut budget = MetadataBudget::new(options.limits.output.metadata_bytes);
            validate_input(&input, options.limits.output, &mut budget)?;
            let (author, committer) = signatures(metadata, &mut budget)?;
            gazette_core::release::validate_worktree_parent(metadata.parent.as_ref(), parent_source)
                .map_err(|error| error.reason)?;
            if options.storage_bytes == 0 {
                return Err("deployment repository storage ceiling is zero");
            }
            let mut index = retained_index(&input)?;
            let (root, identity) = directory(root)?;
            let administrative = root.join(".git");
            absent(&administrative)?;
            let history = match (&metadata.parent, parent_source) {
                (Some(parent), Some(source)) => Some(history::capture(source, &root, parent, options.limits.history, options.storage_bytes, &mut budget)?),
                _ => None,
            };
            recheck_directory(&root, identity)?;
            // Claim only this exact absent name. Invalid existing metadata is not
            // made adoptable by native init's weaker valid-repository reinit check.
            effect = WorktreeEffect::MetadataMayRemain;
            fs::DirBuilder::new().mode(0o700).create(&administrative)
                .map_err(|_| "deployment metadata creation failed; preserve and reconcile the exact path")?;
            let mut initialization = RepositoryInitOptions::new();
            initialization.bare(false).no_reinit(true).mkdir(false).mkpath(false)
                .no_dotgit_dir(false).external_template(false).template_path(Path::new(""))
                .initial_head("refs/heads/gazette-unborn").object_format(ObjectFormat::Sha1);
            // Explicit empty template_path suppresses both external and built-in
            // template files in the pinned native provider. No origin is supplied.
            let repo = Repository::init_opts(&root, &initialization)
                .map_err(|_| "independent deployment repository initialization failed")?;
            fs::write(administrative.join("config"), CONFIGURATION)
                .map_err(|_| "owned deployment configuration cannot be established")?;
            let config = Config::open(&administrative.join("config"))
                .map_err(|_| "owned deployment configuration cannot be opened")?;
            // Only owned local configuration informs subsequent writes; source or
            // host identity/filter/signature configuration is not an input.
            repo.set_config(&config).map_err(|_| "owned deployment configuration cannot be bound")?;
            drop(repo);
            let selected = SelectedRepository::open_worktree(&root, &mut budget)?;
            recheck_directory(&root, identity)?;
            if let Some(history) = history { history.write(&selected.repo)?; }
            let commit = write_objects(&selected.repo, &input, metadata,
                (&author, &committer), &mut index, &mut budget)?;
            effect = WorktreeEffect::CommitCreated(commit.clone());
            let oid = Oid::from_str(commit.as_str()).map_err(|_| "invalid prepared commit coordinate")?;
            let tree = selected.repo.find_commit(oid).and_then(|commit| commit.tree())
                .map_err(|_| "prepared deployment tree cannot be read")?;
            let mut native_index = selected.repo.index().map_err(|_| "deployment index cannot be opened")?;
            native_index.read_tree(&tree).map_err(|_| "deployment index cannot load the exact tree")?;
            native_index.write().map_err(|_| "deployment index write failed")?;
            selected.repo.set_head_detached(oid).map_err(|_| "deployment HEAD detachment failed")?;
            selected.recheck()?;
            let observed = inspect_selected(&selected, &commit, input.inventory().identity(), metadata, options.limits, &mut budget)?;
            custody::synchronize(&selected.root, options.storage_bytes, &mut budget)?;
            selected.recheck()?;
            let reopened = SelectedRepository::open_worktree(&root, &mut budget)?;
            let result = inspect_selected(&reopened, &commit, input.inventory().identity(), metadata, options.limits, &mut budget)?;
            if result != observed { return Err("deployment evidence changed during synchronization"); }
            recheck_directory(&root, identity)?;
            selected.recheck()?;
            reopened.recheck()?;
            Ok(result)
        })().map_err(|reason| WorktreeError { reason, effect })
    }

    fn inspect_worktree(
        &self,
        root: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: WorktreeLimits,
    ) -> std::result::Result<WorktreeReport, WorktreeError> {
        (|| {
            let mut budget = MetadataBudget::new(limits.output.metadata_bytes);
            signatures(metadata, &mut budget)?;
            let selected = SelectedRepository::open_worktree(root, &mut budget)?;
            let report =
                inspect_selected(&selected, commit, output, metadata, limits, &mut budget)?;
            selected.recheck()?;
            Ok(report)
        })()
        .map_err(WorktreeError::refused)
    }
}

impl WorktreeSync for Git {
    fn synchronize_worktree(
        &self,
        root: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        options: gazette_core::release::WorktreeSyncOptions,
    ) -> std::result::Result<WorktreeReport, gazette_core::release::WorktreeSyncError> {
        use gazette_core::release::WorktreeSyncError;
        if !options.allow_synchronize {
            return Err(WorktreeSyncError::refused(
                "explicit administrative synchronization permission is required",
            ));
        }
        if options.storage_bytes == 0 {
            return Err(WorktreeSyncError::refused(
                "deployment repository storage ceiling is zero",
            ));
        }
        let mut attempted = false;
        (|| {
            let mut budget = MetadataBudget::new(options.limits.output.metadata_bytes);
            signatures(metadata, &mut budget)?;
            let selected = SelectedRepository::open_worktree(root, &mut budget)?;
            let before = inspect_selected(
                &selected,
                commit,
                output,
                metadata,
                options.limits,
                &mut budget,
            )?;
            // Reject known storage exhaustion before entering the effect boundary.
            custody::inspect_storage(&selected.root, options.storage_bytes, &mut budget)?;
            selected.recheck()?;
            attempted = true;
            // selected.root is .git: publication output has a separate FS owner.
            custody::synchronize(&selected.root, options.storage_bytes, &mut budget)?;
            selected.recheck()?;
            let reopened = SelectedRepository::open_worktree(root, &mut budget)?;
            let after = inspect_selected(
                &reopened,
                commit,
                output,
                metadata,
                options.limits,
                &mut budget,
            )?;
            if after != before {
                return Err("deployment evidence changed during synchronization");
            }
            selected.recheck()?;
            reopened.recheck()?;
            Ok(after)
        })()
        .map_err(|reason| WorktreeSyncError { reason, attempted })
    }
}

impl SelectedRepository {
    pub(super) fn open_worktree(root: &Path, budget: &mut MetadataBudget) -> Result<Self> {
        let (workdir, workdir_identity) = directory(root)?;
        let (administrative, identity) = directory(&workdir.join(".git"))?;
        check_namespace(&administrative, budget)?;
        let config_path = administrative.join("config");
        let config_metadata = fs::symlink_metadata(&config_path)
            .map_err(|_| "owned deployment configuration is missing")?;
        if !config_metadata.is_file()
            || config_metadata.nlink() != 1
            || config_metadata.len() != CONFIGURATION.len() as u64
        {
            return Err("deployment configuration differs from its exact owned profile");
        }
        // Bound the read even if this trusted-local file changes after metadata.
        use std::io::Read;
        let mut bytes = Vec::with_capacity(CONFIGURATION.len() + 1);
        fs::File::open(&config_path)
            .map_err(|_| "owned configuration cannot be read")?
            .take(CONFIGURATION.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "owned configuration cannot be read")?;
        if bytes != CONFIGURATION {
            return Err("deployment configuration differs from its exact owned profile");
        }
        let repo = Repository::open_ext(&workdir, RepositoryOpenFlags::NO_SEARCH, &[] as &[&str])
            .map_err(|_| "exact independent deployment worktree required")?;
        if repo.is_bare()
            || repo.is_worktree()
            || fs::canonicalize(repo.path()).ok().as_ref() != Some(&administrative)
            || fs::canonicalize(repo.commondir()).ok().as_ref() != Some(&administrative)
            || repo
                .workdir()
                .and_then(|path| fs::canonicalize(path).ok())
                .as_ref()
                != Some(&workdir)
        {
            return Err("deployment repository is not independent metadata at the exact worktree");
        }
        let config = Config::open(&administrative.join("config"))
            .map_err(|_| "owned deployment configuration cannot be inspected")?;
        if config.get_bool("core.logallrefupdates").ok() != Some(false) {
            return Err("deployment reflogs must be explicitly disabled");
        }
        repo.set_config(&config)
            .map_err(|_| "owned deployment configuration cannot be bound")?;
        absent(&administrative.join("logs"))?;
        if repo
            .references()
            .map_err(|_| "deployment references cannot be inspected")?
            .next()
            .is_some()
        {
            return Err(
                "independent preparation must not contain branch or other named references",
            );
        }
        if repo
            .remotes()
            .map_err(|_| "deployment remotes cannot be inspected")?
            .iter()
            .next()
            .is_some()
        {
            return Err("independent preparation must not configure remotes");
        }
        Ok(Self {
            repo,
            root: administrative,
            identity,
            workdir: Some((workdir, workdir_identity)),
        })
    }
}

fn inspect_selected(
    selected: &SelectedRepository,
    id: &CommitId,
    output: TreeIdentity,
    metadata: &CommitMetadata,
    limits: WorktreeLimits,
    budget: &mut MetadataBudget,
) -> Result<WorktreeReport> {
    let commit = inspect(&selected.repo, id, output, metadata, limits.output, budget)?;
    if let Some(parent) = &metadata.parent {
        history::verify(&selected.repo, parent, limits.history, budget)?;
    }
    let oid = Oid::from_str(id.as_str()).map_err(|_| "invalid deployment commit coordinate")?;
    let head = selected
        .repo
        .find_reference("HEAD")
        .map_err(|_| "deployment HEAD is missing")?;
    if head.kind() != Some(ReferenceType::Direct) || head.target() != Some(oid) {
        return Err("deployment HEAD is not detached at the exact prepared commit");
    }
    let tree = selected
        .repo
        .find_commit(oid)
        .and_then(|commit| commit.tree())
        .map_err(|_| "deployment tree cannot be read")?;
    let mut expected = Index::new().map_err(|_| "deployment comparison index unavailable")?;
    expected
        .read_tree(&tree)
        .map_err(|_| "deployment comparison tree cannot be loaded")?;
    let mut actual = Index::open(&selected.root.join("index"))
        .map_err(|_| "deployment index is missing or corrupt")?;
    actual
        .read(true)
        .map_err(|_| "deployment index cannot be read back")?;
    if actual.has_conflicts() || actual.len() != expected.len() {
        return Err("deployment index does not contain the complete prepared tree");
    }
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        budget
            .charge(actual.path.len().saturating_add(128))
            .map_err(|_| "deployment index metadata ceiling exceeded")?;
        if actual.path != expected.path
            || actual.id != expected.id
            || actual.mode != expected.mode
            || actual.flags & 0xf000 != 0
            || actual.flags_extended != 0
        {
            return Err("deployment index differs from the exact prepared tree");
        }
    }
    Ok(WorktreeReport { commit })
}

fn directory(path: &Path) -> Result<(PathBuf, (u64, u64))> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "deployment directory is missing")?;
    if !metadata.is_dir() {
        return Err("deployment path must be a real directory");
    }
    let root = fs::canonicalize(path).map_err(|_| "deployment directory cannot be resolved")?;
    let identity = (metadata.dev(), metadata.ino());
    recheck_directory(&root, identity)?;
    Ok((root, identity))
}

fn recheck_directory(path: &Path, expected: (u64, u64)) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "deployment directory identity changed")?;
    if !metadata.is_dir() || (metadata.dev(), metadata.ino()) != expected {
        return Err("deployment directory identity changed");
    }
    Ok(())
}

fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("deployment administrative path already exists or cannot be inspected"),
    }
}
