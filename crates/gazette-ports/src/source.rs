//! Local source access contracts implemented by native adapters.

use gazette_core::source::{
    CapturedSource, CommitId, EntryPath, Error, Inventory, Limits, Listing, ListingLimits,
    MetadataBudget, RevisionLimits, Subdirectory,
};
use std::path::Path;

/// Confined metadata discovery without reading or hashing regular-file payloads.
///
/// Include hidden entries, refuse links and special files, enforce metadata
/// bounds, and fail the whole listing on errors or observed directory changes.
/// A listing establishes available paths, never captured content or permission.
pub trait SourceListing {
    /// List a complete selected directory, resolving its optional subdirectory
    /// beneath the held root without following links.
    ///
    /// # Errors
    /// Rejects unsupported, changing or over-limit entries.
    fn list(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: ListingLimits,
    ) -> Result<Listing, Error>;

    /// List only 1–128 unique literal selections, their ancestors and descendants.
    /// Unselected siblings are not opened; every selection must exist.
    ///
    /// # Errors
    /// Rejects missing, unsupported, changing or over-limit entries.
    fn list_including(
        &self,
        root: &Path,
        include: &[EntryPath],
        limits: ListingLimits,
    ) -> Result<Listing, Error>;

    /// List a tree except 1–128 unique literal paths and their descendants.
    ///
    /// # Errors
    /// Rejects invalid exclusions, unsupported, changing or over-limit entries.
    fn list_excluding(
        &self,
        root: &Path,
        exclude: &[EntryPath],
        limits: ListingLimits,
    ) -> Result<Listing, Error>;
}

/// Read-only source boundary; implementations own native access and confinement.
///
/// A reader must enforce the selected limits, reject links and special files,
/// include hidden entries, and fail rather than return a partial observation.
/// No database, network, process execution or source writes are permitted.
/// Successful observation is not an immutable snapshot or publication admission.
/// Construct the inventory through the shared builder; ordering and structural
/// invariants belong to that core contract, not an adapter-local implementation.
pub trait SourceReader {
    /// Read only the selected directory, resolving each subdirectory component
    /// beneath the held root without following symlinks.
    ///
    /// # Errors
    ///
    /// Returns an error for unreadable, unsupported, changing or over-limit input.
    fn read(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<Inventory, Error>;
}

/// Confined in-memory capture, distinct from an inventory-only observation.
pub trait SourceCapture: SourceReader {
    /// List filtered metadata without reading file payloads; unsupported providers fail closed.
    ///
    /// # Errors
    /// Refuses unsafe or changing entries, exhausted metadata bounds or unsupported listing.
    fn list_filtered(
        &self,
        root: &Path,
        _subdirectory: Option<&Subdirectory>,
        _filter: &dyn SourceFilter,
        _limits: ListingLimits,
    ) -> Result<Listing, Error> {
        Err(Error::Unsupported(root.to_owned()))
    }

    /// Retain bytes from the same confined handles used to observe the source.
    ///
    /// Retained payload bytes obey the caller's file/total limits. No source,
    /// staging, database, network or process writes are permitted.
    ///
    /// # Errors
    ///
    /// Propagates source failures or rejects inconsistent captured payloads.
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error>;
    /// Capture only the named root-relative files or directories and their descendants.
    ///
    /// Include necessary directory ancestors, preserving root-relative paths. Require
    /// 1–128 unique literal selections and fail if any selected path is missing.
    /// Unselected siblings are not opened. Selected entries retain the ordinary
    /// capture limits, symlink refusal and change detection. Unsupported adapters
    /// return an error without reading the source.
    ///
    /// # Errors
    /// Rejects unsupported selection, missing paths, unsafe or changing input and limits.
    fn capture_including(
        &self,
        root: &Path,
        _include: &[EntryPath],
        _limits: Limits,
    ) -> Result<CapturedSource, Error> {
        Err(Error::Unsupported(root.to_owned()))
    }

    /// Capture a tree while omitting exact root-relative paths and their descendants.
    ///
    /// Exclusions are literal [`EntryPath`] values, not globs or ignore-file rules.
    /// All other entries, including hidden entries, obey the ordinary capture
    /// contract. Unsupported adapters fail closed rather than silently capturing
    /// excluded files. The default implementation performs no source reads.
    ///
    /// # Errors
    /// Reject unsupported selection, unsafe paths, changing inputs and limits.
    fn capture_excluding(
        &self,
        root: &Path,
        _exclude: &[EntryPath],
        _limits: Limits,
    ) -> Result<CapturedSource, Error> {
        Err(Error::Unsupported(root.to_owned()))
    }

    /// Capture a selected tree through an independently supplied path filter.
    /// Literal exclusions apply first; rejected entries are never opened.
    /// Ordinary capture and explicitly included selections remain separate operations.
    ///
    /// # Errors
    /// Refuses unsupported filtering, invalid exclusions and ordinary capture failures.
    fn capture_filtered(
        &self,
        root: &Path,
        _subdirectory: Option<&Subdirectory>,
        _exclude: &[EntryPath],
        _filter: &dyn SourceFilter,
        _limits: Limits,
    ) -> Result<CapturedSource, Error> {
        Err(Error::Unsupported(root.to_owned()))
    }
}

/// Native selection of working paths, independent of their captured contents.
pub trait SourceFilter {
    /// Whether an admitted directory's children are selected for examination.
    /// False retains the directory entry without enumerating or opening its children.
    fn descend(&self, _path: &EntryPath) -> bool {
        true
    }

    /// Whether this admitted path is only an ancestor and must be a directory.
    fn ancestor_only(&self, _path: &str) -> bool {
        false
    }

    /// Whether one path relative to the selected subtree belongs in the capture.
    ///
    /// # Errors
    /// Propagates a failure to establish the current selection.
    fn allows(&self, path: &EntryPath) -> Result<bool, Error>;
}

/// Exact-file local capture used by explicit configuration manifests.
///
/// Implementations must resolve every supplied [`EntryPath`] beneath one held
/// root without following links, reject duplicates and return exactly those files.
/// Unselected siblings are neither read nor admitted to the resulting inventory.
pub trait SelectedSourceCapture {
    /// Capture the exact selected files and their necessary directory ancestors.
    ///
    /// # Errors
    /// Rejects duplicates, absent/non-file selections, links, changes and limits.
    fn capture_selected(
        &self,
        root: &Path,
        paths: &[EntryPath],
        limits: Limits,
    ) -> Result<CapturedSource, Error>;
}

/// Local committed-tree reader; no network, checkout, filters or mutation.
pub trait RevisionReader {
    /// Select tracked and nonignored working inputs at an exact repository root.
    /// The optional subtree retains the root repository's ignore rules. Administrative
    /// Git entries are omitted. No repository means no implicit filter; ancestors are
    /// never searched. This observes working selection, not revision identity.
    ///
    /// # Errors
    /// Refuses an invalid repository, unavailable ignore rules or exhausted metadata bounds.
    fn working_source_filter(
        &self,
        _root: &Path,
        _subdirectory: Option<&Subdirectory>,
        _limits: RevisionLimits,
    ) -> Result<Option<Box<dyn SourceFilter>>, Error> {
        Ok(None)
    }

    /// Check a literal local branch at the exact worktree root, without checkout.
    ///
    /// A detached or unborn HEAD, an invalid name, an ancestor repository or a
    /// different branch fails. This checks no commit or working-byte identity.
    /// Unsupported readers fail before source capture.
    ///
    /// # Errors
    /// Returns an error if the branch cannot be established or does not match.
    fn verify_branch(&self, root: &Path, _expected: &str) -> Result<(), Error> {
        Err(Error::Unsupported(root.to_owned()))
    }

    /// Read the selected subtree from an exact commit in the exact repository.
    ///
    /// Implementations must verify checked-out HEAD before and after object
    /// reads, enforce limits, reject links/gitlinks and never search ancestors.
    ///
    /// # Errors
    ///
    /// Fails for a missing/wrong repository or object, a different HEAD,
    /// unsupported entries, corruption or exceeded ceilings.
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: Limits,
        metadata: &mut MetadataBudget,
    ) -> Result<Inventory, Error>;

    /// Read only paths admitted by a confined selection and their ancestors.
    /// Rejected trees and blobs must not be opened. HEAD, object integrity and
    /// metadata bounds retain the ordinary revision contract. No whole-tree fallback.
    ///
    /// # Errors
    /// Refuses unsupported selection and the ordinary revision failures.
    fn read_revision_filtered(
        &self,
        root: &Path,
        _subdirectory: &Subdirectory,
        _commit: &CommitId,
        _filter: &dyn SourceFilter,
        _limits: Limits,
        _metadata: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        Err(Error::Unsupported(root.to_owned()))
    }

    /// List pinned source metadata without opening file blobs.
    ///
    /// # Errors
    /// Retains revision/confinement checks and fails closed on unsupported enumeration.
    fn list_revision_filtered(
        &self,
        root: &Path,
        _subdirectory: &Subdirectory,
        _commit: &CommitId,
        _filter: &dyn SourceFilter,
        _limits: ListingLimits,
        _metadata: &mut MetadataBudget,
    ) -> Result<Listing, Error> {
        Err(Error::Unsupported(root.to_owned()))
    }

    /// Resolve a gitlink from an exact checked-out parent commit, without
    /// opening or fetching the child. Charge all examined metadata to the budget.
    ///
    /// # Errors
    ///
    /// Fails for a wrong parent pin, non-gitlink path, missing/corrupt objects or
    /// exceeded metadata budget. Parent HEAD must be checked before and after.
    fn read_gitlink(
        &self,
        parent: &Path,
        commit: &CommitId,
        path: &Subdirectory,
        metadata: &mut MetadataBudget,
    ) -> Result<CommitId, Error>;
}
