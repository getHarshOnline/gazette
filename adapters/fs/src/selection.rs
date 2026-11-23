//! Choose source entries before opening them; ordinary reads include everything.

use std::collections::BTreeSet;
use std::path::Path;

use gazette_core::source::{EntryPath, Error};
use gazette_ports::source::SourceFilter;

#[derive(Clone, Copy)]
pub(super) enum Selection<'a> {
    Excluding(&'a [EntryPath]),
    Including(&'a [EntryPath]),
    Filtered {
        exclude: &'a [EntryPath],
        filter: &'a dyn SourceFilter,
    },
}

impl Selection<'_> {
    pub(super) fn descend(self, path: &EntryPath) -> bool {
        match self {
            Self::Filtered { filter, .. } => filter.descend(path),
            _ => true,
        }
    }

    pub(super) fn ancestor_only(self, path: &str) -> bool {
        match self {
            Self::Including(paths) => !paths
                .iter()
                .any(|selected| below(path.as_bytes(), selected.as_str().as_bytes())),
            Self::Filtered { filter, .. } => filter.ancestor_only(path),
            Self::Excluding(_) => false,
        }
    }

    pub(super) fn admits(self, prefix: &str, name: &[u8]) -> bool {
        if matches!(self, Self::Excluding([])) {
            return true;
        }
        let mut path = prefix.as_bytes().to_vec();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(name);
        match self {
            Self::Excluding(paths) | Self::Filtered { exclude: paths, .. } => {
                !paths.iter().any(|p| p.as_str().as_bytes() == path)
            }
            Self::Including(paths) => paths.iter().any(|p| {
                let selected = p.as_str().as_bytes();
                // Ancestors are needed to reach a selection; descendants of a
                // selected directory belong to that complete directory tree.
                below(&path, selected) || below(selected, &path)
            }),
        }
    }

    pub(super) fn allows(self, path: &EntryPath) -> Result<bool, Error> {
        match self {
            Self::Filtered { filter, .. } => filter.allows(path),
            _ => Ok(true),
        }
    }
}

fn below(path: &[u8], parent: &[u8]) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|tail| tail.starts_with(b"/"))
}

pub(super) fn validate(root: &Path, paths: &[EntryPath]) -> Result<(), Error> {
    if paths.is_empty()
        || paths.len() > 128
        || paths.iter().collect::<BTreeSet<_>>().len() != paths.len()
    {
        return Err(Error::InvalidInventory {
            path: root.to_owned(),
            reason: "source selections require 1–128 unique relative paths",
        });
    }
    Ok(())
}
