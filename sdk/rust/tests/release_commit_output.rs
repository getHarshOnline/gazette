//! Native Git effects and complete pre-reserved result delivery, not release approval.

use gazette_sdk::{
    release_commit::{self, output},
    source,
};
use git2::{ObjectType, Oid, Repository};
use serde_json::Value;
use std::{fs, path::PathBuf};

struct Fixture {
    _root: tempfile::TempDir,
    repository: PathBuf,
    output: PathBuf,
    expected: source::TreeIdentity,
    metadata: release_commit::CommitMetadata,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("disposable Git fixture");
        let repository = root.path().join("local.git");
        Repository::init_bare(&repository).expect("explicit existing bare repository");
        let output = root.path().join("reviewed");
        fs::create_dir(&output).expect("retained output");
        fs::write(output.join("index.html"), b"exact publication bytes\n").expect("native output");
        let expected = source::inspect(&output, None, Default::default())
            .expect("independent pin")
            .identity();
        let signature = release_commit::CommitSignature {
            name: "Private fixture".into(),
            email: "private@example.invalid".into(),
            seconds: 100,
            offset_minutes: 0,
        };
        let metadata = release_commit::CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Exact private metadata\n".into(),
        };
        Self {
            _root: root,
            repository,
            output,
            expected,
            metadata,
        }
    }
    fn prepare(&self) -> Result<release_commit::CommitReport, release_commit::CommitError> {
        release_commit::prepare(
            &self.repository,
            &self.output,
            self.expected,
            &self.metadata,
            &[],
            release_commit::CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
    }
    fn retain(
        &self,
        commit: &source::CommitId,
        id: source::Sha256Digest,
        storage_bytes: u64,
    ) -> Result<release_commit::CustodyReport, release_commit::CustodyError> {
        release_commit::retain(
            &self.repository,
            id,
            commit,
            self.expected,
            &self.metadata,
            std::slice::from_ref(&self.output),
            release_commit::CustodyOptions {
                allow_retain: true,
                storage_bytes,
                ..Default::default()
            },
        )
    }
}

fn value(encoded: output::Encoded) -> Value {
    serde_json::from_slice(&encoded.bytes).expect("complete compact JSON")
}

fn schema<T: schemars::JsonSchema>(value: &Value) {
    let schema = serde_json::to_value(schemars::schema_for!(T)).expect("native schema");
    jsonschema::validator_for(&schema)
        .expect("validator")
        .validate(value)
        .expect("actual report satisfies native schema");
}

fn minimum() -> usize {
    let (mut low, mut high) = (1, 4096);
    assert!(output::Reservation::new(high).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        if output::Reservation::new(middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

#[test]
fn native_prepare_retain_and_readers_share_unchanged_complete_reports() {
    let fixture = Fixture::new();
    let maximum = minimum();
    let reserve = output::Reservation::new(maximum).expect("capacity before objects");
    let prepared = fixture.prepare();
    let encoded = reserve.prepare(&prepared).expect("bounded original result");
    assert!(!encoded.is_error && encoded.bytes.len() <= maximum);
    let report = value(encoded);
    let prepared = prepared.expect("native object readback");
    assert_eq!(
        report,
        serde_json::to_value(&prepared).expect("unchanged native report")
    );
    schema::<output::CommitReport<'static>>(&report);
    let id = source::Sha256Digest::of(b"exact custody");
    let reserve = output::Reservation::new(maximum).expect("capacity before ref effect");
    let retained = fixture.retain(&prepared.commit, id, 256 << 20);
    let encoded = reserve.retain(&retained).expect("retained result");
    assert!(!encoded.is_error);
    let report = value(encoded);
    assert_eq!(
        report,
        serde_json::to_value(retained.as_ref().expect("native custody"))
            .expect("unchanged native report")
    );
    schema::<output::CustodyReport<'static>>(&report);
    let native = release_commit::inspect_custody(
        &fixture.repository,
        id,
        &prepared.commit,
        fixture.expected,
        &fixture.metadata,
        Default::default(),
    )
    .expect("independent read only");
    assert_eq!(
        report,
        serde_json::to_value(native).expect("native verification")
    );
    assert_eq!(
        source::inspect(&fixture.output, None, Default::default())
            .expect("source unchanged")
            .identity(),
        fixture.expected
    );
}

#[test]
fn actual_failed_object_write_preserves_uncertainty_without_an_invented_oid() {
    let fixture = Fixture::new();
    let blob = Oid::hash_object(ObjectType::Blob, b"exact publication bytes\n")
        .expect("native object identity")
        .to_string();
    fs::write(
        fixture.repository.join("objects").join(&blob[..2]),
        b"not a directory",
    )
    .expect("deterministic native object obstruction");
    let reserved = output::Reservation::new(minimum()).expect("pre-effect reservation");
    let result = fixture.prepare();
    assert!(matches!(
        result.as_ref().expect_err("native write fails").effect,
        release_commit::CommitEffect::ObjectsMayRemain
    ));
    let encoded = reserved.prepare(&result).expect("late failure preserved");
    assert!(encoded.is_error);
    let report = value(encoded);
    assert_eq!(report["effect"], "objects_may_remain");
    assert!(report.get("commit").is_none());
    schema::<output::CommitReport<'static>>(&report);
    assert_eq!(
        Repository::open_bare(&fixture.repository)
            .expect("selected repo")
            .references()
            .expect("refs")
            .count(),
        0
    );
}

#[test]
fn actual_late_custody_failure_preserves_reference_and_exact_native_effect() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare().expect("existing objects");
    let storage = source::inspect(&fixture.repository, None, Default::default())
        .expect("current complete storage")
        .total_bytes();
    let id = source::Sha256Digest::of(b"late retention failure");
    let reserved = output::Reservation::new(minimum()).expect("pre-effect reservation");
    let result = fixture.retain(&prepared.commit, id, storage);
    assert_eq!(
        result
            .as_ref()
            .expect_err("new reference exceeds sync storage bound")
            .effect,
        release_commit::CustodyEffect::ReferenceMayRemain
    );
    let encoded = reserved.retain(&result).expect("native failure delivery");
    assert!(encoded.is_error);
    let report = value(encoded);
    assert_eq!(report["effect"], "reference_may_remain");
    schema::<output::CustodyReport<'static>>(&report);
    let inspected = release_commit::inspect_custody(
        &fixture.repository,
        id,
        &prepared.commit,
        fixture.expected,
        &fixture.metadata,
        Default::default(),
    )
    .expect("read-only evidence without repeating retention");
    assert_eq!(inspected.commit, prepared);
}

#[test]
fn scalar_extremes_all_effects_and_oversized_diagnostics_fit_preproven_capacity() {
    let maximum = minimum();
    assert!(output::Reservation::new(maximum - 1).is_err());
    assert!(output::Reservation::new(0).is_err());
    assert!(output::Reservation::new((16 << 20) + 1).is_err());
    let fixture = Fixture::new();
    let mut receipt = fixture.prepare().expect("real native report");
    receipt.entries = usize::MAX;
    receipt.bytes = u64::MAX;
    receipt.parent = Some(receipt.commit.clone());
    let result = Ok(receipt.clone());
    let encoded = output::Reservation::new(maximum)
        .expect("capacity")
        .prepare(&result)
        .expect("maximum scalar representation");
    assert!(encoded.bytes.len() <= maximum);
    schema::<output::CommitReport<'static>>(&value(encoded));
    // Native reports are publicly constructible. These are representation proofs,
    // not claims that artificial maxima or a failed-readback OID were produced above.
    for effect in [
        release_commit::CommitEffect::NotAttempted,
        release_commit::CommitEffect::ObjectsMayRemain,
        release_commit::CommitEffect::CommitCreated(receipt.commit),
    ] {
        for reason in ["\0".repeat(256), "é".repeat(129)] {
            let reason: &'static str = Box::leak(reason.into_boxed_str());
            let result = Err(release_commit::CommitError {
                reason,
                effect: effect.clone(),
            });
            let encoded = output::Reservation::new(maximum)
                .expect("capacity")
                .prepare(&result)
                .expect("complete conservative effect");
            assert!(encoded.is_error && encoded.bytes.len() <= maximum);
            let projected = value(encoded);
            assert_eq!(
                projected["effect"],
                serde_json::to_value(&effect).expect("same original enum")
            );
            if reason.len() > 256 {
                assert_eq!(projected["code"], "release_commit_diagnostic_limit");
                assert_eq!(
                    projected["reason_sha256"],
                    source::Sha256Digest::of(reason.as_bytes()).to_string()
                );
                assert!(projected.get("reason").is_none());
            } else {
                assert_eq!(projected["reason"], reason);
            }
            schema::<output::CommitReport<'static>>(&projected);
        }
    }
    for effect in [
        release_commit::CustodyEffect::NotAttempted,
        release_commit::CustodyEffect::ReferenceMayRemain,
    ] {
        let reason: &'static str = Box::leak("x".repeat(257).into_boxed_str());
        let result = Err(release_commit::CustodyError { reason, effect });
        let projected = value(
            output::Reservation::new(maximum)
                .expect("capacity")
                .retain(&result)
                .expect("overflow reason retains effect"),
        );
        assert_eq!(
            projected["effect"],
            serde_json::to_value(effect).expect("original effect")
        );
        assert_eq!(projected["code"], "release_custody_diagnostic_limit");
        schema::<output::CustodyReport<'static>>(&projected);
    }
}
