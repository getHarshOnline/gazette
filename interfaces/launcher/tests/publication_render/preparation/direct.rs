//! One real operator request reaches retained rendering and durable native preparation.

#[path = "direct/candidate.rs"]
pub(in crate::preparation) mod candidate;
#[path = "direct/coordinator.rs"]
mod coordinator;
#[path = "direct/default_selection.rs"]
pub(in crate::preparation) mod default_selection;
#[path = "direct/output_policy.rs"]
pub(super) mod output_policy;
#[path = "direct/preflight.rs"]
mod preflight;

use std::{
    collections::BTreeMap,
    io::Write,
    process::{Command, Stdio},
};

use gazette_sdk::{build, release_commit, source, store};
use serde_json::{Value, json};

use super::super::{cli, policy::configured, worktree::metadata};
use super::{
    check::disclosure_policy, enable_site, fs, initialize, options, private_directory,
    published_fixture, read, unchanged, with_destinations,
};

const ARGUMENTS: &[&str] = &[
    "prepare-sites",
    "--local",
    "--allow-read",
    "--create-stage",
    "--record-preparation",
    "--advance-writer-generation",
    "--reserve-local-coordination",
    "--allow-coordination-files",
    "--initialize-worktree",
];

pub(super) fn input(
    config: &str,
    journal: &std::path::Path,
    header: store::JournalHeader,
    parent: &std::path::Path,
) -> Value {
    json!({
        "workspace": {"kind":"supplied","source":config},
        "journal": journal,
        "expected_header": header,
        "coordinator": null,
        "selection": {"content": {"collection:home":"include", "content:one":"include", "content:other":"include"}, "assets": {}},
        "disclosure": disclosure_policy(),
        "candidate_check": null,
        "sites": (["other", "one"].map(|site| json!({
            "render": {"site": format!("site:{site}"),   "model_output":"model.json", "graph":null},
            "stage_parent": parent,
            "stage_name": site,
            "preparation_id": id(site),
            "metadata": metadata(),
            "parent_source": null,
        }))),
    })
}

pub(super) fn id(site: &str) -> source::Sha256Digest {
    source::Sha256Digest::of(format!("site:{site}").as_bytes())
}

pub(super) fn recipe(input: &Value) -> build::LocalPreparationRecipe {
    build::LocalPreparationRecipe::parse_json(
        &serde_json::to_vec(input).expect("request"),
        4 * 1024 * 1024,
        2,
    )
    .expect("complete native recipe")
}

pub(super) fn permitted() -> build::LocalPreparationRunOptions {
    build::LocalPreparationRunOptions {
        allow_read: true,
        preparation: build::LocalPreparationOptions {
            stage: options(),
            worktree: release_commit::WorktreeOptions {
                allow_initialize: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

fn result(input: &Value, arguments: &[&str]) -> (std::process::Output, Value) {
    let output = cli(&input.to_string(), arguments);
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("complete native JSON: {e}; {output:?}"));
    (output, value)
}

fn observed(
    database: &std::path::Path,
    site: &str,
    header: store::JournalHeader,
) -> store::PreparationRecord {
    store::inspect_preparation(database, id(site), Some(&header), read())
        .expect("independent native inspection")
        .record
        .expect("durable preparation")
}

#[test]
fn direct_cli_and_sdk_share_complete_two_site_preparation_and_native_readback() {
    let fixture = published_fixture(&[]);
    let mut records = Vec::new();
    for direct in [true, false] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let request = input(
            &fixture.config(),
            root.path(),
            journal.header,
            parent.path(),
        );
        let value = if direct {
            let (output, value) = result(&request, ARGUMENTS);
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(value["status"], "local-preparations-complete-not-certified");
            value["sites"].clone()
        } else {
            serde_json::to_value(
                recipe(&request)
                    .execute(permitted())
                    .expect("shared SDK operation"),
            )
            .expect("native receipts")
        };
        assert_eq!(value[0]["site"], "site:other");
        assert_eq!(value[1]["site"], "site:one");
        let header: store::JournalHeader =
            serde_json::from_value(value[1]["preparation"]["journal"].clone())
                .expect("last actual coordinates");
        assert!(header.writer_fence > journal.header.writer_fence);
        let mut run = Vec::new();
        for (i, site) in ["other", "one"].iter().enumerate() {
            let record = observed(&journal.database, site, header);
            assert_eq!(
                serde_json::to_value(&record).expect("record"),
                value[i]["preparation"]["record"]
            );
            let commit = &record
                .worktree
                .as_ref()
                .expect("Git intent")
                .completion
                .as_ref()
                .expect("Git completion")
                .report
                .commit;
            let joined = release_commit::inspect_journal_worktree(
                release_commit::JournalWorktreeInspectionRequest {
                    database: &journal.database,
                    preparation_id: id(site),
                    expected_header: Some(&header),
                    commit,
                    protected_roots: &[fixture.directory.path().to_owned()],
                },
                release_commit::JournalWorktreeInspectionOptions {
                    read: read(),
                    ..Default::default()
                },
            )
            .expect("independent journal/Git/output readback");
            let model: Value = serde_json::from_slice(
                &fs::read(record.intent.address.path().join("model.json"))
                    .expect("actual machine output"),
            )
            .expect("shared model");
            assert!(model["pages"][format!("content:{site}")].is_object());
            let native = git2::Repository::open(record.intent.address.path())
                .expect("independent Git repository");
            assert!(native.head_detached().expect("detached"));
            assert_eq!(native.remotes().expect("local only").len(), 0);
            let _ = joined;
            run.push((record.intent.output, commit.clone()));
        }
        records.push(run);
    }
    assert_eq!(
        records[0], records[1],
        "independent CLI and SDK runs produce identical outputs and commits"
    );
}

fn deployable_seven_site_config(mut config: String, sites: &[&str]) -> String {
    for site in sites {
        let slug = site
            .strip_prefix("site:")
            .expect("fixture site has the canonical prefix");
        let disabled = format!("[[sites]]\nid='{site}'\npublisher='owner:one'\ndeployable=false\n");
        assert_eq!(config.matches(&disabled).count(), 1);
        config = config.replacen(
            &disabled,
            &format!("[[sites]]\nid='{site}'\npublisher='owner:one'\ndeployable=true\n"),
            1,
        );
        let publication = format!("site='{site}'\npublic_base_url=");
        assert_eq!(config.matches(&publication).count(), 1);
        config = config.replacen(
            &publication,
            &format!(
                "site='{site}'\ndestination={{kind='git',remote='https://example.invalid/{slug}.git',branch='publication'}}\npublic_base_url="
            ),
            1,
        );
    }
    config
}

fn seven_site_input(
    config: &str,
    journal: &std::path::Path,
    header: store::JournalHeader,
    parent: &std::path::Path,
    sites: &[&str],
) -> Value {
    let content = sites
        .iter()
        .map(|site| {
            let slug = site.strip_prefix("site:").expect("fixture site prefix");
            (format!("content:{slug}"), "include")
        })
        .collect::<BTreeMap<_, _>>();
    json!({
        "workspace": {"kind":"supplied","source":config},
        "journal": journal,
        "expected_header": header,
        "coordinator": null,
        "selection": {"content": content, "assets": {}},
        "disclosure": disclosure_policy(),
        "candidate_check": null,
        "sites": sites.iter().map(|site| {
            let slug = site.strip_prefix("site:").expect("fixture site prefix");
            json!({
                "render": {"site": site,   "model_output":"model.json", "graph":null},
                "stage_parent": parent,
                "stage_name": slug,
                "preparation_id": id(slug),
                "metadata": metadata(),
                "parent_source": null,
            })
        }).collect::<Vec<_>>(),
    })
}

#[test]
fn seven_site_two_fresh_journals_prepare_identical_trees_and_commits() {
    let (fixture, config, sites) = crate::sites::seven_site_fixture();
    let config = deployable_seven_site_config(config, &sites);
    let reversed = sites.iter().rev().copied().collect::<Vec<_>>();
    let mut runs = Vec::new();

    for requested in [&sites, &reversed] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let request = seven_site_input(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            requested,
        );
        let (output, value) = result(&request, ARGUMENTS);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(value["status"], "local-preparations-complete-not-certified");
        let receipts = value["sites"]
            .as_array()
            .expect("complete seven-site result");
        assert_eq!(receipts.len(), sites.len());
        let header: store::JournalHeader = serde_json::from_value(
            receipts.last().expect("nonempty preparation set")["preparation"]["journal"].clone(),
        )
        .expect("last actual coordinates");
        assert!(header.writer_fence > journal.header.writer_fence);

        let mut stable = BTreeMap::new();
        for (index, site) in requested.iter().enumerate() {
            let slug = site.strip_prefix("site:").expect("fixture site prefix");
            assert_eq!(receipts[index]["site"], *site);
            let record = observed(&journal.database, slug, header);
            assert_eq!(
                serde_json::to_value(&record).expect("record"),
                receipts[index]["preparation"]["record"]
            );
            let retention = record.retention.as_ref().expect("retained output");
            let check = record.check.as_ref().expect("disclosure check");
            let worktree = record.worktree.as_ref().expect("Git intent");
            let completion = worktree.completion.as_ref().expect("Git completion");
            let joined = release_commit::inspect_journal_worktree(
                release_commit::JournalWorktreeInspectionRequest {
                    database: &journal.database,
                    preparation_id: id(slug),
                    expected_header: Some(&header),
                    commit: &completion.report.commit,
                    protected_roots: &[fixture.directory.path().to_owned()],
                },
                release_commit::JournalWorktreeInspectionOptions {
                    read: read(),
                    ..Default::default()
                },
            )
            .expect("independent journal/Git/output readback");
            assert_eq!(joined.worktree.output.identity(), record.intent.output);

            let stage = record.intent.address.path();
            let files = joined
                .worktree
                .output
                .entries()
                .iter()
                .filter_map(|entry| match entry.content {
                    source::Content::Directory => None,
                    source::Content::File { .. } => Some((
                        entry.path.as_str().to_owned(),
                        fs::read(stage.join(entry.path.as_str())).expect("exact staged bytes"),
                    )),
                })
                .collect::<BTreeMap<_, _>>();
            let model: Value =
                serde_json::from_slice(files.get("model.json").expect("site machine output"))
                    .expect("site model JSON");
            assert_eq!(model["pages"].as_object().expect("site pages").len(), 1);
            assert!(model["pages"][format!("content:{slug}")].is_object());

            let native = git2::Repository::open(&stage).expect("independent Git repository");
            assert!(native.head_detached().expect("detached"));
            assert_eq!(native.remotes().expect("local only").len(), 0);

            let projection = json!({
                "site": site,
                "inventory": joined.worktree.output,
                "intent": {
                    "destination": &record.intent.destination,
                    "output": record.intent.output,
                    "entries": record.intent.entries,
                    "bytes": record.intent.bytes,
                    "evidence_sha256": record.intent.evidence_sha256,
                },
                "retention": {
                    "output": retention.output,
                    "entries": retention.entries,
                    "bytes": retention.bytes,
                },
                "check_evidence_sha256": check.evidence_sha256,
                "worktree": {
                    "metadata": &worktree.metadata,
                    "parent_source": &worktree.parent_source,
                    "check_sha256": worktree.check_sha256,
                    "completion": &completion.report,
                },
            });
            assert!(
                stable
                    .insert((*site).to_owned(), (projection, files))
                    .is_none()
            );
        }
        assert_eq!(stable.len(), sites.len());
        runs.push(stable);
    }

    assert_eq!(
        runs[0], runs[1],
        "two separate processes, empty journals and opposite request orders retain byte-identical seven-site outputs and commits"
    );
}

#[test]
fn decoder_rejects_ambiguous_shapes_before_native_reads() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("database baseline");
    let parent = private_directory();
    let valid = input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    );
    recipe(&valid);
    let mut invalid = Vec::new();
    for pointer in [
        "",
        "/sites/0",
        "/sites/0/render",
        "/sites/0/metadata",
        "/sites/0/metadata/author",
        "/expected_header",
        "/selection",
        "/disclosure",
        "/disclosure/rules/rule:claim",
        "/disclosure/rules/rule:claim/pattern",
    ] {
        let mut value = valid.clone();
        value
            .pointer_mut(pointer)
            .expect("object")
            .as_object_mut()
            .expect("named object")
            .insert("unrecognized".into(), json!(true));
        invalid.push(value);
    }
    for field in [
        "parent_source",
        "metadata",
        "stage_parent",
        "preparation_id",
    ] {
        let mut value = valid.clone();
        value["sites"][0]
            .as_object_mut()
            .expect("site")
            .remove(field);
        invalid.push(value);
    }
    for pointer in ["/sites/0/render/graph", "/sites/0/metadata/parent"] {
        let mut value = valid.clone();
        let (parent_pointer, field) = pointer.rsplit_once('/').expect("parent");
        value
            .pointer_mut(parent_pointer)
            .expect("owner")
            .as_object_mut()
            .expect("object")
            .remove(field);
        invalid.push(value);
    }
    let mut positional = valid.clone();
    positional["sites"][0]["metadata"]["author"] =
        json!(["Local fixture", "fixture@example.invalid", 100, 330]);
    invalid.push(positional);
    let mut duplicate = valid.clone();
    duplicate["sites"][1] = duplicate["sites"][0].clone();
    invalid.push(duplicate);
    let mut relative = valid.clone();
    relative["journal"] = json!("relative");
    invalid.push(relative);
    for value in invalid {
        assert!(
            build::LocalPreparationRecipe::parse_json(
                &serde_json::to_vec(&value).expect("request"),
                4 * 1024 * 1024,
                2
            )
            .is_err(),
            "{value}"
        );
    }
    let duplicate = valid.to_string().replace(
        "\"stage_name\":\"other\"",
        "\"stage_name\":\"other\",\"stage_\\u006eame\":\"other\"",
    );
    assert_ne!(duplicate, valid.to_string());
    assert!(
        build::LocalPreparationRecipe::parse_json(duplicate.as_bytes(), 4 * 1024 * 1024, 2)
            .is_err()
    );
    assert!(build::LocalPreparationRecipe::parse_json(valid.to_string().as_bytes(), 1, 2).is_err());
    assert!(
        build::LocalPreparationRecipe::parse_json(valid.to_string().as_bytes(), 4 * 1024 * 1024, 1)
            .is_err()
    );
    unchanged(&journal, id("one"), &baseline);
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("no stage effects")
            .count(),
        0
    );
}

#[test]
fn single_site_uses_the_same_workspace_recipe_and_records_configured_checker() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let observation = private_directory();
    let check = observation.path().join("received.json");
    let (config, _) = configured(&fixture, &check, 0);
    let mut request = input(&config, root.path(), journal.header, parent.path());
    request["sites"]
        .as_array_mut()
        .expect("site choices")
        .remove(0);
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--allow-policy-execution");
    let (output, value) = result(&request, &arguments);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(value["sites"].as_array().expect("complete set").len(), 1);
    assert_eq!(value["sites"][0]["site"], "site:one");
    let header = serde_json::from_value(value["sites"][0]["preparation"]["journal"].clone())
        .expect("observed header");
    let record = observed(&journal.database, "one", header);
    assert!(record.check.is_some() && record.worktree.is_some());
    assert!(check.exists(), "native checker ran before Git preparation");
    assert!(parent.path().join("one/guide/index.html").is_file());
    assert!(!parent.path().join("other").exists());
    let absent = store::inspect_preparation(&journal.database, id("other"), Some(&header), read())
        .expect("unrequested site is not prepared");
    assert!(absent.record.is_none());
}

#[test]
fn one_failed_render_never_prepares_its_successful_sibling() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let mut request = input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    );
    request["sites"][1]["render"]["model_output"] = json!("guide/index.html");
    let (output, value) = result(&request, ARGUMENTS);
    assert!(!output.status.success());
    assert_eq!(value["status"], "render-failed");
    assert_eq!(value["requested"], json!(["site:other", "site:one"]));
    assert_eq!(value["candidate_checks"], json!({}));
    assert_eq!(
        value["failures"]
            .as_object()
            .expect("exact failed site")
            .len(),
        1
    );
    assert!(value["failures"]["site:one"]["code"].is_string());
    unchanged(&journal, id("one"), &baseline);
    unchanged(&journal, id("other"), &baseline);
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("no successful subset")
            .count(),
        0
    );
}

#[test]
fn every_effect_permission_is_independent_and_refused_before_preparation() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("database baseline");
    let parent = private_directory();
    let request = input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    );
    for permission in ARGUMENTS.iter().skip(2) {
        let arguments = ARGUMENTS
            .iter()
            .copied()
            .filter(|arg| arg != permission)
            .collect::<Vec<_>>();
        let (output, value) = result(&request, &arguments);
        assert!(!output.status.success(), "permission {permission}: {value}");
        assert_ne!(value["status"], "local-preparations-complete-not-certified");
        unchanged(&journal, id("one"), &baseline);
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no partial stage")
                .count(),
            0
        );
    }
    let missing_mode = cli(&request.to_string(), &["prepare-sites"]);
    assert!(!missing_mode.status.success());
    assert!(missing_mode.stdout.is_empty());
    unchanged(&journal, id("other"), &baseline);
}

#[test]
fn second_site_failure_retains_complete_prefix_and_newer_native_evidence() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let sentinel = parent.path().join("one");
    fs::write(&sentinel, b"unrelated existing child").expect("disposable conflicting destination");
    let request = input(
        &fixture.config(),
        root.path(),
        journal.header,
        parent.path(),
    );
    let (output, value) = result(&request, ARGUMENTS);
    assert!(!output.status.success());
    assert_eq!(value["status"], "incomplete");
    assert_eq!(value["requested"], json!(["site:other", "site:one"]));
    assert_eq!(value["completed"].as_array().expect("prefix").len(), 1);
    assert_eq!(value["completed"][0]["site"], "site:other");
    assert_eq!(value["failure"]["site"], "site:one");
    assert_eq!(value["failure"]["phase"]["kind"], "stage");
    let evidence = &value["failure"]["phase"]["evidence"];
    assert_eq!(evidence["writer_acquired"], true);
    let current: store::JournalHeader = serde_json::from_value(evidence["journal"].clone())
        .expect("failed phase current generation");
    let prior: store::JournalHeader =
        serde_json::from_value(value["completed"][0]["preparation"]["journal"].clone())
            .expect("completed prefix coordinates");
    assert!(current.writer_fence > prior.writer_fence);
    let first = observed(&journal.database, "other", current);
    assert!(
        first
            .worktree
            .as_ref()
            .expect("worktree")
            .completion
            .is_some()
    );
    let second = observed(&journal.database, "one", current);
    assert_eq!(
        serde_json::to_value(&second).expect("latest durable record"),
        evidence["receipt"]["record"]
    );
    assert!(second.retention.is_none() && second.worktree.is_none());
    assert_eq!(
        fs::read(sentinel).expect("preserved child"),
        b"unrelated existing child"
    );
}

#[test]
fn response_limit_refuses_before_effects_and_closed_pipe_never_erases_effects_or_retries() {
    let fixture = published_fixture(&[]);
    for closed in [false, true] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let request = input(
            &fixture.config(),
            root.path(),
            journal.header,
            parent.path(),
        );
        let output = if closed {
            let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
                .args(ARGUMENTS)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("real CLI");
            drop(child.stdout.take());
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(request.to_string().as_bytes())
                .expect("complete request");
            child.wait_with_output().expect("closed response channel")
        } else {
            let mut arguments = ARGUMENTS.to_vec();
            arguments.extend(["--max-output-bytes", "1"]);
            cli(&request.to_string(), &arguments)
        };
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8(output.stderr).expect("diagnostic");
        if !closed {
            assert!(
                diagnostic.contains("local_preparation_input"),
                "{diagnostic}"
            );
            for site in ["other", "one"] {
                assert!(
                    store::inspect_preparation(&journal.database, id(site), None, read())
                        .expect("unchanged native journal")
                        .record
                        .is_none(),
                    "response capacity is admitted before effects"
                );
            }
            continue;
        }
        assert!(diagnostic.contains("do not retry"), "{diagnostic}");
        for site in ["other", "one"] {
            let inspection = store::inspect_preparation(&journal.database, id(site), None, read())
                .expect("native reconciliation");
            assert!(
                inspection
                    .record
                    .expect("not rolled back")
                    .worktree
                    .expect("Git intent")
                    .completion
                    .is_some()
            );
        }
    }
}

#[test]
fn configured_checker_requires_its_own_permission_and_failure_is_not_certification() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let observation = private_directory();
    let check = observation.path().join("received.json");
    let (config, _) = configured(&fixture, &check, 7);
    let request = input(&config, root.path(), journal.header, parent.path());
    let (output, _) = result(&request, ARGUMENTS);
    assert!(!output.status.success());
    assert!(!check.exists());
    unchanged(&journal, id("other"), &baseline);
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--allow-policy-execution");
    let (output, value) = result(&request, &arguments);
    assert!(!output.status.success());
    assert_eq!(value["failure"]["phase"]["kind"], "policy");
    assert_eq!(value["failure"]["phase"]["evidence"]["exit"]["code"], 7);
    assert_eq!(
        value["failure"]["phase"]["evidence"]["invocation"]["request_sha256"],
        json!(source::Sha256Digest::of(
            &fs::read(&check).expect("exact checker input")
        ))
    );
    assert!(
        check.exists(),
        "configured native process was actually invoked"
    );
    assert_eq!(value["completed"], json!([]));
    assert!(value["failure"]["prior_receipt"]["record"]["retention"].is_object());
    assert_eq!(
        value["failure"]["prior_receipt"]["record"]["worktree"],
        Value::Null
    );
    let inspection = store::inspect_preparation(&journal.database, id("one"), None, read())
        .expect("second site remains unstarted");
    assert!(inspection.record.is_none());
}

#[test]
fn disabled_destination_set_refuses_before_any_configured_checker_or_native_effect() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("baseline");
    let parent = private_directory();
    let observation = private_directory();
    let check = observation.path().join("must-not-run.json");
    let (config, _) = configured(&fixture, &check, 0);
    let request = input(
        &with_destinations(config),
        root.path(),
        journal.header,
        parent.path(),
    );
    let mut allowed = permitted();
    allowed.preparation.policy.allow_execute = true;
    let error = recipe(&request)
        .execute(allowed)
        .expect_err("disabled destination-bearing set must stop before checking");
    let build::LocalPreparationRunError::Before { code, message } = error else {
        panic!("early typed refusal required: {error:?}")
    };
    assert_eq!(code, "publication_not_deployable");
    assert!(message.contains("site:one") && message.contains("site:other"));
    assert!(!check.exists(), "configured checker must not run");
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &baseline);
    }
    assert_eq!(
        fs::read_dir(parent.path()).expect("stage parent").count(),
        0
    );
}
