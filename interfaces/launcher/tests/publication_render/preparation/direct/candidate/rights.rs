//! Synthetic owner-record conformance through native policy custody, not real-owner approval.

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::{process, release};
use std::{collections::BTreeMap, path::Path};

const NAME: &str = "Fixture Author";
const LICENSE: &str = "https://example.invalid/content-license";
const PRIVATE_CONTACT: &str = "SYNTHETIC-CONTACT-NOT-PERMITTED";

// This finite synthetic policy intentionally consumes only the delivered immutable
// request and retained record. No clock, provider, remote identity lookup or mode
// flag determines its decisions. It is not a reusable policy implementation.
const CHECKER: &str = r#"
import {readFileSync, appendFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const [log] = process.argv.slice(1);
const input = readFileSync(0);
const request = JSON.parse(input);
appendFileSync(log, input.toString() + '\n');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const document = Buffer.from(request.policy_source.document_base64, 'base64');
const policy = JSON.parse(document);
const record = policy.record;
if (sha(document) !== request.policy_source.sha256 ||
    sha(JSON.stringify(record)) !== policy.record_sha256 ||
    record.subject !== 'owner:one' || record.owner !== 'authority:synthetic-owner' ||
    record.revision !== 'record:one') process.exit(61);
const coordinate = {id:policy.id, revision:policy.revision, source_sha256:sha(document)};
const site = request.format === 'gazette-candidate-policy/1'
  ? request.review.request.site : request.candidate_check.review.request.site;
const permitted = record.status === 'active' && record.destinations.includes(site) &&
  policy.evaluated_at >= record.valid_from && policy.evaluated_at < record.valid_until &&
  record.public_fields.includes('name') && record.public_fields.includes('license');
let response;
if (request.format === 'gazette-candidate-policy/1') {
  if (request.response !== null || coordinate.id !== request.requirement.policy.id ||
      coordinate.revision !== request.requirement.policy.revision ||
      coordinate.source_sha256 !== request.requirement.policy.source_sha256) process.exit(62);
  const decisions = [];
  let names = 0, licences = 0;
  function visit(surface, pointer, value) {
    if (value === record.name) names++;
    if (value === record.license) licences++;
    const allowed = permitted && value !== record.private_contact;
    decisions.push({surface, pointer, outcome:allowed ? 'allow' : 'hold', evidence:[record.evidence]});
    if (Array.isArray(value)) value.forEach((item, i) => visit(surface, pointer + '/' + i, item));
    else if (value !== null && typeof value === 'object') {
      for (const key of Object.keys(value).sort())
        visit(surface, pointer + '/' + key.replaceAll('~','~0').replaceAll('/','~1'), value[key]);
    }
  }
  request.review.request.surfaces.forEach((surface, index) => visit(index, '', surface.value));
  if (!names || !licences || !decisions.length) process.exit(63);
  response = {format:'gazette-review-response/1',request_sha256:request.review.sha256,
    policy:coordinate,decisions};
} else if (request.format === 'gazette-output-policy/1') {
  const files = Object.fromEntries(Object.entries(request.files_base64)
    .map(([path, value]) => [path, Buffer.from(value, 'base64').toString()]));
  const page = JSON.parse(files['model.json']).pages['content:one'];
  const faithful = page.content.license === record.license &&
    page.attributions.some(item => item.role === 'author' &&
      item.principal.id === record.subject && item.principal.name === record.name);
  // An exact synthetic forbidden contact token is a byte-level disclosure rule,
  // not an HTML parser or a purported general public-field policy.
  const leaked = Object.values(files).some(bytes => bytes.includes(record.private_contact));
  const allowed = permitted && faithful && !leaked && request.disclosure.findings.length === 0 &&
    request.candidate_check.response.decisions.every(item => item.outcome === 'allow');
  response = {format:'gazette-output-response/1',request_sha256:sha(input),policy:coordinate,
    outcome:allowed ? 'allow' : 'hold',evidence:[record.evidence]};
} else process.exit(64);
const output = JSON.stringify(response);
process.stdout.write(output);
"#;

fn policy(case: &str) -> Vec<u8> {
    let record = json!({
        "subject":"owner:one", "owner":"authority:synthetic-owner", "revision":"record:one",
        "name":NAME, "license":LICENSE, "private_contact":PRIVATE_CONTACT,
        "public_fields":["name","license"], "evidence":"evidence:synthetic-consent",
        "status":if matches!(case, "revoked" | "archived-hold") { "revoked" } else { "active" },
        "destinations":[if case == "scope" { "site:other" } else { "site:one" }],
        "valid_from":100, "valid_until":200
    });
    serde_json::to_vec(&json!({
        "id":"policy:synthetic-rights", "revision":"policy-revision:one", "evaluated_at":150,
        "record_sha256":source::Sha256Digest::of(&serde_json::to_vec(&record).unwrap()),
        "record":record
    }))
    .expect("exact synthetic instance policy, not a Gazette contract")
}

fn configuration(
    fixture: &crate::Fixture,
    log: &Path,
    policy: &[u8],
) -> (
    String,
    build::CandidatePolicyRequirement,
    build::OutputPolicyRequirement,
) {
    let (program, pin) = super::super::output_policy::executable();
    let command = process::Command {
        program: program.clone(),
        executable: Some(*pin),
        arguments: vec![
            "--input-type=module".into(),
            "--eval".into(),
            CHECKER.into(),
            log.to_str().unwrap().into(),
        ],
        directory: log.parent().unwrap().to_str().unwrap().into(),
        environment: BTreeMap::new(),
    };
    let policy = build::ReviewPolicy {
        id: "policy:synthetic-rights".parse().unwrap(),
        revision: "policy-revision:one".parse().unwrap(),
        source_sha256: source::Sha256Digest::of(policy),
    };
    let command_sha256 = source::Sha256Digest::of(&serde_json::to_vec(&command).unwrap());
    let candidate = build::CandidatePolicyRequirement {
        policy: policy.clone(),
        command_sha256,
    };
    let output = build::OutputPolicyRequirement {
        policy,
        command_sha256,
        response_bytes: 1 << 20,
    };
    let config = enable_site(fixture.config()
        .replace("kind='person'\n", &format!("kind='person'\nname='{NAME}'\n"))
        .replace("site='site:one'\npublic_base_url=", "site='site:one'\ndestination={kind='git',remote='https://example.invalid/one.git',branch='publication'}\npublic_base_url="), "one");
    let config = format!(
        "policy_source={{source='source:policy',entry='policy.json'}}\npolicy_check={{program={},executable={{bytes={},sha256='{}'}},arguments={},directory={},environment={{}}}}\n{}[[sources]]\nid='source:policy'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n",
        serde_json::to_string(&command.program).unwrap(),
        pin.bytes,
        pin.sha256,
        serde_json::to_string(&command.arguments).unwrap(),
        serde_json::to_string(&command.directory).unwrap(),
        config,
        fixture.directory.path().to_str().unwrap(),
        fixture.commit,
    );
    (config, candidate, output)
}

#[test]
fn retained_owner_record_gates_candidate_output_and_qualified_reopen_without_granting_authority() {
    for case in ["allow", "revoked", "scope", "archived-hold", "theme-leak"] {
        let policy = policy(case);
        let lifecycle = if case == "archived-hold" {
            "archived"
        } else {
            "published"
        };
        let page = crate::PAGE.replace("lifecycle='draft'", &format!(
            "lifecycle='{lifecycle}'\nlicense='{LICENSE}'\nattributions=[{{principal='owner:one',role='author'}}]"
        ));
        let template = format!(
            "{}{{% for item in attributions %}}<p>{{{{ item.principal.name }}}}</p>{{% endfor %}}{{% if content.license is defined %}}<a href=\"{{{{ content.license }}}}\">Licence</a>{{% endif %}}{}",
            crate::TEMPLATE,
            if case == "theme-leak" {
                PRIVATE_CONTACT
            } else {
                ""
            }
        );
        let fixture = published_fixture(&[
            ("one/page.md", page.as_bytes()),
            ("theme/policy.json", &policy),
            ("theme/templates/page.html", template.as_bytes()),
        ]);
        let root = private_directory();
        let journal = initialize(root.path());
        let baseline = fs::read(&journal.database).unwrap();
        let parent = private_directory();
        let log = parent.path().join("rights.jsonl");
        let (config, candidate, output_policy) = configuration(&fixture, &log, &policy);
        let mut request = checked_request(&config, root.path(), journal.header, parent.path());
        request["sites"] = json!([request["sites"][1].clone()]);
        request["candidate_policies"] = json!({"site:one":candidate});
        request["sites"][0]["output_policy"] = json!(output_policy);
        let qualification = if matches!(case, "allow" | "theme-leak") {
            let inputs = crate::sites::acquire_set(&config, &["site:one"]);
            let renders = captured::render(&inputs, &candidate);
            let rendered = renders.sites()[&"site:one".parse().unwrap()]
                .as_ref()
                .unwrap();
            let mut required =
                super::super::output_policy::qualification::expected(rendered, &output_policy);
            required.candidate_policy = Some(candidate.clone());
            request["sites"][0]["qualification"] = json!(required);
            request["sites"][0]
                .as_object_mut()
                .unwrap()
                .remove("output_policy");
            Some(required)
        } else {
            None
        };
        let prior_calls = if log.exists() { events(&log).len() } else { 0 };
        let mut arguments = ARGUMENTS.to_vec();
        arguments.push("--allow-policy-execution");
        let (native, report) = result(&request, &arguments);
        let calls = events(&log);
        let actual = &calls[prior_calls..];
        assert_eq!(actual[0]["format"], "gazette-candidate-policy/1");
        assert!(
            !actual[0]["review"]["request"]["surfaces"]
                .to_string()
                .contains(PRIVATE_CONTACT)
        );
        if case == "archived-hold" {
            assert!(
                actual[0]["review"]["request"]["surfaces"]
                    .to_string()
                    .contains("archived"),
                "archived is a candidate passed to owner policy, not a hardcoded Engine exclusion"
            );
        }
        if matches!(case, "revoked" | "scope" | "archived-hold") {
            assert!(!native.status.success(), "{case}: {report}");
            assert_eq!(report["status"], "candidate-failed");
            assert_eq!(actual.len(), 1);
            unchanged(&journal, id("one"), &baseline);
            assert!(!parent.path().join("one").exists());
            continue;
        }
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[1]["format"], "gazette-output-policy/1");
        assert!(
            actual[1]["candidate_check"]["response"]["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["outcome"] == "allow")
        );
        let observed =
            store::inspect_preparation(&journal.database, id("one"), None, read()).unwrap();
        let record = observed.record.as_ref().unwrap();
        if case == "theme-leak" {
            assert!(!native.status.success(), "{report}");
            assert_eq!(
                report["failure"]["phase"]["evidence"]["code"],
                "output_policy_response_disposition"
            );
            assert!(record.retention.is_some());
            assert!(record.check.is_none());
            assert!(record.worktree.is_none());
            continue;
        }
        assert!(native.status.success(), "{native:?}; {report}");
        let intent: Value = serde_json::from_str(&record.intent.evidence).unwrap();
        assert_eq!(intent["candidate_check"]["requirement"], json!(candidate));
        assert_eq!(
            STANDARD
                .decode(
                    intent["candidate_check"]["policy_source"]["document_base64"]
                        .as_str()
                        .unwrap()
                )
                .unwrap(),
            policy
        );
        let check: Value = serde_json::from_str(&record.check.as_ref().unwrap().evidence).unwrap();
        assert_eq!(
            check["technical_certificate"]["requirements"],
            json!(qualification)
        );
        let selection = release::PreparationSelection {
            preparation_id: id("one"),
            site: "site:one".parse().unwrap(),
            domain: "one.example".into(),
            commit: record
                .worktree
                .as_ref()
                .unwrap()
                .completion
                .as_ref()
                .unwrap()
                .report
                .commit
                .clone(),
            expected_remote_oid_or_unborn: release::RemoteState::Unborn,
            output_policy: None,
            qualification: qualification.map(release::PreparationAdmission::authored),
        };
        let batch = release::inspect_preparations(
            release::PreparationBatchRequest {
                database: &journal.database,
                expected: observed.journal.header,
                selections: std::slice::from_ref(&selection),
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
        .expect("independent retained-record qualification, not policy approval");
        assert_eq!(
            json!(batch)["status"],
            "supported-profile-technical-preparations"
        );
        assert_eq!(
            events(&log),
            calls,
            "reopen never reruns policy or native effects"
        );
    }
}
