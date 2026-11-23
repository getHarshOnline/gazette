//! Native checkpoint delivery and exact immutable lookup after bounded receipt loss.

use super::*;
use gazette_sdk::release_commit::journal_output::{self, Reservation};
use serde_json::Value;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

fn minimum(path: &Path, id: Sha256Digest) -> usize {
    let (mut low, mut high) = (1, 32 << 10);
    assert!(Reservation::new(path, id, high).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        if Reservation::new(path, id, middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

fn validate(value: &Value) {
    let schema = serde_json::to_value(schemars::schema_for!(journal_output::CheckpointReport<'_>))
        .expect("shared schema");
    jsonschema::validator_for(&schema)
        .expect("validator")
        .validate(value)
        .expect("actual projection satisfies discovery");
}

#[test]
fn original_native_checkpoint_full_and_compact_lookup_share_exact_history() {
    let mut fixture = Fixture::new();
    fixture.metadata.message = "large exact private recipe\n".repeat(2048);
    fixture.prepared = release_commit::prepare(
        &fixture.repository,
        &fixture.output,
        fixture.output_identity,
        &fixture.metadata,
        &[],
        CommitOptions {
            allow_write_objects: true,
            ..Default::default()
        },
    )
    .expect("real existing commit with large native metadata");
    let id = Sha256Digest::of(b"large native checkpoint custody");
    let maximum = minimum(&fixture.journal, id);
    assert!(Reservation::new(&fixture.journal, id, maximum - 1).is_err());
    let before = fixture.header();
    let snapshot = fixture.snapshot();
    let reservation =
        Reservation::new(&fixture.journal, id, maximum).expect("pre-effect compact capacity");
    let result = fixture.checkpoint(id, before, options());
    let encoded = reservation
        .checkpoint(&result)
        .expect("exact native evidence survives overflow");
    assert!(
        encoded.is_error,
        "native success is not complete report delivery"
    );
    assert!(encoded.bytes.len() <= maximum);
    let value: Value = serde_json::from_slice(&encoded.bytes).expect("compact receipt");
    validate(&value);
    let receipt = result
        .as_ref()
        .expect("native checkpoint actually completed");
    assert_eq!(value["code"], "checkpoint_report_limit");
    assert!(value["failure"].is_null());
    assert_eq!(
        value["locator"],
        serde_json::json!(fixture.journal.as_os_str().as_bytes())
    );
    assert_eq!(
        value["receipt"]["journal"],
        serde_json::to_value(receipt.journal).expect("current observation")
    );
    assert_eq!(
        value["receipt"]["created"],
        serde_json::to_value(receipt.checkpoint.created).expect("original observation")
    );
    assert_eq!(
        value["receipt"]["commit"],
        serde_json::to_value(&fixture.prepared).expect("exact native identity")
    );
    let history = release_commit::inspect_checkpoint(
        &fixture.initialized.database,
        id,
        Some(&receipt.journal),
        read(),
    )
    .expect("independent historical lookup");
    assert_eq!(history.checkpoint.as_ref(), Some(&receipt.checkpoint));
    let full = Reservation::new(&fixture.journal, id, 256 << 10)
        .expect("full response capacity")
        .checkpoint(&result)
        .expect("no rerun for full projection");
    assert!(!full.is_error);
    let full: Value = serde_json::from_slice(&full.bytes).expect("native full receipt");
    assert_eq!(
        full,
        serde_json::to_value(receipt).expect("unchanged native shape")
    );
    validate(&full);
    let replay_reservation =
        Reservation::new(&fixture.journal, id, maximum).expect("reserve before replay");
    let replay = fixture.checkpoint(id, receipt.journal, options());
    let replay_json: Value = serde_json::from_slice(
        &replay_reservation
            .checkpoint(&replay)
            .expect("replayed compact evidence")
            .bytes,
    )
    .expect("JSON");
    assert_eq!(
        replay_json["receipt"]["created"],
        value["receipt"]["created"]
    );
    assert_ne!(
        replay_json["receipt"]["journal"],
        value["receipt"]["journal"]
    );
    assert_eq!(fixture.snapshot(), snapshot);
    let wrong = Reservation::new(
        &fixture.journal,
        Sha256Digest::of(b"wrong selection"),
        maximum,
    )
    .expect("independent reservation")
    .checkpoint(&result);
    let Err(wrong) = wrong else {
        panic!("caller cannot bind a different returned row");
    };
    assert!(std::ptr::eq(wrong.result, &result));
}

#[test]
fn denied_native_checkpoint_has_no_invented_header_or_effect_and_bounds_do_not_write() {
    let fixture = Fixture::new();
    let id = Sha256Digest::of(b"denied checkpoint projection");
    let before = fixture.header();
    let snapshot = fixture.snapshot();
    assert!(Reservation::new(&fixture.journal, id, 0).is_err());
    assert!(Reservation::new(&fixture.journal, id, (16 << 20) + 1).is_err());
    let reservation = Reservation::new(&fixture.journal, id, 32 << 10)
        .expect("reserve before denied native call");
    let result = fixture.checkpoint(id, before, CheckpointOptions::default());
    assert!(result.is_err());
    let encoded = reservation.checkpoint(&result).expect("exact refusal");
    assert!(encoded.is_error);
    let value: Value = serde_json::from_slice(&encoded.bytes).expect("failure JSON");
    validate(&value);
    assert_eq!(value["failure"]["writer_acquired"], false);
    assert!(value["failure"]["generation"].is_null());
    assert!(value["failure"]["effect"].is_null());
    assert!(value["receipt"].is_null());
    assert_eq!(fixture.header(), before);
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn exact_unix_locator_bytes_are_reserved_without_touching_non_utf8_native_paths() {
    // Representation proof only: APFS cannot create arbitrary non-UTF8 fixture names.
    let path = PathBuf::from(std::ffi::OsString::from_vec(
        b"/unused/\xff\0\"locator".to_vec(),
    ));
    let id = Sha256Digest::of(b"lossless locator representation");
    let minimum = minimum(&path, id);
    assert!(Reservation::new(&path, id, minimum).is_ok());
    assert!(Reservation::new(&path, id, minimum - 1).is_err());
    let fixture = Fixture::new();
    let result = fixture.checkpoint(id, fixture.header(), options());
    let receipt = result.expect("native independent receipt for representation proof");
    let mut oversized = receipt;
    oversized.checkpoint.metadata.message = "x".repeat(64 << 10);
    let original = Ok(oversized);
    let encoded = Reservation::new(&path, id, minimum)
        .expect("exact caller-byte bound")
        .checkpoint(&original)
        .expect("pure caller-supplied projection");
    let value: Value = serde_json::from_slice(&encoded.bytes).expect("compact projection");
    assert_eq!(
        value["locator"],
        serde_json::json!(path.as_os_str().as_bytes())
    );
    validate(&value);
}
