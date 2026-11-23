//! Direct recovery consumes native history, never a new recipe or compilation.

use std::process::{Command, Output, Stdio};

use serde_json::Value;

use super::{
    Fixture, Retained, acquire, fs, metadata, options, published_fixture, read, release, render,
    request, source, store,
};

const PERMISSIONS: [&str; 5] = [
    "--synchronize-worktree",
    "--record-preparation",
    "--advance-writer-generation",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];

fn command(stage: &Retained, commit: &source::CommitId, header: store::JournalHeader) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("recover-journal-worktree")
        .arg(&stage.database)
        .arg(stage.receipt.record.intent.id.to_string())
        .arg(commit.as_str())
        .arg("--expected-header")
        .arg(serde_json::to_string(&header).expect("retained exact header"))
        .stdin(Stdio::null());
    command
}

fn pending(stage: &Retained, fixture: &Fixture) -> (source::CommitId, store::PreparationReceipt) {
    let recipe = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let mut bounded = options();
    bounded.worktree.storage_bytes = 1;
    let failure = release::prepare_journal_worktree(request(stage, &recipe, &sources), bounded)
        .expect_err("actual commit creation precedes the native synchronization budget failure");
    let release::WorktreeEffect::CommitCreated(commit) = failure.effect else {
        panic!("native commit identity must survive preparation failure")
    };
    let receipt = *failure
        .receipt
        .expect("durable intent precedes real Git effects");
    assert!(
        receipt
            .record
            .worktree
            .as_ref()
            .expect("stored recipe")
            .completion
            .is_none()
    );
    (commit, receipt)
}

fn history(stage: &Retained) -> store::PreparationInspection {
    store::inspect_preparation(
        &stage.database,
        stage.receipt.record.intent.id,
        None,
        read(),
    )
    .expect("independent exact native history")
}

fn native_bytes(stage: &Retained, fixture: &Fixture) -> [source::Inventory; 2] {
    [stage.root.as_path(), fixture.directory.path()].map(|path| {
        source::inspect(path, None, Default::default())
            .expect("complete output/Git and input inventory")
    })
}

fn refused(output: Output) -> String {
    assert!(
        !output.status.success(),
        "unexpected recovery success: {output:?}"
    );
    if !output.stdout.is_empty() {
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("failed native recovery projection");
        assert!(report.get("failure").is_some());
        assert!(report.get("record").is_none());
    }
    String::from_utf8(output.stderr).expect("redacted native diagnostic")
}

#[test]
fn executable_completes_existing_effects_and_exact_replay_preserves_original_completion() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("actual publication");
    let stage = Retained::new(&rendered);
    fs::write(
        fixture.directory.path().join("one/page.md"),
        b"LATER UNACQUIRED SOURCE",
    )
    .expect("later native source edit must not be recaptured");
    let (commit, pending) = pending(&stage, &fixture);
    let before = native_bytes(&stage, &fixture);
    let output = command(&stage, &commit, pending.journal)
        .args(PERMISSIONS)
        .arg("--protected-root")
        .arg(fixture.directory.path())
        .output()
        .expect("direct executable recovery");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete local receipt");
    let observed = history(&stage);
    let record = observed.record.as_ref().expect("durable completed record");
    assert_eq!(
        report["record"],
        serde_json::to_value(record).expect("native record")
    );
    assert_eq!(
        report["journal"],
        serde_json::to_value(observed.journal.header).expect("observed coordinates")
    );
    assert_eq!(report["inserted"], true);
    assert_eq!(record.intent, pending.record.intent);
    assert_eq!(record.retention, pending.record.retention);
    let completed = record
        .worktree
        .as_ref()
        .expect("same recipe")
        .completion
        .as_ref()
        .expect("real completion");
    assert_eq!(completed.report.commit, commit);
    assert_eq!(
        completed.report.output,
        rendered.site().output().inventory().identity()
    );
    assert_eq!(completed.recorded, observed.journal.header);
    assert_eq!(
        observed.journal.header.writer_fence,
        pending.journal.writer_fence + 1
    );
    assert_eq!(native_bytes(&stage, &fixture), before);

    let replay = command(&stage, &commit, observed.journal.header)
        .args(PERMISSIONS)
        .arg("--protected-root")
        .arg(fixture.directory.path())
        .output()
        .expect("explicit exact replay");
    assert!(replay.status.success(), "{replay:?}");
    let replay: Value = serde_json::from_slice(&replay.stdout).expect("complete replay receipt");
    assert_eq!(replay["inserted"], false);
    assert_eq!(replay["record"], report["record"]);
    let current = history(&stage);
    assert_eq!(current.record.as_ref(), Some(record));
    assert_eq!(
        current.journal.header.writer_fence,
        observed.journal.header.writer_fence + 1
    );
    assert_eq!(native_bytes(&stage, &fixture), before);
    let verified = release::inspect_journal_worktree(
        release::JournalWorktreeInspectionRequest {
            database: &stage.database,
            preparation_id: record.intent.id,
            expected_header: Some(&current.journal.header),
            commit: &commit,
            protected_roots: &[fixture.directory.path().to_owned()],
        },
        release::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("independent current HEAD/index/output and historical completion join");
    assert_eq!(verified.worktree.git.commit, completed.report);
    assert_eq!(
        verified.worktree.output,
        *rendered.site().output().inventory()
    );
}

#[test]
fn recovery_cli_preinspection_refusals_never_advance_writer_or_repair_native_state() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let (commit, pending) = pending(&stage, &fixture);
    let before = native_bytes(&stage, &fixture);
    for omitted in PERMISSIONS {
        let error = refused(
            command(&stage, &commit, pending.journal)
                .args(PERMISSIONS.into_iter().filter(|flag| *flag != omitted))
                .output()
                .expect("explicit permission refusal"),
        );
        assert!(
            error.contains("no successful recorded-worktree recovery report returned"),
            "must reach the native permission gate: {error}"
        );
        let actual = history(&stage);
        assert_eq!(actual.journal.header, pending.journal);
        assert_eq!(actual.record.as_ref(), Some(&pending.record));
        assert_eq!(native_bytes(&stage, &fixture), before);
    }
    refused(
        command(&stage, &commit, stage.receipt.journal)
            .args(PERMISSIONS)
            .output()
            .expect("stale header refusal"),
    );
    for (flag, value) in [
        ("--max-storage-bytes", "1"),
        ("--max-entries", "0"),
        ("--max-total-bytes", "0"),
        ("--max-git-metadata-bytes", "1"),
    ] {
        let error = refused(
            command(&stage, &commit, pending.journal)
                .args(PERMISSIONS)
                .arg(flag)
                .arg(value)
                .output()
                .expect("native preinspection ceiling"),
        );
        assert!(
            error.contains("no successful recorded-worktree recovery report returned"),
            "must reach the native preinspection bound: {error}"
        );
        let actual = history(&stage);
        assert_eq!(actual.journal.header, pending.journal);
        assert_eq!(actual.record.as_ref(), Some(&pending.record));
        assert_eq!(native_bytes(&stage, &fixture), before);
    }
    refused(
        command(&stage, &commit, pending.journal)
            .args(PERMISSIONS)
            .arg("--protected-root")
            .arg(&stage.root)
            .output()
            .expect("source overlap refusal"),
    );
    let wrong: source::CommitId = "1111111111111111111111111111111111111111"
        .parse()
        .expect("exact different commit");
    refused(
        command(&stage, &wrong, pending.journal)
            .args(PERMISSIONS)
            .output()
            .expect("wrong existing commit refusal"),
    );
    fs::write(
        stage.root.join("guide/index.html"),
        b"ALTERED RETAINED OUTPUT",
    )
    .expect("local drift");
    let changed = native_bytes(&stage, &fixture);
    refused(
        command(&stage, &commit, pending.journal)
            .args(PERMISSIONS)
            .output()
            .expect("known drift fails before recovery writer"),
    );
    assert_eq!(native_bytes(&stage, &fixture), changed);
    let actual = history(&stage);
    assert_eq!(actual.journal.header, pending.journal);
    assert_eq!(actual.record.as_ref(), Some(&pending.record));
}

#[test]
fn missing_recorded_recipe_and_malformed_arguments_never_initialize_or_acquire() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let commit: source::CommitId = "1111111111111111111111111111111111111111"
        .parse()
        .expect("syntactic exact OID");
    let before = native_bytes(&stage, &fixture);
    refused(
        command(&stage, &commit, stage.receipt.journal)
            .args(PERMISSIONS)
            .output()
            .expect("missing worktree phase"),
    );
    assert!(!stage.root.join(".git").exists());
    assert_eq!(history(&stage).journal.header, stage.receipt.journal);
    assert_eq!(history(&stage).record.as_ref(), Some(&stage.receipt.record));
    assert_eq!(native_bytes(&stage, &fixture), before);

    let root = tempfile::tempdir().expect("isolated absent database root");
    let absent = root.path().join("missing.sqlite");
    let id = stage.receipt.record.intent.id.to_string();
    let header = serde_json::to_string(&stage.receipt.journal).expect("header");
    for args in [
        vec![id.as_str(), commit.as_str()], // required explicit header omitted
        vec![id.as_str(), "HEAD", "--expected-header", header.as_str()],
        vec![
            "not-a-digest",
            commit.as_str(),
            "--expected-header",
            header.as_str(),
        ],
        vec![id.as_str(), commit.as_str(), "--expected-header", "{}"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("recover-journal-worktree")
            .arg(&absent)
            .args(args)
            .args(PERMISSIONS)
            .stdin(Stdio::null())
            .output()
            .expect("malformed coordinate transport");
        let error = refused(output);
        assert!(
            error.contains("error:"),
            "argument failure must not masquerade as native recovery: {error}"
        );
        assert!(
            fs::read_dir(root.path())
                .expect("no native access effects")
                .next()
                .is_none()
        );
    }
}

#[test]
fn late_native_sync_failure_keeps_pending_history_and_reports_known_progress() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let (commit, pending) = pending(&stage, &fixture);
    let before = native_bytes(&stage, &fixture);
    let error = refused(
        command(&stage, &commit, pending.journal)
            .args(PERMISSIONS)
            .args(["--max-git-storage-bytes", "1"])
            .output()
            .expect("native late synchronization ceiling"),
    );
    assert!(error.contains("output sync Verified"), "{error}");
    assert!(error.contains("Git sync NotAttempted"), "{error}");
    let actual = history(&stage);
    assert_eq!(
        actual.journal.header.writer_fence,
        pending.journal.writer_fence + 1
    );
    assert_eq!(actual.record.as_ref(), Some(&pending.record));
    assert_eq!(native_bytes(&stage, &fixture), before);
}

#[test]
fn lost_executable_receipt_preserves_completed_native_recovery_without_automatic_retry() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let (commit, pending) = pending(&stage, &fixture);
    let before = native_bytes(&stage, &fixture);
    let output = command(&stage, &commit, pending.journal)
        .args(PERMISSIONS)
        .stdout(fs::File::open("/dev/null").expect("nonwritable output descriptor"))
        .output()
        .expect("execute recovery with failed receipt sink");
    assert!(
        !output.status.success(),
        "lost receipt is not a successful delivery"
    );
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("secondary receipt carrier");
    let delivered: serde_json::Value =
        serde_json::from_str(diagnostic.lines().next().expect("original receipt JSON"))
            .expect("complete native receipt");
    let actual = history(&stage);
    assert_eq!(
        delivered["record"],
        serde_json::to_value(actual.record.as_ref().expect("native record"))
            .expect("original history")
    );
    assert_eq!(
        delivered["journal"],
        serde_json::to_value(actual.journal.header).expect("observed generation")
    );
    assert_eq!(
        actual.journal.header.writer_fence,
        pending.journal.writer_fence + 1,
        "no implicit second writer or automatic retry"
    );
    let record = actual.record.as_ref().expect("native history preserved");
    assert_eq!(record.intent, pending.record.intent);
    assert_eq!(record.retention, pending.record.retention);
    let completion = record
        .worktree
        .as_ref()
        .expect("recipe")
        .completion
        .as_ref()
        .expect("actual completion survives failed receipt output");
    assert_eq!(completion.report.commit, commit);
    assert_eq!(completion.recorded, actual.journal.header);
    assert_eq!(native_bytes(&stage, &fixture), before);
}

#[test]
fn insufficient_response_capacity_stops_before_native_recovery_or_generation() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("render");
    let stage = Retained::new(&rendered);
    let (commit, pending) = pending(&stage, &fixture);
    let before = native_bytes(&stage, &fixture);
    let diagnostic = refused(
        command(&stage, &commit, pending.journal)
            .args(PERMISSIONS)
            .args(["--max-output-bytes", "1"])
            .output()
            .expect("real executable reservation refusal"),
    );
    assert!(diagnostic.contains("journal_response_limit"));
    let actual = history(&stage);
    assert_eq!(actual.journal.header, pending.journal);
    assert_eq!(actual.record.as_ref(), Some(&pending.record));
    assert_eq!(native_bytes(&stage, &fixture), before);
}
