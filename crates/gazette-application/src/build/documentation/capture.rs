//! Capture manual navigation and linked companions before assembling a target.

use super::*;
use crate::{
    build::binding,
    configuration::ResolvedWorkspace,
    source::{AcquiredInput, Batch, BatchError, SourceSelection},
};
use gazette_core::{
    documentation::projection::{MANUAL_PROFILE, ManualDestinations},
    markdown,
    source::{EntryPath, Error, RevisionLimits, Subdirectory},
};
use gazette_ports::{markdown::Markdown, source::SourceFilter};

#[derive(Default)]
struct ManualPlan {
    pages: BTreeMap<EntryPath, EntryPath>,
    inputs: Vec<Selection>,
}

pub(super) fn acquire(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    markdown: &dyn Markdown,
    configuration: ResolvedWorkspace,
    site: &Key,
    limits: PreparationLimits,
) -> Result<AcquiredInputs, DocumentationError> {
    let workspace = &configuration.compilation().workspace;
    workspace
        .require_resolved_facts()
        .map_err(CompositionError::Facts)?;
    let (_, _, build) = selection(workspace, site)?;
    let mut manuals = BTreeMap::<Key, ManualPlan>::new();
    let mut references = BTreeSet::new();
    let mut generators = BTreeSet::new();
    for module in &build.modules {
        if let Some(manual) = &module.manual {
            let plan = manuals.entry(manual.source.clone()).or_default();
            plan.inputs
                .extend(manual.assets.iter().map(|asset| asset.input.clone()));
            for item in &module.navigation {
                if let DocumentationNavigation::Manual { pages, .. } = item {
                    for page in pages {
                        let Selection::File { path } = &page.input else {
                            unreachable!("validated manual file")
                        };
                        if plan
                            .pages
                            .insert(path.clone(), page.route.clone())
                            .is_some()
                        {
                            return Err(invalid(
                                path,
                                "manual source page is selected more than once",
                            ));
                        }
                        plan.inputs.push(page.input.clone());
                    }
                }
            }
        }
        references.extend(
            module
                .navigation
                .iter()
                .filter_map(DocumentationNavigation::reference)
                .map(|(_, _, input)| input.source().clone()),
        );
    }
    for module in &build.modules {
        for (_, _, input) in module
            .navigation
            .iter()
            .filter_map(DocumentationNavigation::reference)
        {
            match input {
                DocumentationInput::Generated { source, .. } => {
                    generators.insert(source.clone());
                }
                DocumentationInput::Artifact {
                    source, selection, ..
                } => {
                    if let Some(plan) = manuals.get_mut(source) {
                        plan.inputs.push(selection.clone());
                    }
                }
            }
        }
    }
    let selected: BTreeSet<_> = manuals
        .keys()
        .cloned()
        .chain(references.iter().cloned())
        .collect();
    if selected.len() > limits.sources {
        return Err(CompositionError::TooManySources {
            limit: limits.sources,
        }
        .into());
    }
    // Resolve every native coordinate before the first source read.
    let mut requests = BTreeMap::new();
    for key in selected {
        let source = workspace
            .source(&key)
            .ok_or_else(|| CompositionError::MissingBinding(key.clone()))?;
        let request = binding::selection(&configuration, &source.location).map_err(|error| {
            CompositionError::SourceRoot {
                binding: key.clone(),
                error,
            }
        })?;
        requests.insert(key, request);
    }
    let mut batch = Batch::new(limits);
    let mut sources = BTreeMap::new();
    let mut catalogs = BTreeMap::new();
    for (key, request) in requests {
        let input = if let Some(plan) = manuals.get(&key).filter(|_| !generators.contains(&key)) {
            let captured = capture_manual(
                reader,
                revisions,
                markdown,
                &key,
                &request,
                plan,
                batch.remaining_limits(),
            )?;
            batch.retain(captured)
        } else {
            // A reference/generator source retains its independently configured input contract.
            batch.acquire(reader, revisions, &request)
        }
        .map_err(|error| match error {
            BatchError::Native(error) => CompositionError::Source {
                binding: key.clone(),
                error,
            },
            BatchError::Aggregate(resource) => CompositionError::Aggregate {
                binding: key.clone(),
                resource,
            },
        })?;
        if manuals.contains_key(&key) {
            catalogs.insert(
                key.clone(),
                super::catalog::discover(reader, revisions, &request, limits.per_source)
                    .map_err(|error| error.to_string()),
            );
        }
        sources.insert(key, input);
    }
    Ok(AcquiredInputs::from_documentation_sources(
        configuration,
        site.clone(),
        sources,
        catalogs,
    ))
}

fn capture_manual(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    markdown: &dyn Markdown,
    key: &Key,
    source: &SourceSelection,
    plan: &ManualPlan,
    limits: RevisionLimits,
) -> Result<AcquiredInput, DocumentationError> {
    let (root, subtree) = coordinates(source);
    let branch = match source {
        SourceSelection::IncludedFilesystem {
            expected_branch, ..
        }
        | SourceSelection::WorkingRepository {
            expected_branch, ..
        } => expected_branch.as_deref(),
        _ => None,
    };
    let native = |error| {
        DocumentationError::Composition(CompositionError::Source {
            binding: key.clone(),
            error,
        })
    };
    if let Some(branch) = branch {
        revisions.verify_branch(root, branch).map_err(native)?;
    }
    if let SourceSelection::IncludedFilesystem { include, .. } = source {
        // Every explicit include remains required, even when no manual page uses it.
        // Observe those entries and their ancestors without descending selected trees.
        let required: Vec<_> = include
            .iter()
            .cloned()
            .map(|path| Selection::File { path })
            .collect();
        let filter = ManualFilter {
            inputs: &required,
            source,
            ignore: None,
        };
        let listing = reader
            .list_filtered(
                root,
                None,
                &filter,
                source::ListingLimits {
                    entries: limits.metadata_bytes,
                    depth: limits.content.depth,
                    path_bytes: limits.metadata_bytes,
                },
            )
            .map_err(native)?;
        for path in include {
            if !listing.entries().contains_key(path) {
                return Err(native(Error::InvalidInventory {
                    path: path.as_str().into(),
                    reason: "required source selection is missing",
                }));
            }
        }
    }
    let ignore = working_filter(revisions, source, limits).map_err(native)?;
    let mut selected = plan.inputs.clone();
    let filter = ManualFilter {
        inputs: &selected,
        source,
        ignore: ignore.as_deref(),
    };
    let captured = reader
        .capture_filtered(root, subtree.as_ref(), &[], &filter, limits.content)
        .map_err(native)?;
    for input in &selected {
        input.content(&captured.as_input()).map_err(|_| {
            invalid(
                input.path().map_or("manual", EntryPath::as_str),
                "selected manual input is absent, ignored or outside its source bounds",
            )
        })?;
    }
    let mut catalog = ManualDestinations::default();
    for (input, route) in &plan.pages {
        catalog
            .page(input, route)
            .map_err(DocumentationError::Target)?;
    }
    let mut companions = BTreeSet::new();
    for path in plan.pages.keys() {
        let bytes = captured
            .file(path)
            .ok_or_else(|| invalid(path, "selected manual page is not a file"))?;
        let text =
            std::str::from_utf8(bytes).map_err(|_| invalid(path, "manual page must be UTF-8"))?;
        let document = markdown
            .parse(text, MANUAL_PROFILE, markdown::Limits::default())
            .map_err(documentation::Error::Markdown)
            .map_err(DocumentationError::Target)?;
        for link in &document.syntax().links {
            if let Some(companion) = catalog
                .linked_companion(path, &link.destination)
                .map_err(DocumentationError::Target)?
                && captured.file(&companion).is_none()
            {
                companions.insert(companion);
            }
        }
    }
    let captured = if companions.is_empty() {
        captured
    } else {
        let extra: Vec<_> = companions
            .into_iter()
            .map(|path| Selection::File { path })
            .collect();
        let filter = ManualFilter {
            inputs: &extra,
            source,
            ignore: ignore.as_deref(),
        };
        let shared_ancestors = captured
            .inventory()
            .entries()
            .iter()
            .filter(|entry| {
                entry.content == Content::Directory
                    && extra.iter().any(|selection| {
                        selection
                            .path()
                            .is_some_and(|path| below(path.as_str(), entry.path.as_str()))
                    })
            })
            .count();
        let remaining = source::Limits {
            entries: limits.content.entries - captured.inventory().entries().len()
                + shared_ancestors,
            total_bytes: limits.content.total_bytes - captured.inventory().total_bytes(),
            ..limits.content
        };
        let linked = reader
            .capture_filtered(root, subtree.as_ref(), &[], &filter, remaining)
            .map_err(native)?;
        for input in &extra {
            input.content(&linked.as_input()).map_err(|_| {
                invalid(
                    input.path().map_or("manual", EntryPath::as_str),
                    "linked manual companion is absent, ignored or outside its source bounds",
                )
            })?;
        }
        selected.extend(extra);
        merge(captured, linked, limits.content).map_err(native)?
    };
    if let Some(branch) = branch {
        revisions.verify_branch(root, branch).map_err(native)?;
    }
    let filter = ManualFilter {
        inputs: &selected,
        source,
        ignore: None,
    };
    crate::source::bind_selected(revisions, source, &filter, limits, captured).map_err(native)
}

pub(super) fn working_filter(
    revisions: &impl RevisionReader,
    source: &SourceSelection,
    limits: RevisionLimits,
) -> Result<Option<Box<dyn SourceFilter>>, Error> {
    if let SourceSelection::Submodule {
        parent,
        submodule_path,
        subdirectory,
        ..
    } = source
    {
        return revisions.working_source_filter(
            &parent.join(submodule_path.to_string()),
            Some(subdirectory),
            limits,
        );
    }
    let (root, subtree) = coordinates(source);
    let filter = revisions.working_source_filter(root, subtree.as_ref(), limits)?;
    if filter.is_none() && matches!(source, SourceSelection::WorkingRepository { .. }) {
        return Err(Error::Unsupported(root.to_owned()));
    }
    Ok(filter)
}

pub(super) fn coordinates(source: &SourceSelection) -> (&Path, Option<Subdirectory>) {
    match source {
        SourceSelection::Filesystem { root, subdirectory }
        | SourceSelection::WorkingRepository {
            root, subdirectory, ..
        } => (root, subdirectory.clone()),
        SourceSelection::FilteredFilesystem { root, .. }
        | SourceSelection::IncludedFilesystem { root, .. } => (root, None),
        SourceSelection::Repository {
            root, subdirectory, ..
        } => (root, Some(subdirectory.clone())),
        SourceSelection::Submodule {
            parent,
            submodule_path,
            subdirectory,
            ..
        } => (parent, Some(submodule_path.join(subdirectory))),
    }
}

pub(super) struct ManualFilter<'a> {
    pub(super) inputs: &'a [Selection],
    pub(super) source: &'a SourceSelection,
    pub(super) ignore: Option<&'a dyn SourceFilter>,
}

fn below(path: &str, parent: &str) -> bool {
    path.strip_prefix(parent)
        .is_some_and(|tail| tail.starts_with('/'))
}

impl SourceFilter for ManualFilter<'_> {
    fn descend(&self, path: &EntryPath) -> bool {
        self.inputs.iter().any(|input| match input {
            Selection::Directory { path: None } => true,
            Selection::File { path: selected } => below(selected.as_str(), path.as_str()),
            Selection::Directory {
                path: Some(selected),
            } => {
                path == selected
                    || below(selected.as_str(), path.as_str())
                    || below(path.as_str(), selected.as_str())
            }
        })
    }

    fn ancestor_only(&self, path: &str) -> bool {
        if let SourceSelection::IncludedFilesystem { include, .. } = self.source
            && !include
                .iter()
                .any(|selected| path == selected.as_str() || below(path, selected.as_str()))
        {
            return true;
        }
        !self.inputs.iter().any(|input| match input {
            Selection::Directory { path: None } => true,
            Selection::File { path: selected } => path == selected.as_str(),
            Selection::Directory {
                path: Some(selected),
            } => path == selected.as_str() || below(path, selected.as_str()),
        })
    }

    fn allows(&self, path: &EntryPath) -> Result<bool, Error> {
        let path_text = path.as_str();
        let selected = self.inputs.iter().any(|input| match input {
            Selection::Directory { path: None } => true,
            Selection::File { path } | Selection::Directory { path: Some(path) } => {
                path_text == path.as_str()
                    || below(path.as_str(), path_text)
                    || (matches!(input, Selection::Directory { .. })
                        && below(path_text, path.as_str()))
            }
        });
        if !selected {
            return Ok(false);
        }
        match self.source {
            SourceSelection::IncludedFilesystem { include, .. }
                if !include.iter().any(|item| {
                    path == item
                        || below(path_text, item.as_str())
                        || below(item.as_str(), path_text)
                }) =>
            {
                return Ok(false);
            }
            SourceSelection::FilteredFilesystem { exclude, .. }
                if exclude
                    .iter()
                    .any(|item| path == item || below(path_text, item.as_str())) =>
            {
                return Ok(false);
            }
            _ => {}
        }
        self.ignore.map_or(Ok(true), |ignore| ignore.allows(path))
    }
}

fn merge(
    first: CapturedSource,
    second: CapturedSource,
    limits: source::Limits,
) -> Result<CapturedSource, Error> {
    let (first_inventory, first_files) = first.into_parts();
    let (second_inventory, second_files) = second.into_parts();
    let mut entries = BTreeMap::new();
    for entry in first_inventory
        .entries()
        .iter()
        .chain(second_inventory.entries())
    {
        if let Some(previous) = entries.insert(entry.path.clone(), entry.content)
            && (previous != entry.content || previous != Content::Directory)
        {
            return Err(Error::Changed(PathBuf::from(entry.path.as_str())));
        }
    }
    let mut inventory = InventoryBuilder::new(limits);
    for (path, content) in entries {
        inventory.insert(path, content)?;
    }
    let mut files = first_files.into_files();
    files.extend(second_files.into_files());
    CapturedSource::from_parts(inventory.finish(), files)
}

fn invalid(path: impl fmt::Display, reason: &'static str) -> DocumentationError {
    DocumentationError::Target(documentation::Error::Invalid {
        context: path.to_string(),
        reason,
    })
}
