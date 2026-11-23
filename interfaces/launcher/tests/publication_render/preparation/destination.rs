//! Workspace destinations bind private preparation without changing public output or sending Git.

use gazette_sdk::{process, release_commit as release, workspace::Key};

use super::super::{policy::configured, worktree::metadata};
use super::check::disclosure_policy;
use super::{
    Value, acquire, build, enable_site, fs, initialize, options, private_directory,
    published_fixture, read, render, request, store,
};

const REMOTE: &str = "https://destination.example.invalid/private-publish-619827.git";
const BRANCH: &str = "private-publication-928371";

fn with_destination(config: &str, remote: &str, branch: &str) -> String {
    let anchor = "site='site:one'\npublic_base_url=";
    assert!(
        config.contains(anchor),
        "exact fixture brand publication anchor"
    );
    enable_site(config.replacen(
        anchor,
        &format!("site='site:one'\ndestination={{kind='git',remote={remote:?},branch={branch:?}}}\npublic_base_url="),
        1,
    ), "one")
}

fn process_options() -> process::Options {
    process::Options {
        allow_execute: true,
        ..Default::default()
    }
}

fn local_options() -> build::LocalPreparationOptions {
    build::LocalPreparationOptions {
        allow_anchor_write: false,
        stage: options(),
        policy: process_options(),
        disclosure: Default::default(),
        worktree: release::WorktreeOptions {
            allow_initialize: true,
            ..Default::default()
        },
    }
}

#[test]
fn exact_destination_reaches_native_checker_and_complete_private_history_without_public_leakage() {
    for configured_destination in [false, true] {
        let fixture = published_fixture(&[]);
        let observation = private_directory();
        let result = observation.path().join("actual-checker-input.json");
        let (base, _) = configured(&fixture, &result, 0);
        let config = if configured_destination {
            with_destination(&base, REMOTE, BRANCH)
        } else {
            base.clone()
        };
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("actual configured retained output");
        let plain_inputs = acquire(&base);
        let plain = render(&plain_inputs).expect("same output without destination selection");
        assert_eq!(
            rendered.site().output().inventory(),
            plain.site().output().inventory()
        );
        for entry in rendered.site().output().inventory().entries() {
            if let Some(bytes) = rendered.site().output().file(&entry.path) {
                assert_eq!(Some(bytes), plain.site().output().file(&entry.path));
                for private in [REMOTE, BRANCH] {
                    assert!(
                        !bytes
                            .windows(private.len())
                            .any(|part| part == private.as_bytes()),
                        "private destination leaked into {}",
                        entry.path
                    );
                }
            }
        }
        let site: Key = "site:one".parse().expect("site");
        let expected = inputs.compilation().workspace.destination(&site);
        assert_eq!(expected.is_some(), configured_destination);
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let parent = private_directory();
        let recipe = metadata();
        let completed = build::prepare_local(
            &rendered,
            build::LocalPreparationRequest {
                output_policy: None,
                qualification: None,
                coordinator: None,
                stage: request(
                    journal_root.path(),
                    journal.header,
                    parent.path(),
                    "destination",
                ),
                metadata: &recipe,
                parent_source: None,
                disclosure: &disclosure_policy(),
            },
            local_options(),
        )
        .expect("local preparation never contacts its configured remote");
        assert_eq!(completed.record.intent.destination.as_ref(), expected);
        let delivered: Value =
            serde_json::from_slice(&fs::read(&result).expect("actual native checker input"))
                .expect("private request JSON");
        assert_eq!(
            delivered["destination"],
            serde_json::to_value(expected).expect("typed optional destination")
        );
        if configured_destination {
            assert_eq!(
                delivered["destination"],
                serde_json::json!({"kind":"git","remote":REMOTE,"branch":BRANCH})
            );
        } else {
            assert!(delivered["destination"].is_null());
        }
        let check = completed.record.check.as_ref().expect("stored check");
        let evidence: Value = serde_json::from_str(&check.evidence).expect("durable checks");
        assert_eq!(
            delivered["disclosure"], evidence["disclosure"],
            "connected preparation must deliver the report it records"
        );
        assert!(
            evidence["policy"]["evidence"]["disclosure_sha256"].is_string(),
            "the configured checker must bind the required scan"
        );
        let worktree = completed
            .record
            .worktree
            .as_ref()
            .expect("actual Git preparation");
        assert_eq!(worktree.check_sha256, Some(check.evidence_sha256));
        let commit = &worktree
            .completion
            .as_ref()
            .expect("real Git completion")
            .report
            .commit;
        let protected = [fixture.directory.path().to_owned()];
        let inspection = release::inspect_journal_worktree(
            release::JournalWorktreeInspectionRequest {
                database: &journal.database,
                preparation_id: completed.record.intent.id,
                expected_header: Some(&completed.journal),
                commit,
                protected_roots: &protected,
            },
            release::JournalWorktreeInspectionOptions {
                read: read(),
                ..Default::default()
            },
        )
        .expect("independent real store, output and Git read-back");
        assert_eq!(
            inspection
                .history
                .record
                .expect("retained original destination"),
            completed.record
        );
    }
}

#[test]
fn changed_remote_or_branch_cannot_reuse_same_output_preparation_or_check() {
    for (remote, branch) in [
        ("https://different.example.invalid/repository.git", BRANCH),
        (REMOTE, "different-publication-branch"),
    ] {
        let fixture = published_fixture(&[]);
        let observation = private_directory();
        let result = observation.path().join("checker-input.json");
        let (base, _) = configured(&fixture, &result, 0);
        let config = with_destination(&base, REMOTE, BRANCH);
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("original output");
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let parent = private_directory();
        let stage = request(
            journal_root.path(),
            journal.header,
            parent.path(),
            "destination",
        );
        let original =
            build::prepare_stage(&rendered, stage, options()).expect("real original stage");
        let root = original.record.intent.address.path();
        let initial_stage = gazette_sdk::source::capture_pinned(
            &root,
            original.record.intent.output,
            Default::default(),
        )
        .expect("native original stage capture");
        let changed_inputs = acquire(&with_destination(&base, remote, branch));
        let changed =
            render(&changed_inputs).expect("same rendering with changed private destination");
        assert_eq!(
            changed.site().output().inventory(),
            rendered.site().output().inventory()
        );
        let checked = build::check_preparation(
            &changed,
            &disclosure_policy(),
            process_options(),
            Default::default(),
            None,
        )
        .expect("real checker for changed configuration");
        let before = fs::read(&journal.database).expect("database before refused check");
        let check_failure = build::record_preparation_check(
            &checked,
            build::PreparationCheckRequest {
                journal: journal_root.path(),
                expected: original.journal,
                preparation: &original.record,
            },
            build::PreparationCheckOptions {
                record: options().record,
                writer: options().writer,
                ..Default::default()
            },
        )
        .expect_err("different destination cannot borrow original stage identity");
        assert!(matches!(
            check_failure.cause.as_ref(),
            build::PreparationFailure::Mismatch
        ));
        assert!(!check_failure.writer_acquired);
        assert_eq!(
            fs::read(&journal.database).expect("unchanged check history"),
            before
        );
        let sentinel = b"no checker reexecution after rejected stage identity";
        fs::write(&result, sentinel).expect("observation sentinel");
        let recipe = metadata();
        let reuse = build::prepare_local(
            &changed,
            build::LocalPreparationRequest {
                output_policy: None,
                qualification: None,
                coordinator: None,
                stage: build::PreparationRequest {
                    expected: original.journal,
                    ..stage
                },
                metadata: &recipe,
                parent_source: None,
                disclosure: &disclosure_policy(),
            },
            local_options(),
        )
        .expect_err("same ID and output do not permit destination replacement");
        let build::LocalPreparationFailure::Stage(failure) = reuse.cause.as_ref() else {
            panic!("must fail original stage binding: {reuse:?}");
        };
        assert!(matches!(
            failure.cause.as_ref(),
            build::PreparationFailure::Record(_)
        ));
        let observed = store::inspect_preparation(
            &journal.database,
            stage.id,
            failure.journal.as_ref(),
            read(),
        )
        .expect("actual latest generation/history");
        assert_eq!(observed.record.expect("original history"), original.record);
        assert_eq!(
            gazette_sdk::source::capture_pinned(
                &root,
                original.record.intent.output,
                Default::default()
            )
            .expect("stage preserved")
            .inventory(),
            initial_stage.inventory()
        );
        assert_eq!(fs::read(&result).expect("checker unchanged"), sentinel);
        assert!(!root.join(".git").exists());
    }
}

#[test]
fn forged_typed_destination_fails_independently_of_unchanged_evidence_and_output() {
    let fixture = published_fixture(&[]);
    let observation = private_directory();
    let result = observation.path().join("checker-input.json");
    let (base, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&with_destination(&base, REMOTE, BRANCH));
    let rendered = render(&inputs).expect("retained original output");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let stage = request(
        journal_root.path(),
        journal.header,
        parent.path(),
        "destination",
    );
    let original =
        build::prepare_stage(&rendered, stage, options()).expect("real original preparation");
    let checked = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        process_options(),
        Default::default(),
        None,
    )
    .expect("actual matching checker invocation");
    let changed_inputs = acquire(&with_destination(&base, REMOTE, "forged-private-branch"));
    let mut forged = original.record.clone();
    forged.intent.destination = changed_inputs
        .compilation()
        .workspace
        .destination(&"site:one".parse().expect("site"))
        .cloned();
    assert_ne!(
        forged.intent.destination,
        original.record.intent.destination
    );
    assert_eq!(forged.intent.evidence, original.record.intent.evidence);
    assert_eq!(
        forged.intent.evidence_sha256,
        original.record.intent.evidence_sha256
    );
    assert_eq!(forged.intent.output, original.record.intent.output);
    let before = fs::read(&journal.database).expect("native database bytes");
    let error = build::record_preparation_check(
        &checked,
        build::PreparationCheckRequest {
            journal: journal_root.path(),
            expected: original.journal,
            preparation: &forged,
        },
        build::PreparationCheckOptions {
            record: options().record,
            writer: options().writer,
            ..Default::default()
        },
    )
    .expect_err("typed destination is independently bound to the retained workspace");
    assert!(matches!(
        error.cause.as_ref(),
        build::PreparationFailure::Mismatch
    ));
    assert!(!error.writer_acquired);
    let observed =
        store::inspect_preparation(&journal.database, stage.id, Some(&original.journal), read())
            .expect("unchanged original generation");
    assert_eq!(observed.record.expect("unchanged history"), original.record);
    assert_eq!(
        fs::read(&journal.database).expect("unchanged database bytes"),
        before
    );
    assert!(!original.record.intent.address.path().join(".git").exists());
}
