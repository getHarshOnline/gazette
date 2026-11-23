//! One local application operation composes existing native custody phases, not approval.

use gazette_sdk::{process, release_commit as release};

use super::super::{bytes, policy::configured, worktree::metadata};
use super::check::disclosure_policy;
use super::{
    Value, acquire, build, fs, initialize, options, private_directory, published_fixture, read,
    render, request, source, store, unchanged,
};

fn permitted() -> build::LocalPreparationOptions {
    build::LocalPreparationOptions {
        allow_anchor_write: false,
        stage: options(),
        policy: process::Options {
            allow_execute: true,
            ..Default::default()
        },
        disclosure: Default::default(),
        worktree: release::WorktreeOptions {
            allow_initialize: true,
            ..Default::default()
        },
    }
}

#[test]
fn standalone_local_preparation_records_absent_policy_and_real_checked_git_custody() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("complete retained publication");
    let output = rendered.site().output();
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let recipe = metadata();
    let rules = disclosure_policy();
    let mut operation = permitted();
    operation.policy.allow_execute = false;
    let completed = build::prepare_local(
        &rendered,
        build::LocalPreparationRequest {
            output_policy: None,
            qualification: None,
            coordinator: None,
            stage: request(journal_root.path(), journal.header, parent.path(), "local"),
            metadata: &recipe,
            parent_source: None,
            disclosure: &rules,
        },
        operation,
    )
    .expect("no configured checker requires no execution permission");
    assert_eq!(
        completed.record.intent.output,
        output.inventory().identity()
    );
    assert!(completed.record.retention.is_some());
    let check = completed
        .record
        .check
        .as_ref()
        .expect("required local check");
    let evidence: Value = serde_json::from_str(&check.evidence).expect("actual stored evidence");
    assert_eq!(evidence["policy"]["status"], "not-configured");
    assert_eq!(
        evidence["disclosure"]["status"],
        "required-supported-profile-evidence"
    );
    let worktree = completed
        .record
        .worktree
        .as_ref()
        .expect("durable Git intent");
    assert_eq!(worktree.check_sha256, Some(check.evidence_sha256));
    let commit = &worktree
        .completion
        .as_ref()
        .expect("native Git completion")
        .report
        .commit;
    let root = completed.record.intent.address.path();
    for entry in output.inventory().entries() {
        if let source::Content::File { .. } = entry.content {
            assert_eq!(
                fs::read(root.join(entry.path.as_str())).expect("actual staged output file"),
                output.file(&entry.path).expect("retained output file")
            );
        }
    }
    assert_eq!(
        fs::read(root.join("guide/index.html")).expect("human page"),
        bytes(output, "guide/index.html")
    );
    assert_eq!(
        fs::read(root.join("model.json")).expect("machine model"),
        bytes(output, "model.json")
    );
    let protected = [fixture.directory.path().to_owned()];
    let inspected = release::inspect_journal_worktree(
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
    .expect("independent native history, Git and complete output agree");
    assert_eq!(
        inspected.history.record.expect("native record"),
        completed.record
    );
    let recovered = release::recover_journal_worktree(
        release::RecoverWorktreeRequest {
            journal: journal_root.path(),
            expected: completed.journal,
            preparation: &completed.record,
            commit,
            protected_roots: &protected,
        },
        release::RecoverWorktreeOptions {
            record: options().record,
            writer: options().writer,
            worktree: release::WorktreeSyncOptions {
                allow_synchronize: true,
                ..Default::default()
            },
        },
    )
    .expect("same existing native recovery path");
    assert_eq!(recovered.record, completed.record);
    assert_eq!(
        rendered.site().output().inventory().identity(),
        output.inventory().identity()
    );
}

#[test]
fn configured_local_preparation_consumes_retained_bytes_once_and_replay_never_recreates_stage() {
    let fixture = published_fixture(&[]);
    let observation = private_directory();
    let result = observation.path().join("checker-input.json");
    let (config, command) = configured(&fixture, &result, 0);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("native retained render");
    let output = rendered.site().output();
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "NOT REACQUIRED",
    )
    .expect("later native source edit");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "NOT RERENDERED",
    )
    .expect("later native Theme edit");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let recipe = metadata();
    let rules = disclosure_policy();
    let stage = request(journal_root.path(), journal.header, parent.path(), "local");
    let completed = build::prepare_local(
        &rendered,
        build::LocalPreparationRequest {
            output_policy: None,
            qualification: None,
            coordinator: None,
            stage,
            metadata: &recipe,
            parent_source: None,
            disclosure: &rules,
        },
        permitted(),
    )
    .expect("configured real subprocess and complete local preparation");
    let delivered = fs::read(&result).expect("actual checker stdin observation");
    let input: Value = serde_json::from_slice(&delivered).expect("native output request");
    assert_eq!(input["format"], "gazette-output-policy/1");
    assert_eq!(
        input["inventory"],
        serde_json::to_value(output.inventory()).expect("retained inventory")
    );
    assert_eq!(
        input["render"],
        serde_json::to_value(rendered.site().evidence()).expect("retained rendering evidence")
    );
    let evidence: Value = serde_json::from_str(
        &completed
            .record
            .check
            .as_ref()
            .expect("stored check")
            .evidence,
    )
    .expect("complete check evidence");
    assert_eq!(evidence["policy"]["status"], "checked");
    assert_eq!(
        evidence["policy"]["evidence"]["status"],
        "output-policy-check-passed-unverified"
    );
    assert_eq!(
        evidence["policy"]["evidence"]["invocation"]["request_sha256"],
        source::Sha256Digest::of(&delivered).to_string()
    );
    assert_eq!(
        evidence["policy"]["evidence"]["invocation"]["command_sha256"],
        source::Sha256Digest::of(&serde_json::to_vec(&command).expect("native command encoding"))
            .to_string()
    );
    let root = completed.record.intent.address.path();
    assert_eq!(
        fs::read(root.join("guide/index.html")).expect("actual page"),
        bytes(output, "guide/index.html")
    );
    let sentinel = b"no automatic checker retry";
    fs::write(&result, sentinel).expect("observer sentinel");
    // Only this operation's disposable staged tree is removed; replay must not recreate it.
    fs::remove_dir_all(&root).expect("dispose exact owned stage");
    let replay = build::prepare_local(
        &rendered,
        build::LocalPreparationRequest {
            output_policy: None,
            qualification: None,
            coordinator: None,
            stage: build::PreparationRequest {
                expected: completed.journal,
                ..stage
            },
            metadata: &recipe,
            parent_source: None,
            disclosure: &rules,
        },
        permitted(),
    )
    .expect_err("historical stage intent is not permission to recreate or rerun");
    let build::LocalPreparationFailure::Stage(error) = replay.cause.as_ref() else {
        panic!("exact replay must stop at existing stage history: {replay:?}");
    };
    assert!(matches!(
        error.cause.as_ref(),
        build::PreparationFailure::Replay
    ));
    let current = error.journal.expect("observed replay generation");
    let history = store::inspect_preparation(&journal.database, stage.id, Some(&current), read())
        .expect("historical record remains readable");
    assert_eq!(
        history.record.expect("historical completion"),
        completed.record
    );
    assert!(!root.exists());
    assert_eq!(
        fs::read(&result).expect("no subprocess repetition"),
        sentinel
    );
}

#[test]
fn later_phase_permissions_and_recipe_fail_before_stage_or_writer_advancement() {
    for case in [
        "git-permission",
        "process-permission",
        "recipe",
        "parent-overlap",
        "empty-rules",
        "missing-class",
        "depth",
    ] {
        let fixture = published_fixture(&[]);
        let observation = private_directory();
        let result = observation.path().join("must-not-exist.json");
        let (config, _) = configured(&fixture, &result, 0);
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("real publication");
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let database = fs::read(&journal.database).expect("initial database bytes");
        let parent = private_directory();
        let stage = request(journal_root.path(), journal.header, parent.path(), "local");
        let mut recipe = metadata();
        let mut operation = permitted();
        let mut parent_source = None;
        let mut rules = disclosure_policy();
        match case {
            "git-permission" => operation.worktree.allow_initialize = false,
            "process-permission" => operation.policy.allow_execute = false,
            "recipe" => recipe.message = "invalid\0message".into(),
            "parent-overlap" => {
                recipe.parent = Some(fixture.commit.to_string().parse().expect("exact valid OID"));
                parent_source = Some(release::ParentSource::Bare {
                    path: parent.path().to_owned(),
                });
            }
            "empty-rules" => rules.rules.clear(),
            "missing-class" => {
                rules
                    .rules
                    .remove(&"rule:secret".parse().expect("secret rule"));
            }
            "depth" => operation.disclosure.encoded_depth = 33,
            _ => unreachable!(),
        }
        let error = build::prepare_local(
            &rendered,
            build::LocalPreparationRequest {
                output_policy: None,
                qualification: None,
                coordinator: None,
                stage,
                metadata: &recipe,
                parent_source: parent_source.as_ref(),
                disclosure: &rules,
            },
            operation,
        )
        .expect_err("known later phase refusal must precede the first write");
        match (case, error.cause.as_ref()) {
            (
                "process-permission",
                build::LocalPreparationFailure::Policy(build::PolicyCheckError::Refused(_)),
            ) => {}
            ("git-permission", build::LocalPreparationFailure::Worktree(failure)) => {
                assert!(matches!(
                    failure.cause.as_ref(),
                    release::JournalWorktreeFailure::Permission
                ))
            }
            ("recipe", build::LocalPreparationFailure::Worktree(failure)) => assert!(matches!(
                failure.cause.as_ref(),
                release::JournalWorktreeFailure::Recipe(_)
            )),
            ("parent-overlap", build::LocalPreparationFailure::Stage(failure)) => {
                assert!(matches!(
                    failure.cause.as_ref(),
                    build::PreparationFailure::Separation(_)
                ))
            }
            ("empty-rules" | "missing-class", build::LocalPreparationFailure::Checks(failure)) => {
                assert!(matches!(
                    failure.cause.as_ref(),
                    build::PreparationFailure::Disclosure(build::DisclosureProbeError::Policy(_))
                ))
            }
            ("depth", build::LocalPreparationFailure::Checks(failure)) => assert!(matches!(
                failure.cause.as_ref(),
                build::PreparationFailure::Disclosure(build::DisclosureProbeError::Limit(
                    "encoded depth configuration"
                ))
            )),
            _ => panic!("wrong preflight phase for {case}: {error:?}"),
        }
        assert!(error.receipt.is_none(), "{case}: no phase completed");
        unchanged(&journal, stage.id, &database);
        assert!(!parent.path().join("local").exists(), "{case}");
        assert!(!result.exists(), "{case}: no native checker");
    }
}

#[test]
fn late_git_failure_retains_prior_check_and_newer_native_intent_without_completion() {
    let fixture = published_fixture(&[]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("real retained publication");
    let journal_root = private_directory();
    let journal = initialize(journal_root.path());
    let parent = private_directory();
    let recipe = metadata();
    let stage = request(journal_root.path(), journal.header, parent.path(), "local");
    let mut operation = permitted();
    operation.worktree.storage_bytes = 1;
    let error = build::prepare_local(
        &rendered,
        build::LocalPreparationRequest {
            output_policy: None,
            qualification: None,
            coordinator: None,
            stage,
            metadata: &recipe,
            parent_source: None,
            disclosure: &disclosure_policy(),
        },
        operation,
    )
    .expect_err("real post-commit synchronization budget failure is not overall success");
    let prior = error.receipt.as_ref().expect("completed check phase");
    let check = prior.record.check.as_ref().expect("durable check survived");
    assert!(prior.record.worktree.is_none());
    let build::LocalPreparationFailure::Worktree(failure) = error.cause.as_ref() else {
        panic!("expected native Git phase failure: {error:?}");
    };
    assert!(matches!(
        failure.cause.as_ref(),
        release::JournalWorktreeFailure::Worktree(_)
    ));
    assert!(failure.writer_acquired);
    let release::WorktreeEffect::CommitCreated(commit) = &failure.effect else {
        panic!("must retain actually created native OID: {failure:?}");
    };
    let later = failure
        .receipt
        .as_ref()
        .expect("newer durable worktree intent");
    assert!(later.journal.writer_fence > prior.journal.writer_fence);
    assert_eq!(failure.journal, Some(later.journal));
    assert_eq!(later.record.check, prior.record.check);
    let intent = later
        .record
        .worktree
        .as_ref()
        .expect("intent recorded before native Git");
    assert_eq!(intent.check_sha256, Some(check.evidence_sha256));
    assert_eq!(intent.metadata, recipe);
    assert!(intent.completion.is_none());
    let root = later.record.intent.address.path();
    let protected = [
        fixture.directory.path().to_owned(),
        journal_root.path().to_owned(),
    ];
    let observed = release::inspect_worktree(
        release::WorktreeRequest {
            root: &root,
            expected: later.record.intent.output,
            metadata: &recipe,
            parent_source: None,
            protected_roots: &protected,
        },
        commit,
        Default::default(),
    )
    .expect("independent real Git/output inspection after failure");
    assert_eq!(observed.output, *rendered.site().output().inventory());
    let history =
        store::inspect_preparation(&journal.database, stage.id, Some(&later.journal), read())
            .expect("read-only inspection does not manufacture completion");
    assert_eq!(history.record.expect("latest native record"), later.record);
}

#[test]
fn rejected_checker_or_disclosure_preserves_retained_stage_without_git() {
    for case in ["checker", "disclosure"] {
        let fixture = published_fixture(&[]);
        let observation = private_directory();
        let result = observation.path().join("actual-input.json");
        let (config, _) = configured(&fixture, &result, if case == "checker" { 7 } else { 0 });
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("real publication");
        let journal_root = private_directory();
        let journal = initialize(journal_root.path());
        let parent = private_directory();
        let stage = request(journal_root.path(), journal.header, parent.path(), "local");
        let recipe = metadata();
        let mut rules = disclosure_policy();
        if case == "disclosure" {
            rules.rules.insert(
                "rule:retained".parse().expect("rule"),
                build::DisclosureRule {
                    class: build::DisclosureClass::Secret,
                    pattern: build::DisclosurePattern::Literal {
                        value: "Retained".into(),
                    },
                },
            );
        }
        let error = build::prepare_local(
            &rendered,
            build::LocalPreparationRequest {
                output_policy: None,
                qualification: None,
                coordinator: None,
                stage,
                metadata: &recipe,
                parent_source: None,
                disclosure: &rules,
            },
            permitted(),
        )
        .expect_err("failed later phase preserves rather than rolls back stage");
        match (case, error.cause.as_ref()) {
            (
                "checker",
                build::LocalPreparationFailure::Policy(build::PolicyCheckError::Rejected {
                    ..
                }),
            ) => {}
            ("disclosure", build::LocalPreparationFailure::Checks(failure)) => assert!(
                matches!(failure.cause.as_ref(), build::PreparationFailure::Disclosure(build::DisclosureProbeError::Findings { findings }) if !findings.is_empty())
            ),
            _ => panic!("wrong phase for {case}: {error:?}"),
        }
        let retained = error
            .receipt
            .as_ref()
            .expect("last completed real stage receipt");
        assert!(retained.record.retention.is_some());
        assert!(retained.record.check.is_none());
        assert!(retained.record.worktree.is_none());
        let root = retained.record.intent.address.path();
        assert_eq!(
            fs::read(root.join("guide/index.html")).expect("retained output survives"),
            bytes(rendered.site().output(), "guide/index.html")
        );
        assert!(!root.join(".git").exists());
        let observed = store::inspect_preparation(
            &journal.database,
            stage.id,
            Some(&retained.journal),
            read(),
        )
        .expect("independent unchanged stage history");
        assert_eq!(observed.record.expect("stage history"), retained.record);
        if case == "checker" {
            let delivered: Value =
                serde_json::from_slice(&fs::read(&result).expect("real checker ran"))
                    .expect("delivered native request");
            assert_eq!(delivered["format"], "gazette-output-policy/1");
            assert_eq!(
                delivered["disclosure"]["status"],
                "required-supported-profile-evidence"
            );
        } else {
            assert!(
                !result.exists(),
                "disclosure failure must precede checker execution"
            );
        }
    }
}
