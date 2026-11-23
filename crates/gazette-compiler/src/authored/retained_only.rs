//! Exact retention dispositions preserve corpus bytes without granting publication use.

use std::collections::BTreeMap;

use gazette_core::{publication::Key, source::EntryPath};

use super::{Error, PageResource};
use crate::{
    corpus::{File, ParsedCorpus},
    workspace::Compilation,
};

pub(super) fn validate(
    compilation: &Compilation,
    corpus: &ParsedCorpus<'_>,
    asset_metadata: &BTreeMap<EntryPath, u64>,
    maximum: usize,
) -> Result<(), Error> {
    let mut count = 0usize;
    for source in compilation.workspace.content_bindings() {
        let Some(binding) = source.tree_binding() else {
            continue;
        };
        for entry in &binding.retained_only {
            let path = source
                .logical_path(entry)
                .map_err(|_| Error::RetainedOnly {
                    path: entry.clone(),
                    reason: "invalid exact source-relative coordinate",
                })?;
            count = count
                .checked_add(1)
                .filter(|count| *count <= maximum)
                .ok_or_else(|| Error::RetainedOnly {
                    path: path.clone(),
                    reason: "retained-only admission exceeds the input-count ceiling",
                })?;
            let reason = match corpus.files().get(&path) {
                Some(File::Asset(_)) => None,
                Some(_) => Some("entry is Markdown or a notebook, not a regular opaque input"),
                None if asset_metadata.contains_key(&path) => None,
                None => Some("entry is absent or not a retained regular file"),
            };
            if let Some(reason) = reason {
                return Err(Error::RetainedOnly { path, reason });
            }
        }
    }
    Ok(())
}

pub(super) fn dependencies(
    compilation: &Compilation,
    paths: &BTreeMap<Key, EntryPath>,
    resources: &BTreeMap<Key, Vec<PageResource>>,
    notebooks: &BTreeMap<Key, Vec<EntryPath>>,
) -> Result<(), Error> {
    for (owner, owner_path) in paths {
        let Some(source) = compilation.workspace.content_source(owner_path) else {
            continue;
        };
        for entry in resources
            .get(owner)
            .into_iter()
            .flatten()
            .flat_map(|resource| &resource.inputs)
            .chain(notebooks.get(owner).into_iter().flatten())
        {
            let path = source
                .logical_path(entry)
                .map_err(|_| Error::RetainedOnly {
                    path: entry.clone(),
                    reason: "dependency has an invalid source-relative coordinate",
                })?;
            if source.is_retained_only(&path) {
                return Err(Error::RetainedOnly {
                    path,
                    reason: "retained-only entry is a declared resource, program or notebook dependency",
                });
            }
        }
    }
    Ok(())
}
