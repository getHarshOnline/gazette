//! Durable worktree intent and completion across the real `SQLite`, Git and filesystem owners.

#[path = "journal_inspection.rs"]
mod inspection;
#[path = "journal_recovery.rs"]
mod recovery;

use gazette_sdk::{release_commit as release, store};

use super::{
    Fixture, acquire, bytes, fs,
    preparation::{published_fixture, render},
    source,
    worktree::{Retained, metadata},
};

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn options() -> release::JournalWorktreeOptions {
    release::JournalWorktreeOptions {
        record: store::PreparationWriteOptions { allow_record: true },
        writer: store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
        },
        worktree: release::WorktreeOptions {
            allow_initialize: true,
            ..Default::default()
        },
    }
}

fn request<'a>(
    stage: &'a Retained,
    metadata: &'a release::CommitMetadata,
    sources: &'a [std::path::PathBuf],
) -> release::JournalWorktreeRequest<'a> {
    release::JournalWorktreeRequest {
        journal: stage.journal.path(),
        expected: stage.receipt.journal,
        preparation: &stage.receipt.record,
        metadata,
        parent_source: None,
        protected_roots: sources,
    }
}

fn inspect(stage: &Retained, header: store::JournalHeader) -> store::PreparationRecord {
    store::inspect_preparation(
        &stage.database,
        stage.receipt.record.intent.id,
        Some(&header),
        read(),
    )
    .expect("independent preparation history")
    .record
    .expect("original durable preparation")
}

#[test]
fn exact_journal_intent_and_commit_completion_follow_the_real_retained_publication() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("connected publication");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let sources = [fixture.directory.path().to_owned()];
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "UNACQUIRED SOURCE",
    )
    .expect("later native source edit");
    let result = release::prepare_journal_worktree(request(&stage, &metadata, &sources), options())
        .expect("connected journalled worktree");
    assert!(result.inserted);
    assert_eq!(result.record.intent, stage.receipt.record.intent);
    assert_eq!(result.record.retention, stage.receipt.record.retention);
    let worktree = result
        .record
        .worktree
        .as_ref()
        .expect("durable pre-write recipe");
    assert!(worktree.parent_source.is_none());
    assert_eq!(worktree.metadata, metadata);
    assert_eq!(worktree.created, result.journal);
    let completion = worktree
        .completion
        .as_ref()
        .expect("durable native completion");
    assert_eq!(completion.recorded, result.journal);
    assert_eq!(
        completion.report.output,
        rendered.site().output().inventory().identity()
    );
    assert_eq!(
        completion.report.entries,
        rendered.site().output().inventory().entries().len()
    );
    assert_eq!(
        completion.report.bytes,
        rendered.site().output().inventory().total_bytes()
    );
    assert_eq!(completion.report.parent, metadata.parent);
    let protected = [sources[0].clone(), stage.journal.path().to_owned()];
    let inspected = release::inspect_worktree(
        release::WorktreeRequest {
            root: &stage.root,
            expected: result.record.intent.output,
            metadata: &metadata,
            parent_source: None,
            protected_roots: &protected,
        },
        &completion.report.commit,
        Default::default(),
    )
    .expect("independent native HEAD, index, objects and complete output");
    assert_eq!(inspected.git.commit, completion.report);
    assert_eq!(inspected.output, *rendered.site().output().inventory());
    assert_eq!(
        fs::read(stage.root.join("guide/index.html")).expect("native HTML"),
        bytes(rendered.site().output(), "guide/index.html")
    );
    assert_eq!(
        fs::read(stage.root.join("model.json")).expect("native JSON"),
        bytes(rendered.site().output(), "model.json")
    );
    assert_eq!(inspect(&stage, result.journal), result.record);

    fs::write(
        stage.root.join("guide/index.html"),
        "changed after completion",
    )
    .expect("later worktree edit");
    assert_eq!(
        inspect(&stage, result.journal),
        result.record,
        "historical lookup does not inspect or certify current output"
    );
    release::inspect_worktree(
        release::WorktreeRequest {
            root: &stage.root,
            expected: result.record.intent.output,
            metadata: &metadata,
            parent_source: None,
            protected_roots: &protected,
        },
        &completion.report.commit,
        Default::default(),
    )
    .expect_err("historical completion cannot hide changed native bytes");
    assert_eq!(
        fs::read(stage.root.join("guide/index.html")).expect("history never repairs"),
        b"changed after completion"
    );
}

#[test]
fn exact_replay_never_reinitializes_and_conflicting_recipe_never_replaces_history() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let result = release::prepare_journal_worktree(request(&stage, &metadata, &sources), options())
        .expect("initial worktree");
    let before = source::inspect(&stage.root.join(".git"), None, Default::default())
        .expect("native metadata before replay");
    let request = release::JournalWorktreeRequest {
        expected: result.journal,
        preparation: &result.record,
        ..request(&stage, &metadata, &sources)
    };
    let failure = release::prepare_journal_worktree(request, options())
        .expect_err("exact replay is not permission to adopt");
    assert!(matches!(
        failure.cause.as_ref(),
        release::JournalWorktreeFailure::Replay
    ));
    assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
    assert!(failure.writer_acquired);
    let replay = failure.receipt.expect("original history returned");
    assert!(!replay.inserted);
    assert_eq!(replay.record, result.record);
    assert_eq!(failure.journal, Some(replay.journal));
    assert_eq!(
        source::inspect(&stage.root.join(".git"), None, Default::default())
            .expect("native metadata after replay"),
        before
    );
    assert_eq!(inspect(&stage, replay.journal), result.record);

    let mut changed = metadata.clone();
    changed.message.push_str("different exact recipe\n");
    let failure = release::prepare_journal_worktree(
        release::JournalWorktreeRequest {
            expected: replay.journal,
            preparation: &replay.record,
            metadata: &changed,
            ..request
        },
        options(),
    )
    .expect_err("same preparation cannot acquire a different recipe");
    assert!(matches!(
        failure.cause.as_ref(),
        release::JournalWorktreeFailure::Record(_)
    ));
    assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
    let header = failure.journal.unwrap_or(replay.journal);
    assert_eq!(inspect(&stage, header), result.record);
    assert_eq!(
        source::inspect(&stage.root.join(".git"), None, Default::default())
            .expect("conflict never changes Git"),
        before
    );
}

#[test]
fn missing_retention_and_tampered_record_cannot_initialize_native_metadata() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let metadata = metadata();
    let sources = [fixture.directory.path().to_owned()];
    for case in 0..4 {
        let stage = Retained::new(&rendered);
        let mut supplied = stage.receipt.record.clone();
        match case {
            0 => supplied.retention = None,
            1 => supplied.intent.evidence.push(' '),
            2 => supplied.retention.as_mut().expect("retention").bytes += 1,
            _ => supplied.intent.address.name = "another-stage".into(),
        }
        let failure = release::prepare_journal_worktree(
            release::JournalWorktreeRequest {
                preparation: &supplied,
                ..request(&stage, &metadata, &sources)
            },
            options(),
        )
        .expect_err("caller cannot fabricate retained stage history");
        assert_eq!(
            failure.effect,
            release::WorktreeEffect::NotAttempted,
            "case {case}"
        );
        if case == 0 || case == 2 {
            assert!(matches!(
                failure.cause.as_ref(),
                release::JournalWorktreeFailure::Mismatch
            ));
            assert!(!failure.writer_acquired);
        }
        assert!(!stage.root.join(".git").exists());
        assert!(
            !stage
                .root
                .parent()
                .expect("stage parent")
                .join("another-stage")
                .exists()
        );
        assert_eq!(
            inspect(&stage, failure.journal.unwrap_or(stage.receipt.journal)),
            stage.receipt.record
        );
        assert_eq!(
            source::inspect(&stage.root, None, Default::default())
                .expect("original stage survives"),
            *rendered.site().output().inventory()
        );
    }
}

#[test]
fn permission_recipe_and_root_separation_gates_precede_journal_and_git_writes() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let database = fs::read(&stage.database).expect("original journal bytes");
    for case in 0..5 {
        let mut options = options();
        match case {
            0 => options.record.allow_record = false,
            1 => options.writer.allow_generation_advance = false,
            2 => options.writer.session.allow_local_reservation = false,
            3 => options.writer.session.read.allow_coordination_files = false,
            _ => options.worktree.allow_initialize = false,
        }
        let failure =
            release::prepare_journal_worktree(request(&stage, &metadata, &sources), options)
                .expect_err("each permission must be checked before effects");
        assert_eq!(
            failure.effect,
            release::WorktreeEffect::NotAttempted,
            "case {case}"
        );
        assert!(failure.journal.is_none(), "case {case}: {failure:?}");
        assert!(!failure.writer_acquired);
        assert!(failure.receipt.is_none());
        assert_eq!(
            fs::read(&stage.database).expect("no writer acquired"),
            database
        );
        assert!(!stage.root.join(".git").exists());
    }
    let mut invalid = metadata.clone();
    invalid.message.clear();
    let failure = release::prepare_journal_worktree(request(&stage, &invalid, &sources), options())
        .expect_err("invalid exact recipe");
    assert!(matches!(
        failure.cause.as_ref(),
        release::JournalWorktreeFailure::Recipe(_)
    ));
    assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
    assert!(!failure.writer_acquired);
    assert!(failure.journal.is_none());
    for protected in [[stage.journal.path().to_owned()], [stage.root.clone()]] {
        let failure =
            release::prepare_journal_worktree(request(&stage, &metadata, &protected), options())
                .expect_err("journal and output cannot overlap declared native sources");
        assert!(matches!(
            failure.cause.as_ref(),
            release::JournalWorktreeFailure::Separation(_)
        ));
        assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
        assert!(failure.journal.is_none());
        assert!(!failure.writer_acquired);
    }
    assert_eq!(
        fs::read(&stage.database).expect("all preeffect refusals preserve journal"),
        database
    );
    assert_eq!(inspect(&stage, stage.receipt.journal), stage.receipt.record);
    assert!(!stage.root.join(".git").exists());
}

#[test]
fn post_commit_failure_preserves_durable_recipe_known_oid_and_absent_completion() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let metadata = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let mut limits = options();
    limits.worktree.storage_bytes = 1;
    let failure = release::prepare_journal_worktree(request(&stage, &metadata, &sources), limits)
        .expect_err("late native synchronization failure");
    assert!(matches!(
        failure.cause.as_ref(),
        release::JournalWorktreeFailure::Worktree(_)
    ));
    assert!(failure.writer_acquired);
    let commit = match &failure.effect {
        release::WorktreeEffect::CommitCreated(commit) => commit.clone(),
        effect => panic!("known commit must survive failure: {effect:?}"),
    };
    let receipt = failure.receipt.expect("durable pre-write worktree intent");
    let worktree = receipt
        .record
        .worktree
        .as_ref()
        .expect("recipe survives native failure");
    assert_eq!(worktree.metadata, metadata);
    assert!(worktree.completion.is_none());
    assert_eq!(failure.journal, Some(receipt.journal));
    assert_eq!(inspect(&stage, receipt.journal), receipt.record);
    let protected = [sources[0].clone(), stage.journal.path().to_owned()];
    let observed = release::inspect_worktree(
        release::WorktreeRequest {
            root: &stage.root,
            expected: receipt.record.intent.output,
            metadata: &metadata,
            parent_source: None,
            protected_roots: &protected,
        },
        &commit,
        Default::default(),
    )
    .expect("later read-back does not silently record completion");
    assert_eq!(observed.output, *rendered.site().output().inventory());
    assert_eq!(inspect(&stage, receipt.journal), receipt.record);
    let before = source::inspect(&stage.root.join(".git"), None, Default::default())
        .expect("preserved Git residue");
    let replay = release::prepare_journal_worktree(
        release::JournalWorktreeRequest {
            expected: receipt.journal,
            preparation: &receipt.record,
            ..request(&stage, &metadata, &sources)
        },
        options(),
    )
    .expect_err("retry cannot adopt valid-looking native residue");
    assert!(matches!(
        replay.cause.as_ref(),
        release::JournalWorktreeFailure::Replay
    ));
    assert_eq!(replay.effect, release::WorktreeEffect::NotAttempted);
    assert_eq!(
        replay.receipt.expect("unchanged pending history").record,
        receipt.record
    );
    assert_eq!(
        source::inspect(&stage.root.join(".git"), None, Default::default())
            .expect("no retry writes"),
        before
    );
}

#[test]
fn offline_parent_history_and_provenance_survive_disposal_of_the_parent_worktree() {
    const HISTORICAL: &[u8] = b"%PDF-1.7\n\0\xffonly in the parent publication";
    let home = format!("{}\n[Historical download](history.pdf)\n", super::HOME);
    let parent_fixture = published_fixture(&[
        ("one/home.md", home.as_bytes()),
        ("one/history.pdf", HISTORICAL),
    ]);
    let config = parent_fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]",
    );
    let parent_inputs = acquire(&config);
    let parent_render = render(&parent_inputs).expect("parent publication");
    let parent = Retained::new(&parent_render);
    let parent_metadata = metadata();
    let parent_sources = [parent_fixture.directory.path().to_owned()];
    let first = release::prepare_journal_worktree(
        request(&parent, &parent_metadata, &parent_sources),
        options(),
    )
    .expect("first independent journalled worktree");
    let parent_commit = first
        .record
        .worktree
        .as_ref()
        .expect("parent intent")
        .completion
        .as_ref()
        .expect("parent completion")
        .report
        .commit
        .clone();

    let child_page = super::PAGE
        .replace("lifecycle='draft'", "lifecycle='published'")
        .replace("**Retained** body", "**Second generation** body");
    let child_fixture = published_fixture(&[("one/page.md", child_page.as_bytes())]);
    let child_inputs = acquire(&child_fixture.config());
    let child_render = render(&child_inputs).expect("child publication");
    let child = Retained::new(&child_render);
    let mut child_metadata = metadata();
    child_metadata.parent = Some(parent_commit.clone());
    child_metadata.message = "Exact second generation\n".into();
    let parent_source = release::ParentSource::Worktree {
        path: parent.root.clone(),
    };
    let sources = [
        parent_fixture.directory.path().to_owned(),
        child_fixture.directory.path().to_owned(),
    ];
    let parent_before = source::inspect(&parent.root.join(".git"), None, Default::default())
        .expect("parent metadata before import");
    let next = release::prepare_journal_worktree(
        release::JournalWorktreeRequest {
            parent_source: Some(&parent_source),
            ..request(&child, &child_metadata, &sources)
        },
        options(),
    )
    .expect("offline parent closure imported before exact child commit");
    let worktree = next.record.worktree.as_ref().expect("child intent");
    assert_eq!(worktree.parent_source.as_ref(), Some(&parent_source));
    assert_eq!(worktree.metadata, child_metadata);
    let completion = worktree.completion.as_ref().expect("child completion");
    assert_eq!(completion.report.parent.as_ref(), Some(&parent_commit));
    assert_eq!(
        completion.report.output,
        child_render.site().output().inventory().identity()
    );
    assert_eq!(inspect(&child, next.journal), next.record);
    assert_eq!(inspect(&parent, first.journal), first.record);
    assert_eq!(
        source::inspect(&parent.root.join(".git"), None, Default::default())
            .expect("parent never mutated by import"),
        parent_before
    );
    for entry in child_render.site().output().inventory().entries() {
        if let source::Content::File { .. } = entry.content {
            assert_eq!(
                fs::read(child.root.join(entry.path.as_str())).expect("exact child output"),
                bytes(child_render.site().output(), entry.path.as_str())
            );
        }
    }
    assert!(
        !child.root.join("assets/history.pdf").exists(),
        "history is not merged into current publication output"
    );

    // A different explicit source kind cannot replace the durable acquisition provenance,
    // even though it names the same local path and exact pinned parent.
    let different_source = release::ParentSource::Bare {
        path: parent.root.clone(),
    };
    let child_git_before = source::inspect(&child.root.join(".git"), None, Default::default())
        .expect("child metadata before conflicting replay");
    let failure = release::prepare_journal_worktree(
        release::JournalWorktreeRequest {
            expected: next.journal,
            preparation: &next.record,
            parent_source: Some(&different_source),
            ..request(&child, &child_metadata, &sources)
        },
        options(),
    )
    .expect_err("typed source provenance cannot change on replay");
    assert!(matches!(
        failure.cause.as_ref(),
        release::JournalWorktreeFailure::Record(_)
    ));
    assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
    let current = failure.journal.unwrap_or(next.journal);
    assert_eq!(inspect(&child, current), next.record);
    assert_eq!(
        source::inspect(&child.root.join(".git"), None, Default::default())
            .expect("conflict does not adopt or rewrite child"),
        child_git_before
    );

    // Only this test-owned retained worktree is removed; authored repositories and
    // both journals remain. Verification must use the child's own complete objects.
    fs::remove_dir_all(&parent.root).expect("dispose exact fixture parent worktree");
    let protected = [
        sources[0].clone(),
        sources[1].clone(),
        child.journal.path().to_owned(),
    ];
    let observed = release::inspect_worktree(
        release::WorktreeRequest {
            root: &child.root,
            expected: completion.report.output,
            metadata: &child_metadata,
            parent_source: Some(&parent_source),
            protected_roots: &protected,
        },
        &completion.report.commit,
        Default::default(),
    )
    .expect("child independently verifies without original parent source");
    assert_eq!(observed.git.commit, completion.report);
    assert_eq!(observed.output, *child_render.site().output().inventory());
    let repository = git2::Repository::open(&child.root).expect("independent child repository");
    let oid = git2::Oid::from_str(completion.report.commit.as_str()).expect("child OID");
    let commit = repository.find_commit(oid).expect("child object");
    assert_eq!(commit.parent_count(), 1);
    assert_eq!(
        commit
            .parent_id(0)
            .expect("exact native parent")
            .to_string(),
        parent_commit.as_str()
    );
    let historical_tree = commit
        .parent(0)
        .expect("copied parent commit")
        .tree()
        .expect("copied parent tree");
    let historical = historical_tree
        .get_path(std::path::Path::new("assets/history.pdf"))
        .expect("parent-only blob retained");
    assert_eq!(
        repository
            .find_blob(historical.id())
            .expect("independent copied historical blob")
            .content(),
        HISTORICAL
    );
    assert_eq!(inspect(&child, current), next.record);
    assert_eq!(
        fs::read(child_fixture.directory.path().join("one/page.md"))
            .expect("child source preserved"),
        child_page.as_bytes()
    );
    assert_eq!(
        fs::read(parent_fixture.directory.path().join("one/history.pdf"))
            .expect("parent authored asset preserved"),
        HISTORICAL
    );
}

#[test]
fn parent_selection_mismatch_and_native_source_overlap_never_initialize_git() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("retained publication");
    let stage = Retained::new(&rendered);
    let root_metadata = metadata();
    let mut child_metadata = metadata();
    child_metadata.parent = Some(
        fixture
            .commit
            .to_string()
            .parse()
            .expect("exact syntactic parent pin"),
    );
    let sources = [fixture.directory.path().to_owned()];
    let selected = release::ParentSource::Worktree {
        path: fixture.directory.path().to_owned(),
    };
    let database = fs::read(&stage.database).expect("original journal");
    for (metadata, parent_source) in [(&root_metadata, Some(&selected)), (&child_metadata, None)] {
        let failure = release::prepare_journal_worktree(
            release::JournalWorktreeRequest {
                parent_source,
                ..request(&stage, metadata, &sources)
            },
            options(),
        )
        .expect_err("root selection and parent acquisition are mutually consistent");
        assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
        assert!(!failure.writer_acquired);
        assert!(failure.receipt.is_none());
        assert!(!stage.root.join(".git").exists());
        assert_eq!(
            fs::read(&stage.database).expect("no intent for inconsistent selection"),
            database
        );
    }
    for path in [stage.root.clone(), stage.journal.path().to_owned()] {
        let overlap = release::ParentSource::Worktree { path };
        let failure = release::prepare_journal_worktree(
            release::JournalWorktreeRequest {
                parent_source: Some(&overlap),
                ..request(&stage, &child_metadata, &sources)
            },
            options(),
        )
        .expect_err("parent acquisition cannot share the output or private journal");
        assert_eq!(failure.effect, release::WorktreeEffect::NotAttempted);
        assert!(!stage.root.join(".git").exists());
        let current = failure.journal.unwrap_or(stage.receipt.journal);
        assert_eq!(inspect(&stage, current), stage.receipt.record);
    }
    assert_eq!(
        source::inspect(&stage.root, None, Default::default()).expect("untouched retained output"),
        *rendered.site().output().inventory()
    );
}
