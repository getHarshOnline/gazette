//! Synthetic checker-originated evidence reaches real staging and `SQLite`/Git history.
//! This fixture qualifies mechanics, not an owner-approved publication policy.
//! Uses `GAZETTE_TEST_NODE` or portable test-only PATH selection for the checker runtime.

use std::{collections::BTreeMap, path::Path, sync::OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::{process, release};

use super::super::super::Fixture;
use super::*;

#[path = "output_policy/qualification.rs"]
pub(in crate::preparation) mod qualification;

const POLICY: &[u8] = br#"{"id":"policy:synthetic-output","revision":"revision:one","allowed_title":"One <escaped>","allowed_other_title":"Other"}"#;

pub(in crate::preparation) fn policy_fixture(overrides: &[(&str, &[u8])]) -> Fixture {
    let mut files = vec![("theme/policy.json", POLICY)];
    files.extend_from_slice(overrides);
    published_fixture(&files)
}

// The checker reads the retained policy document, never expected response coordinates.
const CHECKER: &str = r#"
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const [mode, log] = process.argv.slice(1);
const bytes = readFileSync(0);
writeFileSync(log, bytes);
const request = JSON.parse(bytes);
if (request.format !== 'gazette-output-policy/1' || request.candidate_check !== null ||
    'output_policy' in request || 'policy' in request) process.exit(51);
const model = JSON.parse(Buffer.from(request.files_base64['model.json'], 'base64'));
const subject = model.site === 'site:one' ? ['content:one', 'Retained', 'allowed_title'] :
    model.site === 'site:other' ? ['content:other', 'Other body', 'allowed_other_title'] : null;
if (!subject || !model.pages[subject[0]].body_html.includes(subject[1]) ||
    request.disclosure.findings.length !== 0) process.exit(52);
const sha = value => createHash('sha256').update(value).digest('hex');
const document = Buffer.from(request.policy_source.document_base64, 'base64');
const policy = JSON.parse(document);
if (sha(document) !== request.policy_source.sha256 ||
    request.policy_source.binding.source !== 'source:policy' ||
    request.policy_source.binding.entry !== 'policy.json') process.exit(53);
const response = {
  format: 'gazette-output-response/1', request_sha256: sha(bytes),
  policy: {id: policy.id, revision: policy.revision, source_sha256: sha(document)},
  outcome: model.pages[subject[0]].content.title === policy[subject[2]] ? 'allow' : 'hold',
  evidence: ['evidence:rendered-model', 'evidence:disclosure']
};
if (mode === 'policy') response.policy.revision = 'revision:private-wrong';
if (mode === 'request') response.request_sha256 = sha('not the delivered request');
if (mode === 'hold') response.outcome = 'hold';
if (mode === 'evidence') response.evidence = [];
if (mode === 'repeated-evidence') response.evidence.push(response.evidence[0]);
if (mode === 'policy-array') response.policy = Object.values(response.policy);
if (mode === 'outcome-map') response.outcome = {allow: {}};
if (mode === 'unknown') response.private_response_marker = 'never expose this secret';
let text = JSON.stringify(mode === 'root-array' ? Object.values(response) : response, null, 2);
if (mode === 'duplicate') text = text.replace('"outcome": "allow"',
  '"outcome": "allow", "out\\u0063ome": "allow"');
text = '\n  ' + text + '\n\t';
if (mode === 'large') text = ' '.repeat(96 * 1024) + text;
writeFileSync(log + '.response', text);
process.stdout.write(text, () => { if (mode === 'exit') process.exitCode = 17; });
"#;

pub(super) fn executable() -> &'static (String, process::ExecutablePin) {
    static NODE: OnceLock<(String, process::ExecutablePin)> = OnceLock::new();
    NODE.get_or_init(|| {
        let path = crate::native::node();
        let bytes = fs::read(path).expect("pin existing executable bytes");
        (
            path.to_str().expect("native fixture path").to_owned(),
            process::ExecutablePin {
                bytes: u64::try_from(bytes.len()).expect("native file length"),
                sha256: source::Sha256Digest::of(&bytes),
            },
        )
    })
}

fn configuration(
    fixture: &Fixture,
    log: &Path,
    mode: &str,
) -> (String, build::OutputPolicyRequirement) {
    let (config, required) = source_configuration(fixture, log, mode);
    (
        enable_site(
            config.replace(
                "site='site:one'\npublic_base_url=",
                "site='site:one'\ndestination={kind='git',remote='https://example.invalid/one.git',branch='publication'}\npublic_base_url=",
            ),
            "one",
        ),
        required,
    )
}

pub(in crate::preparation) fn source_configuration(
    fixture: &Fixture,
    log: &Path,
    mode: &str,
) -> (String, build::OutputPolicyRequirement) {
    let (program, pin) = executable();
    let command = process::Command {
        program: program.clone(),
        executable: Some(*pin),
        arguments: vec![
            "--input-type=module".into(),
            "--eval".into(),
            CHECKER.into(),
            mode.into(),
            log.to_str().expect("private observation path").into(),
        ],
        directory: log
            .parent()
            .expect("private observation parent")
            .to_str()
            .expect("native directory")
            .into(),
        environment: BTreeMap::new(),
    };
    let required = build::OutputPolicyRequirement {
        policy: build::ReviewPolicy {
            id: "policy:synthetic-output".parse().expect("policy key"),
            revision: "revision:one".parse().expect("revision key"),
            source_sha256: source::Sha256Digest::of(POLICY),
        },
        command_sha256: source::Sha256Digest::of(
            &serde_json::to_vec(&command).expect("independently selected compact command"),
        ),
        response_bytes: 256 << 10,
    };
    let config = format!(
        "policy_source={{source='source:policy',entry='policy.json'}}\npolicy_check={{program={},executable={{bytes={},sha256='{}'}},arguments={},directory={},environment={{}}}}\n{}[[sources]]\nid='source:policy'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n",
        serde_json::to_string(&command.program).expect("quoted native path"),
        pin.bytes,
        pin.sha256,
        serde_json::to_string(&command.arguments).expect("literal arguments"),
        serde_json::to_string(&command.directory).expect("quoted working directory"),
        fixture.config(),
        fixture
            .directory
            .path()
            .to_str()
            .expect("pinned fixture root"),
        fixture.commit,
    );
    (config, required)
}

fn request(
    config: &str,
    root: &Path,
    header: store::JournalHeader,
    parent: &Path,
    required: &build::OutputPolicyRequirement,
) -> Value {
    let mut value = input(config, root, header, parent);
    value["sites"] = json!([value["sites"][1].clone()]);
    assert_eq!(value["sites"][0]["render"]["site"], "site:one");
    value["sites"][0]["output_policy"] = json!(required);
    assert!(value["candidate_check"].is_null());
    value
}

fn execution() -> build::LocalPreparationRunOptions {
    let mut options = permitted();
    options.preparation.policy.allow_execute = true;
    options
}

fn assert_original_stage(record: &store::PreparationRecord, log: &Path) -> Value {
    let bytes = fs::read(log).expect("one actual delivered output request");
    let delivered: Value = serde_json::from_slice(&bytes).expect("delivered JSON");
    assert!(delivered["candidate_check"].is_null());
    assert!(delivered.get("output_policy").is_none());
    assert!(delivered.get("policy").is_none());
    assert_eq!(
        delivered["policy_source"]["binding"],
        json!({"source":"source:policy", "entry":"policy.json"})
    );
    assert_eq!(
        delivered["policy_source"]["document_base64"],
        STANDARD.encode(POLICY)
    );
    assert_eq!(
        delivered["policy_source"]["sha256"],
        json!(source::Sha256Digest::of(POLICY))
    );
    assert!(delivered["policy_source"]["tree"].is_object());
    assert!(delivered["files_base64"].get("policy.json").is_none());
    assert_eq!(delivered["disclosure"]["findings"], json!([]));
    assert!(record.retention.is_some());
    for (path, encoded) in delivered["files_base64"]
        .as_object()
        .expect("complete output")
    {
        let original = STANDARD
            .decode(encoded.as_str().expect("encoded bytes"))
            .expect("base64 transport");
        assert_eq!(
            fs::read(record.intent.address.path().join(path)).expect("retained exact file"),
            original
        );
    }
    let intent: Value = serde_json::from_str(&record.intent.evidence).expect("original intent");
    assert!(intent["candidate_check"].is_null());
    assert_eq!(
        delivered["inventory"]["identity"],
        json!(record.intent.output)
    );
    delivered
}

#[test]
fn pinned_checker_matches_actual_output_without_candidate_and_survives_independent_history() {
    let fixture = policy_fixture(&[]);
    for via_cli in [false, true] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let log = parent.path().join("delivered.json");
        let (config, required) = configuration(&fixture, &log, "allow");
        let input = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        let report = if via_cli {
            let mut arguments = ARGUMENTS.to_vec();
            arguments.push("--allow-policy-execution");
            let (output, report) = result(&input, &arguments);
            assert!(output.status.success(), "{output:?}; {report}");
            assert!(output.stderr.is_empty());
            report
        } else {
            let result = recipe(&input).execute(execution());
            assert!(result.is_ok(), "{result:?}");
            serde_json::to_value(build::LocalPreparationRunReport::new(&result))
                .expect("SDK receipt")
        };
        assert_eq!(
            report["status"],
            "local-preparations-complete-not-certified"
        );
        let record = store::inspect_preparation(&journal.database, id("one"), None, read())
            .expect("fresh native history")
            .record
            .expect("complete original record");
        let delivered = assert_original_stage(&record, &log);
        let check: Value =
            serde_json::from_str(&record.check.as_ref().expect("recorded check").evidence)
                .expect("original private check");
        let evidence = &check["policy"]["evidence"];
        assert_eq!(evidence["policy_source"], delivered["policy_source"]);
        let response =
            fs::read_to_string(log.with_extension("json.response")).expect("original stdout");
        assert!(response.starts_with("\n  ") && response.ends_with("\n\t"));
        assert_eq!(evidence["response"]["response_json"], response);
        assert_eq!(
            evidence["response"]["response_sha256"],
            json!(source::Sha256Digest::of(response.as_bytes()))
        );
        assert_eq!(evidence["response"]["requirement"], json!(required));
        assert_eq!(
            evidence["invocation"]["request_sha256"],
            json!(source::Sha256Digest::of(
                &fs::read(&log).expect("delivered bytes")
            ))
        );
        assert_eq!(
            evidence["invocation"]["command_sha256"],
            json!(required.command_sha256)
        );
        assert!(evidence["candidate_check_sha256"].is_null());
        let completion = record
            .worktree
            .as_ref()
            .expect("native worktree")
            .completion
            .as_ref()
            .expect("actual Git completion");
        let expected = store::inspect_preparation(&journal.database, id("one"), None, read())
            .expect("current journal")
            .journal
            .header;
        let mut selection = release::PreparationSelection {
            preparation_id: id("one"),
            site: "site:one".parse().expect("site"),
            domain: "one.example".into(),
            commit: completion.report.commit.clone(),
            expected_remote_oid_or_unborn: release::RemoteState::Unborn,
            output_policy: Some(required.clone()),
            qualification: None,
        };
        let inspect = |selection: &release::PreparationSelection| {
            release::inspect_preparations(
                release::PreparationBatchRequest {
                    database: &journal.database,
                    expected,
                    selections: std::slice::from_ref(selection),
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
        };
        inspect(&selection)
            .expect("independent expected command and policy match retained native evidence");
        for command in [false, true] {
            let mut wrong = required.clone();
            if command {
                wrong.command_sha256 = source::Sha256Digest::of(b"independent different command");
            } else {
                wrong.policy.revision = "revision:other".parse().expect("different revision");
            }
            selection.output_policy = Some(wrong);
            assert_eq!(
                inspect(&selection)
                    .expect_err("history cannot approve its own selected requirement")
                    .code(),
                "release_preparation_mismatch"
            );
        }
        assert_eq!(
            store::inspect_preparation(&journal.database, id("one"), Some(&expected), read())
                .expect("read-only mismatch preserves record")
                .record
                .as_ref(),
            Some(&record)
        );
        assert_eq!(
            fs::read_to_string(log.with_extension("json.response")).expect("no checker replay"),
            response
        );
    }
}

#[test]
fn independently_wrong_command_or_missing_pin_refuses_before_stage_and_any_candidate_invocation() {
    let fixture = policy_fixture(&[]);
    for missing_pin in [false, true] {
        let root = private_directory();
        let journal = initialize(root.path());
        let before = fs::read(&journal.database).expect("original database");
        let parent = private_directory();
        let log = parent.path().join("not-invoked.json");
        let (mut config, mut required) = configuration(&fixture, &log, "allow");
        if missing_pin {
            let start = config.find("executable={").expect("explicit pin");
            let end = start + config[start..].find("},arguments=").expect("pin boundary") + 2;
            config.replace_range(start..end, "");
            let inputs = super::super::super::acquire(&config);
            let command = inputs
                .configuration()
                .policy_command()
                .expect("bound policy command")
                .expect("unpinned command");
            assert!(command.executable.is_none());
            required.command_sha256 = source::Sha256Digest::of(
                &serde_json::to_vec(command).expect("correct digest cannot substitute for a pin"),
            );
        } else {
            required.command_sha256 =
                source::Sha256Digest::of(b"independently selected other command");
        }
        let mut input = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        // Even an explicitly requested candidate check must not run before this refusal.
        input["candidate_check"] = json!(build::ReviewLimits::default());
        let error = recipe(&input)
            .execute(execution())
            .expect_err("strong binding is a pre-effect gate");
        let report = serde_json::to_value(&error).expect("native refusal");
        assert_eq!(report["status"], "not-started");
        assert_eq!(report["code"], "output_policy_response_binding");
        unchanged(&journal, id("one"), &before);
        assert!(!log.exists());
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no stage or checker effects")
                .count(),
            0
        );
    }
}

#[test]
fn malformed_or_refusing_captured_response_retains_exact_stage_but_never_check_or_git_completion() {
    let fixture = policy_fixture(&[]);
    for (mode, code) in [
        ("policy", "output_policy_response_binding"),
        ("request", "output_policy_response_binding"),
        ("hold", "output_policy_response_disposition"),
        ("evidence", "output_policy_response_evidence"),
        ("repeated-evidence", "output_policy_response_evidence"),
        ("duplicate", "output_policy_response_shape"),
        ("root-array", "output_policy_response_shape"),
        ("policy-array", "output_policy_response_shape"),
        ("outcome-map", "output_policy_response_shape"),
        ("unknown", "output_policy_response_shape"),
        ("exit", "output_policy_response_exit"),
        ("large", "process_output_limit"),
    ] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let log = parent.path().join("delivered.json");
        let (config, mut required) = configuration(&fixture, &log, mode);
        if mode == "large" {
            required.response_bytes = 1024;
        }
        let input = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        let outcome = recipe(&input).execute(execution());
        let error = outcome
            .as_ref()
            .expect_err("zero exit alone cannot satisfy the selected response binding");
        let report = serde_json::to_value(build::LocalPreparationRunReport::new(&outcome))
            .expect("shared native failure report");
        assert_eq!(report["status"], "incomplete", "{mode}: {report}");
        assert_eq!(report["completed"], json!([]));
        assert_eq!(report["candidate_checks"], json!({}));
        assert_eq!(report["failure"]["phase"]["kind"], "policy");
        let failure = &report["failure"]["phase"]["evidence"];
        assert_eq!(failure["code"], code, "{mode}: {report}");
        assert_eq!(
            failure["invocation"]["command_sha256"],
            json!(required.command_sha256)
        );
        let received = fs::read(&log).expect("actual complete checker request");
        assert_eq!(
            failure["invocation"]["request_sha256"],
            json!(source::Sha256Digest::of(&received))
        );
        let original =
            fs::read(log.with_extension("json.response")).expect("original checker stdout");
        if mode != "large" {
            assert_eq!(
                failure["response_sha256"],
                json!(source::Sha256Digest::of(&original))
            );
            assert_eq!(failure["response_bytes"], original.len());
            assert_eq!(failure["exit"]["code"], if mode == "exit" { 17 } else { 0 });
            let build::LocalPreparationRunError::Preparation { error, .. } = error else {
                panic!("expected exact native preparation failure: {error:?}");
            };
            let build::LocalPreparationSetFailure::Site { error, .. } = error.cause.as_ref() else {
                panic!("expected exact failed site");
            };
            let build::LocalPreparationFailure::Policy(build::PolicyCheckError::OutputResponse {
                captured,
                ..
            }) = error.cause.as_ref()
            else {
                panic!("original captured response must survive refusal: {error:?}");
            };
            assert_eq!(captured.stdout, original);
        } else {
            assert!(
                failure["response_sha256"].is_null() && failure["response_bytes"].is_null(),
                "an overflow prefix is not a completed response"
            );
        }
        let encoded = serde_json::to_string(&report).expect("redacted operator report");
        assert!(!encoded.contains("never expose this secret"));
        assert!(!encoded.contains("revision:private-wrong"));
        assert!(!encoded.contains("response_json"));
        let observed = store::inspect_preparation(&journal.database, id("one"), None, read())
            .expect("fresh native observation");
        assert!(observed.journal.header.writer_fence > journal.header.writer_fence);
        let record = observed.record.expect("original retained stage intent");
        assert_original_stage(&record, &log);
        assert!(record.check.is_none() && record.worktree.is_none());
        assert!(!record.intent.address.path().join(".git").exists());
        assert_eq!(report["failure"]["prior_receipt"]["record"], json!(record));
    }
}

#[test]
fn completed_response_survives_later_check_evidence_overflow_without_git_or_hidden_retry() {
    let fixture = policy_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("delivered.json");
    let (config, required) = configuration(&fixture, &log, "large");
    let input = request(
        &config,
        root.path(),
        journal.header,
        parent.path(),
        &required,
    );
    let mut options = execution();
    options.preparation.stage.evidence_bytes = 64 << 10;
    let outcome = recipe(&input).execute(options);
    assert!(
        outcome.is_err(),
        "large completed response cannot fit the independent history bound"
    );
    let report = serde_json::to_value(build::LocalPreparationRunReport::new(&outcome))
        .expect("native evidence");
    assert_eq!(report["status"], "incomplete", "{report}");
    assert_eq!(report["failure"]["phase"]["kind"], "checks");
    let completed = &report["failure"]["output_policy"];
    assert_eq!(completed["status"], "output-policy-check-passed-unverified");
    let original =
        fs::read_to_string(log.with_extension("json.response")).expect("completed exact response");
    assert!(original.len() > options.preparation.stage.evidence_bytes);
    assert!(original.len() <= required.response_bytes);
    assert_eq!(completed["response"]["response_json"], original);
    assert_eq!(
        completed["response"]["response_sha256"],
        json!(source::Sha256Digest::of(original.as_bytes()))
    );
    assert_eq!(completed["response"]["requirement"], json!(required));
    assert_eq!(
        completed["invocation"]["request_sha256"],
        json!(source::Sha256Digest::of(
            &fs::read(&log).expect("actual input")
        ))
    );
    let record = store::inspect_preparation(&journal.database, id("one"), None, read())
        .expect("original staged record remains available")
        .record
        .expect("intent and retention");
    assert!(record.intent.evidence.len() <= options.preparation.stage.evidence_bytes);
    assert_original_stage(&record, &log);
    assert!(record.check.is_none() && record.worktree.is_none());
    assert!(!record.intent.address.path().join(".git").exists());
    assert_eq!(report["failure"]["prior_receipt"]["record"], json!(record));
}

#[test]
fn retained_policy_evaluates_actual_disallowed_output_instead_of_expected_coordinates() {
    let page = super::super::super::PAGE
        .replace("lifecycle='draft'", "lifecycle='published'")
        .replace("One <escaped>", "Disallowed synthetic title");
    let fixture = policy_fixture(&[("one/page.md", page.as_bytes())]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("delivered.json");
    let (config, required) = configuration(&fixture, &log, "allow");
    let outcome = recipe(&request(
        &config,
        root.path(),
        journal.header,
        parent.path(),
        &required,
    ))
    .execute(execution());
    assert!(outcome.is_err());
    let report = serde_json::to_value(build::LocalPreparationRunReport::new(&outcome))
        .expect("native refusal report");
    assert_eq!(
        report["failure"]["phase"]["evidence"]["code"],
        "output_policy_response_disposition"
    );
    let response: Value = serde_json::from_slice(
        &fs::read(log.with_extension("json.response")).expect("actual evaluator response"),
    )
    .expect("response document");
    assert_eq!(response["policy"], json!(required.policy));
    assert_eq!(response["outcome"], "hold");
    let record = store::inspect_preparation(&journal.database, id("one"), None, read())
        .expect("native history")
        .record
        .expect("retained stage");
    let delivered = assert_original_stage(&record, &log);
    let model: Value = serde_json::from_slice(
        &STANDARD
            .decode(
                delivered["files_base64"]["model.json"]
                    .as_str()
                    .expect("encoded model"),
            )
            .expect("retained model bytes"),
    )
    .expect("actual rendered model");
    assert_eq!(
        model["pages"]["content:one"]["content"]["title"],
        "Disallowed synthetic title"
    );
    assert!(record.check.is_none() && record.worktree.is_none());
}

#[test]
fn absent_or_mismatched_policy_source_refuses_before_stage_or_checker() {
    let fixture = policy_fixture(&[]);
    let oversized = vec![b' '; (1 << 20) + 1];
    let oversized_fixture = policy_fixture(&[("theme/policy.json", &oversized)]);
    for case in ["digest", "binding", "source", "file", "large"] {
        let root = private_directory();
        let journal = initialize(root.path());
        let before = fs::read(&journal.database).expect("original database");
        let parent = private_directory();
        let log = parent.path().join("must-not-run.json");
        let selected = if case == "large" {
            &oversized_fixture
        } else {
            &fixture
        };
        let (mut config, mut required) = configuration(selected, &log, "allow");
        match case {
            "digest" => {
                required.policy.source_sha256 = source::Sha256Digest::of(b"different policy bytes")
            }
            "binding" => {
                config = config.replace(
                    "policy_source={source='source:policy',entry='policy.json'}\n",
                    "",
                )
            }
            "source" => {
                config = config.replace("source='source:policy'", "source='source:missing'")
            }
            "file" => config = config.replace("entry='policy.json'", "entry='missing.json'"),
            "large" => required.policy.source_sha256 = source::Sha256Digest::of(&oversized),
            _ => unreachable!(),
        }
        let mut input = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        if case != "source" {
            // Policy-source validation must precede even an explicitly selected candidate process.
            input["candidate_check"] = json!(build::ReviewLimits::default());
        }
        let outcome = recipe(&input).execute(execution());
        let error = outcome
            .as_ref()
            .expect_err("policy input is a pre-effect gate");
        let report = serde_json::to_value(error).expect("native error");
        assert_eq!(report["status"], "not-started", "{case}: {report}");
        unchanged(&journal, id("one"), &before);
        assert!(!log.exists(), "{case}");
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no stage or checker effects")
                .count(),
            0,
            "{case}"
        );
    }
}

#[test]
fn checker_uses_retained_policy_after_native_policy_and_content_change() {
    let fixture = policy_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("delivered.json");
    let (config, required) = configuration(&fixture, &log, "allow");
    let inputs = super::super::super::acquire(&config);
    let policy_tree = inputs
        .sources()
        .iter()
        .find(|(key, _)| key.as_str() == "source:policy")
        .expect("exact retained source")
        .1
        .inventory()
        .identity();
    fs::write(
        fixture.directory.path().join("theme/policy.json"),
        b"not the retained policy",
    )
    .expect("later native policy mutation");
    fs::write(
        fixture.directory.path().join("one/page.md"),
        b"not the retained authored content",
    )
    .expect("later native content mutation");
    let rendered = super::super::render(&inputs).expect("render from retained inputs");
    let metadata = metadata();
    let disclosure = disclosure_policy();
    let completed = build::prepare_local(
        &rendered,
        build::LocalPreparationRequest {
            output_policy: Some(&required),
            qualification: None,
            coordinator: None,
            stage: build::PreparationRequest {
                journal: root.path(),
                expected: journal.header,
                parent: parent.path(),
                name: "one",
                id: id("one"),
            },
            metadata: &metadata,
            parent_source: None,
            disclosure: &disclosure,
        },
        execution().preparation,
    )
    .expect("checker consumes frozen source despite later native mutation");
    let delivered = assert_original_stage(&completed.record, &log);
    assert_eq!(delivered["policy_source"]["tree"], json!(policy_tree));
    let evidence: Value = serde_json::from_str(
        &completed
            .record
            .check
            .as_ref()
            .expect("recorded check")
            .evidence,
    )
    .expect("private retained evidence");
    assert_eq!(
        evidence["policy"]["evidence"]["policy_source"],
        delivered["policy_source"]
    );
}
