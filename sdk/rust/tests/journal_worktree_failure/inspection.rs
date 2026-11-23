//! Joined history/current-byte inspection never completes or repairs preparation.

use super::*;
use gazette_core::source::{CommitId, Inventory};
use gazette_sdk::release_commit::{
    self, JournalWorktreeInspectionOptions, JournalWorktreeInspectionRequest,
};

fn options() -> JournalWorktreeInspectionOptions {
    JournalWorktreeInspectionOptions {
        read: read(),
        ..Default::default()
    }
}

fn request<'a>(
    fixture: &'a Fixture,
    commit: &'a CommitId,
    header: Option<&'a JournalHeader>,
) -> JournalWorktreeInspectionRequest<'a> {
    JournalWorktreeInspectionRequest {
        database: &fixture.database,
        preparation_id: fixture.receipt.record.intent.id,
        expected_header: header,
        commit,
        protected_roots: std::slice::from_ref(&fixture.source),
    }
}

fn complete(fixture: &Fixture) -> PreparationReceipt {
    release_commit::prepare_journal_worktree(
        release::JournalWorktreeRequest {
            journal: &fixture.journal,
            expected: fixture.receipt.journal,
            preparation: &fixture.receipt.record,
            metadata: &fixture.metadata,
            parent_source: None,
            protected_roots: std::slice::from_ref(&fixture.source),
        },
        release::JournalWorktreeOptions {
            record: permission(),
            writer: writer_options(),
            worktree: gazette_core::release::WorktreeOptions {
                allow_initialize: true,
                ..Default::default()
            },
        },
    )
    .expect("actual SQLite/FS/Git preparation")
}

fn snapshot(fixture: &Fixture) -> ([Inventory; 2], Vec<u8>) {
    let trees = [&fixture.source, &fixture.stage].map(|path| {
        FileSystem
            .read(path, None, Default::default())
            .expect("complete native source/output/Git bytes")
    });
    // The read contract permits SQLite WAL/SHM coordination. Database bytes and
    // independently inspected history/fence must not be changed by inspection.
    (
        trees,
        fs::read(&fixture.database).expect("native database bytes"),
    )
}

#[test]
fn joined_completed_history_and_current_worktree_agree_without_writes() {
    let fixture = Fixture::new();
    let receipt = complete(&fixture);
    let report = &receipt
        .record
        .worktree
        .as_ref()
        .expect("recipe")
        .completion
        .as_ref()
        .expect("completion")
        .report;
    let before = snapshot(&fixture);
    for expected in [Some(&receipt.journal), None] {
        let observed = release_commit::inspect_journal_worktree(
            request(&fixture, &report.commit, expected),
            options(),
        )
        .expect("joined native inspection");
        assert_eq!(observed.history.journal.header, receipt.journal);
        assert_eq!(observed.history.record.as_ref(), Some(&receipt.record));
        assert_eq!(observed.worktree.git.commit, *report);
        assert_eq!(observed.worktree.output, *fixture.captured.inventory());
        assert_eq!(
            snapshot(&fixture),
            before,
            "inspection never advances writer or rewrites SQL/Git/output/source bytes"
        );
    }
}

#[test]
fn missing_completion_record_can_be_observed_but_is_never_repaired() {
    let fixture = Fixture::new();
    let failure = fixture.invoke(Boundary::BeforeCompletion);
    assert!(matches!(
        failure.cause.as_ref(),
        JournalWorktreeFailure::Record(PreparationWriteError {
            effect: PreparationEffect::NotAttempted,
            ..
        })
    ));
    let WorktreeEffect::CommitCreated(commit) = &failure.effect else {
        panic!("real commit must precede injected append failure")
    };
    let history = fixture.history();
    assert!(
        history
            .record
            .as_ref()
            .expect("intent")
            .worktree
            .as_ref()
            .expect("recipe")
            .completion
            .is_none()
    );
    let before = snapshot(&fixture);
    let observed = release_commit::inspect_journal_worktree(
        request(&fixture, commit, Some(&history.journal.header)),
        options(),
    )
    .expect("native commit exists without a completion row");
    assert_eq!(observed.history.record, history.record);
    assert_eq!(observed.history.journal.header, history.journal.header);
    assert_eq!(observed.worktree.git.commit.commit, *commit);
    assert_eq!(observed.worktree.output, *fixture.captured.inventory());
    assert!(
        observed
            .history
            .record
            .as_ref()
            .expect("history")
            .worktree
            .as_ref()
            .expect("recipe")
            .completion
            .is_none()
    );
    assert_eq!(snapshot(&fixture), before);
    assert_eq!(
        fixture.history().record,
        history.record,
        "read-back cannot append missing completion"
    );
}

#[test]
fn joined_inspection_refuses_stale_header_wrong_commit_and_protected_overlap() {
    let fixture = Fixture::new();
    let receipt = complete(&fixture);
    let commit = &receipt
        .record
        .worktree
        .as_ref()
        .expect("recipe")
        .completion
        .as_ref()
        .expect("completion")
        .report
        .commit;
    let wrong = "1111111111111111111111111111111111111111"
        .parse()
        .expect("different exact commit");
    let before = snapshot(&fixture);
    let error = release_commit::inspect_journal_worktree(
        request(&fixture, commit, Some(&fixture.receipt.journal)),
        options(),
    )
    .expect_err("stale pre-acquisition fence");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::Journal(_)
    ));
    let error = release_commit::inspect_journal_worktree(
        request(&fixture, &wrong, Some(&receipt.journal)),
        options(),
    )
    .expect_err("wrong pinned commit");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::Mismatch
    ));
    for protected in [&fixture.stage, &fixture.journal] {
        let mut input = request(&fixture, commit, Some(&receipt.journal));
        input.protected_roots = std::slice::from_ref(protected);
        let error = release_commit::inspect_journal_worktree(input, options())
            .expect_err("protected native overlap");
        assert!(matches!(
            error,
            release_commit::JournalWorktreeInspectionError::Separation(_)
        ));
    }
    assert_eq!(snapshot(&fixture), before);
    assert_eq!(fixture.history().record, Some(receipt.record));
}

#[test]
fn changed_output_or_head_refuses_without_repairing_native_state_or_history() {
    for change_head in [false, true] {
        let fixture = Fixture::new();
        let receipt = complete(&fixture);
        let commit = &receipt
            .record
            .worktree
            .as_ref()
            .expect("recipe")
            .completion
            .as_ref()
            .expect("completion")
            .report
            .commit;
        if change_head {
            fs::write(
                fixture.stage.join(".git/HEAD"),
                "1111111111111111111111111111111111111111\n",
            )
            .expect("alter only disposable HEAD");
        } else {
            fs::write(fixture.stage.join("index.html"), "altered output")
                .expect("alter only disposable output");
        }
        let before = snapshot(&fixture);
        let error = release_commit::inspect_journal_worktree(
            request(&fixture, commit, Some(&receipt.journal)),
            options(),
        )
        .expect_err("historical completion cannot hide current corruption");
        assert!(matches!(
            error,
            release_commit::JournalWorktreeInspectionError::Worktree(_)
        ));
        assert_eq!(snapshot(&fixture), before);
        assert_eq!(fixture.history().record, Some(receipt.record));
    }
}

#[test]
fn missing_preparation_retention_or_recipe_refuses_before_git() {
    let fixture = Fixture::new();
    let commit = "1111111111111111111111111111111111111111"
        .parse()
        .expect("unused commit pin");
    let missing = Sha256Digest::of(b"no such preparation");
    let mut input = request(&fixture, &commit, Some(&fixture.receipt.journal));
    input.preparation_id = missing;
    let before = snapshot(&fixture);
    let error =
        release_commit::inspect_journal_worktree(input, options()).expect_err("unknown history");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::MissingPreparation
    ));
    let error = release_commit::inspect_journal_worktree(
        request(&fixture, &commit, Some(&fixture.receipt.journal)),
        options(),
    )
    .expect_err("stage has no Git recipe");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::MissingWorktree
    ));
    assert_eq!(snapshot(&fixture), before);

    let mut writer = Sqlite
        .acquire_writer(&fixture.journal, fixture.receipt.journal, writer_options())
        .expect("fixture-only append");
    let mut intent = fixture.receipt.record.intent.as_input();
    intent.id = missing;
    let unretained = writer
        .record_preparation_intent(intent, permission())
        .expect("real intent without retention");
    writer.close().expect("release writer");
    let mut input = request(&fixture, &commit, Some(&unretained.journal));
    input.preparation_id = missing;
    let before = snapshot(&fixture);
    let error =
        release_commit::inspect_journal_worktree(input, options()).expect_err("unretained intent");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::MissingRetention
    ));
    assert_eq!(snapshot(&fixture), before);
    assert!(
        !fixture.stage.join(".git").exists(),
        "inspection never initialized Git"
    );
}

#[test]
fn writer_generation_change_after_real_git_read_refuses_joined_evidence() {
    use gazette_core::{
        release::{ParentSource, WorktreeError, WorktreeLimits, WorktreeOptions, WorktreeReport},
        source::{TreeIdentity, TreeInput},
    };
    use gazette_ports::release::ReleaseWorktrees;
    use std::cell::Cell;

    struct AdvanceAfterRead<'a> {
        fixture: &'a Fixture,
        header: JournalHeader,
        advanced: Cell<bool>,
    }
    impl ReleaseWorktrees for AdvanceAfterRead<'_> {
        fn prepare_worktree(
            &self,
            root: &Path,
            input: TreeInput<'_>,
            metadata: &CommitMetadata,
            parent_source: Option<&ParentSource>,
            options: WorktreeOptions,
        ) -> Result<WorktreeReport, WorktreeError> {
            Git.prepare_worktree(root, input, metadata, parent_source, options)
        }

        fn inspect_worktree(
            &self,
            root: &Path,
            commit: &CommitId,
            output: TreeIdentity,
            metadata: &CommitMetadata,
            limits: WorktreeLimits,
        ) -> Result<WorktreeReport, WorktreeError> {
            let report = Git.inspect_worktree(root, commit, output, metadata, limits)?;
            if !self.advanced.replace(true) {
                Sqlite
                    .acquire_writer(&self.fixture.journal, self.header, writer_options())
                    .expect("independent fixture writer advances after native Git observation")
                    .close()
                    .expect("fixture writer closes without record changes");
            }
            Ok(report)
        }
    }

    let fixture = Fixture::new();
    let receipt = complete(&fixture);
    let commit = &receipt
        .record
        .worktree
        .as_ref()
        .expect("recipe")
        .completion
        .as_ref()
        .expect("completion")
        .report
        .commit;
    let native_before = snapshot(&fixture).0;
    let git = AdvanceAfterRead {
        fixture: &fixture,
        header: receipt.journal,
        advanced: Cell::new(false),
    };
    let error = release::inspect_journal_worktree(
        &FileSystem,
        &git,
        &Sqlite,
        request(&fixture, commit, Some(&receipt.journal)),
        options(),
    )
    .expect_err("changed journal bracket cannot yield joined evidence");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::Changed
    ));
    assert_eq!(error.code(), "preparation_changed");
    assert!(git.advanced.get());
    let history = fixture.history();
    assert_eq!(
        history.journal.header.writer_fence,
        receipt.journal.writer_fence + 1
    );
    assert_eq!(history.record, Some(receipt.record));
    assert_eq!(
        snapshot(&fixture).0,
        native_before,
        "injected writer does not modify Git/output/source"
    );
}

#[test]
fn same_writer_completion_append_between_reads_refuses_joined_evidence() {
    use gazette_core::{release::CommitReport, store::PreparationRecord};
    use std::cell::{Cell, RefCell};

    struct CompleteBeforeSecondRead {
        writer: RefCell<Writer>,
        record: PreparationRecord,
        report: CommitReport,
        reads: Cell<usize>,
    }
    impl PreparationReader for CompleteBeforeSecondRead {
        fn inspect_preparation(
            &self,
            path: &Path,
            id: Sha256Digest,
            options: ReadOptions,
        ) -> Result<PreparationInspection, Error> {
            let count = self.reads.get();
            self.reads.set(count + 1);
            if count == 1 {
                self.writer
                    .borrow_mut()
                    .record_preparation_commit(
                        PreparationCommitInput {
                            intent: &self.record.intent,
                            retention: self.record.retention.as_ref().expect("real retention"),
                            check: self.record.check.as_ref(),
                            worktree: self.record.worktree.as_ref().expect("real recipe"),
                            report: &self.report,
                        },
                        permission(),
                    )
                    .expect("real completion append under the same held writer");
            }
            // Never return a fabricated snapshot; the second result reads the
            // actual SQLite row inserted after the native Git/FS observations.
            Sqlite.inspect_preparation(path, id, options)
        }
    }

    let fixture = Fixture::new();
    let failure = fixture.invoke(Boundary::BeforeCompletion);
    let WorktreeEffect::CommitCreated(commit) = &failure.effect else {
        panic!("native commit must exist before missing completion")
    };
    let history = fixture.history();
    let protected = [fixture.source.clone(), fixture.journal.clone()];
    let actual = release_commit::inspect_worktree(
        release::WorktreeRequest {
            root: &fixture.stage,
            expected: fixture.captured.inventory().identity(),
            metadata: &fixture.metadata,
            parent_source: None,
            protected_roots: &protected,
        },
        commit,
        Default::default(),
    )
    .expect("independent real native report to append");
    let mut writer = Sqlite
        .acquire_writer(&fixture.journal, history.journal.header, writer_options())
        .expect("writer acquired before first snapshot");
    let header = writer.inspect().expect("held writer coordinates").header;
    let record = fixture
        .history()
        .record
        .expect("original unresolved history");
    assert!(
        record
            .worktree
            .as_ref()
            .expect("recipe")
            .completion
            .is_none()
    );
    let reader = CompleteBeforeSecondRead {
        writer: RefCell::new(writer),
        record: record.clone(),
        report: actual.git.commit,
        reads: Cell::new(0),
    };
    let native_before = snapshot(&fixture).0;
    let error = release::inspect_journal_worktree(
        &FileSystem,
        &Git,
        &reader,
        request(&fixture, commit, Some(&header)),
        options(),
    )
    .expect_err("same-fence record change invalidates joined evidence");
    assert!(matches!(
        error,
        release_commit::JournalWorktreeInspectionError::Changed
    ));
    assert_eq!(error.code(), "preparation_changed");
    assert_eq!(reader.reads.get(), 2);
    reader
        .writer
        .into_inner()
        .close()
        .expect("close fixture writer");
    let after = fixture.history();
    assert_eq!(
        after.journal.header, header,
        "record-only change under the same writer fence"
    );
    let mut completed = after.record.expect("actual completed history");
    let done = completed
        .worktree
        .as_mut()
        .expect("recipe")
        .completion
        .take()
        .expect("completion really appended");
    assert_eq!(done.report.commit, *commit);
    assert_eq!(done.recorded, header);
    assert_eq!(completed, record, "only completion changed");
    assert_eq!(
        snapshot(&fixture).0,
        native_before,
        "inspection and append do not change source/output/Git"
    );
}
