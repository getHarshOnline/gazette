//! Real candidate and output checkers share frozen sources without hidden replay.

#[path = "candidate/absence.rs"]
mod absence;
#[path = "candidate/captured.rs"]
pub(in crate::preparation) mod captured;
#[path = "candidate/evaluating.rs"]
mod evaluating;
#[path = "candidate/evidence.rs"]
mod evidence;
#[path = "candidate/rights.rs"]
mod rights;

use super::*;
use std::io::Read;

const LOG: &str = "GAZETTE_CANDIDATE_FIXTURE_LOG";
const REJECT: &str = "GAZETTE_CANDIDATE_FIXTURE_REJECT";

#[test]
fn checker_child() {
    let Ok(path) = std::env::var(LOG) else {
        return;
    };
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("complete native stdin");
    let request: Value = serde_json::from_slice(&bytes).expect("native request");
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("disposable evidence");
    log.write_all(&bytes).expect("exact request");
    log.write_all(b"\n").expect("request separator");
    log.sync_all().expect("child evidence flush");
    let rejected =
        request["review"]["request"]["site"].as_str() == std::env::var(REJECT).ok().as_deref();
    std::process::exit(if rejected { 23 } else { 0 });
}

fn configuration(
    fixture: &super::super::super::Fixture,
    log: &std::path::Path,
    reject: &str,
) -> String {
    let executable = std::env::current_exe().expect("native checker executable");
    format!(
        "policy_check={{program={:?},arguments=['--exact','preparation::direct::candidate::checker_child'],directory={:?},environment={{{LOG}={:?},{REJECT}={reject:?}}}}}\n{}",
        executable.to_str().expect("executable"),
        log.parent()
            .expect("log parent")
            .to_str()
            .expect("directory"),
        log.to_str().expect("path"),
        fixture.config()
    )
}

fn events(log: &std::path::Path) -> Vec<Value> {
    fs::read_to_string(log)
        .expect("native invocation log")
        .lines()
        .map(|line| serde_json::from_str(line).expect("whole request"))
        .collect()
}

fn executable_pin() -> gazette_sdk::process::ExecutablePin {
    let bytes = fs::read(std::env::current_exe().expect("actual checker executable"))
        .expect("independent selected executable bytes");
    gazette_sdk::process::ExecutablePin {
        bytes: bytes.len() as u64,
        sha256: source::Sha256Digest::of(&bytes),
    }
}

fn pinned_configuration(config: &str, pin: gazette_sdk::process::ExecutablePin) -> String {
    assert_eq!(config.matches("policy_check={program=").count(), 1);
    config.replacen(
        "policy_check={program=",
        &format!(
            "policy_check={{executable={{bytes={},sha256='{}'}},program=",
            pin.bytes, pin.sha256
        ),
        1,
    )
}

fn checked_request(
    config: &str,
    root: &std::path::Path,
    header: store::JournalHeader,
    parent: &std::path::Path,
) -> Value {
    let mut request = input(config, root, header, parent);
    request["candidate_check"] = json!(build::ReviewLimits::default());
    request
}

fn execute(
    request: &Value,
) -> Result<Vec<build::SitePreparationReceipt>, build::LocalPreparationRunError> {
    let mut options = permitted();
    options.preparation.policy.allow_execute = true;
    recipe(request).execute(options)
}

fn reviewed_request(
    config: &str,
    root: &std::path::Path,
    header: store::JournalHeader,
    parent: &std::path::Path,
) -> Value {
    reviewed_request_with_selection(config, root, header, parent, None)
}

fn reviewed_request_with_selection(
    config: &str,
    root: &std::path::Path,
    header: store::JournalHeader,
    parent: &std::path::Path,
    selection: Option<gazette_sdk::authored::Selection>,
) -> Value {
    let mut request = checked_request(config, root, header, parent);
    let sites = [
        "site:one".parse().expect("site"),
        "site:other".parse().expect("site"),
    ];
    let acquired =
        build::acquire_publications(config, &sites, Default::default(), Default::default())
            .expect("same exact complete source union");
    let selection = selection.unwrap_or_else(|| {
        serde_json::from_value(request["selection"].clone()).expect("explicit candidate selection")
    });
    request["selection"] = json!(selection);
    let policy = build::ReviewPolicy {
        id: "policy:synthetic-fixture".parse().expect("policy"),
        revision: "revision:fixture".parse().expect("revision"),
        source_sha256: source::Sha256Digest::of(b"synthetic fixture, never owner approval"),
    };
    let mut responses = serde_json::Map::new();
    for (site, inputs) in acquired.sites() {
        let parsed = inputs
            .as_ref()
            .expect("site sources")
            .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
            .expect("same native parse");
        let selected = parsed
            .resolve(Default::default())
            .expect("same shared meaning")
            .into_selected(selection.clone())
            .expect("same complete selection");
        let review = build::review_selected(&selected, Default::default()).expect("native review");
        let decisions: Vec<_> = review
            .request()
            .surfaces()
            .iter()
            .enumerate()
            .flat_map(|(surface, projection)| {
                projection.uses().map(move |usage| {
                    json!({
                        "surface": surface, "pointer": usage.pointer, "outcome":"allow",
                        "evidence":["evidence:synthetic-fixture"]
                    })
                })
            })
            .collect();
        let response = format!(
            "\r\n{}\n ",
            serde_json::to_string_pretty(&json!({
                "format":"gazette-review-response/1", "request_sha256":review.sha256(),
                "policy":policy, "decisions":decisions,
            }))
            .expect("synthetic response")
        );
        responses.insert(site.to_string(), json!({
            "binding": {"policy":policy,"response_sha256":source::Sha256Digest::of(response.as_bytes())},
            "response_json": response,
        }));
    }
    request["review_responses"] = Value::Object(responses);
    request
}

#[test]
fn supplied_review_matches_derived_default_but_stale_subset_stops_before_checker_or_stage() {
    use super::default_selection;
    use gazette_sdk::publication::content::Disposition;
    for stale in [false, true] {
        let fixture = default_selection::fixture();
        let root = private_directory();
        let journal = initialize(root.path());
        let baseline = fs::read(&journal.database).unwrap();
        let parent = private_directory();
        let log = parent.path().join("derived-review.jsonl");
        let config = configuration(&fixture, &log, "none").replace(
            &fixture.config(),
            &default_selection::configuration(&fixture),
        );
        let derived = default_selection::selection(&config);
        let mut reviewed = derived.clone();
        if stale {
            reviewed.content.insert(
                "content:archived-extra".parse().unwrap(),
                Disposition::Exclude,
            );
        }
        let mut request = reviewed_request_with_selection(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            Some(reviewed),
        );
        request.as_object_mut().unwrap().remove("selection");
        let outcome = execute(&request);
        if stale {
            assert!(
                outcome.is_err(),
                "a valid response for another selection is not approval of this one"
            );
            assert!(
                matches!(
                    &outcome,
                    Err(build::LocalPreparationRunError::Candidate(error))
                        if matches!(&error.cause, build::PolicyCheckError::Response(
                            build::ReviewResponseError::Binding("review request")))
                ),
                "the exact derived review digest must reject the stale subset"
            );
            let report = json!(build::LocalPreparationRunReport::new(&outcome));
            assert_eq!(report["status"], "candidate-failed");
            assert_eq!(
                report["evidence"]["code"],
                "publication_review_response_binding"
            );
            assert_eq!(report["completed"], json!({}));
            assert!(
                !log.exists(),
                "all supplied responses match before the first checker"
            );
            assert_eq!(fs::read(&journal.database).unwrap(), baseline);
            assert!(!parent.path().join("one").exists());
            assert!(!parent.path().join("other").exists());
        } else {
            let receipts = outcome.expect("exact supplied responses bind the derived default");
            assert_eq!(receipts.len(), 2);
            let calls = events(&log);
            assert_eq!(
                calls.len(),
                4,
                "two candidate checks then two output checks"
            );
            let digest = source::Sha256Digest::of(&serde_json::to_vec(&derived).unwrap());
            for call in &calls[..2] {
                assert_eq!(call["format"], "gazette-candidate-policy/1");
                assert_eq!(call["review"]["request"]["selection_sha256"], json!(digest));
                assert!(call["response"].is_object());
            }
        }
    }
}

#[test]
fn late_invalid_or_held_review_stops_the_complete_set_before_checker_or_preparation() {
    for mode in ["coverage", "hold", "policy"] {
        let fixture = published_fixture(&[]);
        let root = private_directory();
        let journal = initialize(root.path());
        let baseline = fs::read(&journal.database).expect("unchanged original database");
        let parent = private_directory();
        let log = parent.path().join("must-not-run.jsonl");
        let config = configuration(&fixture, &log, "none");
        let mut request = reviewed_request(&config, root.path(), journal.header, parent.path());
        let submission = &mut request["review_responses"]["site:other"];
        let mut response: Value =
            serde_json::from_str(submission["response_json"].as_str().expect("raw response"))
                .expect("synthetic response");
        match mode {
            "coverage" => {
                response["decisions"]
                    .as_array_mut()
                    .expect("decisions")
                    .pop();
            }
            "hold" => response["decisions"][0]["outcome"] = json!("hold"),
            "policy" => submission["binding"]["policy"]["revision"] = json!("revision:other"),
            _ => unreachable!(),
        }
        let bytes = serde_json::to_string(&response).expect("changed exact response");
        submission["binding"]["response_sha256"] =
            json!(source::Sha256Digest::of(bytes.as_bytes()));
        submission["response_json"] = json!(bytes);
        let outcome = execute(&request);
        assert!(outcome.is_err(), "{mode} must stop, never repair selection");
        let report = json!(build::LocalPreparationRunReport::new(&outcome));
        assert_eq!(report["status"], "candidate-failed", "{report}");
        assert_eq!(report["site"], "site:other");
        assert_eq!(report["completed"], json!({}));
        assert!(
            !log.exists(),
            "late invalid review cannot run the first checker"
        );
        for site in ["one", "other"] {
            unchanged(&journal, id(site), &baseline);
            assert!(!parent.path().join(site).exists());
        }
    }
}

#[test]
fn exact_candidate_set_precedes_output_checks_and_is_bound_into_durable_evidence() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("checks.jsonl");
    let pin = executable_pin();
    let config = pinned_configuration(&configuration(&fixture, &log, "none"), pin);
    let compiled = build::acquire_publications(
        &config,
        &["site:one".parse().expect("site")],
        Default::default(),
        Default::default(),
    )
    .expect("declared pin survives acquisition");
    let command_digest = source::Sha256Digest::of(
        &serde_json::to_vec(
            compiled
                .configuration()
                .policy_command()
                .expect("bound policy command")
                .expect("configured command"),
        )
        .expect("exact pinned command"),
    );
    let request = reviewed_request(&config, root.path(), journal.header, parent.path());
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--allow-policy-execution");
    let (output, report) = result(&request, &arguments);
    assert!(output.status.success(), "{output:?}");
    let calls = events(&log);
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0]["format"], "gazette-candidate-policy/1");
    assert_eq!(calls[0]["review"]["request"]["site"], "site:one");
    assert_eq!(calls[1]["review"]["request"]["site"], "site:other");
    assert_eq!(calls[1]["format"], "gazette-candidate-policy/1");
    for call in &calls[2..] {
        assert_eq!(call["format"], "gazette-output-policy/1");
    }
    let selected: gazette_sdk::authored::Selection =
        serde_json::from_value(request["selection"].clone()).expect("native selection");
    let selection = source::Sha256Digest::of(&serde_json::to_vec(&selected).expect("selection"));
    for candidate in &calls[..2] {
        assert_eq!(
            candidate["review"]["request"]["selection_sha256"],
            json!(selection)
        );
        let site = candidate["review"]["request"]["site"]
            .as_str()
            .expect("site");
        assert_eq!(
            candidate["response"]["response_json"],
            request["review_responses"][site]["response_json"]
        );
    }
    for site in report["sites"].as_array().expect("full set") {
        let preparation_id =
            serde_json::from_value(site["preparation"]["record"]["intent"]["id"].clone())
                .expect("exact ID");
        let record = store::inspect_preparation(&journal.database, preparation_id, None, read())
            .expect("independent native read-back")
            .record
            .expect("durable record");
        assert_eq!(json!(record), site["preparation"]["record"]);
        let evidence: Value =
            serde_json::from_str(&record.intent.evidence).expect("private evidence");
        assert_eq!(
            evidence["candidate_check"]["invocation"]["command_sha256"],
            json!(command_digest)
        );
        let checked: Value = serde_json::from_str(
            &record
                .check
                .as_ref()
                .expect("retained output check")
                .evidence,
        )
        .expect("exact check evidence");
        assert_eq!(
            checked["policy"]["evidence"]["invocation"]["command_sha256"],
            json!(command_digest)
        );
        let call = calls[..2]
            .iter()
            .find(|call| call["review"]["request"]["site"] == site["site"])
            .expect("own candidate");
        let raw = fs::read_to_string(&log)
            .expect("exact delivered bytes")
            .lines()
            .find(|line| serde_json::from_str::<Value>(line).expect("request") == *call)
            .expect("candidate bytes")
            .to_owned();
        assert_eq!(
            evidence["candidate_check"]["invocation"]["request_sha256"],
            json!(source::Sha256Digest::of(raw.as_bytes()))
        );
        assert_eq!(evidence["candidate_check"]["review"], call["review"]);
        assert_eq!(evidence["candidate_check"]["response"], call["response"]);
        let raw_evidence: std::collections::BTreeMap<String, &serde_json::value::RawValue> =
            serde_json::from_str(&record.intent.evidence).expect("original nested evidence");
        assert_eq!(
            checked["policy"]["evidence"]["candidate_check_sha256"],
            json!(source::Sha256Digest::of(
                raw_evidence["candidate_check"].get().as_bytes()
            ))
        );
        let output_call = calls[2..]
            .iter()
            .find(|call| call["candidate_check"]["review"]["request"]["site"] == site["site"])
            .expect("same site's output checker");
        assert_eq!(output_call["candidate_check"], evidence["candidate_check"]);
        assert!(
            record
                .worktree
                .as_ref()
                .expect("worktree")
                .completion
                .is_some()
        );
    }
    let header: store::JournalHeader =
        serde_json::from_value(report["sites"][1]["preparation"]["journal"].clone())
            .expect("actual header");
    let mut replay = request.clone();
    let stale = execute(&replay);
    assert_eq!(
        json!(build::LocalPreparationRunReport::new(&stale))["code"],
        "local_preparation_history"
    );
    replay["expected_header"] = json!(header);
    let outcome = execute(&replay);
    let report = json!(build::LocalPreparationRunReport::new(&outcome));
    assert_eq!(report["code"], "local_preparation_history");
    assert_eq!(
        events(&log).len(),
        4,
        "existing IDs refuse before a second checker effect"
    );
}

#[test]
fn stale_executable_pin_refuses_cli_sdk_and_mcp_before_candidate_or_preparation_effects() {
    use super::super::mcp::{arguments, call};
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("journal baseline");
    let parent = private_directory();
    let log = parent.path().join("must-not-exist.jsonl");
    let pin = gazette_sdk::process::ExecutablePin {
        sha256: source::Sha256Digest::of(b"different selected executable"),
        ..executable_pin()
    };
    let config = pinned_configuration(&configuration(&fixture, &log, "none"), pin);
    let request = checked_request(&config, root.path(), journal.header, parent.path());
    let outcome = execute(&request);
    let expected = json!(build::LocalPreparationRunReport::new(&outcome));
    assert_eq!(expected["status"], "candidate-failed");
    assert_eq!(expected["completed"], json!({}));
    assert_eq!(expected["evidence"]["process_failure"], "executable");
    assert_eq!(expected["evidence"]["child_custody"], "not-started");
    let build::LocalPreparationRunError::Candidate(_) = outcome.expect_err("actual pin mismatch")
    else {
        panic!("pin failure must reach the candidate execution owner: {expected}");
    };
    let mut args = ARGUMENTS.to_vec();
    args.push("--allow-policy-execution");
    let (output, actual) = result(&request, &args);
    assert!(!output.status.success());
    assert_eq!(actual, expected);
    let mut args = arguments(&request);
    args["allow_policy_execution"] = json!(true);
    assert_eq!(call(args, true), expected);
    assert!(!log.exists(), "mismatched executable never starts");
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &baseline);
        assert!(!parent.path().join(site).exists());
    }
}

#[test]
fn later_candidate_rejection_preserves_completed_prefix_without_stage_or_journal_effects() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let before = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let log = parent.path().join("checks.jsonl");
    let mut request = checked_request(
        &configuration(&fixture, &log, "site:other"),
        root.path(),
        journal.header,
        parent.path(),
    );
    request["sites"][0]["render"]["model_output"] = json!("elsewhere/index.html");
    let outcome = execute(&request);
    let report = json!(build::LocalPreparationRunReport::new(&outcome));
    assert_eq!(report["status"], "candidate-failed");
    assert_eq!(report["site"], "site:other");
    assert_eq!(report["completed"].as_object().expect("prefix").len(), 1);
    assert!(report["completed"]["site:one"].is_object());
    assert_eq!(report["evidence"]["exit"]["code"], 23);
    assert!(report["evidence"]["invocation"].is_object());
    assert_eq!(events(&log).len(), 2);
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &before);
        assert!(!parent.path().join(site).exists());
    }
}

#[test]
fn complete_review_budget_failure_and_missing_checker_do_not_spawn_or_prepare() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let before = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let log = parent.path().join("checks.jsonl");
    let mut request = checked_request(
        &configuration(&fixture, &log, "none"),
        root.path(),
        journal.header,
        parent.path(),
    );
    request["candidate_check"]["bytes"] = json!(1);
    for configured in [true, false] {
        if !configured {
            request["workspace"] = json!({"kind":"supplied","source":fixture.config()});
        }
        let outcome = execute(&request);
        let report = json!(build::LocalPreparationRunReport::new(&outcome));
        assert_eq!(report["status"], "candidate-failed");
        assert_eq!(report["completed"], json!({}));
        assert!(report["evidence"]["invocation"].is_null());
        if !configured {
            assert_eq!(report["evidence"]["code"], "policy_unconfigured");
        }
    }
    assert!(!log.exists());
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &before);
        assert!(!parent.path().join(site).exists());
    }
}

#[test]
fn mcp_candidate_rejection_preserves_exact_native_invocations_before_journal_effects() {
    use super::super::mcp::{arguments, call};
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let log = parent.path().join("checks.jsonl");
    let request = checked_request(
        &configuration(&fixture, &log, "site:other"),
        root.path(),
        journal.header,
        parent.path(),
    );
    let denied = call(arguments(&request), true);
    assert_eq!(denied["status"], "candidate-failed");
    assert_eq!(denied["completed"], json!({}));
    assert!(denied["evidence"]["invocation"].is_null());
    assert!(!log.exists(), "denied execution never invokes the checker");
    let mut args = arguments(&request);
    args["allow_policy_execution"] = json!(true);
    let result = call(args, true);
    assert_eq!(result["status"], "candidate-failed");
    assert_eq!(result["site"], "site:other");
    assert_eq!(result["evidence"]["exit"]["code"], 23);
    assert_eq!(result["completed"].as_object().expect("prefix").len(), 1);
    let delivered = fs::read_to_string(&log).expect("exact native stdin bytes");
    let lines = delivered.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        result["completed"]["site:one"]["request_sha256"],
        json!(source::Sha256Digest::of(lines[0].as_bytes()))
    );
    assert_eq!(
        result["evidence"]["invocation"]["request_sha256"],
        json!(source::Sha256Digest::of(lines[1].as_bytes()))
    );
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &baseline);
        assert!(!parent.path().join(site).exists());
    }
}

#[test]
fn later_render_failure_keeps_every_candidate_invocation_and_never_prepares_a_subset() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let before = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let log = parent.path().join("checks.jsonl");
    let mut request = checked_request(
        &configuration(&fixture, &log, "none"),
        root.path(),
        journal.header,
        parent.path(),
    );
    request["sites"][0]["render"]["model_output"] = json!("elsewhere/index.html");
    let outcome = execute(&request);
    let report = json!(build::LocalPreparationRunReport::new(&outcome));
    assert_eq!(report["status"], "render-failed");
    assert_eq!(report["requested"], json!(["site:other", "site:one"]));
    assert_eq!(
        report["candidate_checks"]
            .as_object()
            .expect("complete checks")
            .len(),
        2
    );
    assert_eq!(
        report["failures"].as_object().expect("site failures").len(),
        1
    );
    assert!(report["failures"]["site:other"]["code"].is_string());
    assert_eq!(
        events(&log).len(),
        2,
        "no output checker on incomplete render set"
    );
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &before);
        assert!(!parent.path().join(site).exists());
    }
}

#[test]
fn source_and_parent_history_separation_precede_sqlite_coordination_and_checker_effects() {
    use std::os::unix::fs::PermissionsExt;
    for parent_source in [false, true] {
        let fixture = published_fixture(&[]);
        let parent = private_directory();
        let history = private_directory();
        let protected = if parent_source {
            history.path()
        } else {
            fixture.directory.path()
        };
        let root = protected.join("private-journal");
        fs::create_dir(&root).expect("fixture journal root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private root");
        let journal = initialize(&root);
        let writer = rusqlite::Connection::open(&journal.database).expect("fixture SQLite owner");
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL; UPDATE gazette_journal SET writer_fence=writer_fence;",
            )
            .expect("real WAL coordination files");
        let paths = [
            journal.database.clone(),
            journal.database.with_extension("sqlite-wal"),
            journal.database.with_extension("sqlite-shm"),
        ];
        let before = paths
            .iter()
            .map(|path| fs::read(path).expect("native main/WAL/SHM bytes"))
            .collect::<Vec<_>>();
        let log = parent.path().join("checks.jsonl");
        let config = configuration(&fixture, &log, "none");
        build::acquire_publications(
            &config,
            &[
                "site:one".parse().expect("site"),
                "site:other".parse().expect("site"),
            ],
            Default::default(),
            Default::default(),
        )
        .expect("selected pinned captures remain valid");
        let mut request = checked_request(&config, &root, journal.header, parent.path());
        if parent_source {
            request["sites"][0]["metadata"]["parent"] = json!(fixture.commit.to_string());
            request["sites"][0]["parent_source"] = json!(release_commit::ParentSource::Bare {
                path: history.path().to_owned()
            });
        }
        let outcome = execute(&request);
        let report = json!(build::LocalPreparationRunReport::new(&outcome));
        assert_eq!(report["code"], "local_preparation_separation", "{report}");
        assert!(!log.exists());
        assert_eq!(
            paths
                .iter()
                .map(|path| fs::read(path).expect("unchanged native state"))
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(fs::read_dir(parent.path()).expect("no stages").count(), 0);
        drop(writer);
    }
}
