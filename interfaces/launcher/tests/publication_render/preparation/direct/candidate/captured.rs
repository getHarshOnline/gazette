//! Actual checker stdout and native custody; synthetic policy is never publisher approval.

#[path = "captured/js_qualification.rs"]
mod js_qualification;
#[path = "captured/qualification.rs"]
pub(in crate::preparation) mod qualification;

use super::*;
use gazette_sdk::release;
use serde_json::value::RawValue;
use std::{collections::BTreeMap, path::Path};

const POLICY: &[u8] = br#"{"id":"policy:synthetic-candidate","revision":"revision:one","forbidden":"forbidden-fixture-value"}"#;

pub(in crate::preparation) fn policy_fixture(
    overrides: &[(&str, &[u8])],
) -> super::super::super::super::Fixture {
    let mut files = vec![
        ("theme/policy.json", POLICY),
        ("theme/templates/graph.json", b"{{ __tera_context }}"),
    ];
    files.extend_from_slice(overrides);
    published_fixture(&files)
}

const CHECKER: &str = r#"
import {readFileSync, appendFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const [mode, log] = process.argv.slice(1);
const input = readFileSync(0);
const request = JSON.parse(input);
appendFileSync(log, input.toString() + '\n');
if (request.format === 'gazette-output-policy/1') {
  if (request.disclosure.findings.length !== 0 ||
      !request.candidate_check.response.decisions.every(d => d.outcome === 'allow')) process.exit(61);
  const sha = bytes => createHash('sha256').update(bytes).digest('hex');
  const document = Buffer.from(request.policy_source.document_base64, 'base64');
  const policy = JSON.parse(document);
  if (sha(document) !== request.policy_source.sha256) process.exit(65);
  const response = {format:'gazette-output-response/1', request_sha256:sha(input),
    policy:{id:policy.id,revision:policy.revision,source_sha256:sha(document)},
    outcome:'allow',evidence:['evidence:synthetic-candidate-and-output']};
  process.stdout.write(JSON.stringify(response), () => process.exit(0));
} else {
if (request.format !== 'gazette-candidate-policy/1' || request.response !== null) process.exit(62);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const document = Buffer.from(request.policy_source.document_base64, 'base64');
const policy = JSON.parse(document);
if (sha(document) !== request.policy_source.sha256 ||
    sha(document) !== request.requirement.policy.source_sha256 ||
    policy.id !== request.requirement.policy.id ||
    policy.revision !== request.requirement.policy.revision ||
    request.policy_source.binding.source !== 'source:policy' ||
    request.policy_source.binding.entry !== 'policy.json') process.exit(63);
const decisions = [];
function visit(surface, pointer, value) {
  decisions.push({surface, pointer, outcome: value === policy.forbidden ? 'hold' : 'allow',
    evidence: ['evidence:synthetic-retained-policy']});
  if (Array.isArray(value)) value.forEach((item, i) => visit(surface, pointer + '/' + i, item));
  else if (value !== null && typeof value === 'object') {
    for (const key of Object.keys(value).sort())
      visit(surface, pointer + '/' + key.replaceAll('~', '~0').replaceAll('/', '~1'), value[key]);
  }
}
request.review.request.surfaces.forEach((surface, index) => visit(index, '', surface.value));
if (!decisions.length) process.exit(64);
const response = {format: 'gazette-review-response/1', request_sha256: request.review.sha256,
  policy: {id: policy.id, revision: policy.revision, source_sha256: sha(document)}, decisions};
const site = request.review.request.site;
const fault = mode === 'later-hold' ? (site === 'site:other' ? 'hold' : 'none') : mode;
if (fault === 'missing') response.decisions.pop();
if (fault === 'duplicate') response.decisions.push(response.decisions[0]);
if (fault === 'policy') response.policy.revision = 'revision:wrong';
if (fault === 'request') response.request_sha256 = sha('not the delivered review');
if (fault === 'hold') response.decisions[0].outcome = 'hold';
if (fault === 'exclude') response.decisions[0].outcome = 'exclude';
const text = fault === 'malformed' ? '{not-json' : '\r\n' + JSON.stringify(response, null, 2) + '\n ';
writeFileSync(log + '.' + site.split(':')[1] + '.response', text);
process.stdout.write(text, () => { if (fault === 'exit') process.exitCode = 17; });
}
"#;

fn configuration(
    fixture: &super::super::super::super::Fixture,
    log: &Path,
    mode: &str,
) -> (String, Value) {
    let (mut config, requirement) = source_configuration(fixture, log, mode);
    for site in ["one", "other"] {
        let anchor = format!("site='site:{site}'\npublic_base_url=");
        assert!(config.contains(&anchor));
        config = config.replace(&anchor, &format!(
            "site='site:{site}'\ndestination={{kind='git',remote='https://example.invalid/{site}.git',branch='publication'}}\npublic_base_url="));
        config = enable_site(config, site);
    }
    (config, requirement)
}

pub(in crate::preparation) fn source_configuration(
    fixture: &super::super::super::super::Fixture,
    log: &Path,
    mode: &str,
) -> (String, Value) {
    source_configuration_with_base(fixture, &fixture.config(), log, mode)
}

pub(in crate::preparation) fn source_configuration_with_base(
    fixture: &super::super::super::super::Fixture,
    base: &str,
    log: &Path,
    mode: &str,
) -> (String, Value) {
    let (program, pin) = super::super::output_policy::executable();
    let command = gazette_sdk::process::Command {
        program: program.clone(),
        executable: Some(*pin),
        arguments: vec![
            "--input-type=module".into(),
            "--eval".into(),
            CHECKER.into(),
            mode.into(),
            log.to_str().expect("fixture log").into(),
        ],
        directory: log
            .parent()
            .expect("private parent")
            .to_str()
            .expect("native path")
            .into(),
        environment: BTreeMap::new(),
    };
    let requirement = json!({
        "policy": {"id":"policy:synthetic-candidate", "revision":"revision:one", "source_sha256":source::Sha256Digest::of(POLICY)},
        "command_sha256":source::Sha256Digest::of(&serde_json::to_vec(&command).expect("independent exact command"))
    });
    let config = format!(
        "policy_source={{source='source:policy',entry='policy.json'}}\npolicy_check={{program={},executable={{bytes={},sha256='{}'}},arguments={},directory={},environment={{}}}}\n{}[[sources]]\nid='source:policy'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n",
        serde_json::to_string(&command.program).expect("program"),
        pin.bytes,
        pin.sha256,
        serde_json::to_string(&command.arguments).expect("arguments"),
        serde_json::to_string(&command.directory).expect("directory"),
        base,
        fixture.directory.path().to_str().expect("retained root"),
        fixture.commit,
    );
    (config, requirement)
}

pub(in crate::preparation) fn render<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    requirement: &build::CandidatePolicyRequirement,
) -> build::RenderedPublicationSet<'a> {
    render_with_graph(inputs, requirement, false)
}

pub(in crate::preparation) fn render_selected_profile<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    requirement: &build::CandidatePolicyRequirement,
    selection: gazette_sdk::authored::Selection,
    include_graph: bool,
) -> build::RenderedPublicationSet<'a> {
    render_profile(inputs, requirement, include_graph, selection)
}

fn render_with_graph<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    requirement: &build::CandidatePolicyRequirement,
    include_graph: bool,
) -> build::RenderedPublicationSet<'a> {
    let selection = serde_json::from_value(json!({
        "content":{"collection:home":"include","content:one":"include","content:other":"include"},"assets":{}
    }))
    .unwrap();
    render_profile(inputs, requirement, include_graph, selection)
}

fn render_profile<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    requirement: &build::CandidatePolicyRequirement,
    include_graph: bool,
    selection: gazette_sdk::authored::Selection,
) -> build::RenderedPublicationSet<'a> {
    let page = "page.html".parse().unwrap();
    let alias = "alias.html".parse().unwrap();
    let model = "model.json".parse().unwrap();
    let graph = "graph.json".parse().unwrap();
    let names: Vec<_> = inputs.sites().keys().map(|key| key.as_str()).collect();
    let mut options = crate::sites::yearless_options(&names, &page, &alias, &model);
    if include_graph {
        for option in options.sites.values_mut() {
            option.graph = Some(build::GraphRenderOptions {
                output: &graph,
                limits: Default::default(),
            });
        }
    }
    options.selection = Some(selection);
    let requirements = inputs
        .sites()
        .keys()
        .map(|key| (key.clone(), requirement.clone()))
        .collect();
    build::check_and_render_publications(
        inputs,
        options,
        Default::default(),
        build::CandidateCheckOptions {
            requirements: Some(&requirements),
            process: gazette_sdk::process::Options {
                allow_execute: true,
                ..Default::default()
            },
            review: Default::default(),
            bytes: 16 << 20,
            response: Default::default(),
            response_bytes: 16 << 20,
        },
    )
    .expect("actual captured candidate responses before shared rendering")
}

fn request(
    config: &str,
    root: &Path,
    header: store::JournalHeader,
    parent: &Path,
    required: &Value,
) -> Value {
    let mut request = checked_request(config, root, header, parent);
    request["candidate_policies"] = json!({"site:one":required,"site:other":required});
    request
}

fn arguments() -> Vec<&'static str> {
    let mut arguments = ARGUMENTS.to_vec();
    arguments.push("--allow-policy-execution");
    arguments
}

#[test]
fn qualification_requires_exact_live_candidate_not_absent_exit_only_or_imported_evidence() {
    for mode in ["absent", "exit-only", "imported", "wrong-requirement"] {
        let fixture = policy_fixture(&[]);
        let root = private_directory();
        let journal = initialize(root.path());
        let baseline = fs::read(&journal.database).unwrap();
        let parent = private_directory();
        let log = parent.path().join("qualification.jsonl");
        let (config, required) = if mode == "wrong-requirement" {
            configuration(&fixture, &log, "none")
        } else {
            let config = pinned_configuration(
                &super::configuration(&fixture, &log, "none"),
                executable_pin(),
            );
            let config = format!(
                "policy_source={{source='source:policy',entry='policy.json'}}\n{config}[[sources]]\nid='source:policy'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n",
                fixture.directory.path().to_str().unwrap(),
                fixture.commit,
            );
            let inputs = crate::sites::acquire_set(&config, &["site:one"]);
            let command = inputs
                .configuration()
                .policy_command()
                .expect("bound policy command")
                .unwrap();
            let required = json!({
                "policy":{"id":"policy:synthetic-candidate","revision":"revision:one","source_sha256":source::Sha256Digest::of(POLICY)},
                "command_sha256":source::Sha256Digest::of(&serde_json::to_vec(command).unwrap())
            });
            (config, required)
        };
        let requirement: build::CandidatePolicyRequirement =
            serde_json::from_value(required.clone()).unwrap();
        let output = build::OutputPolicyRequirement {
            policy: requirement.policy.clone(),
            command_sha256: requirement.command_sha256,
            response_bytes: 256 << 10,
        };
        let inputs = crate::acquire(&config);
        let rendered = crate::preparation::render(&inputs).unwrap();
        let mut qualification =
            super::super::output_policy::qualification::expected(&rendered, &output);
        qualification.candidate_policy = Some(requirement);
        let mut selected = match mode {
            "absent" => input(&config, root.path(), journal.header, parent.path()),
            "imported" => reviewed_request(&config, root.path(), journal.header, parent.path()),
            "wrong-requirement" => request(
                &config,
                root.path(),
                journal.header,
                parent.path(),
                &required,
            ),
            _ => checked_request(&config, root.path(), journal.header, parent.path()),
        };
        selected["sites"] = json!([selected["sites"][1].clone()]);
        for field in ["candidate_policies", "review_responses"] {
            if let Some(values) = selected.get_mut(field).and_then(Value::as_object_mut) {
                values.remove("site:other");
            }
        }
        if mode == "wrong-requirement" {
            qualification
                .candidate_policy
                .as_mut()
                .unwrap()
                .policy
                .revision = "revision:independently-required-other".parse().unwrap();
        }
        selected["sites"][0]["qualification"] = json!(qualification);
        let outcome = execute(&selected);
        let error = outcome.expect_err("only exact captured live requirement qualifies");
        assert!(
            error.to_string().contains("preparation_qualification"),
            "{mode}: {error}"
        );
        unchanged(&journal, id("one"), &baseline);
        assert!(
            !parent.path().join("one").exists(),
            "{mode}: no output stage"
        );
        if matches!(mode, "absent" | "wrong-requirement") {
            assert!(
                !log.exists(),
                "{mode}: preflight refusal must not execute the candidate checker"
            );
        } else {
            let calls = events(&log);
            assert_eq!(calls.len(), 1, "{mode}: no output-policy execution");
            assert_eq!(calls[0]["format"], "gazette-candidate-policy/1");
        }
    }
}

#[test]
fn native_candidate_responses_reach_exact_preparation_history_without_imported_decisions() {
    let fixture = published_fixture(&[("theme/policy.json", POLICY)]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("captured.jsonl");
    let (config, required) = configuration(&fixture, &log, "none");
    let request = request(
        &config,
        root.path(),
        journal.header,
        parent.path(),
        &required,
    );
    assert!(request.get("review_responses").is_none());
    let (output, report) = result(&request, &arguments());
    assert!(output.status.success(), "{output:?}; {report}");
    assert_eq!(
        report["status"],
        "local-preparations-complete-not-certified"
    );
    let calls = events(&log);
    assert_eq!(calls.len(), 4);
    let mut records = Vec::new();
    for (call, short) in calls[..2].iter().zip(["one", "other"]) {
        assert_eq!(call["format"], "gazette-candidate-policy/1");
        assert_eq!(call["review"]["request"]["site"], format!("site:{short}"));
        assert!(call["response"].is_null());
        assert_eq!(call["requirement"], required);
        let stdout = fs::read_to_string(
            parent
                .path()
                .join(format!("captured.jsonl.{short}.response")),
        )
        .expect("original checker bytes");
        assert!(stdout.starts_with("\r\n") && stdout.ends_with("\n "));
        let response: Value = serde_json::from_str(&stdout).expect("native response");
        assert_eq!(response["request_sha256"], call["review"]["sha256"]);
        let record = store::inspect_preparation(&journal.database, id(short), None, read())
            .expect("independent native read-back")
            .record
            .expect("actual history");
        let evidence: Value =
            serde_json::from_str(&record.intent.evidence).expect("private original intent");
        let candidate = &evidence["candidate_check"];
        assert_eq!(candidate["review"], call["review"]);
        assert_eq!(candidate["requirement"], required);
        assert_eq!(candidate["policy_source"], call["policy_source"]);
        assert_eq!(candidate["response"]["response_json"], stdout);
        assert_eq!(candidate["response"]["decisions"], response["decisions"]);
        assert_eq!(
            candidate["response"]["response_sha256"],
            json!(source::Sha256Digest::of(stdout.as_bytes()))
        );
        let rendered_call = calls[2..]
            .iter()
            .find(|item| {
                item["candidate_check"]["review"]["request"]["site"] == format!("site:{short}")
            })
            .expect("same output checker");
        assert_eq!(&rendered_call["candidate_check"], candidate);
        assert!(record.retention.is_some());
        assert!(record.check.is_some());
        assert!(
            record
                .worktree
                .as_ref()
                .expect("native worktree intent")
                .completion
                .is_some()
        );
        let returned = report["sites"]
            .as_array()
            .expect("complete results")
            .iter()
            .find(|item| item["site"] == format!("site:{short}"))
            .expect("same site receipt");
        assert_eq!(json!(record), returned["preparation"]["record"]);
        records.push((format!("site:{short}"), record));
    }
    let header = serde_json::from_value(report["sites"][1]["preparation"]["journal"].clone())
        .expect("final native header");
    let selections: Vec<_> = records
        .iter()
        .map(|(site, record)| release::PreparationSelection {
            output_policy: None,
            qualification: None,
            preparation_id: record.intent.id,
            site: site.parse().expect("site key"),
            domain: format!(
                "{}.example",
                site.strip_prefix("site:").expect("fixture site")
            ),
            commit: record
                .worktree
                .as_ref()
                .expect("native worktree")
                .completion
                .as_ref()
                .expect("native completion")
                .report
                .commit
                .clone(),
            expected_remote_oid_or_unborn: release::RemoteState::Unborn,
        })
        .collect();
    let protected = [fixture.directory.path().to_owned()];
    let inspect = || {
        release::inspect_preparations(
            release::PreparationBatchRequest {
                database: &journal.database,
                expected: header,
                selections: &selections,
                protected_roots: &protected,
            },
            release::PreparationBatchOptions {
                inspection: release_commit::JournalWorktreeInspectionOptions {
                    read: read(),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
    };
    inspect().expect("captured response passes independent whole-history inspection");
    let record = &records[0].1;
    let original: BTreeMap<String, &RawValue> =
        serde_json::from_str(&record.intent.evidence).expect("original raw descendants");
    let candidate = original["candidate_check"].get();
    let changed = super::evidence::changed(
        candidate,
        &["requirement", "policy", "source_sha256"],
        json!(source::Sha256Digest::of(b"different policy source")),
    );
    let fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(&changed).expect("changed candidate fields");
    let envelope = format!(
        "{{\"format\":\"gazette-candidate-policy/1\",\"review\":{},\"response\":null,\"requirement\":{},\"policy_source\":{}}}",
        fields["review"].get(),
        fields["requirement"].get(),
        fields["policy_source"].get()
    );
    let changed = super::evidence::changed(
        &changed,
        &["invocation", "request_sha256"],
        json!(source::Sha256Digest::of(envelope.as_bytes())),
    );
    super::evidence::replace_record(&journal.database, record, &changed);
    let before = fs::read(&journal.database).expect("tampered disposable custody");
    let failure = inspect().expect_err("rebound hashes cannot authorize another policy source");
    assert!(
        matches!(failure, release::PreparationBatchError::Evidence),
        "{failure}"
    );
    assert_eq!(fs::read(&journal.database).expect("no repair"), before);
    assert_eq!(
        events(&log),
        calls,
        "history inspection cannot replay the checker"
    );
    super::evidence::replace_record(&journal.database, record, candidate);
    inspect().expect("restored exact source and response pass independent history");
}

#[test]
fn captured_candidate_failures_preserve_original_custody_before_invalid_theme_is_compiled() {
    for mode in [
        "malformed",
        "missing",
        "duplicate",
        "policy",
        "request",
        "exit",
        "hold",
        "exclude",
    ] {
        let fixture = published_fixture(&[
            ("theme/policy.json", POLICY),
            ("theme/templates/page.html", b"{{ invalid template"),
        ]);
        let root = private_directory();
        let journal = initialize(root.path());
        let baseline = fs::read(&journal.database).expect("original database");
        let parent = private_directory();
        let log = parent.path().join("refused.jsonl");
        let (config, required) = configuration(&fixture, &log, mode);
        let request = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        let (output, report) = result(&request, &arguments());
        assert!(!output.status.success(), "{mode}: {report}");
        assert_eq!(report["status"], "candidate-failed", "{mode}: {report}");
        assert_eq!(report["site"], "site:one");
        assert_eq!(report["completed"], json!({}));
        assert_eq!(report["completed_responses"], json!({}));
        let calls = events(&log);
        assert_eq!(calls.len(), 1, "no output checker after {mode}");
        assert_eq!(calls[0]["format"], "gazette-candidate-policy/1");
        let stdout = fs::read(parent.path().join("refused.jsonl.one.response"))
            .expect("retained failed response");
        assert_eq!(
            report["evidence"]["response_sha256"],
            json!(source::Sha256Digest::of(&stdout))
        );
        assert_eq!(report["evidence"]["response_bytes"], stdout.len());
        for site in ["one", "other"] {
            unchanged(&journal, id(site), &baseline);
            assert!(!parent.path().join(site).exists());
        }
    }
}

#[test]
fn later_candidate_failure_retains_earlier_exact_response_without_preparing_a_prefix() {
    let fixture = published_fixture(&[
        ("theme/policy.json", POLICY),
        ("theme/templates/page.html", b"{{ invalid template"),
    ]);
    let root = private_directory();
    let journal = initialize(root.path());
    let baseline = fs::read(&journal.database).expect("original database");
    let parent = private_directory();
    let log = parent.path().join("later.jsonl");
    let (config, required) = configuration(&fixture, &log, "later-hold");
    let request = request(
        &config,
        root.path(),
        journal.header,
        parent.path(),
        &required,
    );
    let (output, report) = result(&request, &arguments());
    assert!(!output.status.success(), "{report}");
    assert_eq!(report["status"], "candidate-failed");
    assert_eq!(report["site"], "site:other");
    assert_eq!(
        report["completed"]
            .as_object()
            .expect("completed invocations")
            .keys()
            .collect::<Vec<_>>(),
        vec!["site:one"]
    );
    assert_eq!(
        report["completed_responses"]
            .as_object()
            .expect("earlier responses")
            .keys()
            .collect::<Vec<_>>(),
        vec!["site:one"]
    );
    let original =
        fs::read_to_string(parent.path().join("later.jsonl.one.response")).expect("first stdout");
    assert_eq!(
        report["completed_responses"]["site:one"]["response_json"],
        original
    );
    let calls = events(&log);
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|call| call["format"] == "gazette-candidate-policy/1")
    );
    for site in ["one", "other"] {
        unchanged(&journal, id(site), &baseline);
        assert!(!parent.path().join(site).exists());
    }
}
