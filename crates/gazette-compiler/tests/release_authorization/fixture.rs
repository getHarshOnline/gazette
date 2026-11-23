//! Deterministic test issuance, not product signing authority.

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use gazette_compiler::release::Binding;
use gazette_core::source::Sha256Digest;
use serde_json::{Value, json};

pub(super) const DOMAIN: &[u8] = b"WEBSITE_RELEASE_BATCH_AUTHORIZATION_V1\0";

pub(super) fn key(publisher: &str) -> SigningKey {
    SigningKey::from_bytes(&[if publisher == "publisher:one" { 3 } else { 5 }; 32])
}

pub(super) fn declaration() -> Value {
    let digest = "11".repeat(32);
    let oid = "aa".repeat(20);
    let targets: Vec<_> = (0..3).map(|ordinal| json!({
        "ordinal": ordinal, "domain": format!("{ordinal}.example"),
        "remote": format!("https://example.invalid/site-{ordinal}.git"),
        "branch": "main", "refspec": format!("{oid}:refs/heads/main"),
        "expected_remote_oid_or_unborn": if ordinal == 0 { json!({"state":"unborn"}) } else { json!({"state":"commit", "oid":"bb".repeat(20)}) },
        "prepared_release_receipt_ref": digest, "local_commit_oid": oid,
        "output_digest": digest
    })).collect();
    json!({
        "schema_id":"WebsiteReleaseBatchAuthorizationV1",
        "record_id":"record:test", "nonce":"nonce:é😀", "coordinator_id":"coordinator:test",
        "journal_id":vec![7;32], "journal_epoch":1, "writer_fence":9,
        "ordered_targets":targets,
        "validation_evidence_digest":digest, "disclosure_evidence_digest":digest,
        "policy_evidence_digest":digest, "scanner_revision":"scanner:test",
        "generic_rule_set_digest":digest, "website_policy_projection_digest":digest,
        "approved_at":100, "not_before":110, "expires_at":200,
        "canonical_body_sha256":digest,
        "publisher_authorizations":[
            {"publisher_id":"publisher:one", "publisher_key_id":"key:one", "publisher_key_epoch":1,
             "authority_reference":digest, "authorized_target_ordinals":[0,2],
             "signature_envelope":{"schema_id":"gazette.ed25519/v1", "signature":"pending"}},
            {"publisher_id":"publisher:two", "publisher_key_id":"key:two", "publisher_key_epoch":4,
             "authority_reference":digest, "authorized_target_ordinals":[1],
             "signature_envelope":{"schema_id":"gazette.ed25519/v1", "signature":"pending"}}
        ]
    })
}

pub(super) fn body(value: &Value) -> Vec<u8> {
    let mut value = value.clone();
    value
        .as_object_mut()
        .expect("fixture object")
        .remove("canonical_body_sha256");
    for publisher in value["publisher_authorizations"]
        .as_array_mut()
        .expect("publisher list")
    {
        publisher
            .as_object_mut()
            .expect("publisher object")
            .remove("signature_envelope");
    }
    serde_jcs::to_vec(&value).expect("test-only canonicalization")
}

pub(super) fn sign(mut value: Value) -> (Vec<u8>, Binding) {
    let digest = Sha256Digest::of(&body(&value));
    value["canonical_body_sha256"] = json!(digest);
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(digest.as_bytes());
    for publisher in value["publisher_authorizations"]
        .as_array_mut()
        .expect("publishers")
    {
        let signer = key(publisher["publisher_id"]
            .as_str()
            .expect("publisher identity"));
        publisher["signature_envelope"]["signature"] =
            json!(STANDARD.encode(signer.sign(&message).to_bytes()));
    }
    let publishers: Vec<_> = value["publisher_authorizations"].as_array().expect("publishers").iter().map(|publisher| {
        json!({
            "publisher_id":publisher["publisher_id"], "publisher_key_id":publisher["publisher_key_id"],
            "publisher_key_epoch":publisher["publisher_key_epoch"], "authority_reference":publisher["authority_reference"],
            "role":"publisher", "authorized_target_ordinals":publisher["authorized_target_ordinals"],
            "public_key":STANDARD.encode(key(publisher["publisher_id"].as_str().expect("identity")).verifying_key().to_bytes()),
            "state":"active"
        })
    }).collect();
    let binding = serde_json::from_value(json!({
        "canonical_body_sha256":digest, "coordinator_id":value["coordinator_id"],
        "journal":{"schema_version":1, "journal_id":value["journal_id"], "journal_epoch":value["journal_epoch"], "writer_fence":value["writer_fence"]},
        "ordered_targets":value["ordered_targets"], "publishers":publishers,
        "authority_frontier_sha256":"22".repeat(32), "revocation_frontier_sha256":"33".repeat(32),
        "supersession_frontier_sha256":"44".repeat(32), "observed_at":120, "emergency_stop":false
    })).expect("complete test trust inputs; tests retain these before attacking the body");
    (serde_json::to_vec(&value).expect("test transport"), binding)
}
