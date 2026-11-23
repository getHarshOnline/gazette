//! Real command-level local checkpoints and recovery, without release authority.

#[path = "release_checkpoint/mcp.rs"]
mod mcp;

use std::{
    fs,
    io::{ErrorKind, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use gazette_sdk::{
    release_commit::{self, CheckpointOptions, CheckpointRequest, CommitMetadata, CommitSignature},
    source::{self, CommitId, Sha256Digest, TreeIdentity},
    store::{self, JournalHeader, ReadOptions},
};
use git2::Repository;
use serde_json::Value;
use tempfile::TempDir;

const PERMISSIONS: [&str; 4] = [
    "--record-checkpoint",
    "--advance-writer-generation",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];

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

fn tree(path: &Path) -> TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("complete fixture tree observation")
        .identity()
}

fn private(path: &Path) {
    fs::create_dir(path).expect("create disposable directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("private fixture directory");
}

struct Fixture {
    root: TempDir,
    repository: PathBuf,
    output: PathBuf,
    source: PathBuf,
    journal: PathBuf,
    initialized: store::InitializationReceipt,
    expected: TreeIdentity,
    metadata: CommitMetadata,
    prepared: release_commit::CommitReport,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private disposable fixture");
        let repository = root.path().join("deployment.git");
        let output = root.path().join("reviewed");
        let source = root.path().join("authored");
        let journal = root.path().join("journal");
        private(&output);
        private(&source);
        private(&journal);
        fs::write(output.join("index.html"), b"<h1>Exact local output</h1>\n")
            .expect("retained page");
        fs::write(
            source.join("article.md"),
            b"PRIVATE source must remain unchanged\n",
        )
        .expect("protected source");
        let repo = Repository::init_bare(&repository).expect("explicit disposable bare Git");
        repo.config()
            .expect("fixture Git config")
            .set_str("fixture.preserved", "keep exact configured value")
            .expect("fixture sentinel config");
        let expected = tree(&output);
        let metadata = CommitMetadata {
            parent: None,
            author: CommitSignature {
                name: "PRIVATE fixture author".into(),
                email: "author@example.invalid".into(),
                seconds: 100,
                offset_minutes: 330,
            },
            committer: CommitSignature {
                name: "PRIVATE fixture committer".into(),
                email: "committer@example.invalid".into(),
                seconds: 101,
                offset_minutes: -60,
            },
            message: "PRIVATE exact checkpoint recipe\nSecond line\n".into(),
        };
        let prepared = release_commit::prepare(
            &repository,
            &output,
            expected,
            &metadata,
            std::slice::from_ref(&source),
            release_commit::CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("real SDK local Git object preparation");
        repo.reference(
            "refs/heads/fixture-sentinel",
            git2::Oid::from_str(prepared.commit.as_str()).expect("exact prepared OID"),
            false,
            "fixture only",
        )
        .expect("fixture sentinel ref");
        drop(repo);
        let initialized = store::initialize(
            &journal,
            store::InitializeOptions {
                allow_create: true,
                session: options().writer.session,
                ..Default::default()
            },
        )
        .expect("real SDK SQLite initialization");
        Self {
            root,
            repository,
            output,
            source,
            journal,
            initialized,
            expected,
            metadata,
            prepared,
        }
    }

    fn header(&self) -> JournalHeader {
        store::inspect_journal(&self.initialized.database, None, read())
            .expect("fresh native journal header")
            .header
    }

    fn command(
        &self,
        journal: &Path,
        commit: &CommitId,
        id: Sha256Digest,
        header: JournalHeader,
        permissions: &[&str],
    ) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command
            .arg("checkpoint-release-commit")
            .arg(&self.repository)
            .arg(journal)
            .arg(commit.as_str())
            .arg("--checkpoint-id")
            .arg(id.to_string())
            .arg("--expected-tree")
            .arg(serde_json::to_string(&self.expected).expect("exact tree JSON"))
            .arg("--expected-header")
            .arg(serde_json::to_string(&header).expect("exact header JSON"))
            .arg("--protected-root")
            .arg(&self.output)
            .arg("--protected-root")
            .arg(&self.source)
            .args(permissions);
        command
    }

    fn create(&self, id: Sha256Digest, header: JournalHeader) -> Output {
        run(
            self.command(
                &self.journal,
                &self.prepared.commit,
                id,
                header,
                &PERMISSIONS,
            ),
            &serde_json::to_vec(&self.metadata).expect("complete metadata JSON"),
        )
    }

    fn inspect(&self, id: Sha256Digest) -> Value {
        success(inspect(&self.initialized.database, id, Some(self.header())))
    }

    fn snapshot(&self) -> [TreeIdentity; 3] {
        [
            tree(&self.repository),
            tree(&self.output),
            tree(&self.source),
        ]
    }
}

fn run(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Gazette fixture operation");
    if let Err(error) = child.stdin.take().expect("metadata stdin").write_all(input) {
        assert_eq!(error.kind(), ErrorKind::BrokenPipe, "early refusal only");
    }
    child
        .wait_with_output()
        .expect("complete Gazette operation")
}

fn inspect(database: &Path, id: Sha256Digest, expected: Option<JournalHeader>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-release-checkpoint")
        .arg(database)
        .arg(id.to_string())
        .arg("--allow-coordination-files");
    if let Some(header) = expected {
        command
            .arg("--expected-header")
            .arg(serde_json::to_string(&header).expect("inspection binding JSON"));
    }
    run(command, b"")
}

fn success(output: Output) -> Value {
    assert!(output.status.success(), "Gazette failed: {output:?}");
    assert!(
        output.stderr.is_empty(),
        "unexpected diagnostic: {output:?}"
    );
    serde_json::from_slice(&output.stdout).expect("complete private receipt JSON")
}

fn refused(output: Output, code: &str) {
    assert!(!output.status.success(), "unexpected success: {output:?}");
    if !output.stdout.is_empty() {
        let report: Value =
            serde_json::from_slice(&output.stdout).expect("exact failed native outcome");
        assert!(report.get("failure").is_some());
        assert!(report.get("checkpoint").is_none());
    }
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 refusal");
    assert!(diagnostic.contains(code), "{diagnostic}");
    assert!(!diagnostic.contains("PRIVATE"), "private recipe echoed");
}

#[test]
fn cli_sdk_and_native_inspection_share_creation_while_replay_advances_current_generation() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"CLI and SDK exact checkpoint identity");
    let snapshot = fixture.snapshot();
    let first = success(fixture.create(id, fixture.initialized.header));
    let created = fixture.header();
    assert_eq!(
        created.writer_fence,
        fixture.initialized.header.writer_fence + 1
    );
    let native = release_commit::inspect_checkpoint(
        &fixture.initialized.database,
        id,
        Some(&created),
        read(),
    )
    .expect("native read-back");
    let retained = native.checkpoint.expect("exact checkpoint exists");
    assert_eq!(
        first["checkpoint"],
        serde_json::to_value(&retained).expect("native projection")
    );
    assert_eq!(retained.created, created);
    assert_eq!(retained.metadata, fixture.metadata);
    assert_eq!(retained.commit, fixture.prepared);
    assert_eq!(
        retained.repository,
        fixture.repository.to_str().expect("repository spelling")
    );
    let protected = [fixture.output.clone(), fixture.source.clone()];
    let replay = release_commit::checkpoint(
        &fixture.journal,
        created,
        CheckpointRequest {
            id,
            repository: &fixture.repository,
            commit: &fixture.prepared.commit,
            output: fixture.expected,
            metadata: &fixture.metadata,
            protected_roots: &protected,
        },
        options(),
    )
    .expect("exact SDK replay of CLI checkpoint");
    assert_eq!(replay.checkpoint, retained);
    assert_eq!(replay.journal.writer_fence, created.writer_fence + 1);
    let cli_replay = success(fixture.create(id, replay.journal));
    assert_eq!(cli_replay["checkpoint"], first["checkpoint"]);
    assert_eq!(
        fixture.header().writer_fence,
        replay.journal.writer_fence + 1
    );
    assert_eq!(fixture.inspect(id)["checkpoint"], first["checkpoint"]);
    assert_eq!(
        fixture.snapshot(),
        snapshot,
        "checkpoint never rewrites Git or source/output"
    );
}

#[test]
fn changed_valid_commit_under_same_id_refuses_and_preserves_the_original_record() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"conflicting checkpoint");
    let first = success(fixture.create(id, fixture.initialized.header));
    let mut changed = fixture.metadata.clone();
    changed
        .message
        .push_str("PRIVATE independently valid different recipe\n");
    let other = release_commit::prepare(
        &fixture.repository,
        &fixture.output,
        fixture.expected,
        &changed,
        std::slice::from_ref(&fixture.source),
        release_commit::CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("independently verified distinct commit");
    assert_ne!(other.commit, fixture.prepared.commit);
    let before = fixture.header();
    let snapshot = fixture.snapshot();
    refused(
        run(
            fixture.command(&fixture.journal, &other.commit, id, before, &PERMISSIONS),
            &serde_json::to_vec(&changed).expect("different complete metadata"),
        ),
        "store_checkpoint_conflict",
    );
    assert_eq!(
        fixture.header().writer_fence,
        before.writer_fence + 1,
        "writer acquisition is a real effect even when record insertion conflicts"
    );
    assert_eq!(fixture.inspect(id)["checkpoint"], first["checkpoint"]);
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn permissions_metadata_and_stale_header_refuse_without_mistaken_checkpoint_writes() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"pre-effect refusal checkpoint");
    let header = fixture.header();
    let original = fs::read(&fixture.initialized.database).expect("initial database bytes");
    let snapshot = fixture.snapshot();
    let input = serde_json::to_vec(&fixture.metadata).expect("complete metadata");
    for (flag, code) in PERMISSIONS.iter().zip([
        "store_checkpoint_permission",
        "store_writer_permission",
        "store_reservation_permission",
        "store_coordination_permission",
    ]) {
        let permissions: Vec<_> = PERMISSIONS
            .iter()
            .copied()
            .filter(|value| value != flag)
            .collect();
        refused(
            run(
                fixture.command(
                    &fixture.journal,
                    &fixture.prepared.commit,
                    id,
                    header,
                    &permissions,
                ),
                &input,
            ),
            code,
        );
        assert_eq!(fixture.header(), header);
        assert_eq!(
            fs::read(&fixture.initialized.database).expect("unchanged database"),
            original
        );
    }
    let mut omitted = serde_json::to_value(&fixture.metadata).expect("metadata object");
    omitted
        .as_object_mut()
        .expect("native metadata object")
        .remove("parent");
    for malformed in [
        b"{\"PRIVATE\": incomplete".to_vec(),
        serde_json::to_vec(&omitted).expect("missing parent fixture"),
        b"{\"parent\":null,\"par\\u0065nt\":null}".to_vec(),
    ] {
        refused(
            run(
                fixture.command(
                    &fixture.journal,
                    &fixture.prepared.commit,
                    id,
                    header,
                    &PERMISSIONS,
                ),
                &malformed,
            ),
            "commit_metadata_input",
        );
        assert_eq!(fixture.header(), header);
        assert_eq!(
            fs::read(&fixture.initialized.database).expect("unmodified database"),
            original
        );
    }
    let mut wrong = fixture.metadata.clone();
    wrong.message.push_str("PRIVATE mismatched metadata\n");
    refused(
        run(
            fixture.command(
                &fixture.journal,
                &fixture.prepared.commit,
                id,
                header,
                &PERMISSIONS,
            ),
            &serde_json::to_vec(&wrong).expect("valid but mismatched recipe"),
        ),
        "checkpoint_commit_verification",
    );
    assert_eq!(fixture.header(), header);
    let accepted = success(fixture.create(id, header));
    let current = fixture.header();
    let persisted = fs::read(&fixture.initialized.database).expect("accepted database bytes");
    refused(fixture.create(id, header), "store_journal_mismatch");
    assert_eq!(fixture.header(), current);
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("preserved accepted database"),
        persisted
    );
    assert_eq!(fixture.inspect(id)["checkpoint"], accepted["checkpoint"]);
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn checkpoint_store_must_remain_separate_from_repository_and_protected_sources() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"separation checkpoint");
    let nested = fixture.repository.join("private-journal");
    private(&nested);
    let snapshot = fixture.snapshot();
    let header = fixture.header();
    for root in [
        &fixture.repository,
        &fixture.output,
        &fixture.source,
        &nested,
        &fixture.root.path().to_path_buf(),
    ] {
        refused(
            run(
                fixture.command(root, &fixture.prepared.commit, id, header, &PERMISSIONS),
                &serde_json::to_vec(&fixture.metadata).expect("complete metadata"),
            ),
            "checkpoint_source_separation",
        );
        assert_eq!(fixture.header(), header);
        assert_eq!(fixture.snapshot(), snapshot);
    }
    assert_eq!(
        fs::read_dir(nested)
            .expect("refused nested journal")
            .count(),
        0
    );
    assert!(fixture.inspect(id)["checkpoint"].is_null());
}

#[test]
fn lost_stdout_is_recovered_by_read_only_inspection_then_current_git_verification() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"known identity survives receipt delivery failure");
    let snapshot = fixture.snapshot();
    let mut command = fixture.command(
        &fixture.journal,
        &fixture.prepared.commit,
        id,
        fixture.header(),
        &PERMISSIONS,
    );
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::from(
            fs::File::open("/dev/null").expect("read-only descriptor forces actual stdout failure"),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("checkpoint with unavailable report delivery");
    child
        .stdin
        .take()
        .expect("metadata input")
        .write_all(&serde_json::to_vec(&fixture.metadata).expect("complete metadata"))
        .expect("send recipe");
    let result = child.wait_with_output().expect("delivery failure result");
    assert!(!result.status.success());
    let diagnostic = String::from_utf8(result.stderr).expect("delivery diagnostic");
    assert!(
        diagnostic.contains("receipt delivery failed"),
        "{diagnostic}"
    );
    let delivered: Value = serde_json::from_str(
        diagnostic
            .lines()
            .next()
            .expect("exact original receipt survives on secondary carrier"),
    )
    .expect("complete original receipt JSON");
    let current = fixture.header();
    assert_eq!(
        current.writer_fence,
        fixture.initialized.header.writer_fence + 1
    );
    let persisted = fs::read(&fixture.initialized.database).expect("retained journal");
    let observed = fixture.inspect(id);
    assert_eq!(delivered["checkpoint"], observed["checkpoint"]);
    assert_eq!(
        delivered["journal"],
        serde_json::to_value(current).expect("actual observed header")
    );
    assert_eq!(
        observed["checkpoint"]["commit"],
        serde_json::to_value(&fixture.prepared).expect("exact native report")
    );
    let mut verify = Command::new(env!("CARGO_BIN_EXE_gazette"));
    verify
        .arg("verify-release-commit")
        .arg(&fixture.repository)
        .arg(fixture.prepared.commit.as_str())
        .arg("--expected-tree")
        .arg(serde_json::to_string(&fixture.expected).expect("retained expected output"));
    let verified = success(run(
        verify,
        &serde_json::to_vec(&fixture.metadata).expect("exact recipe"),
    ));
    assert_eq!(verified, observed["checkpoint"]["commit"]);
    assert_eq!(
        fixture.header(),
        current,
        "inspection/verification do not retry acquisition"
    );
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("read-only journal reuse"),
        persisted
    );
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn absent_checkpoint_and_unsupported_schema_inspection_never_initialize_or_upgrade() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"absent checkpoint");
    let header = fixture.header();
    let original = fs::read(&fixture.initialized.database).expect("schema baseline");
    let observed = fixture.inspect(id);
    assert!(observed["checkpoint"].is_null());
    assert_eq!(fixture.header(), header);
    assert_eq!(
        fs::read(&fixture.initialized.database).expect("unmodified schema"),
        original
    );
    let missing = fixture.root.path().join("missing.sqlite");
    refused(inspect(&missing, id, None), "store_io");
    assert!(
        !missing.exists(),
        "read-only inspection must not initialize"
    );
    let unsupported = fixture.root.path().join("unsupported.sqlite");
    fs::copy(&fixture.initialized.database, &unsupported).expect("disposable schema fixture copy");
    let connection =
        rusqlite::Connection::open(&unsupported).expect("fixture-only schema marker mutation");
    connection
        .pragma_update(None, "user_version", 999_999)
        .expect("future unsupported version fixture");
    connection.close().expect("close fixture before inspection");
    let unsupported_bytes = fs::read(&unsupported).expect("exact unsupported schema bytes");
    refused(inspect(&unsupported, id, None), "store_journal_schema");
    assert_eq!(
        fs::read(&unsupported).expect("no implicit schema upgrade"),
        unsupported_bytes
    );
    assert_eq!(fixture.header(), header);
}

#[test]
fn response_reservation_precedes_generation_and_large_native_receipt_keeps_exact_lookup() {
    let mut fixture = Fixture::new();
    let id = Sha256Digest::of(b"bounded checkpoint delivery");
    let before = fixture.header();
    let snapshot = fixture.snapshot();
    let mut denied = fixture.command(
        &fixture.journal,
        &fixture.prepared.commit,
        id,
        before,
        &PERMISSIONS,
    );
    denied.args(["--max-output-bytes", "1"]);
    refused(
        run(
            denied,
            &serde_json::to_vec(&fixture.metadata).expect("recipe"),
        ),
        "journal_response_limit",
    );
    assert_eq!(fixture.header(), before);
    assert!(fixture.inspect(id)["checkpoint"].is_null());
    assert_eq!(fixture.snapshot(), snapshot);

    fixture.metadata.message = "large exact private metadata\n".repeat(2048);
    fixture.prepared = release_commit::prepare(
        &fixture.repository,
        &fixture.output,
        fixture.expected,
        &fixture.metadata,
        &[],
        release_commit::CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("real large-metadata commit");
    let mut command = fixture.command(
        &fixture.journal,
        &fixture.prepared.commit,
        id,
        before,
        &PERMISSIONS,
    );
    command.args(["--max-input-bytes", "131072", "--max-output-bytes", "16384"]);
    let result = run(
        command,
        &serde_json::to_vec(&fixture.metadata).expect("complete large recipe"),
    );
    assert!(
        !result.status.success(),
        "limited delivery must remain explicit"
    );
    assert!(result.stdout.len() <= 16384);
    let delivered: Value =
        serde_json::from_slice(&result.stdout).expect("pre-reserved original receipt identity");
    assert_eq!(delivered["code"], "checkpoint_report_limit");
    assert!(delivered["failure"].is_null());
    assert_eq!(
        delivered["receipt"]["commit"],
        serde_json::to_value(&fixture.prepared).expect("native commit evidence")
    );
    let observed = fixture.inspect(id);
    assert_eq!(
        delivered["receipt"]["created"],
        observed["checkpoint"]["created"]
    );
    assert_eq!(
        delivered["receipt"]["journal"],
        observed["journal"]["header"]
    );
    assert_eq!(
        fixture.header().writer_fence,
        before.writer_fence + 1,
        "no hidden retry on oversized result"
    );
}
