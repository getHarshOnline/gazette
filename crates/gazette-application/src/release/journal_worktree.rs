//! Durable intent brackets native worktree initialization without a cross-resource transaction.

use super::{WorktreeRequest, prepare_worktree};
use gazette_core::{
    destination,
    release::{
        CommitError, CommitMetadata, ParentSource, WorktreeEffect, WorktreeError, WorktreeOptions,
        validate_worktree_parent,
    },
    store,
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeReader},
    source::SourceCapture,
    store::{JournalWriter, PreparationWriter, WriterConnector},
};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// An independently retained staging record and exact new Git recipe.
#[derive(Clone, Copy, Debug)]
pub struct JournalWorktreeRequest<'a> {
    /// Existing private journal, never inside a source or output tree.
    pub journal: &'a Path,
    /// Independently observed expected generation before acquiring the writer.
    pub expected: store::JournalHeader,
    /// Original staged output intent and its durable retention record.
    pub preparation: &'a store::PreparationRecord,
    /// Exact commit fields; not author identity approval or publisher authority.
    pub metadata: &'a CommitMetadata,
    /// Exact local parent acquisition provenance, required only for a pinned parent.
    pub parent_source: Option<&'a ParentSource>,
    /// All native source roots. The coordinator additionally protects the journal.
    pub protected_roots: &'a [PathBuf],
}

/// Separate journal and native Git permissions; none permits remote effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct JournalWorktreeOptions {
    /// Explicit preparation-history append permission.
    pub record: store::PreparationWriteOptions,
    /// Local generation acquisition and connection bounds.
    pub writer: store::WriterOptions,
    /// Native initialization permission and resource ceilings, not an artifact recipe.
    pub worktree: WorktreeOptions,
}

impl JournalWorktreeOptions {
    pub(crate) fn validate(
        self,
        metadata: &CommitMetadata,
        parent_source: Option<&ParentSource>,
    ) -> Result<(), JournalWorktreeError> {
        let before = JournalWorktreeError::before;
        if !self.worktree.allow_initialize {
            return Err(before(JournalWorktreeFailure::Permission));
        }
        self.record
            .validate()
            .map_err(|e| before(JournalWorktreeFailure::Journal(e)))?;
        self.writer
            .validate()
            .map_err(|e| before(JournalWorktreeFailure::Journal(e)))?;
        metadata
            .validate()
            .map_err(|e| before(JournalWorktreeFailure::Recipe(e)))?;
        validate_worktree_parent(metadata.parent.as_ref(), parent_source)
            .map_err(|e| before(JournalWorktreeFailure::Worktree(e)))
    }
}

/// Failure phase, separate from record, generation and native Git effects.
#[derive(Debug)]
pub enum JournalWorktreeFailure {
    /// Native metadata creation was not explicitly allowed.
    Permission,
    /// Explicit recipe grammar is invalid.
    Recipe(CommitError),
    /// Source, stage and private journal overlap or cannot be verified.
    Separation(destination::Error),
    /// Local writer acquisition failed, possibly after advancing its generation.
    Writer(store::WriterAcquireError),
    /// Journal inspection, permission or close failed.
    Journal(store::Error),
    /// Exact history append or independent read-back failed.
    Record(store::PreparationWriteError),
    /// Native worktree initialization or paired read-back failed.
    Worktree(WorktreeError),
    /// Existing Git intent requires reconciliation; it never permits recreation.
    Replay,
    /// Supplied or returned evidence disagrees with the exact original staging record.
    Mismatch,
}

/// Preserve the last confirmed history and native effect if a later phase fails.
#[derive(Debug)]
pub struct JournalWorktreeError {
    /// Exact failed boundary.
    pub cause: Box<JournalWorktreeFailure>,
    /// Last independently verified history returned by the store.
    pub receipt: Option<Box<store::PreparationReceipt>>,
    /// Last observed acquired journal coordinates, even if no append returned.
    pub journal: Option<store::JournalHeader>,
    /// Writer acquisition returned successfully, even if its next inspection failed.
    /// This evidence cannot recreate the released lifetime or invent its unobserved header.
    pub writer_acquired: bool,
    /// Known Git effect from this invocation; not the state of a previous attempt.
    pub effect: WorktreeEffect,
}

impl JournalWorktreeError {
    fn before(cause: JournalWorktreeFailure) -> Self {
        Self {
            cause: Box::new(cause),
            receipt: None,
            journal: None,
            writer_acquired: false,
            effect: WorktreeEffect::NotAttempted,
        }
    }
}

impl fmt::Display for JournalWorktreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phase = match self.cause.as_ref() {
            JournalWorktreeFailure::Permission => "worktree permission missing",
            JournalWorktreeFailure::Recipe(_) => "commit recipe invalid",
            JournalWorktreeFailure::Separation(_) => "source/journal/worktree separation failed",
            JournalWorktreeFailure::Writer(_) => "journal writer acquisition failed",
            JournalWorktreeFailure::Journal(_) => "journal verification or close failed",
            JournalWorktreeFailure::Record(_) => "worktree history append failed",
            JournalWorktreeFailure::Worktree(_) => "native worktree preparation failed",
            JournalWorktreeFailure::Replay => {
                "Git intent already exists; reconcile without recreation"
            }
            JournalWorktreeFailure::Mismatch => "worktree preparation evidence mismatch",
        };
        write!(
            f,
            "{phase}; writer acquired {}; native effect {:?}; preserve exact history and worktree; no release or rollback is implied",
            self.writer_acquired, self.effect
        )
    }
}

impl std::error::Error for JournalWorktreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.cause.as_ref() {
            JournalWorktreeFailure::Recipe(e) => Some(e),
            JournalWorktreeFailure::Separation(e) => Some(e),
            JournalWorktreeFailure::Writer(e) => Some(e),
            JournalWorktreeFailure::Journal(e) => Some(e),
            JournalWorktreeFailure::Record(e) => Some(e),
            JournalWorktreeFailure::Worktree(e) => Some(e),
            _ => None,
        }
    }
}

/// Persist the exact Git intent before initialization, then record verified native completion.
/// One local writer reservation spans the phases; no SQL transaction spans Git or filesystem
/// effects. The existing worktree mechanism consumes the retained stage without recompilation.
/// Neither this history nor its native read-back certifies content or authorizes publication.
///
/// # Errors
/// Existing intent stops before native work, including when completion is absent. Native
/// failure leaves the intent unresolved; a missing completion is never proof of no effect.
/// Record/close failures preserve the known commit without promising rollback or retry.
pub fn prepare_journal_worktree<F, G, C>(
    filesystem: &F,
    git: &G,
    store: &C,
    request: JournalWorktreeRequest<'_>,
    options: JournalWorktreeOptions,
) -> Result<store::PreparationReceipt, JournalWorktreeError>
where
    F: SourceCapture + RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    C: WriterConnector,
    C::Writer: PreparationWriter,
{
    let before = JournalWorktreeError::before;
    options.validate(request.metadata, request.parent_source)?;
    let original = request.preparation;
    let retention = original
        .retention
        .as_ref()
        .ok_or_else(|| before(JournalWorktreeFailure::Mismatch))?;
    if retention.intent_id != original.intent.id
        || retention.output != original.intent.output
        || retention.entries != original.intent.entries
        || retention.bytes != original.intent.bytes
    {
        return Err(before(JournalWorktreeFailure::Mismatch));
    }
    let root = original.intent.address.path();
    let mut acquisition_protected = request.protected_roots.to_vec();
    if let Some(source) = request.parent_source {
        acquisition_protected.push(source.path().to_owned());
    }
    filesystem
        .check_root_separation(request.journal, &acquisition_protected)
        .map_err(|e| before(JournalWorktreeFailure::Separation(e)))?;
    acquisition_protected.push(request.journal.to_owned());
    filesystem
        .check_root_separation(&root, &acquisition_protected)
        .map_err(|e| before(JournalWorktreeFailure::Separation(e)))?;
    // The Git phase separately protects its parent during acquisition. Once
    // imported, that locator is provenance, not an ongoing read-back dependency.
    let mut protected = request.protected_roots.to_vec();
    protected.push(request.journal.to_owned());
    let mut writer = store
        .acquire_writer(request.journal, request.expected, options.writer)
        .map_err(|e| before(JournalWorktreeFailure::Writer(e)))?;
    let acquired = writer
        .inspect()
        .map_err(|e| JournalWorktreeError {
            cause: Box::new(JournalWorktreeFailure::Journal(e)),
            receipt: None,
            journal: None,
            writer_acquired: true,
            effect: WorktreeEffect::NotAttempted,
        })?
        .header;
    let mut receipt = writer
        .record_preparation_worktree(
            store::PreparationWorktreeInput {
                intent: &original.intent,
                retention,
                check: original.check.as_ref(),
                metadata: request.metadata,
                parent_source: request.parent_source,
            },
            options.record,
        )
        .map_err(|e| JournalWorktreeError {
            cause: Box::new(JournalWorktreeFailure::Record(e)),
            receipt: None,
            journal: Some(acquired),
            writer_acquired: true,
            effect: WorktreeEffect::NotAttempted,
        })?;
    let mut effect = WorktreeEffect::NotAttempted;
    let result = (|| {
        if receipt.record.intent != original.intent
            || receipt.record.retention != original.retention
            || receipt.record.check != original.check
        {
            return Err(JournalWorktreeFailure::Mismatch);
        }
        let worktree = receipt
            .record
            .worktree
            .as_ref()
            .ok_or(JournalWorktreeFailure::Mismatch)?;
        if worktree.metadata != *request.metadata
            || worktree.parent_source.as_ref() != request.parent_source
            || worktree.check_sha256 != original.check.as_ref().map(|check| check.evidence_sha256)
        {
            return Err(JournalWorktreeFailure::Mismatch);
        }
        if original.worktree.as_ref().is_some_and(|previous| {
            receipt.inserted
                || previous.metadata != worktree.metadata
                || previous.parent_source != worktree.parent_source
                || previous.check_sha256 != worktree.check_sha256
                || previous.created != worktree.created
                || previous
                    .completion
                    .as_ref()
                    .is_some_and(|done| worktree.completion.as_ref() != Some(done))
        }) {
            return Err(JournalWorktreeFailure::Mismatch);
        }
        if !receipt.inserted || worktree.completion.is_some() {
            return Err(JournalWorktreeFailure::Replay);
        }
        writer.inspect().map_err(JournalWorktreeFailure::Journal)?;
        let prepared = prepare_worktree(
            filesystem,
            git,
            WorktreeRequest {
                root: &root,
                expected: original.intent.output,
                metadata: request.metadata,
                parent_source: request.parent_source,
                protected_roots: &protected,
            },
            options.worktree,
        )
        .map_err(|error| {
            effect = error.effect.clone();
            JournalWorktreeFailure::Worktree(error)
        })?;
        effect = WorktreeEffect::CommitCreated(prepared.git.commit.commit.clone());
        writer.inspect().map_err(JournalWorktreeFailure::Journal)?;
        let next = writer
            .record_preparation_commit(
                store::PreparationCommitInput {
                    intent: &original.intent,
                    retention,
                    check: original.check.as_ref(),
                    worktree,
                    report: &prepared.git.commit,
                },
                options.record,
            )
            .map_err(JournalWorktreeFailure::Record)?;
        if next.record.intent != receipt.record.intent
            || next.record.retention != receipt.record.retention
            || next.record.check != receipt.record.check
            || next.record.worktree.as_ref().is_none_or(|record| {
                record.metadata != worktree.metadata
                    || record.parent_source != worktree.parent_source
                    || record.check_sha256 != worktree.check_sha256
                    || record.created != worktree.created
                    || record
                        .completion
                        .as_ref()
                        .is_none_or(|done| done.report != prepared.git.commit)
            })
        {
            return Err(JournalWorktreeFailure::Mismatch);
        }
        receipt = next;
        writer.close().map_err(JournalWorktreeFailure::Journal)
    })();
    match result {
        Ok(()) => Ok(receipt),
        Err(cause) => Err(JournalWorktreeError {
            cause: Box::new(cause),
            journal: Some(receipt.journal),
            receipt: Some(Box::new(receipt)),
            writer_acquired: true,
            effect,
        }),
    }
}
