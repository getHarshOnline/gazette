//! Actual retained render, reserved `SQLite` history and guarded native output writes.

#[path = "materialization/cli.rs"]
mod cli;
#[path = "materialization/command.rs"]
mod command;
#[path = "materialization/interfaces.rs"]
mod interfaces;
#[path = "materialization/set.rs"]
mod set;
#[path = "materialization/single_workspace.rs"]
mod single_workspace;

use std::{os::unix::fs::PermissionsExt, path::Path};

use gazette_sdk::store::{self, JournalHeader};

use super::{Fixture, PAGE, acquire, build, bytes, fs, render, source, tempdir};

fn configuration(fixture: &Fixture, directory: &Path) -> String {
    fixture.config().replacen(
        "site='site:one'\npublic_base_url",
        &format!(
            "site='site:one'\noutput={{directory={:?}}}\npublic_base_url",
            directory.to_str().expect("native coordinate")
        ),
        1,
    )
}

fn session() -> store::SessionOptions {
    store::SessionOptions {
        allow_local_reservation: true,
        read: store::ReadOptions {
            allow_coordination_files: true,
            ..Default::default()
        },
    }
}

fn initialize() -> (tempfile::TempDir, store::InitializationReceipt) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable journal");
    let initial = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session: session(),
            ..Default::default()
        },
    )
    .expect("actual fresh SQLite schema and read-back");
    (root, initial)
}

fn writer(root: &Path, expected: JournalHeader) -> store::Writer {
    store::acquire_writer(
        root,
        expected,
        store::WriterOptions {
            allow_generation_advance: true,
            session: session(),
        },
    )
    .expect("exact new reserved native generation")
}

fn options() -> build::MaterializationOptions {
    build::MaterializationOptions {
        allow_write: true,
        history: store::MaterializationWriteOptions { allow_record: true },
        ..Default::default()
    }
}

#[test]
fn historical_inspection_matches_sdk_cli_and_initialized_mcp_without_opening_destination() {
    let destination = tempdir().expect("selected output");
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let rendered = render(&inputs).expect("actual retained render");
    let (journal, initialized) = initialize();
    let mut current = writer(journal.path(), initialized.header);
    let plan = current
        .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
        .expect("actual native preflight");
    let result = current
        .materialize(
            plan,
            source::Sha256Digest::of(b"interface-history"),
            options(),
        )
        .expect("original output and durable completion");
    let root = result
        .output
        .expect("actual native output")
        .binding()
        .canonical_root()
        .to_owned();
    let header = current.inspect().expect("exact final generation").header;
    current.close().expect("release original writer custody");
    let expected = serde_json::to_value(
        store::inspect_materialization(&initialized.database, &root, Some(&header), session().read)
            .expect("standalone native history"),
    )
    .expect("historical contract");
    let database_before = fs::read(&initialized.database).expect("database baseline");
    // A historical lookup must not require the original native output to still exist.
    drop(destination);
    assert!(!root.exists());
    let header_json = serde_json::to_string(&header).expect("independent coordinates");
    let output = super::cli(
        "",
        &[
            "inspect-materialization",
            initialized.database.to_str().expect("database"),
            "--output-root",
            root.to_str().expect("history root"),
            "--expected-header",
            &header_json,
            "--allow-coordination-files",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("CLI JSON"),
        expected
    );

    crate::mcp_carrier::initialized_interact("2026-07-28", |_, send| {
        let catalog =
            send(serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        let descriptor = catalog["result"]["tools"]
            .as_array()
            .expect("initialized discovery")
            .iter()
            .find(|tool| tool["name"] == "inspect_materialization")
            .expect("same native history operation");
        let arguments = serde_json::json!({
            "path":initialized.database, "output_root":root,
            "expected_header":header, "allow_read":true, "read":session().read,
        });
        jsonschema::validator_for(&descriptor["inputSchema"])
            .expect("input contract")
            .validate(&arguments)
            .expect("exact selected request");
        let good = send(
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"inspect_materialization","arguments":arguments}}),
        );
        assert_eq!(good["result"]["isError"], false, "{good}");
        assert_eq!(good["result"]["structuredContent"], expected);
        jsonschema::validator_for(&descriptor["outputSchema"])
            .expect("output contract")
            .validate(&expected)
            .expect("native serialized history");
        let mut wrong = arguments.clone();
        wrong["expected_header"]["writer_fence"] = serde_json::json!(header.writer_fence + 1);
        let refused = send(
            serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"inspect_materialization","arguments":wrong}}),
        );
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert!(
            refused.to_string().contains("store_journal_mismatch"),
            "{refused}"
        );
        let next = send(
            serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"inspect_materialization","arguments":arguments}}),
        );
        assert_eq!(next["result"]["structuredContent"], expected);
    });
    assert_eq!(
        fs::read(&initialized.database).expect("unchanged durable history"),
        database_before
    );
    assert!(
        !root.exists(),
        "inspection must not recreate or open the destination"
    );
}

#[test]
fn separate_writer_lifetimes_reuse_only_confirmed_output_ownership_and_preserve_user_git() {
    let destination = tempdir().expect("output repository");
    let repository = git2::Repository::init(destination.path()).expect("actual Git administration");
    fs::write(destination.path().join("README.user"), b"owner work").expect("user sentinel");
    let git_before = source::inspect(repository.path(), None, Default::default())
        .expect("admin snapshot")
        .identity();
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let rendered = render(&inputs).expect("retained publication");
    let (journal, initial) = initialize();
    let mut first = writer(journal.path(), initial.header);
    let plan = first
        .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
        .expect("no write before exact native/store plan");
    assert!(plan.previous().is_none());
    let replay_plan = first
        .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
        .expect("second original plan is still bound to the same pre-effect observation");
    let first_id = source::Sha256Digest::of(b"first-configured-generation");
    let result = first
        .materialize(plan, first_id, options())
        .expect("intent, actual write/read-back, completion");
    assert!(result.history.inserted);
    assert!(result.history.record.completion.is_some());
    let replay = first
        .materialize(replay_plan, first_id, options())
        .expect("exact original intent replay returns history without another write");
    assert!(!replay.history.inserted);
    assert!(replay.output.is_none());
    assert_eq!(replay.history.record, result.history.record);
    let native = result
        .output
        .expect("this call actually produced native evidence");
    assert_eq!(native.generated(), rendered.site().output().inventory());
    let output_root = native.binding().canonical_root().to_owned();
    let expected = first
        .inspect()
        .expect("current original writer coordinates")
        .header;
    first.close().expect("close actual held generation");
    let inspected = store::inspect_materialization(
        &initial.database,
        &output_root,
        Some(&expected),
        session().read,
    )
    .expect("read-only history needs no writer or destination observation");
    assert_eq!(
        inspected
            .frontier
            .head
            .expect("completed historical head")
            .intent
            .id,
        first_id
    );
    let mut wrong = expected;
    wrong.writer_fence += 1;
    assert_eq!(
        store::inspect_materialization(
            &initial.database,
            &output_root,
            Some(&wrong),
            session().read,
        )
        .expect_err("independent coordinates are not silently rebound")
        .code(),
        "store_journal_mismatch"
    );
    drop(native); // A later lifetime must not require this in-memory receipt.
    drop(rendered);
    drop(inputs);
    drop(fixture); // Retired native source locators do not grant a later read capability.
    let page = PAGE
        .replace("aliases=['/old.html']", "aliases=[]")
        .replace("**Retained** body", "**Changed** body");
    let changed = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let inputs = acquire(&configuration(&changed, destination.path()));
    let rendered = render(&inputs).expect("new pinned render");
    let mut second = writer(journal.path(), expected);
    let plan = second
        .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
        .expect("stored completed head, independently observed native bytes");
    assert_eq!(plan.previous(), Some(first_id));
    assert!(
        plan.output()
            .changes()
            .iter()
            .any(|change| change.path.as_str() == "old.html" && change.after.is_none())
    );
    let result = second
        .materialize(
            plan,
            source::Sha256Digest::of(b"second-configured-generation"),
            options(),
        )
        .expect("actual replacement and obsolete-file removal");
    assert!(result.output.is_some());
    assert!(!destination.path().join("old.html").exists());
    assert_eq!(
        fs::read(destination.path().join("guide/index.html")).expect("actual changed page"),
        bytes(rendered.site().output(), "guide/index.html")
    );
    assert_eq!(
        fs::read(destination.path().join("README.user")).expect("user work"),
        b"owner work"
    );
    assert_eq!(
        source::inspect(repository.path(), None, Default::default())
            .expect("preserved Git administration")
            .identity(),
        git_before
    );
    second.close().expect("complete current generation");
}

#[test]
fn missing_write_or_history_permission_does_not_append_pending_intent_or_touch_output() {
    for (allow_write, allow_record) in [(false, true), (true, false)] {
        let destination = tempdir().expect("destination");
        let fixture = Fixture::new(&[]);
        let inputs = acquire(&configuration(&fixture, destination.path()));
        let rendered = render(&inputs).expect("retained publication");
        let (journal, initial) = initialize();
        let mut writer = writer(journal.path(), initial.header);
        let before = fs::read(&initial.database).expect("before journal bytes");
        let plan = writer
            .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
            .expect("write-free plan");
        let error = writer
            .materialize(
                plan,
                source::Sha256Digest::of(b"not-permitted"),
                build::MaterializationOptions {
                    allow_write,
                    history: store::MaterializationWriteOptions { allow_record },
                    ..options()
                },
            )
            .expect_err("explicit independent permissions");
        assert_eq!(
            error.code(),
            if allow_write {
                "store_materialization_permission"
            } else {
                "materialization_permission"
            }
        );
        assert_eq!(
            fs::read(&initial.database).expect("no history writes"),
            before
        );
        assert_eq!(
            fs::read_dir(destination.path())
                .expect("no output effects")
                .count(),
            0
        );
        let state = writer
            .inspect_materialization(&destination.path().canonicalize().expect("native root"))
            .expect("unowned current frontier");
        assert!(state.frontier.head.is_none() && state.frontier.active.is_none());
        writer.close().expect("clean lifetime close");
    }
}

#[test]
fn changed_native_destination_after_plan_retains_pending_history_and_reopen_never_retries() {
    let destination = tempdir().expect("destination");
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&configuration(&fixture, destination.path()));
    let rendered = render(&inputs).expect("retained publication");
    let (journal, initial) = initialize();
    let mut current = writer(journal.path(), initial.header);
    let plan = current
        .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
        .expect("original empty native observation");
    fs::write(
        destination.path().join("user.txt"),
        b"concurrent owner work",
    )
    .expect("owner changed before guarded effects");
    let id = source::Sha256Digest::of(b"pending-exact-output");
    let error = current
        .materialize(plan, id, options())
        .expect_err("original observation cannot be silently repaired");
    assert!(matches!(error, build::MaterializationError::Write { id: failed, .. } if failed == id));
    let state = current
        .inspect_materialization(&destination.path().canonicalize().expect("native root"))
        .expect("actual durable pending record");
    assert!(state.frontier.head.is_none());
    let active = state
        .frontier
        .active
        .expect("intent preceded failed writer");
    assert_eq!(active.intent.id, id);
    assert!(active.completion.is_none());
    assert_eq!(active.intent.observed.entries().len(), 0);
    assert_eq!(
        active.intent.generated,
        *rendered.site().output().inventory()
    );
    let expected = current.inspect().expect("current generation").header;
    current
        .close()
        .expect("drop custody without completing intent");
    let mut reopened = writer(journal.path(), expected);
    assert_eq!(
        reopened
            .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
            .expect_err("pending operation blocks new starts")
            .code(),
        "store_materialization_pending"
    );
    assert_eq!(
        fs::read_dir(destination.path())
            .expect("no automatic replay")
            .count(),
        1
    );
    assert_eq!(
        fs::read(destination.path().join("user.txt")).expect("owner work retained"),
        b"concurrent owner work"
    );
    reopened
        .close()
        .expect("observation did not rewrite or reconcile");
}

#[test]
fn journal_inside_configured_output_is_refused_before_any_intent_or_output_write() {
    let (journal, initial) = initialize();
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&configuration(&fixture, journal.path()));
    let rendered = render(&inputs).expect("render does not grant output effects");
    let mut writer = writer(journal.path(), initial.header);
    let before = fs::read(&initial.database).expect("held journal bytes");
    assert_eq!(
        writer
            .plan_materialization(&rendered, &[], Default::default(), options().evidence_bytes)
            .expect_err("actual provider-owned roots cannot be caller-omitted")
            .code(),
        "output_observation"
    );
    assert_eq!(
        fs::read(&initial.database).expect("no output or history overwrite"),
        before
    );
    writer.close().expect("still clean original generation");
}
