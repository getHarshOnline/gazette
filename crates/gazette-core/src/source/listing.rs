//! File availability for discovery, separate from captured content and its identity.

use std::{collections::BTreeMap, path::PathBuf};

use super::{EntryPath, Error};

/// Metadata work and retained path ceilings; file payload size is irrelevant.
#[derive(Clone, Copy, Debug)]
pub struct ListingLimits {
    /// Maximum files and directories beneath the selected root.
    pub entries: usize,
    /// Maximum descendant depth; immediate children have depth one.
    pub depth: usize,
    /// Maximum aggregate UTF-8 bytes of retained relative paths.
    pub path_bytes: usize,
}

impl Default for ListingLimits {
    fn default() -> Self {
        Self {
            entries: 10_000,
            depth: 64,
            path_bytes: 4 << 20,
        }
    }
}

/// An observed entry kind, with no captured bytes or content digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListedEntry {
    /// A directory, including an empty one.
    Directory,
    /// A regular file; its observed size does not establish its current contents.
    File {
        /// Size reported by the confined file handle, without reading its bytes.
        observed_bytes: u64,
    },
}

/// A bounded path catalog. A later read must independently check native content.
#[derive(Debug)]
pub struct Listing(BTreeMap<EntryPath, ListedEntry>);

impl Listing {
    /// Exact relative paths in lexical order, including directory ancestors.
    pub fn entries(&self) -> &BTreeMap<EntryPath, ListedEntry> {
        &self.0
    }
}

/// Structural validation for metadata-only source discovery.
pub struct ListingBuilder {
    entries: BTreeMap<EntryPath, ListedEntry>,
    path_bytes: usize,
    limits: ListingLimits,
}

impl ListingBuilder {
    /// Begin an empty catalog under explicit bounds.
    pub fn new(limits: ListingLimits) -> Self {
        Self {
            entries: BTreeMap::new(),
            path_bytes: 0,
            limits,
        }
    }

    /// Check an entry before opening its handle.
    ///
    /// # Errors
    /// Rejects duplicate paths, missing directory parents and resource overflow.
    pub fn check_path(&self, path: &EntryPath) -> Result<(), Error> {
        let invalid = |reason| Error::InvalidInventory {
            path: PathBuf::from(path.as_str()),
            reason,
        };
        if self.entries.contains_key(path) {
            return Err(invalid("duplicate entry path"));
        }
        if let Some((parent, _)) = path.as_str().rsplit_once('/')
            && self.entries.get(parent) != Some(&ListedEntry::Directory)
        {
            return Err(invalid("parent is absent or is not a directory"));
        }
        for (exceeded, resource) in [
            (self.entries.len() >= self.limits.entries, "entry count"),
            (
                path.as_str().split('/').count() > self.limits.depth,
                "depth",
            ),
            (
                path.as_str().len() > self.limits.path_bytes.saturating_sub(self.path_bytes),
                "path bytes",
            ),
        ] {
            if exceeded {
                return Err(Error::Limit {
                    path: path.as_str().into(),
                    resource,
                });
            }
        }
        Ok(())
    }

    /// Admit one observed entry; failure leaves the catalog unchanged.
    ///
    /// # Errors
    /// Rejects inconsistent paths and exceeded metadata bounds.
    pub fn insert(&mut self, path: EntryPath, entry: ListedEntry) -> Result<(), Error> {
        self.check_path(&path)?;
        self.path_bytes += path.as_str().len();
        self.entries.insert(path, entry);
        Ok(())
    }

    /// Finish only after every selected entry has been observed successfully.
    pub fn finish(self) -> Listing {
        Listing(self.entries)
    }
}
