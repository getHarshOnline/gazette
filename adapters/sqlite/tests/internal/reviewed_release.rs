//! Plain approval uses the same v5 custody, leases and uncertain-attempt history.

use super::*;
use gazette_core::release::{ReleaseBinding, ReviewedApproval, ReviewedBinding, ReviewedPublisher};
use gazette_core::store::{ReleaseAttemptKey, ReleaseAttemptOptions, ReleaseAttemptStart};
use gazette_ports::store::{ReleaseAttemptReader, ReleaseAttemptWriter};

#[test]
fn reviewed_approval_consumes_once_and_retains_uncertain_attempts_without_schema_change() {
    let fixture = Fixture::new();
    let path = fixture.database();
    let mut writer = fixture.writer(fixture.initial, options());
    let original = pending(&mut writer, &path, "signed-source");
    let mut value: serde_json::Value = serde_json::from_str(&original.authorization).unwrap();
    let object = value.as_object_mut().unwrap();
    for field in ["publisher_authorizations", "not_before", "expires_at"] {
        object.remove(field);
    }
    object.insert(
        "schema_id".into(),
        serde_json::json!("WebsiteReviewedReleaseApprovalV1"),
    );
    object.insert("record_id".into(), serde_json::json!("reviewed"));
    object.insert("nonce".into(), serde_json::json!("reviewed"));
    object.insert(
        "ordered_sites".into(),
        serde_json::json!(["site:one", "site:two"]),
    );
    let publishers = vec![
        ReviewedPublisher {
            publisher_id: "publisher:one".parse().unwrap(),
            authorized_target_ordinals: vec![0],
            credential_owner: "owner-one".into(),
            credential_helper: "/explicit/owner-one".into(),
        },
        ReviewedPublisher {
            publisher_id: "publisher:two".parse().unwrap(),
            authorized_target_ordinals: vec![1],
            credential_owner: "owner-two".into(),
            credential_helper: "/explicit/owner-two".into(),
        },
    ];
    object.insert(
        "publisher_approvals".into(),
        serde_json::to_value(&publishers).unwrap(),
    );
    let approval: ReviewedApproval = serde_json::from_value(value).unwrap();
    let binding: ReleaseBinding = ReviewedBinding {
        schema_id: gazette_core::release::ReviewedBindingSchema::V1,
        canonical_body_sha256: approval.canonical_body_sha256,
        coordinator_id: approval.coordinator_id.clone(),
        journal: *original.binding.journal(),
        ordered_targets: approval.ordered_targets.clone(),
        ordered_sites: approval.ordered_sites.clone(),
        publishers,
        observed_at: 2,
        emergency_stop: false,
    }
    .into();
    let raw = serde_json::to_string(&approval).unwrap();
    let registration = writer
        .record_release_authorization(
            &path,
            ReleaseAuthorizationInput {
                authorization: &raw,
                binding: &binding,
                preparations_sha256: original.preparations_sha256,
                preparation_ids: &original.preparation_ids,
            },
            ReleaseAuthorizationWriteOptions { allow_record: true },
        )
        .unwrap()
        .record;
    assert!(matches!(registration.binding, ReleaseBinding::Reviewed(_)));
    let leases = writer
        .acquire_release_leases("reviewed", lease_permission())
        .unwrap();
    let rows = readbacks(&registration);
    let consumed = writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .unwrap()
        .record;
    let start = ReleaseAttemptStart {
        profile: gazette_core::release::DispatchProfile::ReceivePack,
        key: ReleaseAttemptKey {
            authorization_id: "reviewed".into(),
            ordinal: 0,
            number: 1,
        },
        binding: consumed.binding.clone(),
        prestate: rows[0].readback.clone(),
        created: consumed.created,
    };
    let permissions = ReleaseAttemptOptions { allow_record: true };
    assert!(
        writer
            .start_release_attempt(&path, &start, permissions)
            .unwrap()
            .created
    );
    assert!(
        !writer
            .start_release_attempt(&path, &start, permissions)
            .unwrap()
            .created
    );
    let mut retry = start.clone();
    retry.key.number = 2;
    let history = Sqlite
        .inspect_release_attempts(&path, "reviewed", options())
        .unwrap();
    assert_eq!(history.attempts.len(), 1);
    assert!(history.attempts[0].result.is_none());
    assert_eq!(history.attempts[0].start, start);
    assert!(retry.follows(Some(&history.attempts[0])).is_err());
    let error = writer
        .consume_release_authorization(
            &path,
            input(&registration, &leases, &rows),
            consume_permission(),
        )
        .unwrap_err();
    assert!(matches!(error.cause, Error::ReleaseAlreadyConsumed));
    assert_eq!(crate::schema::VERSION, 6);
    assert_eq!(
        crate::schema::digest().to_string(),
        "1c0a88175582ea2a8a023e5f4ce02f47b17241f393fc55fa5329984fbc3c5eb0"
    );
}
