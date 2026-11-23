//! Retained selection syntax is validated without pretending to re-admit a corpus.

use super::{Prepared, Sha256Digest, Value, batch_options, json, read, release, store};

// Fixture-only corruption of the exact disposable journal. All native tree bytes,
// commit objects, intent coordinates and writer generations remain untouched.
fn replace_selection(prepared: &Prepared, selection: Value) {
    let record = &prepared.records[0];
    let mut original: Value =
        serde_json::from_str(&record.intent.evidence).expect("real producer intent evidence");
    original["selection"] = selection;
    let original = serde_json::to_string(&original).expect("changed original evidence");
    let original_hash = Sha256Digest::of(original.as_bytes());
    let mut check: Value = serde_json::from_str(
        &record
            .check
            .as_ref()
            .expect("actual completed check")
            .evidence,
    )
    .expect("real producer check evidence");
    check["preparation_evidence_sha256"] = json!(original_hash);
    let check = serde_json::to_string(&check).expect("rebound check evidence");
    let check_hash = Sha256Digest::of(check.as_bytes());
    let connection =
        rusqlite::Connection::open(&prepared.database).expect("exact disposable journal");
    connection
        .execute_batch("PRAGMA foreign_keys=ON; BEGIN; PRAGMA defer_foreign_keys=ON;")
        .expect("atomic fixture-only linked mutation");
    assert_eq!(
        connection
            .execute(
                "UPDATE gazette_preparation_intents SET evidence=?1,evidence_sha256=?2 WHERE id=?3",
                rusqlite::params![
                    original,
                    original_hash.as_bytes(),
                    record.intent.id.as_bytes()
                ],
            )
            .expect("selected original intent"),
        1
    );
    assert_eq!(connection.execute(
        "UPDATE gazette_preparation_checks SET evidence=?1,evidence_sha256=?2 WHERE intent_id=?3",
        rusqlite::params![check, check_hash.as_bytes(), record.intent.id.as_bytes()],
    ).expect("selected linked check"), 1);
    assert_eq!(
        connection
            .execute(
                "UPDATE gazette_preparation_worktrees SET check_sha256=?1 WHERE intent_id=?2",
                rusqlite::params![check_hash.as_bytes(), record.intent.id.as_bytes()],
            )
            .expect("same existing native worktree linkage"),
        1
    );
    connection
        .execute_batch("COMMIT;")
        .expect("consistent fixture hashes and linkage");
    connection
        .close()
        .expect("close setup connection before inspection");
}

#[test]
fn native_preparation_join_refuses_invalid_selection_shapes_after_all_hashes_are_rebound() {
    let prepared = Prepared::new();
    let original: Value = serde_json::from_str(&prepared.records[0].intent.evidence)
        .expect("actual selected preparation evidence");
    let selected = original["selection"].clone();
    assert!(
        !selected["content"]
            .as_object()
            .expect("content decisions")
            .is_empty()
    );
    assert!(selected["assets"].is_object());
    let valid = prepared.inspect();
    let valid_snapshot = prepared.snapshot();
    prepared.inspect();
    prepared.unchanged(&valid_snapshot);

    for (case, selection) in [
        ("null", Value::Null),
        ("arbitrary object", json!({"apparently_selected":true})),
        ("tuple", json!([selected["content"], selected["assets"]])),
        ("unknown field", {
            let mut value = selected.clone();
            value["approved"] = json!(true);
            value
        }),
        (
            "invalid disposition",
            json!({"content":{"content:one":"allow"},"assets":{}}),
        ),
        (
            "invalid identity",
            json!({"content":{"not-a-key":"include"},"assets":{}}),
        ),
        (
            "invalid path",
            json!({"content":{},"assets":{"../outside":"exclude"}}),
        ),
        ("non-map content", json!({"content":[],"assets":{}})),
        ("non-map assets", json!({"content":{},"assets":[]})),
    ] {
        replace_selection(&prepared, selection);
        // Native store validation succeeds: this is not a malformed hash, missing
        // phase or broken foreign key masquerading as an application refusal.
        let historical = store::inspect_preparation(
            &prepared.database,
            prepared.records[0].intent.id,
            Some(&prepared.header),
            read(),
        )
        .expect("consistent native historical record after fixture setup");
        let snapshot = prepared.snapshot();
        let error =
            release::inspect_preparations(prepared.request(&prepared.selections), batch_options())
                .expect_err("invalid selection cannot enter a successful native batch");
        assert!(
            matches!(error, release::PreparationBatchError::Evidence),
            "{case}: {error}"
        );
        assert_eq!(
            prepared.snapshot(),
            snapshot,
            "reader must not repair {case}"
        );
        let after = store::inspect_preparation(
            &prepared.database,
            prepared.records[0].intent.id,
            Some(&prepared.header),
            read(),
        )
        .expect("same expected journal after refusal");
        assert_eq!(after.journal.header, historical.journal.header);
        assert_eq!(
            after.record, historical.record,
            "no historical rewrite for {case}"
        );
    }

    // Restore the actual complete selection, not an invented permission. The
    // shared reader checks native shape, not current source/corpus admission.
    replace_selection(&prepared, selected);
    let restored_snapshot = prepared.snapshot();
    let restored = prepared.inspect();
    let before = serde_json::to_value(valid).expect("baseline batch projection");
    let after = serde_json::to_value(restored).expect("restored batch projection");
    assert_eq!(after["status"], "supported-profile-technical-preparations");
    assert_eq!(
        after["ordered_targets"]
            .as_array()
            .expect("complete restored targets")
            .len(),
        2
    );
    assert_eq!(
        before["ordered_targets"]
            .as_array()
            .expect("complete original targets")
            .len(),
        2
    );
    for (restored, original) in after["ordered_targets"]
        .as_array()
        .expect("restored native targets")
        .iter()
        .zip(
            before["ordered_targets"]
                .as_array()
                .expect("original native targets"),
        )
    {
        assert_eq!(restored["local_commit_oid"], original["local_commit_oid"]);
        assert_eq!(restored["output_digest"], original["output_digest"]);
    }
    assert_eq!(
        prepared.snapshot(),
        restored_snapshot,
        "valid read is also non-mutating"
    );
}
