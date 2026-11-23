//! Inject lost boundary reports only after real native operations, never substitute effects.

#[path = "journal_worktree_failure/inspection.rs"]
mod inspection;
#[path = "journal_worktree_failure/recovery.rs"]
mod recovery;

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use gazette_adapter_fs::FileSystem;
use gazette_adapter_git::Git;
use gazette_adapter_sqlite::{Sqlite, Writer};
use gazette_application::release::{self, JournalWorktreeError, JournalWorktreeFailure};
use gazette_core::{
    release::{CommitMetadata, CommitSignature, WorktreeEffect},
    source::{CapturedSource, Sha256Digest},
    store::{
        Error, InitializeOptions, JournalHeader, JournalInspection, PreparationCommitInput,
        PreparationEffect, PreparationInput, PreparationInspection, PreparationReceipt,
        PreparationRetentionInput, PreparationWorktreeInput, PreparationWriteError,
        PreparationWriteOptions, ReadOptions, SessionOptions, WriterAcquireError, WriterOptions,
    },
};
use gazette_ports::{
    source::{SourceCapture, SourceReader},
    stage::{StagePlanner, StageTarget},
    store::{
        JournalWriter, PreparationReader, PreparationWriter, StoreInitializer, WriterConnector,
    },
};
use tempfile::TempDir;

use gazette_test_support as native;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Boundary {
    AcquiredInspection,
    BeforeCompletion,
    CompletionReport,
    ClosedReport,
}

struct FaultStore(Boundary);
struct FaultWriter {
    native: Writer,
    boundary: Boundary,
    first_inspection: bool,
}

impl WriterConnector for FaultStore {
    type Writer = FaultWriter;

    fn acquire_writer(
        &self,
        root: &Path,
        expected: JournalHeader,
        options: WriterOptions,
    ) -> Result<FaultWriter, WriterAcquireError> {
        Ok(FaultWriter {
            native: Sqlite.acquire_writer(root, expected, options)?,
            boundary: self.0,
            first_inspection: true,
        })
    }
}

impl JournalWriter for FaultWriter {
    fn inspect(&mut self) -> Result<JournalInspection, Error> {
        let observed = self.native.inspect()?;
        let first = std::mem::replace(&mut self.first_inspection, false);
        if first && self.boundary == Boundary::AcquiredInspection {
            Err(Error::Changed)
        } else {
            Ok(observed)
        }
    }

    fn close(self) -> Result<(), Error> {
        self.native.close()?;
        if self.boundary == Boundary::ClosedReport {
            Err(Error::Changed)
        } else {
            Ok(())
        }
    }
}

impl PreparationWriter for FaultWriter {
    fn record_preparation_check(
        &mut self,
        input: gazette_core::store::PreparationCheckInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.native.record_preparation_check(input, options)
    }

    fn inspect_preparation(&mut self, id: Sha256Digest) -> Result<PreparationInspection, Error> {
        self.native.inspect_preparation(id)
    }

    fn record_preparation_intent(
        &mut self,
        input: PreparationInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.native.record_preparation_intent(input, options)
    }
    fn record_preparation_retention(
        &mut self,
        input: PreparationRetentionInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.native.record_preparation_retention(input, options)
    }
    fn record_preparation_worktree(
        &mut self,
        input: PreparationWorktreeInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.native.record_preparation_worktree(input, options)
    }
    fn record_preparation_commit(
        &mut self,
        input: PreparationCommitInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        if self.boundary == Boundary::BeforeCompletion {
            return Err(PreparationWriteError {
                cause: Error::Changed,
                effect: PreparationEffect::NotAttempted,
            });
        }
        let receipt = self.native.record_preparation_commit(input, options)?;
        if self.boundary == Boundary::CompletionReport {
            Err(PreparationWriteError {
                cause: Error::Changed,
                effect: PreparationEffect::Unverified,
            })
        } else {
            Ok(receipt)
        }
    }
}

fn read() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}
fn writer_options() -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read: read(),
        },
    }
}
fn permission() -> PreparationWriteOptions {
    PreparationWriteOptions { allow_record: true }
}

struct Fixture {
    _root: TempDir,
    source: PathBuf,
    journal: PathBuf,
    database: PathBuf,
    stage: PathBuf,
    captured: CapturedSource,
    receipt: PreparationReceipt,
    metadata: CommitMetadata,
}

impl Fixture {
    fn new() -> Self {
        Self::with_evidence("{\"kind\":\"bounded-native-fixture\"}")
    }

    fn with_evidence(evidence: &str) -> Self {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private fixture root");
        let source = root.path().join("source");
        let journal = root.path().join("journal");
        let parent = root.path().join("stages");
        for path in [&source, &journal, &parent] {
            fs::create_dir(path).expect("independent native root");
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("private root permissions");
        }
        fs::write(source.join("index.html"), b"<h1>Retained bytes</h1>\r\n")
            .expect("real HTML source");
        fs::write(
            source.join("model.json"),
            b"{\"title\":\"Retained bytes\"}\n",
        )
        .expect("real machine source");
        let captured = FileSystem
            .capture(&source, None, Default::default())
            .expect("native complete capture");
        let initialized = Sqlite
            .initialize(
                &journal,
                InitializeOptions {
                    allow_create: true,
                    session: writer_options().session,
                    ..Default::default()
                },
            )
            .expect("actual SQLite schema");
        let mut writer = Sqlite
            .acquire_writer(&journal, initialized.header, writer_options())
            .expect("native stage writer");
        let target = FileSystem
            .plan_stage(&parent, "deployment", &[source.clone(), journal.clone()])
            .expect("held native stage target");
        let intent = writer
            .record_preparation_intent(
                PreparationInput {
                    id: Sha256Digest::of(b"native failure boundary fixture"),
                    address: target.address(),
                    destination: None,
                    output: captured.inventory().identity(),
                    entries: captured.inventory().entries().len() as u64,
                    bytes: captured.inventory().total_bytes(),
                    evidence,
                    evidence_sha256: Sha256Digest::of(evidence.as_bytes()),
                },
                permission(),
            )
            .expect("durable intent before stage creation");
        let retained = target
            .create(captured.as_input(), Default::default())
            .expect("actual stage creation")
            .retain()
            .expect("native synchronization and read-back");
        let receipt = writer
            .record_preparation_retention(
                PreparationRetentionInput {
                    intent: &intent.record.intent,
                    output: retained.inventory().identity(),
                    entries: retained.inventory().entries().len() as u64,
                    bytes: retained.inventory().total_bytes(),
                },
                permission(),
            )
            .expect("durable retention after native stage");
        writer
            .close()
            .expect("stage writer closes before coordinator");
        let signature = CommitSignature {
            name: "Native fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 100,
            offset_minutes: 330,
        };
        let metadata = CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Exact retained fixture\n".into(),
        };
        Self {
            _root: root,
            source,
            journal,
            database: initialized.database,
            stage: retained.path().to_owned(),
            captured,
            receipt,
            metadata,
        }
    }

    fn invoke(&self, boundary: Boundary) -> JournalWorktreeError {
        release::prepare_journal_worktree(
            &FileSystem,
            &Git,
            &FaultStore(boundary),
            release::JournalWorktreeRequest {
                journal: &self.journal,
                expected: self.receipt.journal,
                preparation: &self.receipt.record,
                metadata: &self.metadata,
                parent_source: None,
                protected_roots: std::slice::from_ref(&self.source),
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
        .expect_err("precisely injected boundary failure")
    }

    fn history(&self) -> PreparationInspection {
        Sqlite
            .inspect_preparation(&self.database, self.receipt.record.intent.id, read())
            .expect("independent native journal read")
    }

    fn verify_completed(&self, failure: &JournalWorktreeError, history: &PreparationInspection) {
        assert!(failure.writer_acquired);
        assert_eq!(failure.journal, Some(history.journal.header));
        let record = history.record.as_ref().expect("durable history survives");
        assert_eq!(record.intent, self.receipt.record.intent);
        assert_eq!(record.retention, self.receipt.record.retention);
        let worktree = record.worktree.as_ref().expect("durable worktree intent");
        assert_eq!(worktree.metadata, self.metadata);
        assert!(worktree.parent_source.is_none());
        let completion = worktree
            .completion
            .as_ref()
            .expect("actual committed completion survives report failure");
        assert_eq!(
            failure.effect,
            WorktreeEffect::CommitCreated(completion.report.commit.clone())
        );
        let protected = [self.source.clone(), self.journal.clone()];
        let git_before = FileSystem
            .read(&self.stage.join(".git"), None, Default::default())
            .expect("native Git bytes before inspection");
        let inspected = gazette_sdk::release_commit::inspect_worktree(
            release::WorktreeRequest {
                root: &self.stage,
                expected: self.captured.inventory().identity(),
                metadata: &self.metadata,
                parent_source: None,
                protected_roots: &protected,
            },
            &completion.report.commit,
            Default::default(),
        )
        .expect("independent native HEAD, index, commit and complete output");
        assert_eq!(inspected.git.commit, completion.report);
        assert_eq!(inspected.output, *self.captured.inventory());
        assert_eq!(
            FileSystem
                .read(&self.stage.join(".git"), None, Default::default())
                .expect("Git metadata after read-only verification"),
            git_before
        );
        assert_eq!(
            FileSystem
                .read(&self.source, None, Default::default())
                .expect("source unchanged"),
            *self.captured.inventory()
        );
        for name in ["index.html", "model.json"] {
            assert_eq!(
                fs::read(self.stage.join(name)).expect("retained output file"),
                fs::read(self.source.join(name)).expect("unchanged original source file")
            );
        }
        assert_eq!(
            self.history().record.as_ref(),
            Some(record),
            "native verification does not modify durable history"
        );
    }
}

#[test]
fn lost_first_inspection_retains_acquisition_without_inventing_a_header() {
    let fixture = Fixture::new();
    let failure = fixture.invoke(Boundary::AcquiredInspection);
    assert!(matches!(
        failure.cause.as_ref(),
        JournalWorktreeFailure::Journal(Error::Changed)
    ));
    assert!(failure.writer_acquired);
    assert!(failure.journal.is_none());
    assert!(failure.receipt.is_none());
    assert_eq!(failure.effect, WorktreeEffect::NotAttempted);
    let history = fixture.history();
    assert_eq!(
        history.journal.header,
        JournalHeader {
            writer_fence: fixture.receipt.journal.writer_fence + 1,
            ..fixture.receipt.journal
        }
    );
    assert_eq!(history.record.as_ref(), Some(&fixture.receipt.record));
    assert!(
        history
            .record
            .expect("original stage record")
            .worktree
            .is_none()
    );
    assert!(!fixture.stage.join(".git").exists());
    assert_eq!(
        FileSystem
            .read(&fixture.stage, None, Default::default())
            .expect("untouched stage"),
        *fixture.captured.inventory()
    );
    assert_eq!(
        FileSystem
            .read(&fixture.source, None, Default::default())
            .expect("untouched source"),
        *fixture.captured.inventory()
    );
}

#[test]
fn lost_completion_report_preserves_known_commit_and_prior_verified_receipt() {
    let fixture = Fixture::new();
    let failure = fixture.invoke(Boundary::CompletionReport);
    assert!(matches!(
        failure.cause.as_ref(),
        JournalWorktreeFailure::Record(PreparationWriteError {
            cause: Error::Changed,
            effect: PreparationEffect::Unverified
        })
    ));
    let prior = failure
        .receipt
        .as_ref()
        .expect("last verified intent receipt");
    assert!(
        prior
            .record
            .worktree
            .as_ref()
            .expect("Git intent")
            .completion
            .is_none()
    );
    let history = fixture.history();
    fixture.verify_completed(&failure, &history);
    let mut original = history.record.expect("actual completion record");
    original.worktree.as_mut().expect("Git intent").completion = None;
    assert_eq!(
        prior.record, original,
        "only the lost completion differs from the returned historical receipt"
    );
}

#[test]
fn lost_close_report_preserves_the_latest_completed_receipt_and_commit() {
    let fixture = Fixture::new();
    let failure = fixture.invoke(Boundary::ClosedReport);
    assert!(matches!(
        failure.cause.as_ref(),
        JournalWorktreeFailure::Journal(Error::Changed)
    ));
    let history = fixture.history();
    fixture.verify_completed(&failure, &history);
    let returned = failure.receipt.expect("latest verified completion receipt");
    assert_eq!(Some(&returned.record), history.record.as_ref());
    assert!(returned.inserted);
}

#[test]
fn staging_lost_acquisition_report_preserves_generation_without_intent_or_output() {
    use gazette_sdk::build;

    let private = || {
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private native fixture")
    };
    let source = private();
    let journal = private();
    let parent = private();
    fs::create_dir_all(source.path().join("theme/templates")).expect("Theme directories");
    fs::create_dir(source.path().join("content")).expect("authored directory");
    fs::write(
        source.path().join("theme/theme.toml"),
        "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n",
    )
    .expect("native Theme manifest");
    fs::write(
        source.path().join("theme/templates/page.html"),
        "<h1>{{ content.title }}</h1>{{ body_html | safe }}",
    )
    .expect("native Theme template");
    fs::write(
        source.path().join("theme/templates/alias.html"),
        "<meta http-equiv=\"refresh\" content=\"0; url={{ route.canonical_url }}\"><a href=\"{{ route.canonical_url }}\">Moved</a>",
    )
    .expect("native Theme alias template");
    fs::write(source.path().join("content/home.md"), "+++\n[content]\nid='content:home'\nsite='site:one'\ntitle='Home'\nlifecycle='published'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\nRetained body\n").expect("native authored content");
    let git = |args: &[&str]| {
        let output = std::process::Command::new(native::git())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00+00:00")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00+00:00")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .arg("-C")
            .arg(source.path())
            .args(args)
            .output()
            .expect("disposable fixture Git");
        assert!(output.status.success(), "fixture Git failed: {output:?}");
        String::from_utf8(output.stdout).expect("native Git result")
    };
    git(&["init", "--template=", "--initial-branch=fixture"]);
    git(&["add", "theme", "content"]);
    git(&["commit", "--no-gpg-sign", "-m", "fixture"]);
    let commit = git(&["rev-parse", "HEAD"]);
    let mut config = String::from(
        "[composition]\ntheme={package='theme:one'}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.invalid/'\nauthored=[{source='source:content',target='content'}]\n",
    );
    for key in ["theme", "content"] {
        config.push_str(&format!("[[sources]]\nid='source:{key}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{key}'}}\n", source.path().to_str().expect("native source path"), commit.trim()));
    }
    config.push_str(
        "[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
    );
    let inputs = build::acquire_publication(
        &config,
        &"site:one".parse().expect("site key"),
        Default::default(),
        gazette_sdk::source::PreparationLimits {
            sources: 2,
            entries: 16,
            bytes: 1024 * 1024,
            per_source: gazette_sdk::source::RevisionLimits::default(),
        },
    )
    .expect("actual pinned publication acquisition");
    let parsed =
        build::parse_publication(&inputs, Default::default()).expect("retained publication syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("complete publication graph");
    let selected = resolved
        .into_selected(gazette_sdk::authored::Selection {
            content: std::collections::BTreeMap::from([(
                "content:home".parse().expect("selected content identity"),
                gazette_core::publication::content::Disposition::Include,
            )]),
            assets: std::collections::BTreeMap::new(),
        })
        .expect("complete non-draft publication candidate");
    let rendered = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("machine path")),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("actual native publication rendering");
    assert!(
        rendered
            .site()
            .output()
            .file(&"index.html".parse().expect("HTML path"))
            .is_some()
    );
    let source_before = FileSystem
        .read(source.path(), None, Default::default())
        .expect("source before staging");
    let initialized = Sqlite
        .initialize(
            journal.path(),
            InitializeOptions {
                allow_create: true,
                session: writer_options().session,
                ..Default::default()
            },
        )
        .expect("actual empty private journal");
    let id = Sha256Digest::of(b"stage acquisition report loss");
    let failure = rendered
        .prepare_stage(
            &FileSystem,
            &FaultStore(Boundary::AcquiredInspection),
            build::PreparationRequest {
                journal: journal.path(),
                expected: initialized.header,
                parent: parent.path(),
                name: "deployment",
                id,
            },
            build::PreparationOptions {
                allow_stage: true,
                record: permission(),
                writer: writer_options(),
                ..Default::default()
            },
        )
        .expect_err("real acquired writer loses first caller inspection report");
    assert!(matches!(
        failure.cause.as_ref(),
        build::PreparationFailure::Journal(Error::Changed)
    ));
    assert!(failure.writer_acquired);
    assert!(failure.journal.is_none());
    assert!(failure.receipt.is_none());
    assert!(failure.stage.is_none());
    let observed = Sqlite
        .inspect_preparation(&initialized.database, id, read())
        .expect("independent native history after acquisition");
    assert_eq!(
        observed.journal.header,
        JournalHeader {
            writer_fence: initialized.header.writer_fence + 1,
            ..initialized.header
        }
    );
    assert!(observed.record.is_none(), "no staging intent was attempted");
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("native stage parent")
            .count(),
        0,
        "no stage or residue was created"
    );
    assert_eq!(
        FileSystem
            .read(source.path(), None, Default::default())
            .expect("source after refusal"),
        source_before
    );
}

#[test]
fn parent_loss_after_native_import_does_not_block_journalled_child_completion() {
    use gazette_core::{
        release::{ParentSource, WorktreeError, WorktreeLimits, WorktreeOptions, WorktreeReport},
        source::{CommitId, TreeIdentity, TreeInput},
    };
    use gazette_ports::release::ReleaseWorktrees;

    // The wrapper changes only this disposable fixture's already-imported parent.
    // Every Git effect and every Git inspection still uses the real native adapter.
    struct RemoveParentAfterImport<'a>(&'a Fixture);
    impl ReleaseWorktrees for RemoveParentAfterImport<'_> {
        fn prepare_worktree(
            &self,
            root: &Path,
            input: TreeInput<'_>,
            metadata: &CommitMetadata,
            parent_source: Option<&ParentSource>,
            options: WorktreeOptions,
        ) -> Result<WorktreeReport, WorktreeError> {
            assert_eq!(parent_source.expect("explicit parent").path(), self.0.stage);
            assert_ne!(root, self.0.stage);
            let report = Git.prepare_worktree(root, input, metadata, parent_source, options)?;
            fs::remove_dir_all(&self.0.stage)
                .expect("remove only the test-owned imported parent stage");
            assert!(!self.0.stage.exists());
            Ok(report)
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

    let options = release::JournalWorktreeOptions {
        record: permission(),
        writer: writer_options(),
        worktree: WorktreeOptions {
            allow_initialize: true,
            ..Default::default()
        },
    };
    let parent = Fixture::new();
    let first = release::prepare_journal_worktree(
        &FileSystem,
        &Git,
        &Sqlite,
        release::JournalWorktreeRequest {
            journal: &parent.journal,
            expected: parent.receipt.journal,
            preparation: &parent.receipt.record,
            metadata: &parent.metadata,
            parent_source: None,
            protected_roots: std::slice::from_ref(&parent.source),
        },
        options,
    )
    .expect("actual parent worktree and durable completion");
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
    let child = Fixture::new();
    let mut metadata = child.metadata.clone();
    metadata.parent = Some(parent_commit.clone());
    let source = ParentSource::Worktree {
        path: parent.stage.clone(),
    };
    let protected = [
        parent.source.clone(),
        child.source.clone(),
        parent.journal.clone(),
    ];
    let complete = release::prepare_journal_worktree(
        &FileSystem,
        &RemoveParentAfterImport(&parent),
        &Sqlite,
        release::JournalWorktreeRequest {
            journal: &child.journal,
            expected: child.receipt.journal,
            preparation: &child.receipt.record,
            metadata: &metadata,
            parent_source: Some(&source),
            protected_roots: &protected,
        },
        options,
    )
    .expect("paired read-back and completion do not reopen the imported parent");
    assert!(!parent.stage.exists());
    let worktree = complete.record.worktree.as_ref().expect("child intent");
    assert_eq!(worktree.parent_source.as_ref(), Some(&source));
    assert_eq!(worktree.metadata, metadata);
    let done = worktree
        .completion
        .as_ref()
        .expect("child durable completion");
    assert_eq!(done.report.parent.as_ref(), Some(&parent_commit));
    let mut protected = protected.to_vec();
    protected.push(child.journal.clone());
    let observed = gazette_sdk::release_commit::inspect_worktree(
        release::WorktreeRequest {
            root: &child.stage,
            expected: child.captured.inventory().identity(),
            metadata: &metadata,
            parent_source: Some(&source),
            protected_roots: &protected,
        },
        &done.report.commit,
        Default::default(),
    )
    .expect("fresh independent native child inspection");
    assert_eq!(observed.git.commit, done.report);
    assert_eq!(observed.output, *child.captured.inventory());
    for name in ["index.html", "model.json"] {
        assert_eq!(
            fs::read(child.stage.join(name)).expect("exact child bytes"),
            fs::read(child.source.join(name)).expect("original captured bytes")
        );
    }
    let parent_history = parent.history();
    assert_eq!(parent_history.journal.header, first.journal);
    assert_eq!(parent_history.record, Some(first.record));
    let child_history = child.history();
    assert_eq!(child_history.journal.header, complete.journal);
    assert_eq!(child_history.record, Some(complete.record));
    for fixture in [&parent, &child] {
        assert!(fixture.database.is_file());
        assert_eq!(
            FileSystem
                .read(&fixture.source, None, Default::default())
                .expect("authored source remains unchanged"),
            *fixture.captured.inventory()
        );
    }
}
