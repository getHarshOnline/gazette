//! One bounded target-tree builder, shared by file and directory formats.

use crate::Error;
use gazette_core::source::{
    CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::PathBuf;

#[cfg(test)]
#[path = "../tests/internal/output.rs"]
mod tests;

/// Target-owned title metadata; authored frontmatter is never admitted here.
pub(super) fn titled_markdown(title: &str, body: &str) -> Result<String, Error> {
    let title = serde_json::to_string(title)?;
    Ok(format!("---\ntitle: {title}\n---\n\n{body}"))
}

/// Read only the exact scalar envelope emitted above from retained target bytes.
pub(super) fn native_markdown_title(source: &str) -> Option<String> {
    let (title, body) = source.strip_prefix("---\ntitle: ")?.split_once('\n')?;
    body.starts_with("---\n\n")
        .then(|| serde_json::from_str(title).ok())
        .flatten()
}

pub(super) fn json(
    value: &impl serde::Serialize,
    context: &str,
    ceiling: u64,
) -> Result<Vec<u8>, Error> {
    let limit = || {
        Error::Tree(gazette_core::source::Error::Limit {
            path: PathBuf::from(context),
            resource: "file or total bytes during JSON serialization",
        })
    };
    let mut writer = BoundedBuffer {
        bytes: Vec::new(),
        ceiling,
    };
    serde_json::to_writer_pretty(&mut writer, value).map_err(|error| {
        if error.is_io() {
            limit()
        } else {
            Error::Json(error)
        }
    })?;
    writer.write_all(b"\n").map_err(|_| limit())?;
    Ok(writer.bytes)
}

pub(super) struct BoundedBuffer {
    bytes: Vec<u8>,
    ceiling: u64,
}

impl BoundedBuffer {
    pub(super) fn new(ceiling: u64) -> Self {
        Self {
            bytes: Vec::new(),
            ceiling,
        }
    }
    pub(super) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.ceiling - self.bytes.len() as u64 {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "JSON byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct Output {
    inventory: InventoryBuilder,
    files: BTreeMap<EntryPath, Vec<u8>>,
    directories: BTreeSet<EntryPath>,
    fragment_navigation: bool,
    provider_companions: BTreeSet<EntryPath>,
}

impl Output {
    pub(super) fn new(limits: Limits) -> Self {
        Self {
            inventory: InventoryBuilder::new(limits),
            files: BTreeMap::new(),
            directories: BTreeSet::new(),
            fragment_navigation: false,
            provider_companions: BTreeSet::new(),
        }
    }

    pub(super) fn file_ceiling(&self) -> u64 {
        self.inventory.file_ceiling()
    }

    pub(super) fn provider_companion(&mut self, path: &str) -> Result<(), Error> {
        self.provider_companions.insert(path.parse()?);
        Ok(())
    }

    pub(super) fn provider_ignores(&self, existing: &[u8]) -> Result<Vec<u8>, Error> {
        let mut output = BoundedBuffer::new(self.file_ceiling());
        let limit = |_| crate::invalid(".mintignore", "provider ignore output ceiling exceeded");
        output.write_all(existing).map_err(limit)?;
        if !existing.is_empty() && !existing.ends_with(b"\n") {
            output.write_all(b"\n").map_err(limit)?;
        }
        for path in &self.provider_companions {
            output.write_all(b"/").map_err(limit)?;
            for character in path.as_str().chars() {
                if character.is_control() {
                    return Err(crate::invalid(
                        path.as_str(),
                        "control in provider ignore path",
                    ));
                }
                if matches!(character, '\\' | '*' | '?' | '[' | ']' | '!' | '#' | ' ') {
                    output.write_all(b"\\").map_err(limit)?;
                }
                output
                    .write_all(character.encode_utf8(&mut [0; 4]).as_bytes())
                    .map_err(limit)?;
            }
            output.write_all(b"\n").map_err(limit)?;
        }
        Ok(output.finish())
    }

    pub(super) fn require_fragment_navigation(&mut self) {
        self.fragment_navigation = true;
    }

    pub(super) fn file(&self, path: &str) -> Option<&[u8]> {
        let path: EntryPath = path.parse().ok()?;
        self.files.get(&path).map(Vec::as_slice)
    }

    pub(super) fn directory(&mut self, path: &str) -> Result<(), Error> {
        let path: EntryPath = path.parse()?;
        let mut parent = String::new();
        for component in path.as_str().split('/') {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(component);
            let directory: EntryPath = parent.parse()?;
            if self.directories.insert(directory.clone()) {
                self.inventory.insert(directory, Content::Directory)?;
            }
        }
        Ok(())
    }

    pub(super) fn reserve(&mut self, path: &str, bytes: &[u8]) -> Result<EntryPath, Error> {
        let path: EntryPath = path.parse()?;
        if let Some((parent, _)) = path.as_str().rsplit_once('/') {
            self.directory(parent)?;
        }
        self.inventory.insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )?;
        Ok(path)
    }

    pub(super) fn retain(&mut self, path: EntryPath, bytes: &[u8]) {
        self.files.insert(path, bytes.to_vec());
    }

    pub(super) fn add(&mut self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        let path = self.reserve(path, bytes)?;
        self.retain(path, bytes);
        Ok(())
    }

    pub(super) fn add_owned(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), Error> {
        let path = self.reserve(path, &bytes)?;
        self.files.insert(path, bytes);
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<CapturedSource, Error> {
        self.add(
            "gazette-provider-math.css",
            include_bytes!("provider-math.css"),
        )?;
        if self.fragment_navigation {
            self.add(
                "gazette-layout-navigation.js",
                include_bytes!("layout-navigation.js"),
            )?;
            self.add(
                "gazette-fragment-navigation.css",
                include_bytes!("fragment-navigation.css"),
            )?;
        }
        Ok(CapturedSource::from_parts(
            self.inventory.finish(),
            self.files,
        )?)
    }
}
