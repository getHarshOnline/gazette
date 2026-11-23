//! Canonical structural observations built under one set of resource ceilings.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use super::{Content, Entry, EntryPath, Error, Limits, TreeIdentity};

/// A structurally consistent observation, not proof of native bytes or permission.
///
/// Paths are unique and sorted, every nested entry has an explicit directory
/// parent, and total bytes are derived from its files. Only the builder creates
/// this value; consumers cannot mutate entries independently of the total.
#[derive(Debug, Serialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct Inventory {
    entries: Vec<Entry>,
    total_bytes: u64,
    identity: TreeIdentity,
}

impl Inventory {
    /// Descendants in lexical path order, including explicit empty directories.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Sum of all regular-file byte counts.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Versioned structural identity derived from the immutable complete entries.
    pub fn identity(&self) -> TreeIdentity {
        self.identity
    }
}

/// Incremental structural validation shared by every native source reader.
///
/// Insert directories before their children. Native readers must still prove
/// byte identity, confinement and stability, and propagate any read failure
/// instead of finishing a partial observation.
pub struct InventoryBuilder {
    entries: BTreeMap<EntryPath, Content>,
    total_bytes: u64,
    limits: Limits,
}

impl InventoryBuilder {
    /// Begin one observation with the caller's declared ceilings.
    pub fn new(limits: Limits) -> Self {
        Self {
            entries: BTreeMap::new(),
            total_bytes: 0,
            limits,
        }
    }

    /// Maximum bytes the next file may contain under both file and total limits.
    pub fn file_ceiling(&self) -> u64 {
        self.limits
            .file_bytes
            .min(self.limits.total_bytes - self.total_bytes)
    }

    /// Capacity for an additional inventory at this same root. Per-file/depth
    /// ceilings remain unchanged; only aggregate bytes and entry count decrease.
    /// This reserves nothing: merging still checks paths, collisions and limits.
    pub fn remaining_limits(&self) -> Limits {
        Limits {
            entries: self.limits.entries - self.entries.len(),
            total_bytes: self.limits.total_bytes - self.total_bytes,
            ..self.limits
        }
    }

    /// Check a prospective entry before opening or decoding its native content.
    ///
    /// # Errors
    ///
    /// Rejects duplicates, missing/non-directory parents, and depth/count limits.
    pub fn check_path(&self, path: &EntryPath) -> Result<(), Error> {
        let invalid = |reason| Error::InvalidInventory {
            path: PathBuf::from(path.as_str()),
            reason,
        };
        if self.entries.contains_key(path) {
            return Err(invalid("duplicate entry path"));
        }
        if let Some((parent, _)) = path.as_str().rsplit_once('/')
            && !matches!(self.entries.get(parent), Some(Content::Directory))
        {
            return Err(invalid("parent is absent or is not a directory"));
        }
        if path.as_str().split('/').count() > self.limits.depth {
            return Err(Error::Limit {
                path: PathBuf::from(path.as_str()),
                resource: "depth",
            });
        }
        if self.entries.len() >= self.limits.entries {
            return Err(Error::Limit {
                path: PathBuf::from(path.as_str()),
                resource: "entry count",
            });
        }
        Ok(())
    }

    /// Admit one complete entry; rejection leaves this builder unchanged.
    ///
    /// # Errors
    ///
    /// Propagates structural/entry limits or rejects file/total byte overflow.
    pub fn insert(&mut self, path: EntryPath, content: Content) -> Result<(), Error> {
        self.check_path(&path)?;
        let bytes = match &content {
            Content::Directory => 0,
            Content::File { bytes, .. } => *bytes,
        };
        if matches!(content, Content::File { .. }) && bytes > self.file_ceiling() {
            return Err(Error::Limit {
                path: PathBuf::from(path.as_str()),
                resource: "file or total bytes",
            });
        }
        // file_ceiling proves that this addition stays within the selected u64 total.
        self.total_bytes += bytes;
        self.entries.insert(path, content);
        Ok(())
    }

    /// Freeze the complete observation; call only after all native reads succeed.
    pub fn finish(self) -> Inventory {
        let entries: Vec<_> = self
            .entries
            .into_iter()
            .map(|(path, content)| Entry { path, content })
            .collect();
        let identity = TreeIdentity::from_entries(&entries);
        Inventory {
            entries,
            total_bytes: self.total_bytes,
            identity,
        }
    }
}
