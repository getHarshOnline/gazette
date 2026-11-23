//! Actual SDK Git verification and private `SQLite` checkpointing, without release authority.

#[path = "release_checkpoint/output.rs"]
mod output;

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use gazette_sdk::{
    release_commit::{
        self, CheckpointError, CheckpointOptions, CheckpointRequest, CommitMetadata, CommitOptions,
        CommitReport, CommitSignature,
    },
    source::{self, CommitId, Sha256Digest, TreeIdentity},
    store::{self, JournalHeader, ReadOptions},
};
use tempfile::TempDir;

use gazette_test_support as native;

fn read() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn options() -> CheckpointOptions {
    CheckpointOptions {
        record: release_commit::CheckpointWriteOptions { allow_record: true },
        writer: store::WriterOptions {
            allow_generation_advance: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read: read(),
            },
        },
        ..Default::default()
    }
}

fn metadata() -> CommitMetadata {
    CommitMetadata {
        parent: None,
        author: CommitSignature {
            name: "Fixture author".into(),
            email: "author@example.invalid".into(),
            seconds: 100,
            offset_minutes: 330,
        },
        committer: CommitSignature {
            name: "Fixture committer".into(),
            email: "committer@example.invalid".into(),
            seconds: 101,
            offset_minutes: -60,
        },
        message: "Exact local preparation\nSecond line\n".into(),
    }
}

fn private(path: &Path) {
    fs::create_dir(path).expect("create disposable private directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("private directory permissions");
}

fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new(native::git())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .arg("--git-dir")
        .arg(repository)
        .args(args)
        .output()
        .expect("fixture-only native Git operation");
    assert!(output.status.success(), "fixture Git failed: {output:?}");
    String::from_utf8(output.stdout).expect("Git fixture response")
}

fn tree(root: &Path) -> TreeIdentity {
    source::inspect(root, None, Default::default())
        .expect("complete native tree observation")
        .identity()
}

struct Fixture {
    _root: TempDir,
    repository: PathBuf,
    output: PathBuf,
    source: PathBuf,
    journal: PathBuf,
    initialized: store::InitializationReceipt,
    output_identity: TreeIdentity,
    metadata: CommitMetadata,
    prepared: CommitReport,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private fixture root");
        let repository = root.path().join("deployment.git");
        let output = root.path().join("reviewed");
        let source = root.path().join("authored");
        let journal = root.path().join("journal");
        private(&output);
        private(&source);
        private(&journal);
        fs::create_dir(output.join("assets")).expect("nested retained output");
        fs::write(output.join("index.html"), b"<h1>Retained output</h1>\n").expect("fixture page");
        fs::write(output.join("assets/data.txt"), b"exact\r\nbytes\n").expect("fixture asset");
        fs::write(source.join("article.md"), b"Private authored source\n")
            .expect("protected source");
        git(
            &repository,
            &[
                "init",
                "--bare",
                "--template=",
                "--initial-branch=unborn",
                repository.to_str().expect("repository path"),
            ],
        );
        git(
            &repository,
            &["config", "fixture.preserved", "exact configured value"],
        );
        let output_identity = tree(&output);
        let metadata = metadata();
        let prepared = release_commit::prepare(
            &repository,
            &output,
            output_identity,
            &metadata,
            std::slice::from_ref(&source),
            CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("real SDK retained-tree detached commit preparation");
        git(
            &repository,
            &[
                "update-ref",
                "refs/heads/fixture-sentinel",
                prepared.commit.as_str(),
            ],
        );
        let initialized = store::initialize(
            &journal,
            store::InitializeOptions {
                allow_create: true,
                session: store::SessionOptions {
                    allow_local_reservation: true,
                    read: read(),
                },
                ..Default::default()
            },
        )
        .expect("real SQLite empty-journal initialization");
        Self {
            _root: root,
            repository,
            output,
            source,
            journal,
            initialized,
            output_identity,
            metadata,
            prepared,
        }
    }

    fn request(&self, id: Sha256Digest) -> CheckpointRequest<'_> {
        CheckpointRequest {
            id,
            repository: &self.repository,
            commit: &self.prepared.commit,
            output: self.output_identity,
            metadata: &self.metadata,
            protected_roots: &[],
        }
    }

    fn checkpoint(
        &self,
        id: Sha256Digest,
        header: JournalHeader,
        options: CheckpointOptions,
    ) -> Result<release_commit::CheckpointReceipt, CheckpointError> {
        let protected = [self.output.clone(), self.source.clone()];
        release_commit::checkpoint(
            &self.journal,
            header,
            CheckpointRequest {
                protected_roots: &protected,
                ..self.request(id)
            },
            options,
        )
    }

    fn header(&self) -> JournalHeader {
        store::inspect_journal(&self.initialized.database, None, read())
            .expect("fresh native journal inspection")
            .header
    }

    fn snapshot(&self) -> [TreeIdentity; 3] {
        [
            tree(&self.repository),
            tree(&self.output),
            tree(&self.source),
        ]
    }
}

#[test]
fn prepare_checkpoint_fresh_inspection_and_native_verify_share_exact_evidence() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"independently selected local checkpoint");
    let before = fixture.snapshot();
    assert!(
        release_commit::inspect_checkpoint(
            &fixture.initialized.database,
            id,
            Some(&fixture.initialized.header),
            read()
        )
        .expect("missing is observed without creation")
        .checkpoint
        .is_none()
    );
    let receipt = fixture
        .checkpoint(id, fixture.initialized.header, options())
        .expect("native verified checkpoint");
    assert_eq!(
        receipt.journal.writer_fence,
        fixture.initialized.header.writer_fence + 1
    );
    assert_eq!(receipt.checkpoint.created, receipt.journal);
    assert_eq!(receipt.checkpoint.id, id);
    assert_eq!(
        receipt.checkpoint.repository,
        fixture.repository.to_str().expect("exact local spelling")
    );
    assert_eq!(receipt.checkpoint.metadata, fixture.metadata);
    assert_eq!(receipt.checkpoint.commit, fixture.prepared);
    let inspected = release_commit::inspect_checkpoint(
        &fixture.initialized.database,
        id,
        Some(&receipt.journal),
        read(),
    )
    .expect("fresh connection reads complete evidence");
    assert_eq!(inspected.journal.header, receipt.journal);
    assert_eq!(inspected.checkpoint.as_ref(), Some(&receipt.checkpoint));
    let verified = release_commit::verify(
        &fixture.repository,
        &fixture.prepared.commit,
        fixture.output_identity,
        &fixture.metadata,
        Default::default(),
    )
    .expect("record does not replace current native verification");
    assert_eq!(verified, receipt.checkpoint.commit);
    assert_eq!(
        fixture.snapshot(),
        before,
        "checkpointing changes no Git object/ref/config or authored/output bytes"
    );

    let replay = fixture
        .checkpoint(id, receipt.journal, options())
        .expect("exact intent replay");
    assert_eq!(
        replay.journal.writer_fence,
        receipt.journal.writer_fence + 1
    );
    assert_eq!(
        replay.checkpoint, receipt.checkpoint,
        "creation coordinates remain historical, not the current writer"
    );
    assert_eq!(fixture.header(), replay.journal);
    let current = release_commit::inspect_checkpoint(
        &fixture.initialized.database,
        id,
        Some(&replay.journal),
        read(),
    )
    .expect("fresh replay read-back");
    assert_eq!(current.checkpoint, Some(receipt.checkpoint));
    assert!(matches!(
        release_commit::inspect_checkpoint(
            &fixture.initialized.database,
            id,
            Some(&receipt.journal),
            read()
        ),
        Err(store::Error::JournalMismatch)
    ));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn same_coordinate_changed_valid_commit_conflicts_without_replacing_original() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"immutable checkpoint coordinate");
    let first = fixture
        .checkpoint(id, fixture.initialized.header, options())
        .expect("initial checkpoint");
    let mut changed = fixture.metadata.clone();
    changed.message.push_str("Changed exact intent\n");
    let second = release_commit::prepare(
        &fixture.repository,
        &fixture.output,
        fixture.output_identity,
        &changed,
        std::slice::from_ref(&fixture.source),
        CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("independently valid different local commit");
    assert_ne!(second.commit, fixture.prepared.commit);
    let before = fixture.snapshot();
    let error = release_commit::checkpoint(
        &fixture.journal,
        first.journal,
        CheckpointRequest {
            commit: &second.commit,
            metadata: &changed,
            ..fixture.request(id)
        },
        options(),
    )
    .expect_err("one record coordinate cannot change meaning");
    assert!(
        matches!(error, CheckpointError::Write(error) if matches!(error.cause, store::Error::CheckpointConflict))
    );
    let current = fixture.header();
    assert_eq!(
        current.writer_fence,
        first.journal.writer_fence + 1,
        "generation acquisition is not rolled back by record conflict"
    );
    let preserved = release_commit::inspect_checkpoint(
        &fixture.initialized.database,
        id,
        Some(&current),
        read(),
    )
    .expect("original immutable row survives conflict");
    assert_eq!(preserved.checkpoint, Some(first.checkpoint));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn missing_wrong_type_or_mismatched_native_evidence_fails_before_generation() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"must remain absent");
    let missing: CommitId = "2222222222222222222222222222222222222222"
        .parse()
        .expect("syntactically valid absent object");
    let tree_object: CommitId = git(
        &fixture.repository,
        &[
            "rev-parse",
            &format!("{}^{{tree}}", fixture.prepared.commit.as_str()),
        ],
    )
    .trim()
    .parse()
    .expect("actual tree object, not a commit");
    let before = fixture.snapshot();
    let database = fs::read(&fixture.initialized.database).expect("initial database bytes");
    for commit in [&missing, &tree_object] {
        let error = release_commit::checkpoint(
            &fixture.journal,
            fixture.initialized.header,
            CheckpointRequest {
                commit,
                ..fixture.request(id)
            },
            options(),
        )
        .expect_err("native object must be an existing exact commit");
        assert!(matches!(
            error,
            CheckpointError::Git {
                writer_acquired: false,
                ..
            }
        ));
        assert_eq!(fixture.header(), fixture.initialized.header);
    }
    let mut wrong_recipe = fixture.metadata.clone();
    wrong_recipe.author.seconds += 1;
    let error = release_commit::checkpoint(
        &fixture.journal,
        fixture.initialized.header,
        CheckpointRequest {
            metadata: &wrong_recipe,
            ..fixture.request(id)
        },
        options(),
    )
    .expect_err("matching OID alone cannot replace independent recipe");
    assert!(matches!(
        error,
        CheckpointError::Git {
            writer_acquired: false,
            ..
        }
    ));
    assert_eq!(fixture.header(), fixture.initialized.header);
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("unmodified journal bytes"),
        database
    );
    assert!(
        release_commit::inspect_checkpoint(
            &fixture.initialized.database,
            id,
            Some(&fixture.initialized.header),
            read()
        )
        .expect("absence after refusal")
        .checkpoint
        .is_none()
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn every_checkpoint_effect_permission_is_independent_and_denied_before_writer_acquisition() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"permission refusal");
    let before = fixture.snapshot();
    let database = fs::read(&fixture.initialized.database).expect("original journal bytes");
    for permission in 0..4 {
        let mut selected = options();
        match permission {
            0 => selected.record.allow_record = false,
            1 => selected.writer.allow_generation_advance = false,
            2 => selected.writer.session.allow_local_reservation = false,
            _ => selected.writer.session.read.allow_coordination_files = false,
        }
        let error = fixture
            .checkpoint(id, fixture.initialized.header, selected)
            .expect_err("each effect needs explicit permission");
        let CheckpointError::Policy(cause) = error else {
            panic!("permission refused before native acquisition")
        };
        assert!(match permission {
            0 => matches!(cause, store::Error::CheckpointPermission),
            1 => matches!(cause, store::Error::WriterPermission),
            2 => matches!(cause, store::Error::ReservationPermission),
            _ => matches!(cause, store::Error::CoordinationPermission),
        });
        assert_eq!(fixture.header(), fixture.initialized.header);
    }
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("no effect on journal"),
        database
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn journal_cannot_overlap_repository_or_declared_source_in_either_direction() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"root separation");
    let before = fixture.snapshot();
    let journal_before = tree(&fixture.journal);
    for root in [&fixture.repository, &fixture.output, &fixture.source] {
        let protected = [fixture.output.clone(), fixture.source.clone()];
        let error = release_commit::checkpoint(
            root,
            fixture.initialized.header,
            CheckpointRequest {
                protected_roots: &protected,
                ..fixture.request(id)
            },
            options(),
        )
        .expect_err("journal cannot be a protected native root");
        assert!(matches!(
            error,
            CheckpointError::Separation {
                writer_acquired: false,
                ..
            }
        ));
    }
    let ancestor = fixture
        .journal
        .parent()
        .expect("shared private fixture ancestor");
    let error = release_commit::checkpoint(
        ancestor,
        fixture.initialized.header,
        fixture.request(id),
        options(),
    )
    .expect_err("journal cannot contain repository");
    assert!(matches!(
        error,
        CheckpointError::Separation {
            writer_acquired: false,
            ..
        }
    ));
    let nested = fixture.repository.join("private-journal");
    private(&nested);
    let repo_with_nested = tree(&fixture.repository);
    let error = release_commit::checkpoint(
        &nested,
        fixture.initialized.header,
        fixture.request(id),
        options(),
    )
    .expect_err("journal cannot be inside repository");
    assert!(matches!(
        error,
        CheckpointError::Separation {
            writer_acquired: false,
            ..
        }
    ));
    assert!(
        fs::read_dir(&nested)
            .expect("empty refused root")
            .next()
            .is_none()
    );
    assert_eq!(tree(&fixture.repository), repo_with_nested);
    assert_eq!(tree(&fixture.output), before[1]);
    assert_eq!(tree(&fixture.source), before[2]);
    assert_eq!(tree(&fixture.journal), journal_before);
    assert_eq!(fixture.header(), fixture.initialized.header);
}

// These two fault seams exercise application effect reporting, not an alternative
// SDK success path: all successful native operations still use actual providers.
mod interrupted_delivery {
    use super::*;
    use gazette_adapter_git::Git;
    use gazette_adapter_sqlite::{Sqlite, Writer};
    use gazette_core::{
        release::CommitError,
        source::{RevisionLimits, TreeInput},
        store::{
            CheckpointInput, CheckpointReceipt, CheckpointWriteError, CheckpointWriteOptions,
            JournalInspection, WriterAcquireError, WriterOptions,
        },
    };
    use gazette_ports::{
        release::ReleaseCommits,
        store::{CheckpointWriter, JournalWriter, WriterConnector},
    };
    use std::{cell::Cell, io};

    struct SecondVerificationFails(Cell<usize>);

    impl ReleaseCommits for SecondVerificationFails {
        fn prepare_commit(
            &self,
            _repository: &Path,
            _input: TreeInput<'_>,
            _metadata: &CommitMetadata,
            _options: CommitOptions,
        ) -> Result<CommitReport, CommitError> {
            panic!("checkpointing must never prepare Git objects")
        }

        fn verify_commit(
            &self,
            repository: &Path,
            commit: &CommitId,
            output: TreeIdentity,
            metadata: &CommitMetadata,
            limits: RevisionLimits,
        ) -> Result<CommitReport, CommitError> {
            self.0.set(self.0.get() + 1);
            if self.0.get() == 2 {
                return Err(CommitError::refused(
                    "fixture second native verification failure",
                ));
            }
            Git.verify_commit(repository, commit, output, metadata, limits)
        }
    }

    struct CloseDeliveryFails;
    struct CloseWriter(Writer);

    impl WriterConnector for CloseDeliveryFails {
        type Writer = CloseWriter;

        fn acquire_writer(
            &self,
            root: &Path,
            expected: JournalHeader,
            options: WriterOptions,
        ) -> Result<Self::Writer, WriterAcquireError> {
            Sqlite
                .acquire_writer(root, expected, options)
                .map(CloseWriter)
        }
    }

    impl JournalWriter for CloseWriter {
        fn inspect(&mut self) -> Result<JournalInspection, store::Error> {
            self.0.inspect()
        }

        fn close(self) -> Result<(), store::Error> {
            self.0.close()?;
            Err(store::Error::Io(io::Error::other(
                "fixture final close delivery failure",
            )))
        }
    }

    impl CheckpointWriter for CloseWriter {
        fn record_checkpoint(
            &mut self,
            input: CheckpointInput<'_>,
            options: CheckpointWriteOptions,
        ) -> Result<CheckpointReceipt, CheckpointWriteError> {
            self.0.record_checkpoint(input, options)
        }
    }

    #[test]
    fn second_verification_failure_preserves_advanced_generation_without_checkpoint() {
        let fixture = Fixture::new();
        let id = Sha256Digest::of(b"interrupted second native verification");
        let before = fixture.snapshot();
        let git = SecondVerificationFails(Cell::new(0));
        let reservation =
            release_commit::journal_output::Reservation::new(&fixture.journal, id, 32 << 10)
                .expect("pre-effect journal response reservation");
        let original = gazette_application::release::checkpoint_commit(
            &Sqlite,
            &gazette_adapter_fs::FileSystem,
            &git,
            &fixture.journal,
            fixture.initialized.header,
            fixture.request(id),
            options(),
        );
        let encoded = reservation
            .checkpoint(&original)
            .expect("original generation boundary is deliverable");
        let delivered: serde_json::Value =
            serde_json::from_slice(&encoded.bytes).expect("native failure JSON");
        assert_eq!(delivered["failure"]["writer_acquired"], true);
        assert!(
            delivered["failure"]["generation"].is_null(),
            "no invented acquired coordinates"
        );
        assert!(delivered["receipt"].is_null());
        let error = original.expect_err("retain native failure");
        assert_eq!(git.0.get(), 2);
        assert!(matches!(
            error,
            CheckpointError::Git {
                writer_acquired: true,
                ..
            }
        ));
        let current = fixture.header();
        assert_eq!(
            current.writer_fence,
            fixture.initialized.header.writer_fence + 1
        );
        assert!(
            release_commit::inspect_checkpoint(
                &fixture.initialized.database,
                id,
                Some(&current),
                read()
            )
            .expect("native fresh absence after interrupted verification")
            .checkpoint
            .is_none()
        );
        assert_eq!(fixture.snapshot(), before);
    }

    #[test]
    fn final_close_failure_preserves_verified_receipt_and_native_record() {
        let fixture = Fixture::new();
        let id = Sha256Digest::of(b"interrupted final receipt delivery");
        let before = fixture.snapshot();
        let reservation =
            release_commit::journal_output::Reservation::new(&fixture.journal, id, 32 << 10)
                .expect("reserve before actual checkpoint append");
        let original = gazette_application::release::checkpoint_commit(
            &CloseDeliveryFails,
            &gazette_adapter_fs::FileSystem,
            &Git,
            &fixture.journal,
            fixture.initialized.header,
            fixture.request(id),
            options(),
        );
        let encoded = reservation
            .checkpoint(&original)
            .expect("verified receipt survives failed native close");
        assert!(encoded.is_error);
        let delivered: serde_json::Value =
            serde_json::from_slice(&encoded.bytes).expect("original failure and receipt");
        assert_eq!(delivered["failure"]["writer_acquired"], true);
        assert_eq!(delivered["receipt"]["checkpoint"]["id"], id.to_string());
        let error = original.expect_err("original close failure retained");
        let CheckpointError::Close { receipt, cause } = error else {
            panic!("completed record evidence must survive close failure")
        };
        assert!(matches!(cause, store::Error::Io(_)));
        assert_eq!(receipt.checkpoint.commit, fixture.prepared);
        assert_eq!(
            receipt.journal.writer_fence,
            fixture.initialized.header.writer_fence + 1
        );
        assert_eq!(fixture.header(), receipt.journal);
        let stored = release_commit::inspect_checkpoint(
            &fixture.initialized.database,
            id,
            Some(&receipt.journal),
            read(),
        )
        .expect("fresh native record after lost delivery");
        assert_eq!(stored.checkpoint.as_ref(), Some(&receipt.checkpoint));
        assert_eq!(fixture.snapshot(), before);
    }
}
