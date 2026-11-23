//! Report eligible Markdown omitted from the configured manual and reference inputs.

use super::*;

impl DocumentationAssembly<'_> {
    /// Sources whose bounded metadata catalogue could not be completed.
    /// An empty omitted-page list does not establish completeness for these sources.
    pub fn unselected_manual_pages_incomplete(&self) -> BTreeMap<&Key, &str> {
        self.inputs
            .manual_catalogs
            .iter()
            .filter_map(|(source, result)| {
                result.as_ref().err().map(|error| (source, error.as_str()))
            })
            .collect()
    }

    /// Eligible Markdown names omitted from pages, assets and reference inputs.
    /// Metadata enumeration reads no unselected page bodies; inspect incomplete catalogs too.
    /// Names are grouped by their owning source and do not add chapters to navigation.
    pub fn unselected_manual_pages(&self) -> BTreeMap<&Key, Vec<&source::EntryPath>> {
        let sources: BTreeSet<_> = self.manual.iter().map(|item| item.source).collect();
        let mut omitted = BTreeMap::new();
        for source in sources {
            let selected = self
                .manual
                .iter()
                .filter(|item| item.source == source)
                .map(|item| item.selection)
                .chain(
                    self.artifacts
                        .iter()
                        .filter(|item| item.source == source && item.generation.is_none())
                        .map(|item| item.selection),
                )
                .collect::<Vec<_>>();
            let available: Vec<_> = match self.inputs.manual_catalogs.get(source) {
                Some(Ok(paths)) => paths.iter().collect(),
                Some(Err(_)) => Vec::new(),
                None => self.inputs.sources()[source]
                    .inventory()
                    .entries()
                    .iter()
                    .filter(|entry| {
                        matches!(entry.content, Content::File { .. })
                            && entry.path.as_str().ends_with(".md")
                    })
                    .map(|entry| &entry.path)
                    .collect(),
            };
            let pages: Vec<_> = available
                .into_iter()
                .filter(|path| !selected.iter().any(|selection| includes(selection, path)))
                .collect();
            if !pages.is_empty() {
                omitted.insert(source, pages);
            }
        }
        omitted
    }
}

fn includes(selection: &Selection, input: &source::EntryPath) -> bool {
    match selection {
        Selection::File { path } => path == input,
        Selection::Directory { path: None } => true,
        Selection::Directory { path: Some(path) } => input
            .as_str()
            .strip_prefix(path.as_str())
            .is_some_and(|tail| tail.starts_with('/')),
    }
}
