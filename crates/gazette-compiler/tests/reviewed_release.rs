//! Plain approval and signed custody remain distinct over the same delivery targets.

use gazette_compiler::release::{self, *};
use gazette_core::source::Sha256Digest;
use serde_json::json;

fn fixture() -> (ReviewedApproval, ReviewedBinding) {
    let signed: Authorization =
        serde_json::from_slice(include_bytes!("release_authorization/authorization.json")).unwrap();
    let mut targets = signed.ordered_targets;
    let mut second = targets[0].clone();
    second.ordinal = 1;
    second.domain = "second.example".into();
    second.remote = "https://example.invalid/owner-two/site.git".into();
    targets.push(second);
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
    let mut approval = ReviewedApproval {
        schema_id: ReviewedApprovalSchema::WebsiteReviewedReleaseApprovalV1,
        record_id: signed.record_id,
        nonce: signed.nonce,
        coordinator_id: signed.coordinator_id,
        journal_id: signed.journal_id,
        journal_epoch: signed.journal_epoch,
        writer_fence: signed.writer_fence,
        ordered_targets: targets,
        ordered_sites: vec!["site:one".parse().unwrap(), "site:two".parse().unwrap()],
        publisher_approvals: publishers,
        validation_evidence_digest: signed.validation_evidence_digest,
        disclosure_evidence_digest: signed.disclosure_evidence_digest,
        policy_evidence_digest: signed.policy_evidence_digest,
        scanner_revision: signed.scanner_revision,
        generic_rule_set_digest: signed.generic_rule_set_digest,
        website_policy_projection_digest: signed.website_policy_projection_digest,
        approved_at: signed.approved_at,
        canonical_body_sha256: Sha256Digest::of(b"unconfirmed"),
    };
    approval.canonical_body_sha256 = reviewed_body_digest(&approval, Limits::default()).unwrap();
    let binding = ReviewedBinding {
        schema_id: ReviewedBindingSchema::V1,
        canonical_body_sha256: approval.canonical_body_sha256,
        coordinator_id: approval.coordinator_id.clone(),
        journal: gazette_core::store::JournalHeader {
            schema_version: 5,
            journal_id: approval.journal_id,
            journal_epoch: approval.journal_epoch,
            writer_fence: approval.writer_fence,
        },
        ordered_targets: approval.ordered_targets.clone(),
        ordered_sites: approval.ordered_sites.clone(),
        publishers: approval.publisher_approvals.clone(),
        observed_at: approval.approved_at,
        emergency_stop: false,
    };
    (approval, binding)
}

#[test]
fn reviewed_complete_set_binds_output_publisher_and_explicit_credentials() {
    let (approval, binding) = fixture();
    verify_reviewed(
        &serde_json::to_vec(&approval).unwrap(),
        &binding,
        Limits::default(),
    )
    .unwrap();
    for field in ["output_digest", "local_commit_oid", "remote"] {
        let mut changed = serde_json::to_value(&approval).unwrap();
        changed["ordered_targets"][0][field] = match field {
            "remote" => json!("https://other.invalid/site.git"),
            "local_commit_oid" => json!("bb".repeat(20)),
            _ => json!("22".repeat(32)),
        };
        assert!(
            verify_reviewed(
                &serde_json::to_vec(&changed).unwrap(),
                &binding,
                Limits::default()
            )
            .is_err()
        );
    }
    let mut changed = approval.clone();
    changed.publisher_approvals[0].credential_owner = "ambient-login".into();
    changed.canonical_body_sha256 = reviewed_body_digest(&changed, Limits::default()).unwrap();
    let mut expected = binding.clone();
    expected.canonical_body_sha256 = changed.canonical_body_sha256;
    assert!(
        verify_reviewed(
            &serde_json::to_vec(&changed).unwrap(),
            &expected,
            Limits::default()
        )
        .is_err()
    );
    let mut stopped = binding.clone();
    stopped.emergency_stop = true;
    assert!(matches!(
        verify_reviewed(
            &serde_json::to_vec(&approval).unwrap(),
            &stopped,
            Limits::default()
        ),
        Err(Error::Inactive)
    ));
}

#[test]
fn reviewed_scope_shape_and_limits_refuse_partial_overlapping_or_unsafe_sets() {
    for case in 0..7 {
        let (mut approval, mut binding) = fixture();
        match case {
            0 => {
                approval.publisher_approvals.pop();
            }
            1 => approval.publisher_approvals[1].authorized_target_ordinals = vec![0],
            2 => approval.ordered_sites[1] = approval.ordered_sites[0].clone(),
            3 => {
                approval.ordered_sites.pop();
            }
            4 => approval.publisher_approvals[0].credential_helper = "/helper --ambient".into(),
            5 => approval.publisher_approvals[0].credential_owner = " ".into(),
            _ => approval.ordered_targets[0].refspec.insert(0, '+'),
        }
        approval.canonical_body_sha256 =
            reviewed_body_digest(&approval, Limits::default()).unwrap();
        binding.canonical_body_sha256 = approval.canonical_body_sha256;
        binding.ordered_sites = approval.ordered_sites.clone();
        binding.publishers = approval.publisher_approvals.clone();
        binding.ordered_targets = approval.ordered_targets.clone();
        assert!(
            verify_reviewed(
                &serde_json::to_vec(&approval).unwrap(),
                &binding,
                Limits::default()
            )
            .is_err(),
            "case {case}"
        );
    }
    let (approval, binding) = fixture();
    let bytes = serde_json::to_vec(&approval).unwrap();
    assert!(
        verify_reviewed(
            &bytes,
            &binding,
            Limits {
                targets: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        verify_reviewed(
            &bytes,
            &binding,
            Limits {
                bytes: bytes.len() - 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    let duplicate = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("\"approved_at\":", "\"approved_at\":0,\"approved_at\":");
    assert!(verify_reviewed(duplicate.as_bytes(), &binding, Limits::default()).is_err());
    assert!(verify_reviewed(b"[]", &binding, Limits::default()).is_err());
    let signed = release::parse_binding(
        include_bytes!("release_authorization/binding.json"),
        Limits::default(),
    )
    .unwrap();
    assert!(release::verify(&bytes, &signed, Limits::default()).is_err());
}

#[test]
fn legacy_signed_document_and_binding_keep_exact_wire_bytes() {
    let original: Authorization =
        serde_json::from_slice(include_bytes!("release_authorization/authorization.json")).unwrap();
    let binding: Binding =
        serde_json::from_slice(include_bytes!("release_authorization/binding.json")).unwrap();
    let document = ReleaseDocument::Signed(Box::new(original.clone()));
    let stored: ReleaseBinding = binding.clone().into();
    assert_eq!(
        serde_json::to_vec(&document).unwrap(),
        serde_json::to_vec(&original).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&stored).unwrap(),
        serde_json::to_vec(&binding).unwrap()
    );
    assert_eq!(
        serde_json::from_slice::<ReleaseDocument>(&serde_json::to_vec(&original).unwrap()).unwrap(),
        document
    );
    assert_eq!(
        serde_json::from_slice::<ReleaseBinding>(&serde_json::to_vec(&binding).unwrap()).unwrap(),
        stored
    );
    assert_eq!(
        Sha256Digest::of(include_bytes!("release_authorization/authorization.json")).to_string(),
        "55b9cc751e1abb94d85199644da8439641584a376bf04cb9e38972d78999732f"
    );
    release::verify(
        include_bytes!("release_authorization/authorization.json"),
        &binding,
        Limits::default(),
    )
    .unwrap();
}
