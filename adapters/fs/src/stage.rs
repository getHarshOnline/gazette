//! Fresh private stage creation, exact read-back, and identity-checked cleanup.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use gazette_core::source::{
    Content, EntryPath, Error as SourceError, Inventory, InventoryBuilder, Limits, Subdirectory,
    TreeInput,
};
use gazette_core::stage::{Address, Error, Location, RetainedStage};
use gazette_ports::destination::RootSeparation;
use gazette_ports::stage::{StagePlanner, StageRetention, StageTarget, StageWriter};
use rustix::fs::{self, AtFlags, Mode, OFlags};

use super::destination::DestinationRoots;
use super::synchronization::synchronize_entry;
use super::{FileSystem, io_error, read_directory};

/// A newly created private tree whose complete bytes passed read-back verification.
///
/// Keep this guard alive while reading the stage. Call `close` for observable
/// cleanup or `retain` for explicit synchronized retention. Drop makes a
/// best-effort cleanup attempt only. Temporary writes are fully read back but
/// are not synchronized to durable storage until `retain`. A process crash or failed
/// cleanup can leave residue, which is never adopted or removed by a later run
/// merely because of its name. This is not an immutable artifact, a durable
/// ownership manifest, or publication permission.
/// The caller must keep the staging parent under trusted exclusive control.
/// Identity checks detect interference but are not atomic protection against
/// hostile processes running as the same user or a privileged user.
#[must_use = "keep the stage guard alive, then explicitly close or retain it"]
pub struct StagedTree {
    tree: Option<OwnedTree>,
    path: PathBuf,
    inventory: Inventory,
}

impl StagedTree {
    /// Ephemeral native path; valid only while the guard and observation remain valid.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Exact complete tree observed after writing, not a later freshness guarantee.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Synchronize and retain this exact stage without copying or renaming it.
    ///
    /// Consumes the cleanup guard before checking or synchronizing anything. On
    /// success all files, descendant directories, the root and its parent have
    /// passed native synchronization, with complete read-back before and after.
    /// The returned evidence has no automatic cleanup behavior. Keep its inventory
    /// independently and reverify bytes before subsequent use.
    ///
    /// This requires the same trusted exclusive parent control as creation. It
    /// is not certification, publication, a recovery journal or hardware power-loss
    /// qualification. It never replaces, repairs or adopts an existing tree.
    ///
    /// # Errors
    ///
    /// Every failure preserves the tree and reports unresolved residue, including
    /// a changed identity, nonprivate mode, hard-linked file or failed flush. No
    /// cleanup is attempted: a failed receipt is not proof that retention failed.
    pub fn retain(mut self) -> Result<RetainedStage, Error> {
        let tree = self.tree.take().ok_or_else(|| Error::Residue {
            cause: SourceError::Changed(self.path.clone()),
            path: self.path.clone(),
            cleanup: None,
        })?;
        let inventory = tree
            .synchronize(&self.inventory)
            .map_err(|cause| Error::Residue {
                cause,
                path: tree.path.clone(),
                cleanup: None,
            })?;
        Ok(RetainedStage::new(tree.path, inventory))
    }

    /// Remove only this unchanged tree, reporting rather than hiding residue.
    ///
    /// # Errors
    ///
    /// Leaves residue on detected replacement, changed/extra content or native
    /// cleanup failure. The returned locator is not future deletion authority.
    pub fn close(mut self) -> Result<(), Error> {
        if let Some(tree) = self.tree.take() {
            tree.cleanup(Some(&self.inventory))
                .map_err(|cause| Error::Residue {
                    cause,
                    path: self.path.clone(),
                    cleanup: None,
                })?;
        }
        Ok(())
    }
}

impl StageRetention for StagedTree {
    fn retain(self) -> Result<RetainedStage, Error> {
        StagedTree::retain(self)
    }
}

impl fmt::Debug for StagedTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StagedTree")
            .field("path", &self.path)
            .field("identity", &self.inventory.identity())
            .finish()
    }
}

impl Drop for StagedTree {
    fn drop(&mut self) {
        if let Some(tree) = self.tree.take() {
            let _ = tree.cleanup(Some(&self.inventory));
        }
    }
}

impl RootSeparation for FileSystem {
    fn check_root_separation(
        &self,
        parent: &Path,
        protected_roots: &[PathBuf],
    ) -> Result<(), gazette_core::destination::Error> {
        observe_stage_parent(parent, protected_roots).map(|_| ())
    }
}

fn observe_stage_parent(
    parent: &Path,
    protected_roots: &[PathBuf],
) -> Result<DestinationRoots, gazette_core::destination::Error> {
    let roots = DestinationRoots::observe(parent, protected_roots)?;
    roots.recheck()?;
    Ok(roots)
}

/// Held native parent/source observations, not reconstructible from its address.
/// Planning has no write or cleanup effect. Creation consumes this target once
/// and requires the same trusted exclusive namespace as ordinary staging.
pub struct PlannedStage {
    address: Address,
    roots: DestinationRoots,
}

impl fmt::Debug for PlannedStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlannedStage")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl StagePlanner for FileSystem {
    type Target = PlannedStage;

    fn plan_stage(
        &self,
        parent: &Path,
        name: &str,
        protected_roots: &[PathBuf],
    ) -> Result<PlannedStage, Error> {
        validate_name(name)?;
        let roots = observe_stage_parent(parent, protected_roots)?;
        let address = Address {
            parent: roots.canonical().to_owned(),
            name: name.to_owned(),
        };
        // The address is an intent locator, not a promise that its child is absent.
        // The one atomic creation attempt owns collision refusal; planning must
        // remain possible when a journal needs to reconcile an existing locator.
        roots.recheck()?;
        Ok(PlannedStage { address, roots })
    }
}

impl StageTarget for PlannedStage {
    type Stage = StagedTree;

    fn address(&self) -> &Address {
        &self.address
    }

    fn create(self, input: TreeInput<'_>, limits: Limits) -> Result<StagedTree, Error> {
        check_limits(&input, limits)?;
        create_stage(self.roots, Some(&self.address.name), input, limits, None)
    }
}

fn validate_name(name: &str) -> Result<(), SourceError> {
    let selected: Subdirectory = name
        .parse()
        .map_err(|_| SourceError::InvalidName(name.into()))?;
    if selected.components().count() != 1 {
        return Err(SourceError::InvalidName(name.into()));
    }
    Ok(())
}

impl StageWriter for FileSystem {
    type Stage = StagedTree;

    fn stage(
        &self,
        location: Location<'_>,
        protected_roots: &[PathBuf],
        input: TreeInput<'_>,
        limits: Limits,
    ) -> Result<StagedTree, Error> {
        let name = match location {
            Location::Fresh(_) | Location::FreshBounded { .. } => None,
            Location::Named { name, .. } | Location::NamedBounded { name, .. } => {
                validate_name(name)?;
                Some(name)
            }
        };
        check_limits(&input, limits)?;
        let directory_bytes = match location {
            Location::FreshBounded {
                directory_bytes, ..
            }
            | Location::NamedBounded {
                directory_bytes, ..
            } => {
                if !(1..=1 << 20).contains(&directory_bytes) {
                    return Err(SourceError::InvalidInventory {
                        path: location.parent().into(),
                        reason: "invalid stage directory byte bound",
                    }
                    .into());
                }
                Some(directory_bytes)
            }
            _ => None,
        };
        let roots = observe_stage_parent(location.parent(), protected_roots)?;
        create_stage(roots, name, input, limits, directory_bytes)
    }
}

fn check_limits(input: &TreeInput<'_>, limits: Limits) -> Result<(), Error> {
    let mut bounds = InventoryBuilder::new(limits);
    for entry in input.inventory().entries() {
        bounds.insert(entry.path.clone(), entry.content)?;
    }
    Ok(())
}

fn create_stage(
    roots: DestinationRoots,
    name: Option<&str>,
    input: TreeInput<'_>,
    limits: Limits,
    directory_bytes: Option<usize>,
) -> Result<StagedTree, Error> {
    roots.recheck()?;
    let mut tree = OwnedTree::create(roots, limits, name, directory_bytes)?;
    let result = tree
        .write(&input)
        .and_then(|()| tree.verify(input.inventory()));
    match result {
        Ok(inventory) => Ok(StagedTree {
            path: tree.path.clone(),
            tree: Some(tree),
            inventory,
        }),
        Err(cause) => Err(tree.failure(cause)),
    }
}

struct Node {
    path: EntryPath,
    identity: (u64, u64),
    directory: bool,
}

struct OwnedTree {
    roots: DestinationRoots,
    root: File,
    name: String,
    path: PathBuf,
    identity: (u64, u64),
    nodes: Vec<Node>,
    limits: Limits,
}

impl OwnedTree {
    fn synchronize(&self, expected: &Inventory) -> Result<Inventory, SourceError> {
        self.verify(expected)?;
        // Inventory order puts each parent before its descendants. Reverse order
        // persists child data and namespace before the containing directory.
        for node in self.nodes.iter().rev() {
            self.check_root()?;
            self.check_node(node)?;
            let (parent, name) = parent_and_name(&self.root, &node.path)?;
            let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
            if node.directory {
                flags |= OFlags::DIRECTORY;
            }
            let file = File::from(fs::openat(&parent, name, flags, Mode::empty()).map_err(
                |e| {
                    io_error(
                        "open staged entry for synchronization",
                        Path::new(node.path.as_str()),
                        e,
                    )
                },
            )?);
            synchronize_entry(
                &file,
                Path::new(node.path.as_str()),
                node.identity,
                node.directory,
            )?;
            self.check_node(node)?;
        }
        synchronize_entry(&self.root, &self.path, self.identity, true)?;
        self.roots
            .directory()
            .sync_all()
            .map_err(|e| io_error("synchronize stage parent", self.roots.canonical(), e))?;
        self.verify(expected)
    }

    fn verify(&self, expected: &Inventory) -> Result<Inventory, SourceError> {
        self.roots.recheck()?;
        self.check_root()?;
        let observed = read_directory(&self.root, self.limits, None)?;
        if &observed != expected {
            return Err(SourceError::ContentMismatch(self.path.clone()));
        }
        for node in &self.nodes {
            self.check_node(node)?;
        }
        self.roots.recheck()?;
        self.check_root()?;
        Ok(observed)
    }

    fn failure(self, cause: SourceError) -> Error {
        match self.cleanup(None) {
            Ok(()) => Error::Operation(cause),
            Err(cleanup) => Error::Residue {
                cause,
                path: self.path,
                cleanup: Some(Box::new(cleanup)),
            },
        }
    }

    fn create(
        roots: DestinationRoots,
        limits: Limits,
        requested_name: Option<&str>,
        directory_bytes: Option<usize>,
    ) -> Result<Self, Error> {
        // Randomness selects an exclusive ephemeral locator, never artifact identity.
        // Existing names are never opened, reused, removed or treated as owned.
        let attempts = if requested_name.is_some() { 1 } else { 16 };
        for _ in 0..attempts {
            let name = if let Some(name) = requested_name {
                name.to_owned()
            } else {
                let mut random = [0_u8; 16];
                getrandom::fill(&mut random).map_err(|e| {
                    io_error(
                        "choose private stage name",
                        roots.canonical(),
                        io::Error::other(e.to_string()),
                    )
                })?;
                format!(
                    ".gazette-stage-{}",
                    random
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                )
            };
            let path = roots.canonical().join(&name);
            if directory_bytes
                .is_some_and(|maximum| path.as_os_str().as_encoded_bytes().len() > maximum)
            {
                return Err(SourceError::InvalidInventory {
                    path,
                    reason: "stage directory byte bound exceeded before creation",
                }
                .into());
            }
            match fs::mkdirat(
                roots.directory(),
                name.as_str(),
                Mode::RUSR | Mode::WUSR | Mode::XUSR,
            ) {
                Err(rustix::io::Errno::EXIST) if requested_name.is_none() => continue,
                Err(rustix::io::Errno::EXIST) => {
                    return Err(io_error(
                        "refuse existing named stage",
                        &path,
                        rustix::io::Errno::EXIST,
                    )
                    .into());
                }
                Err(e) => return Err(Error::Operation(io_error("create private stage", &path, e))),
                Ok(()) => (),
            }
            let opened = open_child(roots.directory(), &name).and_then(|root| {
                let metadata = root
                    .metadata()
                    .map_err(|e| io_error("inspect private stage", &path, e))?;
                Ok((root, (metadata.dev(), metadata.ino())))
            });
            let (root, identity) = opened.map_err(|cause| Error::Residue {
                cause,
                path: path.clone(),
                cleanup: None,
            })?;
            return Ok(Self {
                roots,
                root,
                name,
                path,
                identity,
                nodes: Vec::new(),
                limits,
            });
        }
        Err(Error::Operation(io_error(
            "create unique stage",
            roots.canonical(),
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "private stage-name retry limit reached",
            ),
        )))
    }

    fn check_root(&self) -> Result<(), SourceError> {
        self.roots.recheck_target()?;
        let stat = fs::statat(
            self.roots.directory(),
            self.name.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|e| io_error("recheck stage identity", &self.path, e))?;
        if self.identity != (stat.st_dev as u64, stat.st_ino as u64) {
            return Err(SourceError::Changed(self.path.clone()));
        }
        Ok(())
    }

    fn write(&mut self, input: &TreeInput<'_>) -> Result<(), SourceError> {
        for entry in input.inventory().entries() {
            self.check_root()?;
            let (parent, name) = parent_and_name(&self.root, &entry.path)?;
            let directory = matches!(entry.content, Content::Directory);
            let mut file = if directory {
                fs::mkdirat(&parent, name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(|e| {
                    io_error("create staged directory", Path::new(entry.path.as_str()), e)
                })?;
                open_child(&parent, name)?
            } else {
                File::from(
                    fs::openat(
                        &parent,
                        name,
                        OFlags::WRONLY
                            | OFlags::CREATE
                            | OFlags::EXCL
                            | OFlags::NOFOLLOW
                            | OFlags::CLOEXEC,
                        Mode::RUSR | Mode::WUSR,
                    )
                    .map_err(|e| {
                        io_error("create staged file", Path::new(entry.path.as_str()), e)
                    })?,
                )
            };
            let metadata = file
                .metadata()
                .map_err(|e| io_error("inspect staged entry", Path::new(entry.path.as_str()), e))?;
            self.nodes.push(Node {
                path: entry.path.clone(),
                identity: (metadata.dev(), metadata.ino()),
                directory,
            });
            if !directory {
                let bytes = input
                    .file(&entry.path)
                    .ok_or_else(|| SourceError::ContentMismatch(entry.path.as_str().into()))?;
                file.write_all(bytes)
                    .and_then(|()| file.flush())
                    .map_err(|e| {
                        io_error(
                            "write and flush staged file",
                            Path::new(entry.path.as_str()),
                            e,
                        )
                    })?;
            }
        }
        Ok(())
    }

    fn cleanup(&self, expected: Option<&Inventory>) -> Result<(), SourceError> {
        self.check_root()?;
        let observed = read_directory(&self.root, self.limits, None)?;
        if expected.is_some_and(|expected| expected != &observed)
            || observed.entries().len() != self.nodes.len()
        {
            return Err(SourceError::Changed(self.path.clone()));
        }
        for (entry, node) in observed.entries().iter().zip(&self.nodes) {
            if entry.path != node.path
                || matches!(entry.content, Content::Directory) != node.directory
            {
                return Err(SourceError::Changed(self.path.clone()));
            }
            self.check_node(node)?;
        }
        for node in self.nodes.iter().rev() {
            self.check_node(node)?;
            let (parent, name) = parent_and_name(&self.root, &node.path)?;
            fs::unlinkat(
                &parent,
                name,
                if node.directory {
                    AtFlags::REMOVEDIR
                } else {
                    AtFlags::empty()
                },
            )
            .map_err(|e| io_error("remove owned stage entry", Path::new(node.path.as_str()), e))?;
        }
        self.check_root()?;
        fs::unlinkat(
            self.roots.directory(),
            self.name.as_str(),
            AtFlags::REMOVEDIR,
        )
        .map_err(|e| io_error("remove owned private stage", &self.path, e))?;
        Ok(())
    }

    fn check_node(&self, node: &Node) -> Result<(), SourceError> {
        let (parent, name) = parent_and_name(&self.root, &node.path)?;
        let stat = fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|e| {
            io_error(
                "recheck owned stage entry",
                Path::new(node.path.as_str()),
                e,
            )
        })?;
        if node.identity != (stat.st_dev as u64, stat.st_ino as u64) {
            return Err(SourceError::Changed(node.path.as_str().into()));
        }
        Ok(())
    }
}

fn open_child(parent: &File, name: &str) -> Result<File, SourceError> {
    fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| io_error("open staged directory", Path::new(name), e))
}

pub(super) fn parent_and_name<'a>(
    root: &File,
    path: &'a EntryPath,
) -> Result<(File, &'a str), SourceError> {
    let mut parts = path.as_str().split('/').peekable();
    let mut parent = root
        .try_clone()
        .map_err(|e| io_error("hold stage directory", Path::new(path.as_str()), e))?;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return Ok((parent, part));
        }
        parent = open_child(&parent, part)?;
    }
    Err(SourceError::InvalidName(path.as_str().into()))
}

#[cfg(test)]
#[path = "../tests/internal/stage.rs"]
mod tests;
