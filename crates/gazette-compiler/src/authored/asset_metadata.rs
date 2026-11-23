//! Asset availability for authoring is separate from captured publication input.

use super::Error;
use crate::{corpus::ParsedCorpus, workspace::Compilation};
use gazette_core::{publication::content, source::EntryPath};
use std::collections::BTreeMap;

pub(super) fn validate(
    compilation: &Compilation,
    corpus: &ParsedCorpus<'_>,
    assets: &BTreeMap<EntryPath, u64>,
    limits: content::Limits,
) -> Result<(), Error> {
    let mut path_bytes = 0usize;
    for (index, path) in assets.keys().enumerate() {
        let fail = |reason| Error::AssetMetadata {
            path: path.clone(),
            reason,
        };
        if index >= limits.nodes {
            return Err(fail("asset observation count exceeds the limit"));
        }
        path_bytes = path_bytes
            .checked_add(path.as_str().len())
            .filter(|bytes| *bytes <= limits.text_bytes)
            .ok_or_else(|| fail("asset observation paths exceed the limit"))?;
        if corpus.files().contains_key(path) {
            return Err(fail("input has both captured bytes and an observation"));
        }
        if compilation.workspace.content_source(path).is_none() {
            return Err(fail("input has no configured content source"));
        }
        if [
            ".md",
            ".markdown",
            ".ipynb",
            ".landing.yaml",
            ".landing.yml",
        ]
        .iter()
        .any(|suffix| path.as_str().to_ascii_lowercase().ends_with(suffix))
        {
            return Err(fail("document and notebook syntax requires captured bytes"));
        }
    }
    Ok(())
}
