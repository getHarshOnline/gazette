//! Lose only synchronization reports after the actual native flush and read-back.

use super::*;
use gazette_core::{
    destination,
    release::{
        ParentSource, WorktreeError, WorktreeLimits, WorktreeOptions, WorktreeOutputSyncOptions,
        WorktreeReport, WorktreeSyncError,
    },
    source::{Limits, TreeIdentity, TreeInput},
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeOutputSync, WorktreeReader, WorktreeSync},
};

struct LostOutputReport;

impl RootSeparation for LostOutputReport {
    fn check_root_separation(
        &self,
        root: &Path,
        protected: &[PathBuf],
    ) -> Result<(), destination::Error> {
        FileSystem.check_root_separation(root, protected)
    }
}

impl WorktreeReader for LostOutputReport {
    fn read_worktree(
        &self,
        root: &Path,
        protected: &[PathBuf],
        limits: Limits,
    ) -> Result<Inventory, destination::Error> {
        FileSystem.read_worktree(root, protected, limits)
    }
}

impl WorktreeOutputSync for LostOutputReport {
    fn synchronize_worktree_output(
        &self,
        root: &Path,
        expected: &Inventory,
        protected: &[PathBuf],
        options: WorktreeOutputSyncOptions,
    ) -> Result<Inventory, WorktreeSyncError> {
        FileSystem.synchronize_worktree_output(root, expected, protected, options)?;
        Err(WorktreeSyncError {
            reason: "injected loss after native output synchronization",
            attempted: true,
        })
    }
}

struct LostGitReport;

impl ReleaseWorktrees for LostGitReport {
    fn prepare_worktree(
        &self,
        _: &Path,
        _: TreeInput<'_>,
        _: &CommitMetadata,
        _: Option<&ParentSource>,
        _: WorktreeOptions,
    ) -> Result<WorktreeReport, WorktreeError> {
        panic!("recovery must never initialize or recreate Git objects")
    }

    fn inspect_worktree(
        &self,
        root: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        limits: WorktreeLimits,
    ) -> Result<WorktreeReport, WorktreeError> {
        Git.inspect_worktree(root, commit, output, metadata, limits)
    }
}

impl WorktreeSync for LostGitReport {
    fn synchronize_worktree(
        &self,
        root: &Path,
        commit: &CommitId,
        output: TreeIdentity,
        metadata: &CommitMetadata,
        options: WorktreeSyncOptions,
    ) -> Result<WorktreeReport, WorktreeSyncError> {
        Git.synchronize_worktree(root, commit, output, metadata, options)?;
        Err(WorktreeSyncError {
            reason: "injected loss after native Git synchronization",
            attempted: true,
        })
    }
}

#[test]
fn lost_real_sync_reports_are_unverified_not_absent_or_completed() {
    for lose_output in [true, false] {
        let fixture = Fixture::new();
        let (commit, history) = unresolved(&fixture);
        let before = native_bytes(&fixture);
        let error = if lose_output {
            release::recover_journal_worktree(
                &LostOutputReport,
                &Git,
                &Sqlite,
                request(&fixture, &history, &commit),
                options(),
            )
        } else {
            release::recover_journal_worktree(
                &FileSystem,
                &LostGitReport,
                &Sqlite,
                request(&fixture, &history, &commit),
                options(),
            )
        }
        .expect_err("fault is injected only after the real synchronization returns");
        assert!(matches!(
            error.cause.as_ref(),
            WorktreeRecoveryFailure::Synchronization(WorktreeSyncError {
                attempted: true,
                ..
            })
        ));
        assert_eq!(error.progress.verified_commit.as_ref(), Some(&commit));
        assert_eq!(
            error.progress.output_sync,
            if lose_output {
                SynchronizationState::Unverified
            } else {
                SynchronizationState::Verified
            }
        );
        assert_eq!(
            error.progress.git_sync,
            if lose_output {
                SynchronizationState::NotAttempted
            } else {
                SynchronizationState::Unverified
            }
        );
        assert!(error.receipt.is_none());
        let observed = fixture.history();
        assert_eq!(error.progress.journal, Some(observed.journal.header));
        assert_eq!(observed.record, history.record);
        assert_eq!(native_bytes(&fixture), before);
    }
}
