//! Recovery flushes and verifies existing bytes; it never recreates a preparation.

#[path = "recovery/output.rs"]
mod output;
#[path = "recovery/recorded.rs"]
mod recorded;
#[path = "recovery/synchronization.rs"]
mod synchronization;

use super::*;
use gazette_core::{
    release::WorktreeSyncOptions,
    source::{CommitId, Inventory},
    store::PreparationRecord,
};
use gazette_sdk::release_commit::{
    self, RecoverWorktreeOptions, RecoverWorktreeRequest, SynchronizationState,
    WorktreeRecoveryFailure,
};

fn options() -> RecoverWorktreeOptions {
    RecoverWorktreeOptions {
        record: permission(),
        writer: writer_options(),
        worktree: WorktreeSyncOptions {
            allow_synchronize: true,
            ..Default::default()
        },
    }
}

fn request<'a>(
    fixture: &'a Fixture,
    history: &'a PreparationInspection,
    commit: &'a CommitId,
) -> RecoverWorktreeRequest<'a> {
    RecoverWorktreeRequest {
        journal: &fixture.journal,
        expected: history.journal.header,
        preparation: history.record.as_ref().expect("durable preparation"),
        commit,
        protected_roots: std::slice::from_ref(&fixture.source),
    }
}

fn unresolved(fixture: &Fixture) -> (CommitId, PreparationInspection) {
    let failure = fixture.invoke(Boundary::BeforeCompletion);
    let WorktreeEffect::CommitCreated(commit) = failure.effect else {
        panic!("actual native commit precedes the injected append failure")
    };
    let history = fixture.history();
    assert!(completion(history.record.as_ref().expect("record")).is_none());
    (commit, history)
}

fn completion(record: &PreparationRecord) -> Option<&gazette_core::store::PreparationCommit> {
    record
        .worktree
        .as_ref()
        .expect("durable recipe")
        .completion
        .as_ref()
}

fn native_bytes(fixture: &Fixture) -> [Inventory; 2] {
    [&fixture.source, &fixture.stage].map(|path| {
        FileSystem
            .read(path, None, Default::default())
            .expect("complete source/output/Git inventory, including administrative bytes")
    })
}

fn assert_recipe_preserved(before: &PreparationRecord, after: &PreparationRecord) {
    assert_eq!(after.intent, before.intent);
    assert_eq!(after.retention, before.retention);
    let mut without_completion = after.clone();
    without_completion
        .worktree
        .as_mut()
        .expect("recipe")
        .completion = before
        .worktree
        .as_ref()
        .expect("original recipe")
        .completion
        .clone();
    assert_eq!(&without_completion, before);
}

#[test]
fn recovery_records_real_existing_commit_and_replay_preserves_original_completion() {
    let fixture = Fixture::new();
    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    let recovered =
        release_commit::recover_journal_worktree(request(&fixture, &history, &commit), options())
            .expect("existing output and Git are synchronized and read back before append");
    assert!(recovered.inserted);
    assert_recipe_preserved(history.record.as_ref().expect("record"), &recovered.record);
    let completed = completion(&recovered.record).expect("durable native completion");
    assert_eq!(completed.report.commit, commit);
    assert_eq!(fixture.history().record.as_ref(), Some(&recovered.record));
    assert_eq!(native_bytes(&fixture), before);

    // Deliberately keep the original missing-completion request, as when a caller
    // lost the first receipt, but acquire the independently observed current fence.
    let mut replay_request = request(&fixture, &history, &commit);
    replay_request.expected = recovered.journal;
    let replay = release_commit::recover_journal_worktree(replay_request, options())
        .expect("exact replay verifies existing effects, not initialization");
    assert!(!replay.inserted);
    assert_eq!(replay.record, recovered.record);
    assert_eq!(completion(&replay.record), Some(completed));
    assert_eq!(native_bytes(&fixture), before);

    let observed = release_commit::inspect_journal_worktree(
        release_commit::JournalWorktreeInspectionRequest {
            database: &fixture.database,
            preparation_id: recovered.record.intent.id,
            expected_header: Some(&replay.journal),
            commit: &commit,
            protected_roots: std::slice::from_ref(&fixture.source),
        },
        release_commit::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("independent joined history, HEAD/index, objects and whole-output proof");
    assert_eq!(observed.worktree.git.commit, completed.report);
    assert_eq!(observed.worktree.output, *fixture.captured.inventory());
    assert_eq!(observed.history.record, Some(replay.record));
    assert_eq!(native_bytes(&fixture), before);
}

#[test]
fn recovery_permissions_and_stale_writer_pin_preserve_native_state() {
    let fixture = Fixture::new();
    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    for denied in 0..3 {
        let mut denied_options = options();
        match denied {
            0 => denied_options.record.allow_record = false,
            1 => denied_options.writer.allow_generation_advance = false,
            _ => denied_options.worktree.allow_synchronize = false,
        }
        let error = release_commit::recover_journal_worktree(
            request(&fixture, &history, &commit),
            denied_options,
        )
        .expect_err("each consequential local effect requires explicit permission");
        assert!(!error.progress.writer_acquired);
        assert!(error.progress.verified_commit.is_none());
        assert_eq!(
            error.progress.output_sync,
            SynchronizationState::NotAttempted
        );
        assert_eq!(error.progress.git_sync, SynchronizationState::NotAttempted);
        assert_eq!(fixture.history().journal.header, history.journal.header);
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(native_bytes(&fixture), before);
    }
    let mut stale = request(&fixture, &history, &commit);
    stale.expected = fixture.receipt.journal;
    let error = release_commit::recover_journal_worktree(stale, options())
        .expect_err("old generation cannot acquire recovery writer");
    assert!(matches!(
        error.cause.as_ref(),
        WorktreeRecoveryFailure::Writer(_)
    ));
    assert!(!error.progress.writer_acquired);
    assert_eq!(fixture.history().journal.header, history.journal.header);
    assert_eq!(fixture.history().record, history.record);
    assert_eq!(native_bytes(&fixture), before);
}

#[test]
fn recovery_refuses_wrong_commit_and_changed_output_without_repair() {
    for change_output in [false, true] {
        let fixture = Fixture::new();
        let (commit, history) = unresolved(&fixture);
        let supplied = if change_output {
            fs::write(fixture.stage.join("index.html"), b"changed locally\r\n")
                .expect("deliberate native output change");
            commit
        } else {
            "1111111111111111111111111111111111111111"
                .parse::<CommitId>()
                .expect("well-formed different commit")
        };
        let before = native_bytes(&fixture);
        let error = release_commit::recover_journal_worktree(
            request(&fixture, &history, &supplied),
            options(),
        )
        .expect_err("recovery cannot replace a commit or restore altered output");
        assert!(matches!(
            error.cause.as_ref(),
            WorktreeRecoveryFailure::Worktree(_) | WorktreeRecoveryFailure::Mismatch
        ));
        assert_eq!(
            error.progress.output_sync,
            SynchronizationState::NotAttempted
        );
        assert_eq!(error.progress.git_sync, SynchronizationState::NotAttempted);
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(native_bytes(&fixture), before);
    }
}

#[test]
fn recovery_refuses_overlap_and_missing_recipe_without_creating_metadata() {
    let fixture = Fixture::new();
    // A retained stage is insufficient: recovery may not create a Git recipe.
    let history = fixture.history();
    let commit = "1111111111111111111111111111111111111111"
        .parse::<CommitId>()
        .expect("syntactic commit");
    let before = native_bytes(&fixture);
    let error =
        release_commit::recover_journal_worktree(request(&fixture, &history, &commit), options())
            .expect_err("missing recipe is not an invitation to initialize");
    assert!(matches!(
        error.cause.as_ref(),
        WorktreeRecoveryFailure::Missing
    ));
    assert!(!fixture.stage.join(".git").exists());
    assert_eq!(fixture.history().record, history.record);
    assert_eq!(native_bytes(&fixture), before);

    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    for protected in [&fixture.journal, &fixture.stage] {
        let mut overlapping = request(&fixture, &history, &commit);
        overlapping.protected_roots = std::slice::from_ref(protected);
        let error = release_commit::recover_journal_worktree(overlapping, options())
            .expect_err("journal and worktree cannot overlap a protected native source");
        assert!(matches!(
            error.cause.as_ref(),
            WorktreeRecoveryFailure::Separation(_)
        ));
        assert!(!error.progress.writer_acquired);
        assert_eq!(fixture.history().journal.header, history.journal.header);
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(native_bytes(&fixture), before);
    }
}

#[test]
fn recovery_uses_held_native_history_not_a_caller_replacement_recipe() {
    for missing_retention in [true, false] {
        let fixture = Fixture::new();
        let (commit, history) = unresolved(&fixture);
        let mut supplied = history.record.as_ref().expect("original record").clone();
        if missing_retention {
            supplied.retention = None;
        } else {
            supplied.worktree.as_mut().expect("recipe").metadata.message =
                "Caller replacement is not the recorded recipe\n".to_owned();
        }
        let before = native_bytes(&fixture);
        let mut changed = request(&fixture, &history, &commit);
        changed.preparation = &supplied;
        let error = release_commit::recover_journal_worktree(changed, options())
            .expect_err("missing phases or changed recipe must not be reconstructed");
        assert!(if missing_retention {
            matches!(error.cause.as_ref(), WorktreeRecoveryFailure::Missing)
        } else {
            matches!(error.cause.as_ref(), WorktreeRecoveryFailure::Mismatch)
        });
        assert!(error.progress.verified_commit.is_none());
        assert_eq!(
            error.progress.output_sync,
            SynchronizationState::NotAttempted
        );
        assert_eq!(error.progress.git_sync, SynchronizationState::NotAttempted);
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(native_bytes(&fixture), before);
    }
}

#[test]
fn native_git_sync_limit_preserves_verified_commit_without_recording_completion() {
    let fixture = Fixture::new();
    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    let mut bounded = options();
    bounded.worktree.storage_bytes = 1;
    let error =
        release_commit::recover_journal_worktree(request(&fixture, &history, &commit), bounded)
            .expect_err(
                "actual Git administrative storage exceeds the native synchronization budget",
            );
    assert!(matches!(
        error.cause.as_ref(),
        WorktreeRecoveryFailure::Synchronization(_)
    ));
    assert!(error.progress.writer_acquired);
    assert_eq!(error.progress.verified_commit.as_ref(), Some(&commit));
    assert_eq!(error.progress.output_sync, SynchronizationState::Verified);
    assert_eq!(error.progress.git_sync, SynchronizationState::NotAttempted);
    assert_eq!(fixture.history().record, history.record);
    assert_eq!(native_bytes(&fixture), before);
}

#[test]
fn lost_append_and_close_reports_preserve_actual_synchronization_and_durable_history() {
    for boundary in [
        Boundary::BeforeCompletion,
        Boundary::CompletionReport,
        Boundary::ClosedReport,
    ] {
        let fixture = Fixture::new();
        let (commit, history) = unresolved(&fixture);
        let before = native_bytes(&fixture);
        let error = release::recover_journal_worktree(
            &FileSystem,
            &Git,
            &FaultStore(boundary),
            request(&fixture, &history, &commit),
            options(),
        )
        .expect_err("lose only the selected report around real native effects");
        assert!(error.progress.writer_acquired);
        assert_eq!(error.progress.verified_commit.as_ref(), Some(&commit));
        assert_eq!(error.progress.output_sync, SynchronizationState::Verified);
        assert_eq!(error.progress.git_sync, SynchronizationState::Verified);
        let observed = fixture.history();
        assert_eq!(error.progress.journal, Some(observed.journal.header));
        let record = observed.record.as_ref().expect("durable record");
        assert_recipe_preserved(history.record.as_ref().expect("original record"), record);
        if boundary == Boundary::BeforeCompletion {
            assert!(completion(record).is_none());
        } else {
            assert_eq!(
                completion(record)
                    .expect("real completion append")
                    .report
                    .commit,
                commit
            );
        }
        if boundary == Boundary::ClosedReport {
            assert_eq!(
                error.receipt.as_deref().map(|receipt| &receipt.record),
                Some(record)
            );
        }
        assert_eq!(native_bytes(&fixture), before);
    }
}
