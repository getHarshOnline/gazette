//! Exact local output transitions, separate from release and durable ownership.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Component, Path, PathBuf},
};

use super::Error;
use crate::source::{Content, EntryPath, Inventory, InventoryBuilder, Limits, TreeInput};

/// Stable native device/inode facts, not a public identity or ownership token.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct OutputIdentity {
    device: u64,
    inode: u64,
}

impl OutputIdentity {
    /// Compose identity facts obtained from an already held native handle.
    pub fn new(device: u64, inode: u64) -> Self {
        Self { device, inode }
    }

    /// Observed native device coordinate, not a host-independent identifier.
    pub fn device(self) -> u64 {
        self.device
    }

    /// Observed native inode coordinate, not protection against inode reuse.
    pub fn inode(self) -> u64 {
        self.inode
    }
}

/// Kind and identity of the separately held root Git entry, never its mutable metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub enum OutputAdministrationBinding {
    /// An untraversed administrative directory.
    Directory(OutputIdentity),
    /// An uninterpreted worktree pointer file.
    File(OutputIdentity),
}

/// Private native destination evidence from one checked observation.
///
/// Serialization retains private observations, not reusable permission or ownership.
/// There is deliberately no caller-deserialization contract. The adapter proves these
/// facts through held handles; later operations must independently reobserve them.
/// Neither equality nor persistence establishes cross-host fencing or inode-reuse safety.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct OutputBinding {
    canonical_root: PathBuf,
    root: OutputIdentity,
    administration: Option<OutputAdministrationBinding>,
}

impl OutputBinding {
    /// Compose checked native facts; this constructor does not inspect the filesystem.
    ///
    /// # Errors
    /// Refuses a relative or parent-traversing canonical coordinate.
    pub fn new(
        canonical_root: PathBuf,
        root: OutputIdentity,
        administration: Option<OutputAdministrationBinding>,
    ) -> Result<Self, Error> {
        if !canonical_root.is_absolute()
            || canonical_root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(crate::source::Error::InvalidInventory {
                path: canonical_root,
                reason: "output binding requires a canonical absolute root",
            }
            .into());
        }
        Ok(Self {
            canonical_root,
            root,
            administration,
        })
    }

    /// Canonical private native coordinate bound to the held root.
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    /// Stable identity of the held root, excluding timestamps and permissions.
    pub fn root(&self) -> OutputIdentity {
        self.root
    }

    /// Separately held Git entry identity, or its proved absence.
    pub fn administration(&self) -> Option<OutputAdministrationBinding> {
        self.administration
    }
}

/// Complete output inventory bound to the native root/admin observation that produced it.
///
/// This is synchronous evidence, not permission, ownership or an atomic snapshot.
#[derive(Debug, Eq, PartialEq)]
pub struct OutputObservation {
    inventory: Inventory,
    binding: OutputBinding,
}

impl OutputObservation {
    /// Combine a checked inventory with its native adapter observation.
    pub fn new(inventory: Inventory, binding: OutputBinding) -> Self {
        Self { inventory, binding }
    }

    /// Complete non-administrative output tree, including preserved user files.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Private stable native root and administrative-entry facts.
    pub fn binding(&self) -> &OutputBinding {
        &self.binding
    }

    /// Move successful observation evidence without cloning or reconstructing it.
    pub fn into_parts(self) -> (Inventory, OutputBinding) {
        (self.inventory, self.binding)
    }
}

/// Exact proposed entry transition; not generated ownership or write permission.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct OutputChange {
    /// Original root-relative spelling.
    pub path: EntryPath,
    /// Expected content, or absence before creation.
    pub before: Option<Content>,
    /// Intended content, or absence after removing an owned generated file.
    pub after: Option<Content>,
}

/// Checked transition between complete observations, with retained output bytes.
///
/// Construction checks the transition, not the caller's ownership or permission.
/// The application must establish those separately before invoking a writer.
pub struct OutputMutation<'a> {
    root: &'a Path,
    protected: &'a [PathBuf],
    observed: &'a OutputObservation,
    expected: &'a Inventory,
    generated: TreeInput<'a>,
    changes: &'a [OutputChange],
    limits: Limits,
}

impl<'a> OutputMutation<'a> {
    /// Validate a complete exact transition without performing any native effect.
    ///
    /// # Errors
    /// Rejects inconsistent before/after sets, directory replacement/removal,
    /// Git paths, missing retained payloads and exhausted complete-tree bounds.
    pub fn new(
        root: &'a Path,
        protected: &'a [PathBuf],
        observed: &'a OutputObservation,
        expected: &'a Inventory,
        generated: TreeInput<'a>,
        changes: &'a [OutputChange],
        limits: Limits,
    ) -> Result<Self, Error> {
        let invalid = || crate::source::Error::InvalidInventory {
            path: root.to_owned(),
            reason: "inconsistent output transition",
        };
        let mut entries: BTreeMap<_, _> = observed
            .inventory()
            .entries()
            .iter()
            .map(|entry| (entry.path.clone(), entry.content))
            .collect();
        let desired: BTreeMap<_, _> = generated
            .inventory()
            .entries()
            .iter()
            .map(|entry| (&entry.path, entry.content))
            .collect();
        let mut last = None;
        for change in changes {
            if change
                .path
                .as_str()
                .split('/')
                .any(|part| part.eq_ignore_ascii_case(".git"))
                || last.is_some_and(|last| last >= &change.path)
                || entries.get(&change.path).copied() != change.before
                || change.before == change.after
                || change.before == Some(Content::Directory)
                || (change.before.is_some() && change.after == Some(Content::Directory))
                || change
                    .after
                    .is_some_and(|after| desired.get(&change.path) != Some(&after))
            {
                return Err(invalid().into());
            }
            last = Some(&change.path);
            match change.after {
                Some(content) => {
                    entries.insert(change.path.clone(), content);
                }
                None => {
                    entries.remove(&change.path);
                }
            }
        }
        let mut complete = InventoryBuilder::new(limits);
        for (path, content) in entries {
            if path
                .as_str()
                .split('/')
                .any(|part| part.eq_ignore_ascii_case(".git"))
            {
                return Err(invalid().into());
            }
            complete.insert(path, content)?;
        }
        let expected_entries: BTreeMap<_, _> = expected
            .entries()
            .iter()
            .map(|entry| (&entry.path, entry.content))
            .collect();
        if &complete.finish() != expected
            || desired
                .iter()
                .any(|(path, content)| expected_entries.get(path) != Some(content))
        {
            return Err(invalid().into());
        }
        Ok(Self {
            root,
            protected,
            observed,
            expected,
            generated,
            changes,
            limits,
        })
    }

    /// Exact native destination, not a public URL.
    pub fn root(&self) -> &Path {
        self.root
    }
    /// Input roots that must remain disjoint from effects.
    pub fn protected(&self) -> &[PathBuf] {
        self.protected
    }
    /// Complete pre-effect observation.
    pub fn observed(&self) -> &OutputObservation {
        self.observed
    }
    /// Complete intended tree, including unchanged user entries.
    pub fn expected(&self) -> &Inventory {
        self.expected
    }
    /// Already retained payload for a selected generated file.
    pub fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.generated.file(path)
    }
    /// Ordered exact transitions; no inferred removal.
    pub fn changes(&self) -> &[OutputChange] {
        self.changes
    }
    /// Complete observation ceilings.
    pub fn limits(&self) -> Limits {
        self.limits
    }
}

/// Failed local materialization with precise partial-effect evidence.
///
/// Attempted paths may have changed even when the native call returned an error.
/// Residue is a locator, never permission to delete or retry automatically.
#[derive(Debug)]
pub struct OutputWriteError {
    /// Native or invariant cause.
    pub cause: Error,
    /// Ordered paths whose mutating operation may have started.
    pub attempted: Vec<EntryPath>,
    /// Exact temporary entry preserved when its outcome could not be settled.
    pub residue: Option<PathBuf>,
}

impl fmt::Display for OutputWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "output materialization failed: {}; {} path effects attempted",
            self.cause,
            self.attempted.len()
        )?;
        if let Some(path) = &self.residue {
            write!(f, "; preserved residue at {path:?}")?;
        }
        Ok(())
    }
}
impl std::error::Error for OutputWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
