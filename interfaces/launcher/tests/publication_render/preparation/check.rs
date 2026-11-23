//! Actual checker/disclosure observations join immutable `SQLite` preparation and native Git history.

use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::PathBuf};

use gazette_sdk::{build, process, release_commit as release, source, store};

use super::super::{
    policy::configured,
    worktree::{Retained, metadata},
};
use super::{Value, acquire, fs, options, published_fixture, read, render};

pub(super) fn disclosure_policy() -> build::DisclosurePolicy {
    use build::{DisclosureClass as Class, DisclosurePattern as Pattern, DisclosureRule};
    build::DisclosurePolicy {
        rules: BTreeMap::from(
            [
                (
                    "rule:private-path",
                    DisclosureRule {
                        class: Class::LocalPath,
                        pattern: Pattern::PathTree {
                            root: "/private/gazette-fixture-do-not-publish".into(),
                        },
                    },
                ),
                (
                    "rule:private-host",
                    DisclosureRule {
                        class: Class::PrivateEndpoint,
                        pattern: Pattern::Host {
                            name: "private-fixture.internal.invalid".into(),
                        },
                    },
                ),
                (
                    "rule:secret",
                    DisclosureRule {
                        class: Class::Secret,
                        pattern: Pattern::Literal {
                            value: "gazette_fixture_secret_982731".into(),
                        },
                    },
                ),
                (
                    "rule:artifact",
                    DisclosureRule {
                        class: Class::GeneratedArtifact,
                        pattern: Pattern::Literal {
                            value: "private-generated-artifact-982731".into(),
                        },
                    },
                ),
                (
                    "rule:claim",
                    DisclosureRule {
                        class: Class::FixtureClaim,
                        pattern: Pattern::Literal {
                            value: "INTERNAL FIXTURE CLAIM 982731".into(),
                        },
                    },
                ),
                (
                    "rule:internal-id",
                    DisclosureRule {
                        class: Class::InternalIdentifier,
                        pattern: Pattern::Literal {
                            value: "internal-record-982731".into(),
                        },
                    },
                ),
            ]
            .map(|(key, rule)| (key.parse().expect("disclosure rule identity"), rule)),
        ),
    }
}

fn execution() -> process::Options {
    process::Options {
        allow_execute: true,
        ..Default::default()
    }
}

fn checks() -> build::PreparationCheckOptions {
    build::PreparationCheckOptions {
        record: options().record,
        writer: options().writer,
        ..Default::default()
    }
}

fn request<'a>(
    stage: &'a Retained,
    receipt: &'a store::PreparationReceipt,
) -> build::PreparationCheckRequest<'a> {
    build::PreparationCheckRequest {
        journal: stage.journal.path(),
        expected: receipt.journal,
        preparation: &receipt.record,
    }
}

fn inspect(stage: &Retained, header: store::JournalHeader) -> store::PreparationRecord {
    store::inspect_preparation(
        &stage.database,
        stage.receipt.record.intent.id,
        Some(&header),
        read(),
    )
    .expect("independent native history")
    .record
    .expect("existing preparation")
}

fn git_options() -> release::JournalWorktreeOptions {
    release::JournalWorktreeOptions {
        record: options().record,
        writer: options().writer,
        worktree: release::WorktreeOptions {
            allow_initialize: true,
            ..Default::default()
        },
    }
}

fn git_request<'a>(
    stage: &'a Retained,
    receipt: &'a store::PreparationReceipt,
    metadata: &'a release::CommitMetadata,
    sources: &'a [PathBuf],
) -> release::JournalWorktreeRequest<'a> {
    release::JournalWorktreeRequest {
        journal: stage.journal.path(),
        expected: receipt.journal,
        preparation: &receipt.record,
        metadata,
        parent_source: None,
        protected_roots: sources,
    }
}

#[test]
fn unselected_output_cannot_start_preparation_checks_even_with_equal_output() {
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("checker observation");
    let result = observation.path().join("received.json");
    let (config, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&config);
    let selected = render(&inputs).expect("explicit selected publication");
    let stage = Retained::new(&selected);
    let unselected = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("unselected yearless private rendering");
    assert!(unselected.selection().is_none());
    assert_eq!(
        unselected.site().output().inventory(),
        selected.site().output().inventory()
    );
    let database = fs::read(&stage.database).expect("database before check refusal");
    let error = build::check_preparation(
        &unselected,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect_err("selection gate precedes disclosure and checker execution");
    assert!(matches!(*error.cause, build::PreparationFailure::Selection));
    assert!(!error.writer_acquired);
    assert!(error.journal.is_none() && error.receipt.is_none());
    assert_eq!(inspect(&stage, stage.receipt.journal), stage.receipt.record);
    assert_eq!(
        fs::read(&stage.database).expect("unchanged native database"),
        database
    );
    assert!(
        !result.exists(),
        "selection refusal must not invoke the checker"
    );
    assert!(!stage.root.join(".git").exists());
}

#[test]
fn native_check_history_replays_without_checker_execution_and_survives_git_inspection_and_recovery()
{
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("disposable checker observation");
    let result = observation.path().join("received.json");
    let (configuration, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("native retained publication");
    let checked = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("required disclosure precedes actual zero-exit checker");
    let delivered = fs::read(&result).expect("actual checker request before replay sentinel");
    let delivered_fields: BTreeMap<String, &serde_json::value::RawValue> =
        serde_json::from_slice(&delivered).expect("borrow exact delivered report bytes");
    let delivered_disclosure = delivered_fields
        .get("disclosure")
        .expect("required disclosure delivered to checker")
        .get();
    assert_eq!(
        checked
            .policy()
            .expect("configured checker")
            .invocation()
            .request_sha256,
        source::Sha256Digest::of(&delivered),
        "invocation binds the complete request including disclosure"
    );
    let stage = Retained::new(&rendered);
    let original_fields: BTreeMap<String, &serde_json::value::RawValue> =
        serde_json::from_str(&stage.receipt.record.intent.evidence)
            .expect("borrow exact original preparation evidence");
    assert_eq!(
        delivered_fields["repository_source_provenance"].get(),
        original_fields["repository_source_provenance"].get(),
        "checker and original durable preparation bind the same repository source provenance"
    );
    let repository_source: Value =
        serde_json::from_str(delivered_fields["repository_source_provenance"].get())
            .expect("actual embedded repository source provenance");
    assert_eq!(
        repository_source["profile"],
        "gazette-repository-source-provenance/1"
    );
    assert!(
        repository_source["sha256"]
            .as_str()
            .expect("source digest")
            .parse::<source::Sha256Digest>()
            .is_ok()
    );
    assert!(
        repository_source["files"]
            .as_u64()
            .expect("source file count")
            > 0
    );
    assert!(
        repository_source["bytes"]
            .as_u64()
            .expect("source byte count")
            > 0
    );
    let sentinel = b"record/replay must not execute checker";
    fs::write(&result, sentinel).expect("fresh no-execution sentinel");
    let recorded =
        build::record_preparation_check(&checked, request(&stage, &stage.receipt), checks())
            .expect("actual immutable check append");
    assert!(recorded.inserted);
    let check = recorded.record.check.as_ref().expect("durable check row");
    assert_eq!(check.intent_id, stage.receipt.record.intent.id);
    assert_eq!(check.recorded, recorded.journal);
    assert_eq!(
        check.evidence_sha256,
        source::Sha256Digest::of(check.evidence.as_bytes())
    );
    let evidence: Value =
        serde_json::from_str(&check.evidence).expect("retained complete evidence JSON");
    let retained_fields: BTreeMap<String, &serde_json::value::RawValue> =
        serde_json::from_str(&check.evidence).expect("borrow exact durable report bytes");
    assert_eq!(
        retained_fields["disclosure"].get(),
        delivered_disclosure,
        "durable evidence must retain the exact report delivered to the checker"
    );
    assert_eq!(
        evidence["policy"]["evidence"]["disclosure_sha256"],
        serde_json::to_value(source::Sha256Digest::of(delivered_disclosure.as_bytes()))
            .expect("digest of actual delivered report")
    );
    assert_eq!(evidence["format"], "gazette.preparation-check/1");
    assert_eq!(
        evidence["preparation_evidence_sha256"],
        serde_json::to_value(stage.receipt.record.intent.evidence_sha256)
            .expect("original evidence digest")
    );
    assert_eq!(
        evidence["policy"]["evidence"],
        serde_json::to_value(checked.policy().expect("configured checker"))
            .expect("actual output invocation")
    );
    assert_eq!(
        evidence["disclosure"],
        serde_json::to_value(checked.disclosure()).expect("same retained disclosure")
    );
    assert_eq!(
        evidence["disclosure"]["status"],
        "required-supported-profile-evidence"
    );
    assert_eq!(
        evidence["policy"]["evidence"]["status"],
        "output-policy-check-passed-unverified"
    );
    assert_eq!(evidence["policy"]["status"], "checked");
    assert_eq!(inspect(&stage, recorded.journal), recorded.record);
    let replay = build::record_preparation_check(&checked, request(&stage, &recorded), checks())
        .expect("exact replay preserves original check coordinates");
    assert!(!replay.inserted);
    assert_eq!(replay.record, recorded.record);
    assert!(replay.journal.writer_fence > recorded.journal.writer_fence);
    assert_eq!(fs::read(&result).expect("no repeated subprocess"), sentinel);
    let mut altered = replay.record.clone();
    altered
        .check
        .as_mut()
        .expect("previously recorded check")
        .recorded
        .writer_fence += 1;
    let mismatch = build::record_preparation_check(
        &checked,
        build::PreparationCheckRequest {
            journal: stage.journal.path(),
            expected: replay.journal,
            preparation: &altered,
        },
        checks(),
    )
    .expect_err("caller cannot rewrite historical check coordinates");
    assert!(matches!(
        *mismatch.cause,
        build::PreparationFailure::Mismatch
    ));
    let preserved = mismatch
        .receipt
        .expect("native replay receipt preserves the original check");
    assert!(!preserved.inserted);
    assert_eq!(preserved.record, replay.record);
    assert_eq!(inspect(&stage, preserved.journal), replay.record);
    let recipe = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let completed = release::prepare_journal_worktree(
        git_request(&stage, &preserved, &recipe, &sources),
        git_options(),
    )
    .expect("same checked preparation reaches native independent Git worktree");
    assert_eq!(completed.record.check, replay.record.check);
    assert_eq!(
        completed
            .record
            .worktree
            .as_ref()
            .expect("checked Git recipe")
            .check_sha256,
        Some(check.evidence_sha256)
    );
    let commit = completed
        .record
        .worktree
        .as_ref()
        .expect("Git intent")
        .completion
        .as_ref()
        .expect("native completion")
        .report
        .commit
        .clone();
    let joined = release::inspect_journal_worktree(
        release::JournalWorktreeInspectionRequest {
            database: &stage.database,
            preparation_id: completed.record.intent.id,
            expected_header: Some(&completed.journal),
            commit: &commit,
            protected_roots: &sources,
        },
        release::JournalWorktreeInspectionOptions {
            read: read(),
            ..Default::default()
        },
    )
    .expect("current Git/output inspection joined with native check history");
    assert_eq!(
        joined.history.record.expect("stored history"),
        completed.record
    );
    let recovered = release::recover_journal_worktree(
        release::RecoverWorktreeRequest {
            journal: stage.journal.path(),
            expected: completed.journal,
            preparation: &completed.record,
            commit: &commit,
            protected_roots: &sources,
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
    .expect("native synchronization/recovery preserves original check and completion");
    assert_eq!(recovered.record, completed.record);
    assert_eq!(
        fs::read(&result).expect("Git/recovery never invokes policy"),
        sentinel
    );
}

#[test]
fn same_output_with_different_configuration_cannot_borrow_preparation_check_identity() {
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("checker result");
    let result = observation.path().join("received.json");
    let (configuration, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("original rendering");
    let stage = Retained::new(&rendered);
    let changed_inputs = acquire(&format!(
        "# distinct captured configuration\n{configuration}"
    ));
    let changed =
        render(&changed_inputs).expect("same visible output, different acquisition evidence");
    assert_eq!(
        changed.site().output().inventory().identity(),
        rendered.site().output().inventory().identity()
    );
    assert_ne!(
        changed_inputs.compilation().source_sha256,
        inputs.compilation().source_sha256
    );
    let checked = build::check_preparation(
        &changed,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("actual differently bound output check");
    let database = fs::read(&stage.database).expect("before mismatch");
    let error =
        build::record_preparation_check(&checked, request(&stage, &stage.receipt), checks())
            .expect_err("equal output bytes cannot erase configuration/capture identity");
    assert!(matches!(*error.cause, build::PreparationFailure::Mismatch));
    assert!(!error.writer_acquired);
    assert_eq!(inspect(&stage, stage.receipt.journal), stage.receipt.record);
    assert_eq!(
        fs::read(&stage.database).expect("no writer effect"),
        database
    );
}

#[test]
fn configured_check_cannot_be_injected_into_generic_preparation_with_equal_output() {
    let fixture = published_fixture(&[]);
    let configuration = fixture.config();
    let generic_inputs = acquire(&configuration);
    let generic = render(&generic_inputs).expect("native generic publication");
    assert!(
        generic_inputs
            .compilation()
            .workspace
            .policy_check()
            .is_none()
    );
    let stage = Retained::new(&generic);
    let observation = tempfile::tempdir().expect("native checker observation");
    let (configured, _) = configured(&fixture, &observation.path().join("received.json"), 0);
    let checked_inputs = acquire(&configured);
    let checked_render = render(&checked_inputs).expect("separate configured rendering");
    assert_eq!(
        generic.site().output().inventory().identity(),
        checked_render.site().output().inventory().identity(),
        "a private checker configuration does not change publication bytes"
    );
    let checked = build::check_preparation(
        &checked_render,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("genuine configured checker result");
    let before = fs::read(&stage.database).expect("generic journal before injection");
    let error =
        build::record_preparation_check(&checked, request(&stage, &stage.receipt), checks())
            .expect_err("a check from a configured rendering cannot rewrite generic acquisition");
    assert!(matches!(*error.cause, build::PreparationFailure::Mismatch));
    assert!(!error.writer_acquired);
    assert!(error.receipt.is_none());
    assert_eq!(inspect(&stage, stage.receipt.journal), stage.receipt.record);
    assert_eq!(
        fs::read(&stage.database).expect("no journal effect"),
        before
    );
    assert!(!stage.root.join(".git").exists());
}

#[test]
fn disclosure_failures_precede_checker_execution_and_check_records() {
    for case in ["findings", "unknown", "scan-bound"] {
        let fixture = if case == "unknown" {
            published_fixture(&[(
                "theme/templates/page.html",
                b"<i data-value='data:application/octet-stream;base64,AAE='></i>",
            )])
        } else {
            published_fixture(&[])
        };
        let observation = tempfile::tempdir().expect("checker result");
        let result = observation.path().join("received.json");
        let (configuration, _) = configured(&fixture, &result, 0);
        let inputs = acquire(&configuration);
        let rendered = render(&inputs).expect("complete retained render");
        let stage = Retained::new(&rendered);
        let mut selected_rules = disclosure_policy();
        let mut bounds = build::DisclosureProbeLimits::default();
        match case {
            "findings" => {
                selected_rules.rules.insert(
                    "rule:body".parse().expect("rule"),
                    build::DisclosureRule {
                        class: build::DisclosureClass::Secret,
                        pattern: build::DisclosurePattern::Literal {
                            value: "Retained".into(),
                        },
                    },
                );
            }
            "scan-bound" => bounds.entries = 0,
            _ => (),
        }
        let database = fs::read(&stage.database).expect("original journal bytes");
        let error = build::check_preparation(&rendered, &selected_rules, execution(), bounds, None)
            .expect_err("failed prerequisite never records a successful partial check");
        match (case, error.cause.as_ref()) {
            (
                "findings",
                build::PreparationFailure::Disclosure(build::DisclosureProbeError::Findings {
                    findings,
                }),
            ) => assert!(!findings.is_empty()),
            (
                "unknown",
                build::PreparationFailure::Disclosure(build::DisclosureProbeError::Unsupported {
                    ..
                }),
            ) => (),
            (
                "scan-bound",
                build::PreparationFailure::Disclosure(build::DisclosureProbeError::Limit(_)),
            ) => (),
            _ => panic!("unexpected prerequisite failure for {case}: {error:?}"),
        }
        assert!(!error.writer_acquired);
        assert!(
            !result.exists(),
            "{case} must stop before checker execution"
        );
        assert_eq!(inspect(&stage, stage.receipt.journal), stage.receipt.record);
        assert_eq!(
            fs::read(&stage.database).expect("unchanged journal"),
            database
        );
        assert!(!stage.root.join(".git").exists());
    }
}

#[test]
fn complete_check_evidence_bound_precedes_writer_and_does_not_repeat_checker() {
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("checker result");
    let result = observation.path().join("received.json");
    let (configuration, _) = configured(&fixture, &result, 0);
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("complete retained render");
    let checked = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("complete required scan and checker invocation");
    let stage = Retained::new(&rendered);
    let database = fs::read(&stage.database).expect("original journal bytes");
    let sentinel = b"evidence encoding cannot rerun checker";
    fs::write(&result, sentinel).expect("independent checker sentinel");
    let error = build::record_preparation_check(
        &checked,
        request(&stage, &stage.receipt),
        build::PreparationCheckOptions {
            evidence_bytes: 0,
            ..checks()
        },
    )
    .expect_err("incomplete evidence must not advance check history");
    assert!(matches!(
        error.cause.as_ref(),
        build::PreparationFailure::Evidence(gazette_sdk::render::Error::Limit(_))
    ));
    assert!(!error.writer_acquired);
    assert_eq!(inspect(&stage, stage.receipt.journal), stage.receipt.record);
    assert_eq!(
        fs::read(&stage.database).expect("unchanged journal"),
        database
    );
    assert_eq!(fs::read(&result).expect("no checker replay"), sentinel);
    assert!(!stage.root.join(".git").exists());
}

#[test]
fn changed_rules_conflict_and_stale_unchecked_record_cannot_absorb_check_at_git_transition() {
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("checker result");
    let (configuration, _) = configured(&fixture, &observation.path().join("received.json"), 0);
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("retained rendering");
    let checked = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("actual required check and output checker");
    let stage = Retained::new(&rendered);
    let recorded =
        build::record_preparation_check(&checked, request(&stage, &stage.receipt), checks())
            .expect("first rule binding");
    let mut changed = disclosure_policy();
    changed
        .rules
        .get_mut(&"rule:secret".parse().expect("existing rule"))
        .expect("secret rule")
        .pattern = build::DisclosurePattern::Literal {
        value: "gazette_changed_fixture_secret_583290".into(),
    };
    let changed_check =
        build::check_preparation(&rendered, &changed, execution(), Default::default(), None)
            .expect("changed rules require a new scan and matching checker invocation");
    assert_ne!(
        checked
            .policy()
            .expect("original checker")
            .invocation()
            .request_sha256,
        changed_check
            .policy()
            .expect("changed checker")
            .invocation()
            .request_sha256,
        "changed rules must reach the checker request identity"
    );
    let error =
        build::record_preparation_check(&changed_check, request(&stage, &recorded), checks())
            .expect_err("different rule evidence conflicts at the immutable check coordinate");
    assert!(matches!(
        error.cause.as_ref(),
        build::PreparationFailure::Record(_)
    ));
    let header = error
        .journal
        .expect("observed acquired conflict generation");
    assert_eq!(inspect(&stage, header), recorded.record);
    let recipe = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let mut stale = git_request(&stage, &stage.receipt, &recipe, &sources);
    stale.expected = header;
    let error = release::prepare_journal_worktree(stale, git_options())
        .expect_err("current generation does not make an unchecked prior record current");
    assert!(matches!(
        error.cause.as_ref(),
        release::JournalWorktreeFailure::Record(_) | release::JournalWorktreeFailure::Mismatch
    ));
    assert_eq!(
        inspect(&stage, error.journal.expect("current refusal generation")),
        recorded.record
    );
    assert!(
        !stage.root.join(".git").exists(),
        "check mismatch must precede Git initialization"
    );
}

#[test]
fn check_cannot_be_attached_retroactively_after_native_git_preparation() {
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("checker result");
    let (configuration, _) = configured(&fixture, &observation.path().join("received.json"), 0);
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("retained rendering");
    let stage = Retained::new(&rendered);
    let recipe = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let completed = release::prepare_journal_worktree(
        git_request(&stage, &stage.receipt, &recipe, &sources),
        git_options(),
    )
    .expect("existing unchecked local Git mechanism");
    assert!(completed.record.check.is_none());
    let checked = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("later native checker observation");
    let error = build::record_preparation_check(&checked, request(&stage, &completed), checks())
        .expect_err("a later check cannot rewrite the meaning of the original Git preparation");
    assert!(matches!(
        error.cause.as_ref(),
        build::PreparationFailure::Record(_)
    ));
    let stored = inspect(&stage, error.journal.expect("acquired refusal generation"));
    assert_eq!(stored, completed.record);
    assert!(stored.check.is_none());
}

#[test]
fn checked_immutable_render_is_not_current_native_stage_custody() {
    let fixture = published_fixture(&[]);
    let observation = tempfile::tempdir().expect("checker result");
    let (configuration, _) = configured(&fixture, &observation.path().join("received.json"), 0);
    let inputs = acquire(&configuration);
    let rendered = render(&inputs).expect("retained rendering");
    let checked = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution(),
        Default::default(),
        None,
    )
    .expect("actual required scan and immutable output check");
    let stage = Retained::new(&rendered);
    let changed = stage.root.join("guide/index.html");
    fs::set_permissions(&changed, fs::Permissions::from_mode(0o600))
        .expect("disposable staged file mutation permission");
    fs::write(&changed, b"CHANGED STAGED BYTES").expect("native stage drift");
    let recorded =
        build::record_preparation_check(&checked, request(&stage, &stage.receipt), checks())
            .expect("history checks immutable rendered owner, not a misleading live-stage read");
    let recipe = metadata();
    let sources = [fixture.directory.path().to_owned()];
    let error = release::prepare_journal_worktree(
        git_request(&stage, &recorded, &recipe, &sources),
        git_options(),
    )
    .expect_err("existing Git preparation still rejects changed complete native stage");
    assert!(matches!(
        error.cause.as_ref(),
        release::JournalWorktreeFailure::Worktree(_)
    ));
    assert!(!stage.root.join(".git").exists());
    assert_eq!(
        fs::read(&changed).expect("no hidden restage/repair"),
        b"CHANGED STAGED BYTES"
    );
    let stored = inspect(
        &stage,
        error.journal.expect("native-failure journal coordinates"),
    );
    assert_eq!(stored.check, recorded.record.check);
    assert!(
        stored
            .worktree
            .expect("durable attempted Git recipe")
            .completion
            .is_none()
    );
}
