//! Eligible Markdown names are metadata observations, never captured page bodies.
use super::*;
use crate::source::SourceSelection;
use gazette_core::source::{
    EntryPath, Error, ListedEntry, ListingLimits, MetadataBudget, RevisionLimits,
};
use gazette_ports::source::SourceFilter;

pub(super) fn discover(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    source: &SourceSelection,
    limits: RevisionLimits,
) -> Result<Vec<EntryPath>, Error> {
    let (root, subtree) = super::capture::coordinates(source);
    let ignore = match source {
        SourceSelection::Repository { .. } | SourceSelection::Submodule { .. } => None,
        _ => super::capture::working_filter(revisions, source, limits)?,
    };
    let all = [Selection::Directory { path: None }];
    let filter = CatalogFilter(super::capture::ManualFilter {
        inputs: &all,
        source,
        ignore: ignore.as_deref(),
    });
    let listing_limits = ListingLimits {
        // Every entry has a nonempty path charged to this metadata allowance.
        entries: limits.metadata_bytes,
        depth: limits.content.depth,
        path_bytes: limits.metadata_bytes,
    };
    let mut metadata = MetadataBudget::new(limits.metadata_bytes);
    let listing = match source {
        SourceSelection::Repository {
            root,
            subdirectory,
            commit,
        } => revisions.list_revision_filtered(
            root,
            subdirectory,
            commit,
            &filter,
            listing_limits,
            &mut metadata,
        )?,
        SourceSelection::Submodule {
            parent,
            parent_commit,
            submodule_path,
            subdirectory,
        } => {
            let commit =
                revisions.read_gitlink(parent, parent_commit, submodule_path, &mut metadata)?;
            let child_root = parent.join(submodule_path.to_string());
            let listing = revisions.list_revision_filtered(
                &child_root,
                subdirectory,
                &commit,
                &filter,
                listing_limits,
                &mut metadata,
            )?;
            let after =
                revisions.read_gitlink(parent, parent_commit, submodule_path, &mut metadata)?;
            if after != commit {
                return Err(Error::Changed(PathBuf::from(submodule_path.to_string())));
            }
            listing
        }
        SourceSelection::IncludedFilesystem {
            expected_branch: Some(branch),
            ..
        }
        | SourceSelection::WorkingRepository {
            expected_branch: Some(branch),
            ..
        } => {
            revisions.verify_branch(root, branch)?;
            let listing = reader.list_filtered(root, subtree.as_ref(), &filter, listing_limits)?;
            revisions.verify_branch(root, branch)?;
            listing
        }
        _ => reader.list_filtered(root, subtree.as_ref(), &filter, listing_limits)?,
    };
    Ok(listing
        .entries()
        .iter()
        .filter(|(path, kind)| {
            path.as_str().ends_with(".md") && matches!(kind, ListedEntry::File { .. })
        })
        .map(|(path, _)| path.clone())
        .collect())
}

struct CatalogFilter<'a>(super::capture::ManualFilter<'a>);
impl SourceFilter for CatalogFilter<'_> {
    fn descend(&self, path: &EntryPath) -> bool {
        self.0.descend(path)
    }

    fn ancestor_only(&self, path: &str) -> bool {
        self.0.ancestor_only(path)
    }

    fn allows(&self, path: &EntryPath) -> Result<bool, Error> {
        if path.as_str().split('/').any(|part| part == ".git") {
            return Ok(false);
        }
        self.0.allows(path)
    }
}
