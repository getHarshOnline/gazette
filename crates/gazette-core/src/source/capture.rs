//! Retained source bytes whose paths, lengths and identities match an inventory.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use super::{Content, EntryPath, Error, Inventory, Sha256Digest};

/// Owned immutable file payloads. Debug output never prints their contents.
pub struct FileContents(BTreeMap<EntryPath, Vec<u8>>);

impl FileContents {
    /// Consume the immutable payload holder without copying bytes.
    ///
    /// Changed selections must be rebound through `CapturedSource::from_parts`;
    /// the returned map carries no inventory or verification proof.
    pub fn into_files(self) -> BTreeMap<EntryPath, Vec<u8>> {
        self.0
    }

    /// Borrow every captured regular file in exact path order, without payload copies.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&EntryPath, &[u8])> {
        self.0.iter().map(|(path, bytes)| (path, bytes.as_slice()))
    }

    /// Borrow a captured file; directories and absent paths return none.
    pub fn get(&self, path: &EntryPath) -> Option<&[u8]> {
        self.0.get(path).map(Vec::as_slice)
    }
}

impl fmt::Debug for FileContents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileContents")
            .field("file_count", &self.0.len())
            .finish()
    }
}

/// One structurally valid observation plus its exact retained file bytes.
///
/// This is not revision verification, publication admission or a durable receipt.
/// The inventory and payloads are checked together and expose no mutable access.
#[derive(Debug)]
pub struct CapturedSource {
    inventory: Inventory,
    contents: FileContents,
}

impl CapturedSource {
    /// Complete inventory bound to the retained payloads.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Borrow captured bytes without reopening the native source.
    pub fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.contents.get(path)
    }

    /// Borrow this complete immutable capture for a synchronous port without copying payloads.
    /// Construction already proved the inventory/file relationship; neither can change here.
    pub fn as_input(&self) -> TreeInput<'_> {
        TreeInput {
            inventory: &self.inventory,
            files: self
                .contents
                .iter()
                .map(|(path, bytes)| (path.clone(), bytes))
                .collect(),
        }
    }

    /// Verify retained payloads before allowing them across the reader boundary.
    ///
    /// # Errors
    ///
    /// Rejects extra/missing payloads, directory payloads or length/digest mismatch.
    /// This independently checks the retained bytes, not just reader-supplied hashes.
    pub fn from_parts(
        inventory: Inventory,
        files: BTreeMap<EntryPath, Vec<u8>>,
    ) -> Result<Self, Error> {
        verify_files(
            &inventory,
            files.iter().map(|(path, bytes)| (path, bytes.as_slice())),
        )?;
        Ok(Self {
            inventory,
            contents: FileContents(files),
        })
    }

    /// Move the checked observation and immutable payloads into a verified phase.
    pub fn into_parts(self) -> (Inventory, FileContents) {
        (self.inventory, self.contents)
    }
}

/// A checked borrowed tree for synchronous consumers, without copying payloads.
pub struct TreeInput<'a> {
    inventory: &'a Inventory,
    files: BTreeMap<EntryPath, &'a [u8]>,
}

impl<'a> TreeInput<'a> {
    /// Bind the exact complete file set to its structural inventory.
    ///
    /// # Errors
    ///
    /// Rejects extra/missing files, directory payloads and length/digest mismatch.
    pub fn new(
        inventory: &'a Inventory,
        files: BTreeMap<EntryPath, &'a [u8]>,
    ) -> Result<Self, Error> {
        verify_files(inventory, files.iter().map(|(path, bytes)| (path, *bytes)))?;
        Ok(Self { inventory, files })
    }

    /// Complete immutable expected tree.
    pub fn inventory(&self) -> &Inventory {
        self.inventory
    }

    /// Borrow the already checked bytes for a regular file.
    pub fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.files.get(path).copied()
    }
}

fn verify_files<'a>(
    inventory: &Inventory,
    mut retained: impl Iterator<Item = (&'a EntryPath, &'a [u8])>,
) -> Result<(), Error> {
    for entry in inventory.entries() {
        if let Content::File { bytes, sha256 } = entry.content {
            let (path, data) = retained
                .next()
                .ok_or_else(|| invalid(&entry.path, "missing captured file"))?;
            if path != &entry.path {
                return Err(invalid(
                    &entry.path,
                    "captured file set differs from the inventory",
                ));
            }
            if data.len() as u64 != bytes || Sha256Digest::of(data) != sha256 {
                return Err(invalid(
                    &entry.path,
                    "captured bytes differ from the inventory",
                ));
            }
        }
    }
    if let Some((path, _)) = retained.next() {
        return Err(invalid(path, "extra captured file or directory payload"));
    }
    Ok(())
}

fn invalid(path: &EntryPath, reason: &'static str) -> Error {
    Error::InvalidInventory {
        path: PathBuf::from(path.as_str()),
        reason,
    }
}
