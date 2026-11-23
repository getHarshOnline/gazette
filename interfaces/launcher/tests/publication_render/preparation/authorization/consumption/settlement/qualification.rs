//! Synthetic supported-output qualification joins the actual two-site release transaction.
//! Native mechanics evidence only: no publication-owner approval or external destination.

use super::{Fixture, Local, Repository, consume_options, fs, read, release, store};
use crate::preparation::authorization::{batch_options, cli, verify};
use gazette_sdk::source::Sha256Digest;
use serde_json::{Value, json};

fn native_refs(local: &Local) -> Vec<String> {
    local
        .paths
        .iter()
        .map(|path| {
            Repository::open_bare(path)
                .expect("disposable native destination")
                .refname_to_id("refs/heads/publication")
                .expect("exact native branch")
                .to_string()
        })
        .collect()
}

fn assert_admitted_surface_parity(record: &store::PreparationRecord, ordinal: usize) {
    let root = record.intent.address.path();
    let (content, html_path, visible_title, body) = match ordinal {
        0 => (
            "content:one",
            "guide/index.html",
            "One &lt;escaped&gt;",
            "Retained",
        ),
        1 => (
            "content:other",
            "elsewhere/index.html",
            "Other",
            "Other body",
        ),
        _ => panic!("fixture has exactly two publication targets"),
    };
    let read_json = |path: &str| -> Value {
        serde_json::from_slice(&fs::read(root.join(path)).expect("retained machine output"))
            .expect("machine output is valid JSON")
    };
    let model = read_json("model.json");
    let page = &model["pages"][content];
    let canonical = page["route"]["canonical_url"]
        .as_str()
        .expect("one canonical URL from the shared page model");
    let html = String::from_utf8(fs::read(root.join(html_path)).expect("retained HTML"))
        .expect("fixture HTML");
    assert!(
        html.contains(visible_title),
        "HTML title differs from admitted model"
    );
    assert!(html.contains(body), "HTML body differs from admitted model");
    let escaped_canonical = canonical.replace('/', "&#x2F;");
    assert!(
        html.contains(&escaped_canonical),
        "HTML route differs from admitted model"
    );

    let search = read_json("search.json");
    let search_page = search
        .as_array()
        .expect("search documents")
        .iter()
        .find(|item| item["content"]["id"] == content)
        .expect("admitted page in search output");
    assert_eq!(search_page["content"], page["content"]);
    assert_eq!(search_page["route"], page["route"]);
    assert_eq!(search_page["body"], page["body_text"]);

    let json_ld = read_json("site.jsonld");
    let graph_page = json_ld["@graph"]
        .as_array()
        .expect("JSON-LD graph")
        .iter()
        .find(|item| item["identifier"] == content)
        .expect("admitted page in JSON-LD output");
    assert_eq!(graph_page, &page["json_ld"]);
    assert!(
        String::from_utf8(fs::read(root.join("sitemap.xml")).expect("retained sitemap"))
            .expect("sitemap XML")
            .contains(canonical),
        "sitemap route differs from admitted model"
    );
    assert!(
        String::from_utf8(fs::read(root.join("llms.txt")).expect("retained llms output"))
            .expect("llms text")
            .contains(canonical),
        "llms route differs from admitted model"
    );
}

#[test]
fn output_only_technical_qualification_cannot_enter_release_registration() {
    let mut local = Local::qualified();
    assert_eq!(
        json!(local.prepared.inspect())["status"],
        "supported-profile-technical-preparations"
    );
    let (mut writer, bytes, binding) = local.signed(false);
    let error = writer
        .register_release_authorization(
            release::PreparedAuthorizationRequest {
                authorization: &bytes,
                binding: &binding,
                preparations: local.prepared.request(&local.prepared.selections),
            },
            release::RegisterAuthorizationOptions {
                verification: super::super::verification(),
                record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
            },
        )
        .expect_err("output-only technical qualification is not public admission");
    assert!(matches!(
        error,
        release::RegisterAuthorizationError::Verification(
            release::PreparedAuthorizationError::Preparation(
                release::PreparationBatchError::Admission
            )
        )
    ));
    assert_eq!(local.reads(), 0);
    writer.close().expect("close refused registration writer");
}

#[test]
fn captured_candidate_qualified_two_site_outputs_reach_signed_local_settlement() {
    qualified_transaction(Local::candidate_qualified());
}

fn qualified_transaction(local: Local) {
    let batch = local.prepared.inspect();
    assert_eq!(
        json!(batch)["status"],
        "supported-profile-technical-preparations"
    );
    for (ordinal, (record, selection)) in local
        .prepared
        .records
        .iter()
        .zip(&local.prepared.selections)
        .enumerate()
    {
        assert_admitted_surface_parity(record, ordinal);
        let check: Value = serde_json::from_str(&record.check.as_ref().unwrap().evidence).unwrap();
        assert_eq!(
            check["technical_certificate"]["requirements"],
            json!(
                selection
                    .qualification
                    .as_ref()
                    .and_then(release::PreparationAdmission::as_authored)
            )
        );
        assert_eq!(
            check["technical_certificate"]["output"],
            json!(record.intent.output)
        );
        assert!(
            selection.qualification.is_some(),
            "required before preparation from independent render"
        );
        assert!(
            selection.output_policy.is_none(),
            "qualification carries the single policy requirement"
        );
        let expected = selection
            .qualification
            .as_ref()
            .and_then(release::PreparationAdmission::as_authored)
            .unwrap();
        if let Some(required) = &expected.candidate_policy {
            let intent: Value = serde_json::from_str(&record.intent.evidence).unwrap();
            assert_eq!(intent["candidate_check"]["requirement"], json!(required));
            assert_eq!(
                intent["candidate_check"]["response"]["policy"],
                json!(required.policy)
            );
            assert_eq!(
                intent["candidate_check"]["invocation"]["command_sha256"],
                json!(required.command_sha256)
            );
            assert!(
                intent["candidate_check"]["response"]["decisions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|decision| decision["outcome"] == "allow")
            );
        } else {
            assert!(
                check["technical_certificate"]["requirements"]
                    .get("candidate_policy")
                    .is_none(),
                "ordinary profile retains omission"
            );
        }
    }
    let output = cli(
        &local.prepared.envelope().to_string(),
        &["inspect-release-preparations", "--allow-coordination-files"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!(batch)
    );

    let mut historical = local.prepared.selections.clone();
    for selected in &mut historical {
        selected.qualification = None;
    }
    let historical =
        release::inspect_preparations(local.prepared.request(&historical), batch_options()).expect(
            "ordinary historical inspection still independently rederives present certificates",
        );
    assert_eq!(
        json!(historical)["status"],
        "local-preparations-inspected-not-certified"
    );

    let log = local.prepared.checker.as_ref().unwrap().1.clone();
    fs::write(
        &log,
        b"preparation checker must not run during release or replay",
    )
    .unwrap();
    let log_bytes = fs::read(&log).unwrap();
    let mut fixture = Fixture::from_local(local);
    assert_eq!(fixture.consumed.targets.len(), 2);
    assert_eq!(
        fixture.binding.publishers[0].authorized_target_ordinals,
        vec![0, 1]
    );
    assert_eq!(
        fixture
            .consumed
            .targets
            .iter()
            .map(|item| &item.target)
            .collect::<Vec<_>>(),
        fixture.binding.ordered_targets.iter().collect::<Vec<_>>()
    );

    let before = native_refs(&fixture.local);
    let calls = fixture.local.reads();
    let mut narrowed = fixture.binding.clone();
    narrowed.publishers[0].authorized_target_ordinals = vec![0];
    verify(
        &fixture.local.prepared,
        &fixture.local.prepared.selections,
        &fixture.bytes,
        &narrowed,
    )
    .expect_err("one-site fixture authority cannot authorize the signed complete two-site set");
    assert_eq!(native_refs(&fixture.local), before);
    assert_eq!(fixture.local.reads(), calls);

    let mut authority = fixture.local.authority(&fixture.binding, &fixture.binding);
    let duplicate = fixture
        .writer
        .as_mut()
        .unwrap()
        .consume_release_authorization(
            &mut authority,
            &fixture.local.git(),
            fixture.local.request(&fixture.bytes),
            consume_options(),
        )
        .expect_err("one-use authorization cannot be consumed again");
    assert!(matches!(
        duplicate.cause,
        release::ConsumeFailure::Store(store::Error::ReleaseAlreadyConsumed)
    ));
    assert!(duplicate.leases.is_none());
    assert!(duplicate.observations.is_none());
    let retained = store::inspect_release_consumption(
        &fixture.local.prepared.database,
        &fixture.consumed.authorization_id,
        Some(&fixture.local.prepared.header),
        read(),
    )
    .expect("read original consumed qualification, targets and intent without new authority");
    assert_eq!(retained.record, Some(fixture.consumed.clone()));
    assert_eq!(
        fixture.local.reads(),
        calls,
        "duplicate refusal and historical read never resample destinations"
    );

    for ordinal in 0..2 {
        let attempt = fixture
            .dispatch_with_profile(ordinal, 1, release::DispatchProfile::LocalTransaction)
            .expect("real child-free local object transfer and compare-and-swap ref transaction");
        assert_eq!(
            attempt.start.profile,
            release::DispatchProfile::LocalTransaction
        );
        assert!(matches!(
            attempt.result.as_ref().unwrap().readback,
            release::TargetReadback::Observed {
                comparison: release::RefComparison::Intended,
                ..
            }
        ));
        let calls = fixture.local.reads();
        assert_eq!(
            fixture
                .dispatch_with_profile(ordinal, 1, release::DispatchProfile::LocalTransaction)
                .expect("same immutable attempt replay"),
            attempt
        );
        assert_eq!(
            fixture.local.reads(),
            calls,
            "attempt replay never redispatches or rereads"
        );
    }
    for (ordinal, target) in fixture.binding.ordered_targets.iter().enumerate() {
        let destination = Repository::open_bare(&fixture.local.paths[ordinal]).unwrap();
        let oid = destination.refname_to_id("refs/heads/publication").unwrap();
        assert_eq!(oid.to_string(), target.local_commit_oid.as_str());
        let source = Repository::open(
            fixture.local.prepared.records[ordinal]
                .intent
                .address
                .path(),
        )
        .unwrap();
        assert_eq!(
            destination.find_commit(oid).unwrap().tree_id(),
            source.find_commit(oid).unwrap().tree_id(),
            "receiver tree contains the exact qualified prepared output, not merely an observed ref name"
        );
    }
    let settled = fixture
        .reconcile(1)
        .expect("complete two-site native read-back and settlement");
    assert_eq!(
        settled.record.settlement.state,
        store::ReleaseSettlementState::Succeeded
    );
    let calls = fixture.local.reads();
    assert_eq!(
        fixture
            .reconcile(1)
            .expect("exact settlement replay")
            .record,
        settled.record
    );
    assert_eq!(fixture.local.reads(), calls);
    fixture.close();
    let history = fixture.history();
    assert_eq!(history.history.consumption, Some(fixture.consumed.clone()));
    assert_eq!(history.history.attempts.len(), 2);
    assert_eq!(history.records, vec![settled.record]);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-release-reconciliations")
        .arg(&fixture.local.prepared.database)
        .arg(&fixture.consumed.authorization_id)
        .args([
            "--allow-read",
            "--allow-coordination-files",
            "--expected-header",
        ])
        .arg(serde_json::to_string(&fixture.local.prepared.header).unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!(history)
    );
    assert_eq!(
        fs::read(log).unwrap(),
        log_bytes,
        "release/history consume retained checks without rerunning policy"
    );
    assert_eq!(fixture.local.reads(), calls);
}

#[test]
fn website_policy_projection_binds_ordered_public_policy_not_disclosure_policy() {
    let local = Local::candidate_qualified();
    let batch = local.prepared.inspect();
    let rows: Vec<_> = local
        .prepared
        .selections
        .iter()
        .enumerate()
        .map(|(ordinal, selection)| {
            let qualification = selection
                .qualification
                .as_ref()
                .and_then(release::PreparationAdmission::as_authored)
                .unwrap();
            let candidate = qualification.candidate_policy.as_ref().unwrap();
            json!({
                "ordinal": u32::try_from(ordinal).unwrap(),
                "candidate": candidate.policy,
                "output": qualification.output_policy.policy,
            })
        })
        .collect();
    let projection = serde_json::to_vec(&json!({
        "format": "gazette.prepared-website-policy/1",
        "rows": rows,
    }))
    .unwrap();
    assert_eq!(
        batch.evidence().website_policy_projection_digest,
        Sha256Digest::of(&projection)
    );
    assert_ne!(
        batch.evidence().website_policy_projection_digest,
        batch.evidence().disclosure_evidence_digest,
        "public-policy identity must not be substituted by disclosure evidence"
    );
}

#[test]
fn changed_required_candidate_identity_revision_source_or_command_refuses_reopened_release() {
    let local = Local::candidate_qualified();
    let before = fs::read(&local.prepared.database).unwrap();
    let refs = native_refs(&local);
    let log = local.prepared.checker.as_ref().unwrap().1.clone();
    let calls = fs::read(&log).unwrap();
    for field in ["id", "revision", "source", "command"] {
        let mut selections = local.prepared.selections.clone();
        let required = selections[0]
            .qualification
            .as_mut()
            .unwrap()
            .as_authored_mut()
            .unwrap()
            .candidate_policy
            .as_mut()
            .unwrap();
        match field {
            "id" => required.policy.id = "policy:other".parse().unwrap(),
            "revision" => required.policy.revision = "revision:other".parse().unwrap(),
            "source" => {
                required.policy.source_sha256 = Sha256Digest::of(b"different policy source")
            }
            "command" => required.command_sha256 = Sha256Digest::of(b"different pinned command"),
            _ => unreachable!(),
        }
        let expected = if field == "command" {
            "release_preparation_mismatch"
        } else {
            "release_preparation_admission"
        };
        assert_eq!(
            release::inspect_preparations(local.prepared.request(&selections), batch_options())
                .expect_err("independent required candidate binding cannot be changed")
                .code(),
            expected
        );
        assert_eq!(fs::read(&local.prepared.database).unwrap(), before);
        assert_eq!(fs::read(&log).unwrap(), calls);
        assert_eq!(native_refs(&local), refs);
        assert_eq!(
            local.reads(),
            0,
            "qualification refusal precedes destination acquisition"
        );
    }
    local.prepared.inspect();
}

#[test]
fn required_qualification_mismatch_or_absence_refuses_before_consumption_or_effects() {
    let mut local = Local::candidate_qualified();
    let original = local.prepared.selections.clone();
    let (mut writer, bytes, binding) = local.register(false);
    let refs = native_refs(&local);
    for mismatch in ["render", "policy", "target", "set"] {
        local.prepared.selections = original.clone();
        let first = &mut local.prepared.selections[0];
        match mismatch {
            "render" => {
                first
                    .qualification
                    .as_mut()
                    .unwrap()
                    .as_authored_mut()
                    .unwrap()
                    .rendering_sha256 = Sha256Digest::of(b"different selected render")
            }
            "policy" => {
                first
                    .qualification
                    .as_mut()
                    .unwrap()
                    .as_authored_mut()
                    .unwrap()
                    .output_policy
                    .command_sha256 = Sha256Digest::of(b"different selected checker")
            }
            "target" => first.commit = "1111111111111111111111111111111111111111".parse().unwrap(),
            "set" => {
                local.prepared.selections.pop();
            }
            _ => unreachable!(),
        }
        let mut authority = local.authority(&binding, &binding);
        let calls = local.reads();
        writer
            .consume_release_authorization(
                &mut authority,
                &local.git(),
                local.request(&bytes),
                consume_options(),
            )
            .expect_err("signed intent cannot admit changed qualification or target set");
        local.assert_pending(&bytes);
        assert_eq!(
            native_refs(&local),
            refs,
            "refusal preserves both native refs"
        );
        assert_eq!(
            local.reads(),
            calls,
            "refusal precedes provider reads and leases"
        );
    }
    local.prepared.selections = original.clone();
    writer
        .close()
        .expect("cleanly close rejected consumption fixture");

    let mut ordinary = Local::with_profile(true, true, false, false);
    assert_eq!(
        json!(ordinary.prepared.inspect())["status"],
        "local-preparations-inspected-not-certified"
    );
    let (mut writer, bytes, binding) = ordinary.signed(false);
    let refs = native_refs(&ordinary);
    for record in &ordinary.prepared.records {
        let check: Value = serde_json::from_str(&record.check.as_ref().unwrap().evidence).unwrap();
        assert!(check.get("technical_certificate").is_none());
    }
    ordinary.prepared.selections[0].qualification = original[0].qualification.clone();
    assert_eq!(
        release::inspect_preparations(
            ordinary.prepared.request(&ordinary.prepared.selections),
            batch_options()
        )
        .expect_err("explicit required profile refuses absent certificate")
        .code(),
        "release_preparation_mismatch"
    );
    ordinary.prepared.selections[0].qualification = None;
    let registration = writer
        .register_release_authorization(
            release::PreparedAuthorizationRequest {
                authorization: &bytes,
                binding: &binding,
                preparations: ordinary.prepared.request(&ordinary.prepared.selections),
            },
            release::RegisterAuthorizationOptions {
                verification: super::super::verification(),
                record: store::ReleaseAuthorizationWriteOptions { allow_record: true },
            },
        )
        .expect_err("missing qualification cannot register release authority");
    assert!(matches!(
        registration,
        release::RegisterAuthorizationError::Verification(
            release::PreparedAuthorizationError::Preparation(
                release::PreparationBatchError::Admission
            )
        )
    ));
    let mut authority = ordinary.authority(&binding, &binding);
    let consumption = writer
        .consume_release_authorization(
            &mut authority,
            &ordinary.git(),
            ordinary.request(&bytes),
            consume_options(),
        )
        .expect_err("missing explicitly required qualification cannot consume authority");
    assert!(matches!(
        consumption.cause,
        release::ConsumeFailure::Verification(release::PreparedAuthorizationError::Preparation(
            release::PreparationBatchError::Admission
        ))
    ));
    assert!(consumption.leases.is_none());
    assert!(consumption.observations.is_none());
    assert_eq!(native_refs(&ordinary), refs);
    ordinary.prepared.inspect();
    writer
        .close()
        .expect("ordinary historical inspection did not turn into certification");
    let authorization: release::Authorization = serde_json::from_slice(&bytes).unwrap();
    assert!(
        store::inspect_release_attempts(
            &ordinary.prepared.database,
            &authorization.record_id,
            Some(&ordinary.prepared.header),
            read()
        )
        .unwrap()
        .attempts
        .is_empty()
    );
}
