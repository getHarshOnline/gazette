//! Fresh enrollment stays usable through real preparation and subsequent native observations.

use super::*;
use gazette_sdk::release;
use std::path::Path;
use store::coordinator::{self, LocalCoordinatorSelection, LocalCoordinatorState};

fn enroll(anchor: &Path, journal: &Path) -> LocalCoordinatorSelection {
    let seal = coordinator::enroll(
        coordinator::EnrollmentRequest {
            anchor_root: anchor,
            journal_root: journal,
            coordinator_id: "preparation-fixture",
            principal_scope: source::Sha256Digest::of(b"local fixture scope"),
            protected_roots: &[],
        },
        options().writer.session,
        true,
    )
    .expect("fresh native enrollment");
    LocalCoordinatorSelection {
        anchor_root: anchor.to_owned(),
        binding: seal.binding,
    }
}

fn clean(selected: &LocalCoordinatorSelection, database: &Path) -> store::JournalHeader {
    let LocalCoordinatorState::Clean { seal, .. } = coordinator::inspect(
        &selected.anchor_root,
        &selected.binding,
        options().writer.session,
    )
    .expect("independent native anchor inspection") else {
        panic!("successful explicit close must leave a clean seal")
    };
    assert_eq!(seal.binding, selected.binding);
    assert_eq!(
        seal.database_sha256,
        source::Sha256Digest::of(&fs::read(database).expect("native database"))
    );
    store::inspect_journal(database, Some(&seal.header), read())
        .expect("same current journal header");
    seal.header
}

fn guarded_input(
    config: &str,
    selected: &LocalCoordinatorSelection,
    header: store::JournalHeader,
    parent: &Path,
) -> Value {
    let mut request = input(config, &selected.binding.journal_root, header, parent);
    request["coordinator"] = json!(selected);
    request
}

#[test]
fn guarded_cli_and_sdk_prepare_complete_sets_then_record_actual_destination_observations() {
    let fixture = published_fixture(&[]);
    for direct in [false, true] {
        let root = private_directory();
        let journal = initialize(root.path());
        let anchor = private_directory();
        let selected = enroll(anchor.path(), root.path());
        let parent = private_directory();
        let remote = private_directory();
        let isolation = private_directory();
        let mut config = fixture.config();
        for site in ["other", "one"] {
            let destination = remote.path().join(format!("{site}.git"));
            git2::Repository::init_bare(&destination).expect("disposable native destination");
            let from = format!("site='site:{site}'\npublic_base_url=");
            assert!(config.contains(&from));
            config = config.replace(&from, &format!("site='site:{site}'\ndestination={{kind='git',remote='{}',branch='publication'}}\npublic_base_url=", destination.display()));
            config = enable_site(config, site);
        }
        let request = guarded_input(&config, &selected, journal.header, parent.path());
        let value = if direct {
            let mut arguments = ARGUMENTS.to_vec();
            arguments.push("--write-coordinator-anchor");
            let (output, value) = result(&request, &arguments);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(value["status"], "local-preparations-complete-not-certified");
            value["sites"].clone()
        } else {
            let mut settings = permitted();
            settings.preparation.allow_anchor_write = true;
            json!(
                recipe(&request)
                    .execute(settings)
                    .expect("guarded SDK preparation")
            )
        };
        let header = clean(&selected, &journal.database);
        assert_eq!(value[1]["preparation"]["journal"], json!(header));
        let selections = ["other", "one"].map(|site| {
            let record = observed(&journal.database, site, header);
            let completion = record
                .worktree
                .as_ref()
                .expect("worktree intent")
                .completion
                .as_ref()
                .expect("native Git completion");
            assert!(parent.path().join(site).join("model.json").is_file());
            release::PreparationSelection {
                output_policy: None,
                qualification: None,
                preparation_id: id(site),
                site: format!("site:{site}").parse().expect("site"),
                domain: format!("{site}.example"),
                commit: completion.report.commit.clone(),
                expected_remote_oid_or_unborn: release::RemoteState::Unborn,
            }
        });
        let batch = release::inspect_preparations(
            release::PreparationBatchRequest {
                database: &journal.database,
                expected: header,
                selections: &selections,
                protected_roots: &[fixture.directory.path().to_owned()],
            },
            release::PreparationBatchOptions {
                inspection: release_commit::JournalWorktreeInspectionOptions {
                    read: read(),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("independent complete-set Git and journal read-back");
        let targets = release::check_targets(batch.targets(), Default::default())
            .expect("same exact native targets");
        assert_eq!(targets.targets().len(), 2);
        let git = release::GitSelection {
            program: crate::native::git().to_owned(),
            isolation: isolation.path().to_path_buf(),
            credentials: None,
        };
        let receipt = release::record_observation(
            &git,
            release::ObservationSessionRequest {
                root: &selected.binding.journal_root,
                expected: header,
                id: source::Sha256Digest::of(b"after preparation"),
                targets: &targets,
                protected_roots: &[
                    remote.path().to_owned(),
                    fixture.directory.path().to_owned(),
                    parent.path().to_owned(),
                ],
                coordinator: Some(&selected),
            },
            release::ObservationSessionOptions {
                writer: options().writer,
                allow_anchor_write: true,
                observation: release::RecordObservationOptions {
                    read: release::RemoteReadOptions {
                        allow_read: true,
                        ..Default::default()
                    },
                    record: release::RefObservationWriteOptions { allow_record: true },
                },
            },
        )
        .expect("same selected guard remains usable after every preparation phase");
        assert_eq!(clean(&selected, &journal.database), receipt.journal);
        assert_eq!(receipt.record.targets, targets.targets());
        assert_eq!(receipt.record.readbacks.len(), 2);
        for site in ["other", "one"] {
            assert!(
                observed(&journal.database, site, receipt.journal)
                    .worktree
                    .expect("retained worktree")
                    .completion
                    .is_some()
            );
        }
    }
}

#[test]
fn guarded_preparation_refuses_permission_binding_and_cross_site_anchor_overlap_without_effects() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let anchor = private_directory();
    let selected = enroll(anchor.path(), root.path());
    let parent = private_directory();
    let baseline = fs::read(&journal.database).expect("journal baseline");
    let anchor_bytes = fs::read(anchor.path().join("coordinator.json")).expect("anchor baseline");
    for case in ["permission", "binding", "source", "later-stage"] {
        let mut request =
            guarded_input(&fixture.config(), &selected, journal.header, parent.path());
        let mut settings = permitted();
        settings.preparation.allow_anchor_write = case != "permission";
        match case {
            "binding" => {
                request["coordinator"]["binding"]["journal_epoch"] =
                    json!(journal.header.journal_epoch + 1)
            }
            "source" => request["coordinator"]["anchor_root"] = json!(fixture.directory.path()),
            "later-stage" => request["sites"][1]["stage_parent"] = json!(anchor.path()),
            _ => {}
        }
        let outcome = recipe(&request).execute(settings);
        let error = outcome.expect_err("guarded preflight must refuse");
        let build::LocalPreparationRunError::Before { code, .. } = error else {
            panic!("no native preparation for {case}")
        };
        assert_eq!(
            code,
            if matches!(case, "permission" | "binding") {
                "local_preparation_preflight"
            } else {
                "local_preparation_separation"
            }
        );
        assert_eq!(
            fs::read(&journal.database).expect("journal unchanged"),
            baseline
        );
        assert_eq!(
            fs::read(anchor.path().join("coordinator.json")).expect("anchor unchanged"),
            anchor_bytes
        );
        assert_eq!(
            fs::read_dir(parent.path()).expect("stage parent").count(),
            0
        );
    }
    let request = guarded_input(&fixture.config(), &selected, journal.header, parent.path());
    let (output, _) = result(&request, ARGUMENTS);
    assert!(
        !output.status.success(),
        "CLI must independently require anchor-write permission"
    );
    assert_eq!(clean(&selected, &journal.database), journal.header);
}

#[test]
fn guarded_later_site_failure_retains_prefix_and_interrupted_phase_history() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let anchor = private_directory();
    let selected = enroll(anchor.path(), root.path());
    let parent = private_directory();
    fs::write(parent.path().join("one"), b"preserved unrelated child").expect("fixture conflict");
    let request = guarded_input(&fixture.config(), &selected, journal.header, parent.path());
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--write-coordinator-anchor");
    let (output, value) = result(&request, &arguments);
    assert!(!output.status.success());
    assert_eq!(value["status"], "incomplete");
    assert_eq!(value["requested"], json!(["site:other", "site:one"]));
    assert_eq!(
        value["completed"]
            .as_array()
            .expect("completed prefix")
            .len(),
        1
    );
    assert_eq!(value["failure"]["site"], "site:one");
    let current = store::inspect_journal(&journal.database, None, read())
        .expect("actual failed-phase history")
        .header;
    let LocalCoordinatorState::InProgress {
        previous,
        reserved_generation,
        transaction,
    } = coordinator::inspect(
        &selected.anchor_root,
        &selected.binding,
        options().writer.session,
    )
    .expect("independent interrupted anchor inspection")
    else {
        panic!("failed phase does not silently close or repair its guard")
    };
    assert_eq!(reserved_generation, current.writer_fence);
    assert_eq!(
        transaction
            .expect("failed phase retains its committed transaction expectation")
            .header,
        current
    );
    assert_eq!(
        json!(previous.header),
        value["completed"][0]["preparation"]["journal"]
    );
    assert!(
        current.writer_fence
            > serde_json::from_value::<store::JournalHeader>(
                value["completed"][0]["preparation"]["journal"].clone()
            )
            .expect("prior header")
            .writer_fence
    );
    assert!(
        observed(&journal.database, "other", current)
            .worktree
            .expect("first worktree")
            .completion
            .is_some()
    );
    assert!(
        observed(&journal.database, "one", current)
            .retention
            .is_none()
    );
    assert_eq!(
        fs::read(parent.path().join("one")).expect("preserved child"),
        b"preserved unrelated child"
    );
}

#[test]
fn restored_journal_is_refused_before_new_stage_or_anchor_mutation() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let anchor = private_directory();
    let selected = enroll(anchor.path(), root.path());
    let original = fs::read(&journal.database).expect("disposable initial snapshot");
    let parent = private_directory();
    let mut request = guarded_input(&fixture.config(), &selected, journal.header, parent.path());
    let mut settings = permitted();
    settings.preparation.allow_anchor_write = true;
    recipe(&request)
        .execute(settings)
        .expect("first complete native preparation");
    clean(&selected, &journal.database);
    let control = fs::read(anchor.path().join("coordinator.json")).expect("current anchor");
    fs::write(&journal.database, &original).expect("restore only this disposable fixture");
    let next = private_directory();
    for site in request["sites"].as_array_mut().expect("exact sites") {
        site["stage_parent"] = json!(next.path());
    }
    let result = recipe(&request).execute(settings);
    let report = json!(build::LocalPreparationRunReport::new(&result));
    assert!(result.is_err());
    assert_eq!(report["status"], "incomplete");
    assert_eq!(report["completed"], json!([]));
    assert_eq!(report["failure"]["site"], "site:other");
    assert_eq!(report["failure"]["phase"]["kind"], "stage");
    let build::LocalPreparationRunError::Preparation { error, .. } =
        result.as_ref().expect_err("refused")
    else {
        panic!("preparation failure")
    };
    let build::LocalPreparationSetFailure::Site { error, .. } = error.cause.as_ref() else {
        panic!("original first-site failure")
    };
    let build::LocalPreparationFailure::Stage(error) = error.cause.as_ref() else {
        panic!("original staging failure")
    };
    let build::PreparationFailure::Writer(error) = error.cause.as_ref() else {
        panic!("guarded writer refused")
    };
    assert!(matches!(error.cause, store::Error::CoordinatorMismatch));
    assert_eq!(error.generation, store::GenerationEffect::NotAttempted);
    assert_eq!(
        fs::read(&journal.database).expect("no adopted history"),
        original
    );
    assert_eq!(
        fs::read(anchor.path().join("coordinator.json")).expect("no reset"),
        control
    );
    assert_eq!(fs::read_dir(next.path()).expect("no new stages").count(), 0);
    assert!(
        parent.path().join("other/model.json").is_file(),
        "earlier effects survive"
    );
}

#[test]
fn lost_cli_preparation_report_retains_exact_custody_selection_and_native_history() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let anchor = private_directory();
    let selected = enroll(anchor.path(), root.path());
    let parent = private_directory();
    let request = guarded_input(&fixture.config(), &selected, journal.header, parent.path());
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--write-coordinator-anchor");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(&arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real CLI");
    drop(child.stdout.take().expect("close response carrier"));
    child
        .stdin
        .take()
        .expect("private request carrier")
        .write_all(request.to_string().as_bytes())
        .expect("complete request");
    let output = child.wait_with_output().expect("closed response channel");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no partial success report");
    let diagnostic = String::from_utf8(output.stderr).expect("private diagnostic");
    assert!(diagnostic.contains("do not retry"));
    // A closed response carrier cannot carry the private receipt. The exact original
    // selection is independently recoverable from the selected native anchor and journal.
    let current = clean(&selected, &journal.database);
    assert!(current.writer_fence > journal.header.writer_fence);
    for site in ["other", "one"] {
        assert!(
            observed(&journal.database, site, current)
                .worktree
                .expect("retained Git intent")
                .completion
                .is_some()
        );
    }
}
