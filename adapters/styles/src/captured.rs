//! Grass filesystem projection over one immutable, logical source tree.

use std::{
    cell::RefCell,
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

use gazette_core::source::{Content, EntryPath, TreeInput};

pub(crate) struct Files<'a, 'input> {
    source: &'a TreeInput<'input>,
    reads: RefCell<BTreeSet<EntryPath>>,
    failure: RefCell<Option<String>>,
}

impl std::fmt::Debug for Files<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedStylesheets")
            .finish_non_exhaustive()
    }
}

impl<'a, 'input> Files<'a, 'input> {
    pub(crate) fn new(source: &'a TreeInput<'input>) -> Self {
        Self {
            source,
            reads: RefCell::new(BTreeSet::new()),
            failure: RefCell::new(None),
        }
    }

    pub(crate) fn dependencies(&self) -> BTreeSet<EntryPath> {
        self.reads.borrow().clone()
    }

    pub(crate) fn failure(&self) -> Option<String> {
        self.failure.borrow_mut().take()
    }

    fn read_checked(&self, path: &Path) -> io::Result<Vec<u8>> {
        let path = logical(path).filter(supported).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported stylesheet source path",
            )
        })?;
        let bytes = self
            .source
            .file(&path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "stylesheet is not captured"))?;
        let text = std::str::from_utf8(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{path}: stylesheet is not UTF-8"),
            )
        })?;
        if !self.reads.borrow().contains(&path) {
            super::imports::validate(&path, text)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        self.reads.borrow_mut().insert(path);
        Ok(bytes.to_vec())
    }
}

pub(crate) fn supported(path: &EntryPath) -> bool {
    matches!(
        path.as_str().rsplit_once('.').map(|(_, suffix)| suffix),
        Some("scss" | "sass" | "css")
    )
}

// Resolve Sass's relative probes lexically, never through host canonicalize.
// URL syntax, absolute paths and escaping parents are not source identities.
pub(crate) fn logical(path: &Path) -> Option<EntryPath> {
    let text = path.to_str()?;
    if text.starts_with('/') || text.contains(['\\', ':', '?', '#', '%']) {
        return None;
    }
    let mut parts = Vec::new();
    for part in text.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            "" => return None,
            value => parts.push(value),
        }
    }
    parts.join("/").parse().ok()
}

impl grass::Fs for Files<'_, '_> {
    fn is_dir(&self, path: &Path) -> bool {
        self.failure.borrow().is_none()
            && logical(path).is_some_and(|path| {
                self.source
                    .inventory()
                    .entries()
                    .iter()
                    .any(|entry| entry.path == path && matches!(entry.content, Content::Directory))
            })
    }

    fn is_file(&self, path: &Path) -> bool {
        self.failure.borrow().is_none()
            && logical(path)
                .is_some_and(|path| supported(&path) && self.source.file(&path).is_some())
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        // Grass 0.13.4 assumes transitive errors are Raw(span), and panics on
        // Fs IoError instead. Preserve the first exact failure outside Grass;
        // feed only an inert rejection directive, never refused source bytes.
        // Compiler::compile checks this latch before observing either its
        // result or output. No subsequent captured source is read after failure.
        if self.failure.borrow().is_none() {
            match self.read_checked(path) {
                Ok(bytes) => return Ok(bytes),
                Err(error) => *self.failure.borrow_mut() = Some(error.to_string()),
            }
        }
        Ok(b"@error 'captured stylesheet rejected';".to_vec())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        logical(path)
            .map(|path| PathBuf::from(path.as_str()))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid logical stylesheet path",
                )
            })
    }
}
