//! Corrupt disposable native history, retaining valid outer hashes to challenge inner joins.

use std::{collections::BTreeMap, path::Path};

use gazette_sdk::release;
use serde_json::value::RawValue;

use super::*;

pub(super) fn changed(input: &str, path: &[&str], value: Value) -> String {
    let Some((first, remaining)) = path.split_first() else {
        return serde_json::to_string(&value).expect("fixture replacement value");
    };
    let mut fields: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(input).expect("native named evidence object");
    let existing = fields.get(*first).expect("existing producer field");
    let replacement = changed(existing.get(), remaining, value);
    fields.insert(
        (*first).to_owned(),
        RawValue::from_string(replacement).expect("complete replacement JSON"),
    );
    serde_json::to_string(&fields).expect("preserve unrelated raw descendants")
}

fn rebound_candidate(candidate: &str) -> String {
    let fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(candidate).expect("candidate fields");
    // Independently reproduce the documented native envelope, not an application helper.
    let request = format!(
        "{{\"format\":\"gazette-candidate-policy/1\",\"review\":{},\"response\":{}}}",
        fields["review"].get(),
        fields["response"].get(),
    );
    changed(
        candidate,
        &["invocation", "request_sha256"],
        json!(source::Sha256Digest::of(request.as_bytes())),
    )
}

fn rebound_review(candidate: &str) -> String {
    let fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(candidate).expect("candidate fields");
    let review: BTreeMap<String, &RawValue> =
        serde_json::from_str(fields["review"].get()).expect("review fields");
    let request_sha256 = source::Sha256Digest::of(review["request"].get().as_bytes());
    let response: Value = serde_json::from_str(fields["response"].get()).expect("matched response");
    let response_json = changed(
        response["response_json"]
            .as_str()
            .expect("original response bytes"),
        &["request_sha256"],
        json!(request_sha256),
    );
    let candidate = changed(candidate, &["review", "sha256"], json!(request_sha256));
    let candidate = changed(
        &candidate,
        &["response", "request_sha256"],
        json!(request_sha256),
    );
    let candidate = changed(
        &candidate,
        &["response", "response_sha256"],
        json!(source::Sha256Digest::of(response_json.as_bytes())),
    );
    changed(
        &candidate,
        &["response", "response_json"],
        json!(response_json),
    )
}

pub(super) fn replace_record(database: &Path, record: &store::PreparationRecord, candidate: &str) {
    let mut original: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(&record.intent.evidence).expect("original raw evidence");
    original.insert(
        "candidate_check".into(),
        RawValue::from_string(candidate.to_owned()).expect("candidate JSON"),
    );
    let original = serde_json::to_string(&original).expect("changed complete intent evidence");
    let original_sha = source::Sha256Digest::of(original.as_bytes());
    let checked = changed(
        &record.check.as_ref().expect("real check").evidence,
        &["preparation_evidence_sha256"],
        json!(original_sha),
    );
    let checked = changed(
        &checked,
        &["policy", "evidence", "candidate_check_sha256"],
        json!(source::Sha256Digest::of(candidate.as_bytes())),
    );
    let checked_sha = source::Sha256Digest::of(checked.as_bytes());
    let connection = rusqlite::Connection::open(database).expect("disposable native database");
    connection
        .execute_batch("PRAGMA foreign_keys=ON; BEGIN; PRAGMA defer_foreign_keys=ON;")
        .expect("atomic linked fixture mutation");
    assert_eq!(
        connection
            .execute(
                "UPDATE gazette_preparation_intents SET evidence=?1,evidence_sha256=?2 WHERE id=?3",
                rusqlite::params![
                    original,
                    original_sha.as_bytes(),
                    record.intent.id.as_bytes()
                ],
            )
            .expect("one original intent"),
        1
    );
    assert_eq!(connection.execute(
        "UPDATE gazette_preparation_checks SET evidence=?1,evidence_sha256=?2 WHERE intent_id=?3",
        rusqlite::params![checked, checked_sha.as_bytes(), record.intent.id.as_bytes()],
    ).expect("one complete check"), 1);
    assert_eq!(
        connection
            .execute(
                "UPDATE gazette_preparation_worktrees SET check_sha256=?1 WHERE intent_id=?2",
                rusqlite::params![checked_sha.as_bytes(), record.intent.id.as_bytes()],
            )
            .expect("unchanged native worktree with rebound evidence link"),
        1
    );
    connection
        .execute_batch("COMMIT;")
        .expect("consistent outer history");
    connection
        .close()
        .expect("close corruption fixture before reader");
}

#[test]
fn complete_native_preparation_reopen_checks_review_response_context_and_raw_bindings() {
    let fixture = published_fixture(&[]);
    let root = private_directory();
    let journal = initialize(root.path());
    let parent = private_directory();
    let log = parent.path().join("reviewed-checks.jsonl");
    let mut config = pinned_configuration(&configuration(&fixture, &log, "none"), executable_pin());
    for site in ["one", "other"] {
        let anchor = format!("site='site:{site}'\npublic_base_url=");
        assert!(config.contains(&anchor));
        config = config.replace(&anchor, &format!(
            "site='site:{site}'\ndestination={{kind='git',remote='https://example.invalid/{site}.git',branch='publication'}}\npublic_base_url="));
        config = enable_site(config, site);
    }
    let request = reviewed_request(&config, root.path(), journal.header, parent.path());
    let receipts =
        execute(&request).expect("real selected review, checker, stage and native Git preparation");
    assert_eq!(receipts.len(), 2);
    let header = receipts.last().expect("complete set").preparation.journal;
    let selections = receipts
        .iter()
        .map(|site| {
            let record = &site.preparation.record;
            release::PreparationSelection {
                output_policy: None,
                qualification: None,
                preparation_id: record.intent.id,
                site: site.site.clone(),
                domain: format!("{}.example", site.site.as_str().trim_start_matches("site:")),
                commit: record
                    .worktree
                    .as_ref()
                    .expect("worktree")
                    .completion
                    .as_ref()
                    .expect("native commit completion")
                    .report
                    .commit
                    .clone(),
                expected_remote_oid_or_unborn: release::RemoteState::Unborn,
            }
        })
        .collect::<Vec<_>>();
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
    inspect().expect("real candidate evidence passes durable native reopen");
    let record = &receipts[0].preparation.record;
    let original: BTreeMap<String, &RawValue> =
        serde_json::from_str(&record.intent.evidence).expect("exact producer bytes");
    let candidate = original["candidate_check"].get();
    let candidate_value: Value = serde_json::from_str(candidate).expect("actual candidate");
    let response_json = candidate_value["response"]["response_json"]
        .as_str()
        .expect("actual response bytes");
    let duplicate = response_json.replacen('{', "{\"format\":\"gazette-review-response/1\",", 1);
    let duplicate_sha256 = source::Sha256Digest::of(duplicate.as_bytes());
    let duplicate = changed(candidate, &["response", "response_json"], json!(duplicate));
    let duplicate = changed(
        &duplicate,
        &["response", "response_sha256"],
        json!(duplicate_sha256),
    );
    let mut sources = candidate_value["review"]["request"]["sources"].clone();
    let source_key = sources
        .as_object()
        .expect("source map")
        .keys()
        .next()
        .expect("source")
        .clone();
    sources
        .as_object_mut()
        .expect("source map")
        .remove(&source_key);
    let wrong_digest = json!(source::Sha256Digest::of(b"not the original evidence"));
    let mut decisions = candidate_value["response"]["decisions"].clone();
    decisions
        .as_array_mut()
        .expect("matched uses")
        .pop()
        .expect("nonempty matched uses");
    let cases = [
        (
            "site",
            rebound_review(&changed(
                candidate,
                &["review", "request", "site"],
                json!("site:unrelated"),
            )),
        ),
        (
            "configuration",
            rebound_review(&changed(
                candidate,
                &["review", "request", "configuration_sha256"],
                wrong_digest.clone(),
            )),
        ),
        (
            "selection",
            rebound_review(&changed(
                candidate,
                &["review", "request", "selection_sha256"],
                wrong_digest.clone(),
            )),
        ),
        (
            "source set",
            rebound_review(&changed(
                candidate,
                &["review", "request", "sources"],
                sources,
            )),
        ),
        (
            "raw review digest",
            changed(candidate, &["review", "sha256"], wrong_digest),
        ),
        (
            "raw response digest",
            changed(
                candidate,
                &["response", "response_json"],
                json!(format!(" {response_json}")),
            ),
        ),
        ("inner duplicate key", duplicate),
        (
            "retained decisions",
            changed(candidate, &["response", "decisions"], decisions),
        ),
    ];
    let calls = events(&log);
    assert_eq!(calls.len(), 4, "two candidate and two output checks only");
    for (case, candidate) in cases {
        replace_record(&journal.database, record, &rebound_candidate(&candidate));
        let before =
            store::inspect_preparation(&journal.database, record.intent.id, Some(&header), read())
                .expect("valid native outer hashes and links");
        let error = inspect().expect_err("inner corruption must refuse native preparation join");
        assert!(
            matches!(error, release::PreparationBatchError::Evidence),
            "{case}: {error}"
        );
        let after =
            store::inspect_preparation(&journal.database, record.intent.id, Some(&header), read())
                .expect("unchanged native history after rejection");
        assert_eq!(before.record, after.record, "no repair for {case}");
        assert_eq!(before.journal.header, after.journal.header);
        assert_eq!(events(&log), calls, "no checker reexecution for {case}");
    }
    replace_record(&journal.database, record, candidate);
    inspect().expect("restored exact inner evidence still passes native reopen");
    assert_eq!(events(&log), calls);
}
