//! Executable projection of the journal/native join, not a second preparation producer.

use std::process::{Command, Output};

use serde_json::Value;

use super::{
    Retained, acquire, fs, inspect, metadata, options, published_fixture, read, release, render,
    request, source,
};

fn command(stage: &Retained, commit: &source::CommitId) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-journal-worktree")
        .arg(&stage.database)
        .arg(stage.receipt.record.intent.id.to_string())
        .arg(commit.as_str());
    command
}

fn refused(output: Output) -> String {
    assert!(
        !output.status.success(),
        "unexpected successful join: {output:?}"
    );
    assert!(
        output.stdout.is_empty(),
        "failed join must not emit a success report"
    );
    String::from_utf8(output.stderr).expect("redacted UTF-8 error")
}

#[test]
fn cli_returns_the_same_historical_and_current_evidence_as_the_sdk_without_repair() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("retained publication");
    let stage = Retained::new(&rendered);
    let recipe = metadata();
    let protected = [fixture.directory.path().to_owned()];
    let receipt =
        release::prepare_journal_worktree(request(&stage, &recipe, &protected), options())
            .expect("real durable worktree");
    let commit = &receipt
        .record
        .worktree
        .as_ref()
        .expect("worktree intent")
        .completion
        .as_ref()
        .expect("completion")
        .report
        .commit;
    let before = source::inspect(&stage.root.join(".git"), None, Default::default())
        .expect("exact administrative bytes");
    let database = fs::read(&stage.database).expect("durable database before read");
    let expected = release::inspect_journal_worktree(
        release::JournalWorktreeInspectionRequest {
            database: &stage.database,
            preparation_id: stage.receipt.record.intent.id,
            expected_header: Some(&receipt.journal),
            commit,
            protected_roots: &protected,
        },
        release::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("SDK join");
    let output = command(&stage, commit)
        .arg("--allow-coordination-files")
        .arg("--expected-header")
        .arg(serde_json::to_string(&receipt.journal).expect("exact coordinates"))
        .arg("--protected-root")
        .arg(&protected[0])
        .output()
        .expect("execute joined read");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let actual: Value = serde_json::from_slice(&output.stdout).expect("one complete report");
    assert_eq!(
        actual,
        serde_json::to_value(expected).expect("native SDK report")
    );
    assert_eq!(inspect(&stage, receipt.journal), receipt.record);
    assert_eq!(
        fs::read(&stage.database).expect("database after read"),
        database
    );
    assert_eq!(
        source::inspect(&stage.root.join(".git"), None, Default::default()).expect("unchanged Git"),
        before
    );

    fs::write(stage.root.join("guide/index.html"), b"later native bytes")
        .expect("disposable drift");
    refused(
        command(&stage, commit)
            .arg("--allow-coordination-files")
            .output()
            .expect("inspect drift"),
    );
    assert_eq!(
        fs::read(stage.root.join("guide/index.html")).expect("no repair"),
        b"later native bytes"
    );
    assert_eq!(
        inspect(&stage, receipt.journal),
        receipt.record,
        "history is not overwritten by current failure"
    );
}

#[test]
fn permissions_header_output_and_metadata_limits_reach_the_shared_operation() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let recipe = metadata();
    let receipt = release::prepare_journal_worktree(request(&stage, &recipe, &[]), options())
        .expect("worktree");
    let commit = &receipt
        .record
        .worktree
        .as_ref()
        .expect("intent")
        .completion
        .as_ref()
        .expect("completion")
        .report
        .commit;
    let error = refused(
        command(&stage, commit)
            .output()
            .expect("missing permission"),
    );
    assert!(error.contains("store_coordination_permission"), "{error}");
    for (flag, value) in [
        (
            "--expected-header",
            serde_json::to_string(&stage.receipt.journal).expect("stale coordinates"),
        ),
        ("--max-storage-bytes", "1".into()),
        ("--max-entries", "0".into()),
        ("--max-total-bytes", "0".into()),
        ("--max-git-metadata-bytes", "1".into()),
    ] {
        let error = refused(
            command(&stage, commit)
                .arg("--allow-coordination-files")
                .arg(flag)
                .arg(value)
                .output()
                .expect("bounded refusal"),
        );
        assert!(
            error.contains("no joined worktree inspection report produced"),
            "{error}"
        );
        assert_eq!(inspect(&stage, receipt.journal), receipt.record);
    }
}

#[test]
fn cli_requires_exact_coordinates_and_advertises_shared_bounds_without_opening_missing_database() {
    let root = tempfile::tempdir().expect("disposable root");
    let missing = root.path().join("missing.sqlite");
    let id = source::Sha256Digest::of(b"absent preparation").to_string();
    for args in [
        vec![id.as_str()],
        vec![id.as_str(), "HEAD"],
        vec!["not-a-digest", "0123456789012345678901234567890123456789"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("inspect-journal-worktree")
            .arg(&missing)
            .args(args)
            .output()
            .expect("invalid CLI transport");
        refused(output);
        assert!(
            !missing.exists(),
            "argument rejection never initializes a store"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["inspect-journal-worktree", "--help"])
        .output()
        .expect("help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help text");
    for flag in [
        "--allow-coordination-files",
        "--max-storage-bytes",
        "--max-sql-bytes",
        "--max-value-bytes",
        "--max-vm-steps",
        "--max-history-objects",
        "--max-history-bytes",
        "--max-history-object-bytes",
        "--max-git-metadata-bytes",
        "--max-file-bytes",
        "--max-depth",
    ] {
        assert!(help.contains(flag), "missing bound {flag}");
    }
}
