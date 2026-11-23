//! The original native recovery result remains independently inspectable after bounded delivery.

use super::*;
use gazette_sdk::release_commit::journal_output::{self, Reservation};
use serde_json::Value;

fn validate(value: &Value) {
    let schema = serde_json::to_value(schemars::schema_for!(journal_output::RecoveryReport<'_>))
        .expect("shared discovery schema");
    jsonschema::validator_for(&schema)
        .expect("validator")
        .validate(value)
        .expect("native recovery projection matches schema");
}

#[test]
fn recorded_recovery_overflow_retains_original_completion_and_every_observed_header() {
    let evidence = format!("{{\"private\":\"{}\"}}", "x".repeat(64 << 10));
    let fixture = Fixture::with_evidence(&evidence);
    let (commit, history) = unresolved(&fixture);
    let id = fixture.receipt.record.intent.id;
    let before = native_bytes(&fixture);
    let maximum = 16 << 10;
    let reservation =
        Reservation::new(&fixture.database, id, maximum).expect("reserve before native recovery");
    let result = gazette_sdk::release_commit::recover_recorded_worktree(
        gazette_sdk::release_commit::RecoverRecordedWorktreeRequest {
            database: &fixture.database,
            preparation_id: id,
            expected: history.journal.header,
            commit: &commit,
            protected_roots: std::slice::from_ref(&fixture.source),
        },
        options(),
    );
    let encoded = reservation
        .recover(&result)
        .expect("compact original recovery evidence");
    assert!(encoded.is_error);
    assert!(encoded.bytes.len() <= maximum);
    let value: Value = serde_json::from_slice(&encoded.bytes).expect("compact JSON");
    validate(&value);
    assert_eq!(value["code"], "recovery_report_limit");
    let receipt = result
        .as_ref()
        .expect("native recovery completed independently of delivery");
    let expected = serde_json::to_value(receipt).expect("original native receipt");
    assert_eq!(value["receipt"]["journal"], expected["journal"]);
    assert_eq!(
        value["receipt"]["created"],
        expected["record"]["intent"]["created"]
    );
    assert_eq!(
        value["receipt"]["retention"],
        expected["record"]["retention"]
    );
    assert_eq!(
        value["receipt"]["worktree"]["created"],
        expected["record"]["worktree"]["created"]
    );
    assert_eq!(
        value["receipt"]["worktree"]["completion"],
        expected["record"]["worktree"]["completion"]
    );
    assert_eq!(
        value["receipt"]["evidence_sha256"],
        expected["record"]["intent"]["evidence_sha256"]
    );
    assert_eq!(fixture.history().record.as_ref(), Some(&receipt.record));
    let full = Reservation::new(&fixture.database, id, 256 << 10)
        .expect("full delivery budget")
        .recover(&result)
        .expect("same result without another native attempt");
    assert!(!full.is_error);
    let full: Value = serde_json::from_slice(&full.bytes).expect("unchanged full receipt");
    validate(&full);
    assert_eq!(full, expected);
    assert_eq!(native_bytes(&fixture), before);
}

#[test]
fn lost_native_append_and_close_keep_progress_and_original_receipt_in_both_delivery_shapes() {
    for boundary in [Boundary::CompletionReport, Boundary::ClosedReport] {
        let evidence = format!("{{\"private\":\"{}\"}}", "y".repeat(64 << 10));
        let fixture = Fixture::with_evidence(&evidence);
        let (commit, history) = unresolved(&fixture);
        let id = fixture.receipt.record.intent.id;
        let reservation =
            Reservation::new(&fixture.database, id, 16 << 10).expect("pre-effect reservation");
        let result = release::recover_journal_worktree(
            &FileSystem,
            &Git,
            &FaultStore(boundary),
            request(&fixture, &history, &commit),
            options(),
        );
        let error = result
            .as_ref()
            .expect_err("actual native effect, then lost boundary report");
        let encoded = reservation
            .recover(&result)
            .expect("known native effects preserved");
        assert!(encoded.is_error);
        let value: Value = serde_json::from_slice(&encoded.bytes).expect("failure projection");
        validate(&value);
        assert_eq!(
            value["failure"]["progress"],
            serde_json::to_value(&error.progress).expect("original progress")
        );
        assert_eq!(
            value["failure"]["progress"]["journal"],
            serde_json::to_value(fixture.history().journal.header)
                .expect("actually observed generation")
        );
        if boundary == Boundary::CompletionReport {
            assert_eq!(value["failure"]["effect"], "Unverified");
            assert!(value["receipt"].is_null());
            assert!(
                fixture
                    .history()
                    .record
                    .as_ref()
                    .and_then(completion)
                    .is_some()
            );
        } else {
            assert_eq!(value["code"], "recovery_report_limit");
            assert_eq!(
                value["receipt"]["worktree"]["completion"],
                serde_json::to_value(completion(
                    &error.receipt.as_ref().expect("original receipt").record
                ))
                .expect("original completion")
            );
        }
    }
}
