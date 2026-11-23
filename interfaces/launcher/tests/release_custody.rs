//! Native local Git custody through SDK and independent CLI processes, not release approval.

use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use gazette_sdk::{
    release_commit::{self, CommitMetadata, CommitSignature, CustodyOptions},
    source::{self, CommitId, Sha256Digest, TreeIdentity},
};
use git2::Repository;
use serde_json::Value;
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    repository: PathBuf,
    output: PathBuf,
    source: PathBuf,
    expected: TreeIdentity,
    metadata: CommitMetadata,
    prepared: release_commit::CommitReport,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("disposable custody fixture");
        let repository = root.path().join("deployment.git");
        let output = root.path().join("reviewed");
        let source = root.path().join("authored");
        fs::create_dir(&output).expect("retained output directory");
        fs::create_dir(&source).expect("protected source directory");
        fs::write(
            output.join("index.html"),
            b"<h1>Retained publication</h1>\n",
        )
        .expect("exact output bytes");
        fs::write(source.join("article.md"), b"PRIVATE authored source\n")
            .expect("protected authored input");
        let repo =
            Repository::init_bare(&repository).expect("explicit independent bare repository");
        repo.config()
            .expect("native config")
            .set_str("fixture.preserved", "keep exact value")
            .expect("fixture config sentinel");
        let expected = tree(&output);
        let signature = CommitSignature {
            name: "PRIVATE fixture author".into(),
            email: "fixture@example.invalid".into(),
            seconds: 100,
            offset_minutes: 330,
        };
        let metadata = CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "PRIVATE exact recipe\nSecond line\n".into(),
        };
        let prepared = prepare(&repository, &output, &source, expected, &metadata);
        repo.reference(
            "refs/heads/fixture-sentinel",
            git2::Oid::from_str(prepared.commit.as_str()).expect("prepared commit OID"),
            false,
            "fixture only",
        )
        .expect("unrelated reference sentinel");
        Self {
            root,
            repository,
            output,
            source,
            expected,
            metadata,
            prepared,
        }
    }

    fn command(&self, retain: bool, id: Sha256Digest, commit: &CommitId) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command
            .arg(if retain {
                "retain-release-commit"
            } else {
                "inspect-release-custody"
            })
            .arg(&self.repository)
            .arg(commit.as_str())
            .arg("--custody-id")
            .arg(id.to_string())
            .arg("--expected-tree")
            .arg(serde_json::to_string(&self.expected).expect("exact retained identity JSON"));
        if retain {
            command
                .arg("--protected-root")
                .arg(&self.output)
                .arg("--protected-root")
                .arg(&self.source);
        }
        command
    }

    fn input(&self) -> Vec<u8> {
        serde_json::to_vec(&self.metadata).expect("complete native metadata object")
    }

    fn retain(&self, id: Sha256Digest) -> Output {
        let mut command = self.command(true, id, &self.prepared.commit);
        command.arg("--allow-retain");
        run(command, &self.input())
    }

    fn inspect(&self, id: Sha256Digest) -> Output {
        run(
            self.command(false, id, &self.prepared.commit),
            &self.input(),
        )
    }

    fn protected(&self) -> [TreeIdentity; 3] {
        [
            tree(&self.repository.join("objects")),
            tree(&self.output),
            tree(&self.source),
        ]
    }

    fn refs(&self) -> BTreeMap<String, String> {
        let repo = Repository::open_bare(&self.repository).expect("read native refs");
        repo.references()
            .expect("native reference enumeration")
            .map(|reference| {
                let reference = reference.expect("native reference");
                let name = reference.name().expect("fixture ref name").to_owned();
                let target = reference
                    .target()
                    .expect("fixture uses only direct refs")
                    .to_string();
                (name, target)
            })
            .collect()
    }
}

fn tree(path: &Path) -> TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("complete native inventory")
        .identity()
}

fn prepare(
    repository: &Path,
    output: &Path,
    source: &Path,
    expected: TreeIdentity,
    metadata: &CommitMetadata,
) -> release_commit::CommitReport {
    release_commit::prepare(
        repository,
        output,
        expected,
        metadata,
        &[source.to_path_buf()],
        release_commit::CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("real SDK detached object preparation")
}

fn run(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Gazette command");
    if let Err(error) = child.stdin.take().expect("metadata pipe").write_all(input) {
        assert_eq!(
            error.kind(),
            ErrorKind::BrokenPipe,
            "early command refusal only"
        );
    }
    child.wait_with_output().expect("complete Gazette command")
}

fn success(output: Output) -> Value {
    assert!(output.status.success(), "Gazette failed: {output:?}");
    assert!(
        output.stderr.is_empty(),
        "unexpected diagnostic: {output:?}"
    );
    serde_json::from_slice(&output.stdout).expect("complete custody report")
}

fn refused(output: Output, expected: &str) {
    assert!(!output.status.success(), "unexpected success: {output:?}");
    if !output.stdout.is_empty() {
        let failure: Value =
            serde_json::from_slice(&output.stdout).expect("complete native failure projection");
        assert_eq!(failure["code"], "release_custody");
        assert!(
            failure.get("effect").is_some() && failure.get("commit").is_none(),
            "failure is not successful custody"
        );
    }
    let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(diagnostic.contains(expected), "{diagnostic}");
    assert!(!diagnostic.contains("PRIVATE"), "private metadata leaked");
}

#[test]
fn sdk_retention_and_new_cli_inspection_share_exact_custody_without_other_git_changes() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"independently selected custody coordinate");
    let protected = fixture.protected();
    let refs = fixture.refs();
    let config = fs::read(fixture.repository.join("config")).expect("config before retention");
    let head = fs::read(fixture.repository.join("HEAD")).expect("HEAD before retention");
    let report = release_commit::retain(
        &fixture.repository,
        id,
        &fixture.prepared.commit,
        fixture.expected,
        &fixture.metadata,
        &[fixture.output.clone(), fixture.source.clone()],
        CustodyOptions {
            allow_retain: true,
            ..Default::default()
        },
    )
    .expect("real SDK native custody acquisition");
    assert_eq!(report.id, id);
    assert_eq!(report.commit, fixture.prepared);
    let expected = serde_json::to_value(&report).expect("native custody projection");
    let retained_tree = tree(&fixture.repository);
    assert_eq!(
        success(fixture.inspect(id)),
        expected,
        "new process performs read-only native verification"
    );
    assert_eq!(
        tree(&fixture.repository),
        retained_tree,
        "inspection creates no files or refs"
    );
    assert_eq!(
        success(fixture.retain(id)),
        expected,
        "exact CLI replay retains existing ref"
    );
    assert_eq!(
        tree(&fixture.repository),
        retained_tree,
        "exact replay changes no stored bytes"
    );
    let mut current_refs = fixture.refs();
    assert_eq!(
        current_refs.remove(&release_commit::custody_reference(id)),
        Some(fixture.prepared.commit.to_string())
    );
    assert_eq!(current_refs, refs, "no branch, tag or other ref changed");
    assert_eq!(
        fixture.protected(),
        protected,
        "existing objects, output and source stay byte-identical"
    );
    assert_eq!(
        fs::read(fixture.repository.join("config")).expect("config after retention"),
        config
    );
    assert_eq!(
        fs::read(fixture.repository.join("HEAD")).expect("HEAD after retention"),
        head
    );
}

#[test]
fn a_different_valid_commit_cannot_replace_an_existing_custody_coordinate() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"immutable custody conflict");
    let first = success(fixture.retain(id));
    let mut metadata = fixture.metadata.clone();
    metadata
        .message
        .push_str("PRIVATE independently valid alternate recipe\n");
    let other = prepare(
        &fixture.repository,
        &fixture.output,
        &fixture.source,
        fixture.expected,
        &metadata,
    );
    assert_ne!(other.commit, fixture.prepared.commit);
    let baseline = tree(&fixture.repository);
    let mut command = fixture.command(true, id, &other.commit);
    command.arg("--allow-retain");
    refused(
        run(
            command,
            &serde_json::to_vec(&metadata).expect("different exact metadata"),
        ),
        "custody",
    );
    assert_eq!(
        tree(&fixture.repository),
        baseline,
        "conflict must preserve the original ref and objects"
    );
    assert_eq!(success(fixture.inspect(id)), first);
    let native = release_commit::inspect_custody(
        &fixture.repository,
        id,
        &fixture.prepared.commit,
        fixture.expected,
        &fixture.metadata,
        Default::default(),
    )
    .expect("original custody still verifies natively");
    assert_eq!(native.commit, fixture.prepared);
}

#[test]
fn missing_permission_and_malformed_or_mismatched_metadata_leave_no_custody_ref() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"refused custody");
    let baseline = tree(&fixture.repository);
    refused(
        run(
            fixture.command(true, id, &fixture.prepared.commit),
            &fixture.input(),
        ),
        "allow-retain",
    );
    assert_eq!(tree(&fixture.repository), baseline);
    let mut missing_parent = serde_json::to_value(&fixture.metadata).expect("native metadata");
    missing_parent
        .as_object_mut()
        .expect("metadata object")
        .remove("parent");
    for input in [
        b"{\"PRIVATE\": incomplete".to_vec(),
        serde_json::to_vec(&missing_parent).expect("missing explicit parent"),
        b"{\"parent\":null,\"par\\u0065nt\":null}".to_vec(),
    ] {
        let mut command = fixture.command(true, id, &fixture.prepared.commit);
        command.arg("--allow-retain");
        refused(run(command, &input), "metadata");
        assert_eq!(
            tree(&fixture.repository),
            baseline,
            "transport failure precedes native custody"
        );
    }
    let mut mismatch = fixture.metadata.clone();
    mismatch.message.push_str("PRIVATE wrong recipe\n");
    let mut command = fixture.command(true, id, &fixture.prepared.commit);
    command.arg("--allow-retain");
    refused(
        run(
            command,
            &serde_json::to_vec(&mismatch).expect("well-shaped mismatched metadata"),
        ),
        "custody",
    );
    assert_eq!(tree(&fixture.repository), baseline);
    assert!(
        !fixture
            .refs()
            .contains_key(&release_commit::custody_reference(id))
    );
}

#[test]
fn retention_refuses_repository_overlap_with_explicit_source_roots() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"custody source separation");
    let child = fixture.repository.join("protected-source");
    fs::create_dir(&child).expect("nested configured source fixture");
    fs::write(child.join("article.md"), b"PRIVATE nested source").expect("nested source");
    let baseline = tree(&fixture.repository);
    let protected = fixture.protected();
    for root in [
        fixture.repository.as_path(),
        child.as_path(),
        fixture.root.path(),
    ] {
        let mut command = fixture.command(true, id, &fixture.prepared.commit);
        command
            .arg("--allow-retain")
            .arg("--protected-root")
            .arg(root);
        refused(run(command, &fixture.input()), "custody");
        assert_eq!(
            tree(&fixture.repository),
            baseline,
            "same/ancestor/descendant overlap fails before ref writes"
        );
        assert_eq!(fixture.protected(), protected);
    }
    assert!(
        !fixture
            .refs()
            .contains_key(&release_commit::custody_reference(id))
    );
}

#[test]
fn receipt_delivery_failure_retains_custody_for_read_only_recovery_without_retry() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"lost custody receipt");
    let protected = fixture.protected();
    let mut command = fixture.command(true, id, &fixture.prepared.commit);
    command.arg("--allow-retain");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::from(
            fs::File::open("/dev/null").expect("read-only descriptor forces real stdout failure"),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("retention with failed receipt delivery");
    child
        .stdin
        .take()
        .expect("metadata pipe")
        .write_all(&fixture.input())
        .expect("send exact metadata");
    let result = child.wait_with_output().expect("delivery result");
    assert!(!result.status.success());
    let diagnostic = String::from_utf8(result.stderr).expect("delivery diagnostic");
    assert!(diagnostic.contains("delivery failed"), "{diagnostic}");
    assert!(!diagnostic.contains("PRIVATE"));
    let after_effect = tree(&fixture.repository);
    let observed = success(fixture.inspect(id));
    let retained: Value =
        serde_json::from_str(diagnostic.lines().next().expect("retained original report"))
            .expect("bounded JSON on independent carrier");
    assert_eq!(
        retained, observed,
        "independent read agrees with original lost-carrier custody report"
    );
    assert_eq!(
        observed["id"],
        serde_json::to_value(id).expect("native coordinate")
    );
    assert_eq!(
        observed["commit"],
        serde_json::to_value(&fixture.prepared).expect("exact verified commit")
    );
    assert_eq!(
        tree(&fixture.repository),
        after_effect,
        "recovery inspection does not repeat retention"
    );
    assert_eq!(fixture.protected(), protected);
    assert_eq!(
        fixture.refs().get(&release_commit::custody_reference(id)),
        Some(&fixture.prepared.commit.to_string())
    );
}

#[test]
fn inspection_requires_existing_exact_custody_and_cannot_act_as_a_write_command() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"missing custody coordinate");
    let baseline = tree(&fixture.repository);
    refused(fixture.inspect(id), "custody");
    assert_eq!(
        tree(&fixture.repository),
        baseline,
        "absent custody is not created by inspection"
    );
    let retained = success(fixture.retain(id));
    let after = tree(&fixture.repository);
    let other_id = Sha256Digest::of(b"different absent coordinate");
    refused(fixture.inspect(other_id), "custody");
    assert_eq!(tree(&fixture.repository), after);
    let mut command = fixture.command(false, id, &fixture.prepared.commit);
    command.arg("--allow-retain");
    refused(run(command, &fixture.input()), "unexpected argument");
    assert_eq!(tree(&fixture.repository), after);
    assert_eq!(success(fixture.inspect(id)), retained);
}

#[test]
fn response_reservation_precedes_retention_and_late_sync_failure_keeps_exact_ref() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"bounded custody response");
    let before = tree(&fixture.repository);
    let mut command = fixture.command(true, id, &fixture.prepared.commit);
    command.args(["--allow-retain", "--max-output-bytes", "1"]);
    refused(run(command, &fixture.input()), "response_limit");
    assert_eq!(
        tree(&fixture.repository),
        before,
        "no ref before response capacity"
    );
    let storage = source::inspect(&fixture.repository, None, Default::default())
        .expect("native repository storage")
        .total_bytes();
    let mut command = fixture.command(true, id, &fixture.prepared.commit);
    command.args([
        "--allow-retain",
        "--max-storage-bytes",
        &storage.to_string(),
    ]);
    let result = run(command, &fixture.input());
    assert_eq!(result.status.code(), Some(1));
    let projected: Value =
        serde_json::from_slice(&result.stdout).expect("complete failed retention evidence");
    assert_eq!(projected["code"], "release_custody");
    assert_eq!(projected["effect"], "reference_may_remain");
    assert!(
        projected.get("commit").is_none(),
        "failed synchronization is not custody success"
    );
    let observed = success(fixture.inspect(id));
    assert_eq!(
        observed["commit"],
        serde_json::to_value(&fixture.prepared).expect("original verified commit")
    );
    assert_eq!(
        fixture.refs().get(&release_commit::custody_reference(id)),
        Some(&fixture.prepared.commit.to_string())
    );
}
