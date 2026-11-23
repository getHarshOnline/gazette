//! Actual local object preparation/read-back; no publisher or durable release authority.

use std::{
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{release_commit, source};
use git2::Repository;
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

struct Fixture {
    _root: TempDir,
    repository: PathBuf,
    output: PathBuf,
    expected: source::TreeIdentity,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("fixture root");
        let repository = root.path().join("deployment.git");
        Repository::init_bare(&repository).expect("explicit existing bare repository");
        let output = root.path().join("reviewed");
        fs::create_dir_all(output.join("nested")).expect("reviewed directory");
        fs::write(output.join("index.html"), b"<h1>Local review</h1>").expect("page");
        fs::write(output.join("nested/data.txt"), b"exact\r\nbytes\n").expect("nested bytes");
        let expected = source::inspect(&output, None, Default::default())
            .expect("independently retained output")
            .identity();
        Self {
            _root: root,
            repository,
            output,
            expected,
        }
    }

    fn command(&self, prepare: bool, commit: Option<&str>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command.arg(if prepare {
            "prepare-release-commit"
        } else {
            "verify-release-commit"
        });
        command.arg(&self.repository);
        if prepare {
            command.arg(&self.output);
        } else {
            command.arg(commit.expect("explicit verify commit"));
        }
        command.args([
            "--expected-tree",
            &serde_json::to_string(&self.expected).expect("expected identity JSON"),
        ]);
        command
    }

    fn prepare(&self, input: &[u8], extra: &[&str]) -> Output {
        let mut command = self.command(true, None);
        command.arg("--allow-write-objects").args(extra);
        run(command, input)
    }

    fn repository_inventory(&self) -> source::Inventory {
        source::inspect(&self.repository, None, Default::default()).expect("repository observation")
    }

    fn unchanged_output(&self) {
        assert_eq!(
            source::inspect(&self.output, None, Default::default())
                .expect("output re-read")
                .identity(),
            self.expected
        );
    }
}

fn metadata() -> Value {
    json!({
        "parent": null,
        "author": {"name":"PRIVATE_AUTHOR", "email":"private@example.invalid", "seconds":100, "offset_minutes":330},
        "committer": {"name":"PRIVATE_COMMITTER", "email":"committer@example.invalid", "seconds":101, "offset_minutes":-60},
        "message":"PRIVATE_MESSAGE\nexact second line\n"
    })
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("private fixture JSON")
}

fn run(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette CLI");
    if let Err(error) = child.stdin.take().expect("stdin").write_all(input) {
        assert_eq!(
            error.kind(),
            ErrorKind::BrokenPipe,
            "early refusal may close stdin"
        );
    }
    child.wait_with_output().expect("Gazette exit")
}

fn report(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8(output.stdout).expect("report UTF-8");
    for private in [
        "PRIVATE",
        "private@example.invalid",
        "committer@example.invalid",
    ] {
        assert!(!text.contains(private), "metadata leaked: {text}");
    }
    serde_json::from_str(&text).expect("complete report")
}

fn refused(output: Output) -> String {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    if !output.stdout.is_empty() {
        let failure: Value =
            serde_json::from_slice(&output.stdout).expect("complete native failure projection");
        assert_eq!(failure["code"], "release_commit");
        assert!(
            failure.get("effect").is_some() && failure.get("commit").is_none(),
            "failure is not a successful commit report"
        );
    }
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic UTF-8");
    assert!(diagnostic.contains("release_commit"), "{diagnostic}");
    assert!(!diagnostic.contains("PRIVATE"), "{diagnostic}");
    diagnostic
}

#[test]
fn explicit_local_preparation_and_sdk_readback_preserve_sources_refs_and_metadata() {
    let fixture = Fixture::new();
    let metadata = metadata();
    let head = fs::read(fixture.repository.join("HEAD")).expect("initial HEAD");
    let prepared = report(fixture.prepare(&bytes(&metadata), &[]));
    assert_eq!(prepared.as_object().expect("report object").len(), 5);
    assert_eq!(
        prepared["output"],
        serde_json::to_value(fixture.expected).expect("identity")
    );
    assert!(prepared["parent"].is_null());
    assert_eq!(prepared["entries"], 3);
    let oid = prepared["commit"].as_str().expect("exact created OID");
    assert_eq!(
        report(run(fixture.command(false, Some(oid)), &bytes(&metadata))),
        prepared
    );
    let typed = serde_json::from_value::<release_commit::CommitMetadata>(metadata.clone())
        .expect("native metadata");
    let native = release_commit::verify(
        &fixture.repository,
        &oid.parse().expect("exact commit"),
        fixture.expected,
        &typed,
        Default::default(),
    )
    .expect("same SDK readback");
    assert_eq!(serde_json::to_value(native).expect("SDK report"), prepared);
    let repo = Repository::open_bare(&fixture.repository).expect("exact repository");
    let commit = repo
        .find_commit(oid.parse().expect("Git OID"))
        .expect("actual persisted commit");
    assert_eq!(
        commit.message_bytes(),
        metadata["message"].as_str().expect("message").as_bytes()
    );
    assert_eq!(
        commit.author().name().expect("UTF-8 author name"),
        "PRIVATE_AUTHOR"
    );
    assert_eq!(commit.author().when().seconds(), 100);
    assert_eq!(commit.author().when().offset_minutes(), 330);
    assert_eq!(commit.committer().when().offset_minutes(), -60);
    assert_eq!(repo.references().expect("references").count(), 0);
    assert_eq!(
        fs::read(fixture.repository.join("HEAD")).expect("unchanged HEAD"),
        head
    );
    fixture.unchanged_output();
    assert_eq!(
        report(fixture.prepare(&bytes(&metadata), &[])),
        prepared,
        "same recipe, no ambient clock"
    );
    let mut child = metadata.clone();
    child["parent"] = json!(oid);
    let descendant = report(fixture.prepare(&bytes(&child), &[]));
    assert_ne!(descendant["commit"], prepared["commit"]);
    assert_eq!(descendant["parent"], prepared["commit"]);
    assert_eq!(repo.references().expect("still no refs").count(), 0);
    let after_preparation = fixture.repository_inventory();
    let mut mismatch = metadata;
    mismatch["message"] = json!("PRIVATE_WRONG_MESSAGE");
    refused(run(fixture.command(false, Some(oid)), &bytes(&mismatch)));
    assert_eq!(
        fixture.repository_inventory(),
        after_preparation,
        "read-only mismatch"
    );
}

#[test]
fn private_metadata_transport_is_complete_strict_bounded_and_pre_effect() {
    let fixture = Fixture::new();
    let before = fixture.repository_inventory();
    let valid = bytes(&metadata());
    let text = std::str::from_utf8(&valid).expect("fixture text");
    let mut missing = metadata();
    missing.as_object_mut().expect("metadata").remove("parent");
    let mut unknown = metadata();
    unknown["PRIVATE_UNKNOWN"] = json!(true);
    let mut positional = metadata();
    positional["author"] = json!(["PRIVATE_AUTHOR", "private@example.invalid", 100, 330]);
    let mut nested_unknown = metadata();
    nested_unknown["author"]["PRIVATE_UNKNOWN"] = json!(true);
    for input in [
        Vec::new(),
        b"[]".to_vec(),
        b"null".to_vec(),
        b"{}".to_vec(),
        b"PRIVATE_INVALID".to_vec(),
        b"\xff".to_vec(),
        bytes(&missing),
        bytes(&unknown),
        bytes(&positional),
        bytes(&nested_unknown),
        format!("{{\"par\\u0065nt\":null,{}", &text[1..]).into_bytes(),
        text.replacen(
            "\"author\":{",
            "\"author\":{\"na\\u006de\":\"PRIVATE_DUPLICATE\",",
            1,
        )
        .into_bytes(),
        format!("{text} PRIVATE_TRAILING").into_bytes(),
    ] {
        refused(fixture.prepare(&input, &[]));
        assert_eq!(
            fixture.repository_inventory(),
            before,
            "malformed input caused native mutation"
        );
    }
    refused(fixture.prepare(&valid, &["--max-input-bytes", "0"]));
    refused(fixture.prepare(
        &valid,
        &["--max-input-bytes", &(valid.len() - 1).to_string()],
    ));
    assert_eq!(fixture.repository_inventory(), before);
    report(fixture.prepare(&valid, &["--max-input-bytes", &valid.len().to_string()]));
}

#[test]
fn explicit_permission_identity_and_source_separation_gate_object_writes() {
    let fixture = Fixture::new();
    let before = fixture.repository_inventory();
    let metadata = bytes(&metadata());
    assert!(
        refused(run(fixture.command(true, None), b"PRIVATE_INVALID"))
            .contains("--allow-write-objects")
    );
    assert!(
        refused(fixture.prepare(
            &metadata,
            &[
                "--protected-root",
                fixture.repository.to_str().expect("fixture path")
            ]
        ))
        .contains("cannot be separated")
    );
    for bound in [
        "--max-entries",
        "--max-total-bytes",
        "--max-git-metadata-bytes",
    ] {
        refused(fixture.prepare(&metadata, &[bound, "0"]));
    }
    assert_eq!(fixture.repository_inventory(), before);
    fs::write(fixture.output.join("index.html"), b"changed after review").expect("changed fixture");
    refused(fixture.prepare(&metadata, &[]));
    assert_eq!(
        fixture.repository_inventory(),
        before,
        "wrong retained output identity"
    );
}

#[test]
fn verify_rejects_changed_expected_tree_and_prepare_rejects_nested_repository() {
    let fixture = Fixture::new();
    let metadata = bytes(&metadata());
    let prepared = report(fixture.prepare(&metadata, &[]));
    let before = fixture.repository_inventory();
    let empty = tempdir().expect("different tree");
    let wrong = source::inspect(empty.path(), None, Default::default())
        .expect("empty inventory")
        .identity();
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("verify-release-commit")
        .arg(&fixture.repository)
        .arg(prepared["commit"].as_str().expect("OID"))
        .args([
            "--expected-tree",
            &serde_json::to_string(&wrong).expect("wrong identity"),
        ]);
    refused(run(command, &metadata));
    assert_eq!(fixture.repository_inventory(), before);
    let nested = fixture.output.join("nested.git");
    Repository::init_bare(&nested).expect("nested fixture repo");
    let nested_before =
        source::inspect(&nested, None, Default::default()).expect("nested inventory");
    let complete = source::inspect(&fixture.output, None, Default::default())
        .expect("current complete input")
        .identity();
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("prepare-release-commit")
        .arg(&nested)
        .arg(&fixture.output)
        .args([
            "--allow-write-objects",
            "--expected-tree",
            &serde_json::to_string(&complete).expect("identity"),
        ]);
    assert!(refused(run(command, &metadata)).contains("cannot be separated"));
    assert_eq!(
        source::inspect(&nested, None, Default::default()).expect("nested reread"),
        nested_before
    );
}

#[test]
fn failed_report_delivery_retains_the_exact_created_commit_for_read_only_recovery() {
    let fixture = Fixture::new();
    let metadata = metadata();
    let mut command = fixture.command(true, None);
    // A genuinely nonwritable inherited stdout descriptor, not a mocked writer.
    let mut child = command
        .arg("--allow-write-objects")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(
            fs::File::open("/dev/null").expect("read-only null descriptor"),
        ))
        .stderr(Stdio::piped())
        .spawn()
        .expect("native CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&bytes(&metadata))
        .expect("private input");
    let output = child
        .wait_with_output()
        .expect("failed report delivery exit");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic");
    assert!(
        diagnostic.contains("report delivery failed"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("PRIVATE"), "{diagnostic}");
    let retained: Value =
        serde_json::from_str(diagnostic.lines().next().expect("exact retained report"))
            .expect("original bounded JSON on independent carrier");
    let oid = retained["commit"]
        .as_str()
        .expect("exact original created OID");
    let recovered = report(run(fixture.command(false, Some(oid)), &bytes(&metadata)));
    assert_eq!(recovered["commit"], oid);
    assert_eq!(
        recovered, retained,
        "read-only recovery matches the entire original report"
    );
    fixture.unchanged_output();
    assert_eq!(
        Repository::open_bare(&fixture.repository)
            .expect("repo")
            .references()
            .expect("refs")
            .count(),
        0
    );
}

#[test]
fn command_help_does_not_offer_full_release_or_network_effects() {
    for command in ["prepare-release-commit", "verify-release-commit"] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args([command, "--help"])
            .output()
            .expect("help");
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).expect("help UTF-8");
        assert!(help.contains("--expected-tree"));
        assert!(help.contains("--max-input-bytes"));
        assert!(!help.contains("--push"));
        assert!(!help.contains("--sign"));
        assert!(!help.contains("--fetch"));
    }
}

#[test]
fn response_reservation_precedes_writes_and_late_failure_keeps_native_effect() {
    let fixture = Fixture::new();
    let before = fixture.repository_inventory();
    let rejected = fixture.prepare(&bytes(&metadata()), &["--max-output-bytes", "1"]);
    assert!(refused(rejected).contains("response_limit"));
    assert_eq!(
        fixture.repository_inventory(),
        before,
        "no objects before response capacity"
    );
    let blob = git2::Oid::hash_object(git2::ObjectType::Blob, b"<h1>Local review</h1>")
        .expect("native first blob")
        .to_string();
    fs::write(
        fixture.repository.join("objects").join(&blob[..2]),
        b"not a directory",
    )
    .expect("deterministic native write obstruction");
    let result = fixture.prepare(&bytes(&metadata()), &[]);
    assert_eq!(result.status.code(), Some(1));
    let projected: Value =
        serde_json::from_slice(&result.stdout).expect("complete failed operation evidence");
    assert_eq!(projected["code"], "release_commit");
    assert_eq!(projected["effect"], "objects_may_remain");
    assert!(
        projected.get("commit").is_none(),
        "no fake commit after failed ODB write"
    );
    fixture.unchanged_output();
}
