//! Release-object mechanics and explicit acquisition, not signer authority or implicit pushes.

mod setup;
pub use setup::{OutputCheckout, ReleaseParentAcquisition};

/// Pure strict decoder selected by outer composition for one acquired authority response.
///
/// The command adapter owns complete process capture while the compiler owns the binding wire
/// format. This port keeps those responsibilities separate without weakening strict decoding.
pub trait AuthorityBindingDecoder {
    /// Decode one complete captured response into the native binding contract.
    ///
    /// # Errors
    /// Returns a stable redacted parser code for malformed, unsupported or over-limit input.
    fn decode_binding(&self, bytes: &[u8]) -> Result<gazette_core::release::Binding, &'static str>;
}

/// Independently selected current public authority acquisition at an effect boundary.
/// Implementations own freshness, active key/frontier/stop observation and current time;
/// they must not construct expected facts from the untrusted received authorization.
/// A static cached binding is not a conforming acquisition implementation.
pub trait AuthoritySource {
    /// Acquire a complete current independent binding, failing on unavailable authority.
    /// No private keys, signing operation or inferred publisher permission is requested.
    ///
    /// # Errors
    /// Refuses unavailable or invalid selected evidence; callers must not use cached fallback.
    fn acquire_authority(&mut self) -> Result<gazette_core::release::Binding, AuthorityError>;
}

/// Explicit failure to obtain a complete current independent authority observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityError {
    /// Selected authority provider could not establish its required current observation.
    Unavailable,
    /// Selected evidence is malformed or incompatible with its acquisition contract.
    Invalid,
}
impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "current independent release authority is unavailable or invalid; no cached fallback",
        )
    }
}
impl std::error::Error for AuthorityError {}

use gazette_core::{
    release::{
        CommitError, CommitMetadata, CommitOptions, CommitReport, CustodyError, CustodyOptions,
        CustodyReport,
    },
    source::{CommitId, RevisionLimits, Sha256Digest, TreeIdentity, TreeInput},
};
use std::path::Path;

/// Exact native single-target dispatch after application-owned durable start and current gates.
pub trait ReleaseDispatcher {
    /// Reverify the prepared fast-forward recipe and dispatch only its exact reviewed target.
    /// Native destination pre-state enforcement must be atomic with its ref update.
    /// Transport output never replaces independently acquired read-back.
    /// Honor the exact selected profile or refuse it. `LocalTransaction` may not spawn
    /// any effect-capable process or leave background work after this synchronous call.
    /// Its complete mutation lifetime stays within the caller's held writer borrow;
    /// a result or caller-supplied profile label cannot qualify a different mechanism.
    ///
    /// # Errors
    /// Refuses permissions, unsupported mechanisms, changed objects or native uncertainty.
    fn dispatch(
        &self,
        request: gazette_core::release::DispatchRequest<'_>,
        options: gazette_core::release::DispatchOptions,
    ) -> Result<gazette_core::release::DispatchObservation, gazette_core::release::DispatchError>;
}

/// Effectful observation of one exact configured branch, outside deterministic compilation.
pub trait RemoteReader {
    /// Return complete native ref evidence or an explicit failure without partial bytes.
    /// No matching advertisement is not absence; that requires a qualified namespace read.
    ///
    /// # Errors
    /// Refuses permission, unsupported/unsafe selections, transport failure and exhausted bounds.
    fn observe_remote(
        &self,
        remote: &str,
        branch: &str,
        options: gazette_core::release::RemoteReadOptions,
    ) -> Result<gazette_core::release::RefAdvertisement, gazette_core::release::RemoteReadError>;
}

/// Git metadata for a separate deployment repository over already retained output.
/// This does not own source capture, publication admission or a release journal.
pub trait ReleaseWorktrees {
    /// Initialize only absent metadata and prepare a detached commit/index from
    /// retained bytes, without checkout, filters, hooks, signing or network.
    /// A selected parent is acquired completely from the explicit local source
    /// before initialization; its source storage is never shared with the result.
    /// Synchronize and independently inspect the native result. The caller must
    /// establish exclusive control, source separation and exact output equality.
    ///
    /// # Errors
    /// Rejects missing permission, existing metadata, unsafe state or limits;
    /// preserves possible residue and never adopts, cleans up or retries it.
    fn prepare_worktree(
        &self,
        root: &Path,
        input: TreeInput<'_>,
        metadata: &CommitMetadata,
        parent_source: Option<&gazette_core::release::ParentSource>,
        options: gazette_core::release::WorktreeOptions,
    ) -> Result<gazette_core::release::WorktreeReport, gazette_core::release::WorktreeError>;

    /// Inspect exact independent administrative storage, detached HEAD, index and
    /// complete commit and parent history using only this repository. Does not
    /// reopen acquisition sources, inspect filesystem output or infer prior sync.
    ///
    /// # Errors
    /// Refuses unsafe, missing, changed, corrupt, unsupported or over-limit state.
    fn inspect_worktree(
        &self,
        root: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: gazette_core::release::WorktreeLimits,
    ) -> Result<gazette_core::release::WorktreeReport, gazette_core::release::WorktreeError>;
}

/// Bounded filesystem output of an independent deployment worktree.
pub trait WorktreeReader {
    /// Read the entire output, omitting only the literal real root `.git` directory.
    /// Git metadata is validated by `ReleaseWorktrees`, never by this omission.
    /// No ignores, attributes, index selection or filters affect the inventory.
    ///
    /// # Errors
    /// Refuses missing or non-directory metadata, source overlap, links, special
    /// files, nested reserved names, changing bytes and exhausted output bounds.
    fn read_worktree(
        &self,
        root: &Path,
        protected_roots: &[std::path::PathBuf],
        limits: gazette_core::source::Limits,
    ) -> Result<gazette_core::source::Inventory, gazette_core::destination::Error>;
}

/// Existing output-byte synchronization, with no creation or cleanup ownership.
pub trait WorktreeOutputSync {
    /// Verify the entire confined output against the exact inventory before and after
    /// flushing files, directories, root and parent. Omit only a real root `.git`.
    /// Git metadata validation and synchronization remain the Git owner's operation.
    ///
    /// # Errors
    /// Refuses permissions, overlap, unsafe/changed bytes or bounds; preserves residue.
    fn synchronize_worktree_output(
        &self,
        root: &Path,
        expected: &gazette_core::source::Inventory,
        protected_roots: &[std::path::PathBuf],
        options: gazette_core::release::WorktreeOutputSyncOptions,
    ) -> Result<gazette_core::source::Inventory, gazette_core::release::WorktreeSyncError>;
}

/// Existing Git-storage synchronization; never initialization or object recreation.
pub trait WorktreeSync {
    /// Verify exact independent metadata, HEAD/index, commit and parent history;
    /// flush administrative storage and recheck. Never reopen parent acquisition sources.
    /// The caller separately verifies and synchronizes publication output.
    ///
    /// # Errors
    /// Refuses permissions, unsafe/changed metadata or bounds without repair or cleanup.
    fn synchronize_worktree(
        &self,
        root: &Path,
        commit: &CommitId,
        output: gazette_core::source::TreeIdentity,
        metadata: &CommitMetadata,
        options: gazette_core::release::WorktreeSyncOptions,
    ) -> Result<gazette_core::release::WorktreeReport, gazette_core::release::WorktreeSyncError>;
}

/// Local object custody without publishing or mutating deployment branches.
pub trait ReleaseCustody {
    /// Verify exact existing objects, create only an absent custody ref or accept
    /// an exact direct-ref replay, synchronize native storage, and independently
    /// read back the ref and complete objects. Ref replacement/deletion is forbidden.
    /// The caller owns trusted exclusive repository access and source separation.
    ///
    /// # Errors
    /// Refuses permission, conflicts, symbolic refs, unsafe storage or bounds;
    /// preserves possible ref effects and never rolls back or retries automatically.
    fn retain_commit(
        &self,
        repository: &Path,
        id: Sha256Digest,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        options: CustodyOptions,
    ) -> Result<CustodyReport, CustodyError>;

    /// Inspect the exact direct custody ref and complete objects without writes.
    /// Success is current observation, not proof of a preceding synchronization.
    ///
    /// # Errors
    /// Refuses missing/conflicting/symbolic refs, unsafe/missing objects or bounds.
    fn inspect_custody(
        &self,
        repository: &Path,
        id: Sha256Digest,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: RevisionLimits,
    ) -> Result<CustodyReport, CustodyError>;
}

/// Exact object preparation in an explicitly selected independent local repository.
pub trait ReleaseCommits {
    /// Write a detached commit directly from retained bytes and verify it independently.
    /// No branch/index/worktree updates, filters, hooks, signing or network access.
    /// The caller must first verify separation from all protected source roots.
    ///
    /// # Errors
    /// Refuses missing permission, invalid recipes, unsafe repositories, unsupported
    /// trees or bounds; reports possible retained objects after attempted writes.
    fn prepare_commit(
        &self,
        repository: &Path,
        input: TreeInput<'_>,
        metadata: &CommitMetadata,
        options: CommitOptions,
    ) -> Result<CommitReport, CommitError>;

    /// Read exact local objects and compare the full recipe and output identity.
    /// Does not require or modify HEAD and grants no durability or release authority.
    ///
    /// # Errors
    /// Refuses missing, corrupt, unsafe, mismatched, unsupported or over-limit inputs.
    fn verify_commit(
        &self,
        repository: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: RevisionLimits,
    ) -> Result<CommitReport, CommitError>;
}
