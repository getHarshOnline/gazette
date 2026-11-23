//! Reads exact local Git objects, and ref advertisements under separate permission, so
//! a release can be checked against Git as it stands.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. It also writes detached release objects when explicitly
//! permitted. Ref advertisements are bounded.

use std::io;
use std::path::{Path, PathBuf};

use gazette_core::source::{
    CommitId, Content, EntryPath, Error, Inventory, InventoryBuilder, Limits, MetadataBudget,
    Subdirectory, entry_name,
};
use gazette_ports::source::RevisionReader;
use git2::{ObjectType, Odb, OdbObject, Oid, Repository, RepositoryOpenFlags, Tree};
use sha2::{Digest, Sha256};

mod catalog;
mod dispatch;
mod release;
mod remote;
mod working_source;
pub use dispatch::Dispatcher;
pub use release::acquisition::ParentAcquirer;
pub use remote::RemoteReader;

/// Read or explicitly prepare local Git objects; never discover or fetch a repo.
#[derive(Clone, Copy, Debug, Default)]
pub struct Git;

impl RevisionReader for Git {
    fn working_source_filter(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: gazette_core::source::RevisionLimits,
    ) -> Result<Option<Box<dyn gazette_ports::source::SourceFilter>>, Error> {
        working_source::filter(root, subdirectory, limits)
    }

    fn verify_branch(&self, root: &Path, expected: &str) -> Result<(), Error> {
        if !gazette_core::source::valid_branch_name(expected) {
            return Err(invalid(
                root,
                "expected branch must be a literal local branch name",
            ));
        }
        let reference = format!("refs/heads/{expected}");
        let repo = open_worktree(root)?;
        let head = repo
            .head()
            .map_err(|e| git_error("read checked-out branch", root, e))?;
        let name = head
            .name()
            .map_err(|e| git_error("read branch name", root, e))?;
        let actual = head.is_branch().then_some(name);
        if actual != Some(reference.as_str()) {
            return Err(Error::BranchMismatch {
                path: root.to_owned(),
                expected: expected.to_owned(),
                actual: actual
                    .and_then(|name| name.strip_prefix("refs/heads/"))
                    .map(str::to_owned),
            });
        }
        Ok(())
    }

    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: Limits,
        remaining_metadata: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        let (repo, oid) = open_revision(root, commit)?;
        let mut objects = Objects::new(&repo, remaining_metadata)?;
        let root_tree = objects.commit_tree(oid, root)?;
        let selected = objects.select_entry(root_tree, subdirectory, 0o040000)?;
        let tree = objects.tree(selected, Path::new(&subdirectory.to_string()))?;
        let mut inventory = InventoryBuilder::new(limits);
        visit(&mut objects, &tree, "", &mut inventory, TreeProfile::Source)?;
        check_head(&repo, root, oid)?;
        Ok(inventory.finish())
    }

    fn read_revision_filtered(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        filter: &dyn gazette_ports::source::SourceFilter,
        limits: Limits,
        remaining_metadata: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        let (repo, oid) = open_revision(root, commit)?;
        let mut objects = Objects::new(&repo, remaining_metadata)?;
        let root_tree = objects.commit_tree(oid, root)?;
        let selected = objects.select_entry(root_tree, subdirectory, 0o040000)?;
        let tree = objects.tree(selected, Path::new(&subdirectory.to_string()))?;
        let mut inventory = InventoryBuilder::new(limits);
        visit_filtered(
            &mut objects,
            &tree,
            "",
            &mut inventory,
            TreeProfile::Source,
            Some(filter),
        )?;
        check_head(&repo, root, oid)?;
        Ok(inventory.finish())
    }

    fn list_revision_filtered(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        filter: &dyn gazette_ports::source::SourceFilter,
        limits: gazette_core::source::ListingLimits,
        metadata: &mut MetadataBudget,
    ) -> Result<gazette_core::source::Listing, Error> {
        catalog::list(root, subdirectory, commit, filter, limits, metadata)
    }

    fn read_gitlink(
        &self,
        parent: &Path,
        commit: &CommitId,
        path: &Subdirectory,
        remaining_metadata: &mut MetadataBudget,
    ) -> Result<CommitId, Error> {
        let (repo, oid) = open_revision(parent, commit)?;
        let mut objects = Objects::new(&repo, remaining_metadata)?;
        let root_tree = objects.commit_tree(oid, parent)?;
        let child = objects.select_entry(root_tree, path, 0o160000)?;
        check_head(&repo, parent, oid)?;
        child
            .to_string()
            .parse()
            .map_err(|_| invalid(parent, "unsupported child commit coordinate"))
    }
}

fn open_revision(root: &Path, commit: &CommitId) -> Result<(Repository, Oid), Error> {
    let repo = open_worktree(root)?;
    let oid =
        Oid::from_str(commit.as_str()).map_err(|e| git_error("decode selected commit", root, e))?;
    check_head(&repo, root, oid)?;
    Ok((repo, oid))
}

fn open_worktree(root: &Path) -> Result<Repository, Error> {
    let repo = Repository::open_ext(root, RepositoryOpenFlags::NO_SEARCH, &[] as &[&str])
        .map_err(|e| git_error("open exact Git repository", root, e))?;
    let canonical = std::fs::canonicalize(root)
        .map_err(|e| native_error("resolve selected repository", root, e))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| invalid(root, "a checked-out repository is required"))?;
    if std::fs::canonicalize(workdir).map_err(|e| native_error("resolve Git worktree", root, e))?
        != canonical
    {
        return Err(invalid(
            root,
            "selected path is not this repository's worktree root",
        ));
    }
    Ok(repo)
}

fn check_head(repo: &Repository, root: &Path, expected: Oid) -> Result<(), Error> {
    let actual = repo
        .head()
        .map_err(|e| git_error("read checked-out HEAD", root, e))?
        .target()
        .ok_or_else(|| invalid(root, "HEAD does not resolve to a direct object"))?;
    if actual != expected {
        return Err(Error::RevisionMismatch {
            path: root.to_owned(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

struct Objects<'repo, 'budget> {
    repo: &'repo Repository,
    odb: Odb<'repo>,
    remaining_metadata: &'budget mut MetadataBudget,
}

impl<'repo, 'budget> Objects<'repo, 'budget> {
    fn new(
        repo: &'repo Repository,
        remaining_metadata: &'budget mut MetadataBudget,
    ) -> Result<Self, Error> {
        Ok(Self {
            repo,
            odb: repo
                .odb()
                .map_err(|e| git_error("open local object database", repo.path(), e))?,
            remaining_metadata,
        })
    }

    fn commit_tree(&mut self, id: Oid, root: &Path) -> Result<Tree<'repo>, Error> {
        self.metadata(id, ObjectType::Commit, Path::new("<commit>"))?;
        let commit = self
            .repo
            .find_commit(id)
            .map_err(|e| git_error("read selected commit", root, e))?;
        self.tree(commit.tree_id(), Path::new(""))
    }

    fn select_entry(
        &mut self,
        mut tree: Tree<'repo>,
        selection: &Subdirectory,
        terminal_mode: i32,
    ) -> Result<Oid, Error> {
        let mut components = selection.components().peekable();
        let mut path = PathBuf::new();
        while let Some(component) = components.next() {
            path.push(component);
            let entry = tree
                .get_name(component)
                .ok_or_else(|| invalid(&path, "selected path is absent from the commit"))?;
            let terminal = components.peek().is_none();
            if entry.filemode() != if terminal { terminal_mode } else { 0o040000 } {
                return Err(Error::Unsupported(path));
            }
            let id = entry.id();
            drop(entry);
            if terminal {
                return Ok(id);
            }
            tree = self.tree(id, &path)?;
        }
        Err(Error::InvalidName(path))
    }

    fn checked(
        &self,
        id: Oid,
        kind: ObjectType,
        ceiling: usize,
        path: &Path,
    ) -> Result<OdbObject<'_>, Error> {
        let (size, actual_kind) = self
            .odb
            .read_header(id)
            .map_err(|e| git_error("read Git object header", path, e))?;
        if actual_kind != kind {
            return Err(invalid(path, "Git object has the wrong type"));
        }
        if size > ceiling {
            return Err(Error::Limit {
                path: path.to_owned(),
                resource: "Git object bytes",
            });
        }
        let object = self
            .odb
            .read(id)
            .map_err(|e| git_error("read local Git object", path, e))?;
        let actual_id = Oid::hash_object(kind, object.data())
            .map_err(|e| git_error("verify Git object identity", path, e))?;
        if object.kind() != kind || object.len() != size || actual_id != id {
            return Err(invalid(
                path,
                "Git object bytes do not match the requested identity",
            ));
        }
        Ok(object)
    }

    fn metadata(&mut self, id: Oid, kind: ObjectType, path: &Path) -> Result<(), Error> {
        let size = self
            .checked(id, kind, self.remaining_metadata.remaining(), path)?
            .len();
        self.remaining_metadata
            .charge(size)
            .map_err(|_| Error::Limit {
                path: path.to_owned(),
                resource: "Git metadata bytes",
            })?;
        Ok(())
    }

    fn tree(&mut self, id: Oid, path: &Path) -> Result<Tree<'repo>, Error> {
        self.metadata(id, ObjectType::Tree, path)?;
        self.repo
            .find_tree(id)
            .map_err(|e| git_error("decode Git tree", path, e))
    }
}

#[derive(Clone, Copy)]
enum TreeProfile {
    Source,
    Release,
}

fn visit(
    objects: &mut Objects<'_, '_>,
    tree: &Tree<'_>,
    prefix: &str,
    inventory: &mut InventoryBuilder,
    profile: TreeProfile,
) -> Result<(), Error> {
    visit_filtered(objects, tree, prefix, inventory, profile, None)
}

fn visit_filtered(
    objects: &mut Objects<'_, '_>,
    tree: &Tree<'_>,
    prefix: &str,
    inventory: &mut InventoryBuilder,
    profile: TreeProfile,
    filter: Option<&dyn gazette_ports::source::SourceFilter>,
) -> Result<(), Error> {
    for entry in tree {
        let name = entry_name(entry.name_bytes(), Path::new(prefix))?;
        let relative: EntryPath = (if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        })
        .parse()?;
        if let Some(filter) = filter
            && !filter.allows(&relative)?
        {
            continue;
        }
        let path = Path::new(relative.as_str());
        if filter.is_some_and(|filter| filter.ancestor_only(relative.as_str()))
            && entry.filemode() != 0o040000
        {
            return Err(Error::Unsupported(path.to_owned()));
        }
        inventory.check_path(&relative)?;
        if matches!(profile, TreeProfile::Release)
            && (name.eq_ignore_ascii_case(".git")
                || !matches!(entry.filemode_raw(), 0o040000 | 0o100644))
        {
            return Err(Error::Unsupported(path.to_owned()));
        }
        match entry.filemode() {
            0o040000 => {
                if filter.is_some_and(|filter| !filter.descend(&relative)) {
                    inventory.insert(relative, Content::Directory)?;
                    continue;
                }
                let child = objects.tree(entry.id(), path)?;
                if matches!(profile, TreeProfile::Release) && child.is_empty() {
                    return Err(Error::Unsupported(path.to_owned()));
                }
                inventory.insert(relative.clone(), Content::Directory)?;
                visit_filtered(
                    objects,
                    &child,
                    relative.as_str(),
                    inventory,
                    profile,
                    filter,
                )?;
            }
            0o100644 | 0o100755 => {
                let ceiling = inventory.file_ceiling();
                let object = objects.checked(
                    entry.id(),
                    ObjectType::Blob,
                    usize::try_from(ceiling).unwrap_or(usize::MAX),
                    path,
                )?;
                let bytes = object.len() as u64;
                let digest: [u8; 32] = Sha256::digest(object.data()).into();
                inventory.insert(
                    relative,
                    Content::File {
                        bytes,
                        sha256: digest.into(),
                    },
                )?;
            }
            _ => return Err(Error::Unsupported(path.to_owned())),
        }
    }
    Ok(())
}

fn git_error(operation: &'static str, path: &Path, error: git2::Error) -> Error {
    native_error(operation, path, io::Error::other(error))
}

fn native_error(operation: &'static str, path: &Path, source: io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}

fn invalid(path: &Path, reason: &'static str) -> Error {
    native_error(
        "verify Git source",
        path,
        io::Error::new(io::ErrorKind::InvalidData, reason),
    )
}
