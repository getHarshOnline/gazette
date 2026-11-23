//! Translate an already-resolved source coordinate into a native read request.

use crate::source::SourceSelection;
use gazette_compiler::workspace::SourceLocation;

pub(super) fn selection(
    configuration: &crate::configuration::ResolvedWorkspace,
    binding: &SourceLocation,
) -> Result<SourceSelection, crate::configuration::SourceRootUnavailable> {
    match binding {
        SourceLocation::Filesystem { root, subdirectory } => Ok(SourceSelection::Filesystem {
            root: configuration.source_root(root)?,
            subdirectory: subdirectory.clone(),
        }),
        SourceLocation::FilteredFilesystem { root, exclude } => {
            Ok(SourceSelection::FilteredFilesystem {
                root: configuration.source_root(root)?,
                exclude: exclude.clone(),
            })
        }
        SourceLocation::IncludedFilesystem {
            root,
            include,
            expected_branch,
        } => Ok(SourceSelection::IncludedFilesystem {
            root: configuration.source_root(root)?,
            include: include.clone(),
            expected_branch: expected_branch.clone(),
        }),
        SourceLocation::WorkingRepository {
            root,
            subdirectory,
            expected_branch,
        } => Ok(SourceSelection::WorkingRepository {
            root: configuration.source_root(root)?,
            subdirectory: subdirectory.clone(),
            expected_branch: expected_branch.clone(),
        }),
        SourceLocation::Repository {
            root,
            commit,
            subdirectory,
        } => Ok(SourceSelection::Repository {
            root: configuration.source_root(root)?,
            commit: commit.clone(),
            subdirectory: subdirectory.clone(),
        }),
        SourceLocation::Submodule {
            root,
            parent_commit,
            submodule_path,
            subdirectory,
        } => Ok(SourceSelection::Submodule {
            parent: configuration.source_root(root)?,
            parent_commit: parent_commit.clone(),
            submodule_path: submodule_path.clone(),
            subdirectory: subdirectory.clone(),
        }),
    }
}
