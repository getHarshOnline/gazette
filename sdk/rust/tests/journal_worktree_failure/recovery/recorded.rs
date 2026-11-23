//! Direct SDK recovery selects exact journal history before acquiring its writer.

use super::*;
use gazette_sdk::release_commit::{RecoverRecordedWorktreeRequest, WorktreeRecoveryError};

fn selected<'a>(
    fixture: &'a Fixture,
    history: &PreparationInspection,
    commit: &'a CommitId,
) -> RecoverRecordedWorktreeRequest<'a> {
    RecoverRecordedWorktreeRequest {
        database: &fixture.database,
        preparation_id: fixture.receipt.record.intent.id,
        expected: history.journal.header,
        commit,
        protected_roots: std::slice::from_ref(&fixture.source),
    }
}

fn no_writer(error: &WorktreeRecoveryError) {
    assert!(!error.progress.writer_acquired);
    assert!(error.progress.journal.is_none());
    assert!(error.progress.verified_commit.is_none());
    assert_eq!(
        error.progress.output_sync,
        SynchronizationState::NotAttempted
    );
    assert_eq!(error.progress.git_sync, SynchronizationState::NotAttempted);
    assert!(error.receipt.is_none());
}

#[test]
fn direct_recorded_recovery_appends_and_replays_without_changing_native_bytes() {
    let fixture = Fixture::new();
    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    let recovered =
        release_commit::recover_recorded_worktree(selected(&fixture, &history, &commit), options())
            .expect("SDK selects, verifies and synchronizes actual recorded preparation");
    assert!(recovered.inserted);
    assert_recipe_preserved(
        history.record.as_ref().expect("original recipe"),
        &recovered.record,
    );
    assert_eq!(
        completion(&recovered.record)
            .expect("durable completion")
            .report
            .commit,
        commit
    );
    let observed = fixture.history();
    assert_eq!(observed.record.as_ref(), Some(&recovered.record));
    assert_eq!(observed.journal.header, recovered.journal);
    assert_eq!(native_bytes(&fixture), before);

    let replay = release_commit::recover_recorded_worktree(
        selected(&fixture, &observed, &commit),
        options(),
    )
    .expect("exact current recorded replay preserves original completion");
    assert!(!replay.inserted);
    assert_eq!(replay.record, recovered.record);
    assert_eq!(native_bytes(&fixture), before);
    let inspected = release_commit::inspect_journal_worktree(
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
    .expect("independent native history and whole worktree read-back");
    assert_eq!(inspected.history.record, Some(replay.record));
    assert_eq!(inspected.worktree.output, *fixture.captured.inventory());
    assert_eq!(
        inspected.worktree.git.commit,
        completion(&recovered.record)
            .expect("original completion")
            .report
    );
    assert_eq!(native_bytes(&fixture), before);
}

#[test]
fn missing_history_and_stale_coordinates_stop_before_writer_acquisition() {
    let fixture = Fixture::new();
    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    for missing in [true, false] {
        let mut request = selected(&fixture, &history, &commit);
        if missing {
            request.preparation_id = Sha256Digest::of(b"not a recorded preparation");
        } else {
            request.expected = fixture.receipt.journal;
        }
        let error = release_commit::recover_recorded_worktree(request, options())
            .expect_err("exact record identity and original expected header are mandatory");
        no_writer(&error);
        assert!(matches!(
            error.cause.as_ref(),
            WorktreeRecoveryFailure::Inspection(_)
        ));
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(fixture.history().journal.header, history.journal.header);
        assert_eq!(native_bytes(&fixture), before);
    }

    let retained_only = Fixture::new();
    let history = retained_only.history();
    let before = native_bytes(&retained_only);
    let error = release_commit::recover_recorded_worktree(
        selected(&retained_only, &history, &commit),
        options(),
    )
    .expect_err("retained output alone cannot supply an invented Git recipe");
    no_writer(&error);
    assert!(matches!(
        error.cause.as_ref(),
        WorktreeRecoveryFailure::Inspection(_)
    ));
    assert!(!retained_only.stage.join(".git").exists());
    assert_eq!(retained_only.history().record, history.record);
    assert_eq!(
        retained_only.history().journal.header,
        history.journal.header
    );
    assert_eq!(native_bytes(&retained_only), before);
}

#[test]
fn changed_native_bytes_or_explicit_commit_refuse_before_writer_and_never_repair() {
    for change_output in [true, false] {
        let fixture = Fixture::new();
        let (commit, history) = unresolved(&fixture);
        let supplied = if change_output {
            fs::write(
                fixture.stage.join("index.html"),
                b"changed current output\n",
            )
            .expect("intentional fixture output drift");
            commit
        } else {
            "1111111111111111111111111111111111111111"
                .parse()
                .expect("valid different commit pin")
        };
        let before = native_bytes(&fixture);
        let error = release_commit::recover_recorded_worktree(
            selected(&fixture, &history, &supplied),
            options(),
        )
        .expect_err("current native state must match the exact recorded recipe and supplied pin");
        no_writer(&error);
        assert!(matches!(
            error.cause.as_ref(),
            WorktreeRecoveryFailure::Inspection(_)
        ));
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(fixture.history().journal.header, history.journal.header);
        assert_eq!(native_bytes(&fixture), before);
    }
}

#[test]
fn every_effect_permission_is_checked_before_an_unavailable_native_selection() {
    let fixture = Fixture::new();
    let (commit, history) = unresolved(&fixture);
    let before = native_bytes(&fixture);
    let absent = fixture
        ._root
        .path()
        .join("unavailable")
        .join("journal.sqlite3");
    for denied in 0..5 {
        let mut denied_options = options();
        match denied {
            0 => denied_options.record.allow_record = false,
            1 => denied_options.writer.allow_generation_advance = false,
            2 => denied_options.writer.session.allow_local_reservation = false,
            3 => denied_options.writer.session.read.allow_coordination_files = false,
            _ => denied_options.worktree.allow_synchronize = false,
        }
        let mut request = selected(&fixture, &history, &commit);
        request.database = &absent;
        let error = release_commit::recover_recorded_worktree(request, denied_options)
            .expect_err("permission refusal precedes native database or root inspection");
        no_writer(&error);
        let expected_code = match denied {
            0 => "store_preparation_permission",
            1 => "store_writer_permission",
            2 => "store_reservation_permission",
            3 => "store_coordination_permission",
            _ => "preparation_synchronization_permission",
        };
        assert_eq!(error.code(), expected_code);
        assert!(matches!(
            (denied, error.cause.as_ref()),
            (
                0,
                WorktreeRecoveryFailure::Journal(Error::PreparationPermission)
            ) | (1, WorktreeRecoveryFailure::Journal(Error::WriterPermission))
                | (
                    2,
                    WorktreeRecoveryFailure::Journal(Error::ReservationPermission)
                )
                | (
                    3,
                    WorktreeRecoveryFailure::Journal(Error::CoordinationPermission)
                )
                | (4, WorktreeRecoveryFailure::Permission)
        ));
        assert!(
            !absent
                .parent()
                .expect("selected missing directory")
                .exists()
        );
        assert_eq!(fixture.history().record, history.record);
        assert_eq!(fixture.history().journal.header, history.journal.header);
        assert_eq!(native_bytes(&fixture), before);
    }
}
