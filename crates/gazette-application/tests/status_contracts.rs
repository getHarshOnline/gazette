//! Closed outward status vocabularies retain their established JSON spellings.

use gazette_application::{
    build::{
        DisclosureStatus, PolicyCheckStatus, PublicationSetStatus, RegistryStatus,
        ReviewResponseStatus,
    },
    release::{PreparationStatus, PreparedAuthorizationStatus, ReadbackStatus},
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

fn assert_closed<T: JsonSchema>(expected: &[&str]) {
    let schema = schemars::schema_for!(T);
    let actual = schema
        .get("enum")
        .and_then(Value::as_array)
        .map(|values| values.iter().collect::<Vec<_>>())
        .or_else(|| schema.get("const").map(|value| vec![value]))
        .or_else(|| {
            schema
                .get("oneOf")
                .and_then(Value::as_array)
                .map(|variants| {
                    variants
                        .iter()
                        .map(|variant| variant.get("const").expect("closed variant has const"))
                        .collect::<Vec<_>>()
                })
        })
        .expect("status schema is one closed string vocabulary")
        .into_iter()
        .map(|value| value.as_str().expect("status spelling"))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_wire<T: Serialize>(status: T, expected: &str) {
    assert_eq!(
        serde_json::to_value(status).expect("status serializes"),
        Value::String(expected.to_owned())
    );
}

#[test]
fn build_statuses_have_closed_schemas_and_exact_existing_wire_values() {
    assert_closed::<PublicationSetStatus>(&["local_set_rendered", "local_set_incomplete"]);
    assert_wire(PublicationSetStatus::Rendered, "local_set_rendered");
    assert_wire(PublicationSetStatus::Incomplete, "local_set_incomplete");

    assert_closed::<DisclosureStatus>(&["partial-evidence", "required-supported-profile-evidence"]);
    assert_wire(DisclosureStatus::PartialEvidence, "partial-evidence");
    assert_wire(
        DisclosureStatus::RequiredSupportedProfileEvidence,
        "required-supported-profile-evidence",
    );

    assert_closed::<PolicyCheckStatus>(&["policy-check-passed-unverified"]);
    assert_wire(
        PolicyCheckStatus::PassedUnverified,
        "policy-check-passed-unverified",
    );

    assert_closed::<RegistryStatus>(&["resolved-unadmitted"]);
    assert_wire(RegistryStatus::ResolvedUnadmitted, "resolved-unadmitted");

    assert_closed::<ReviewResponseStatus>(&["matched-unverified"]);
    assert_wire(
        ReviewResponseStatus::MatchedUnverified,
        "matched-unverified",
    );
}

#[test]
fn release_statuses_have_closed_schemas_and_exact_existing_wire_values() {
    assert_closed::<PreparedAuthorizationStatus>(&[
        "signature-and-local-preparations-verified-not-certified",
    ]);
    assert_wire(
        PreparedAuthorizationStatus::VerifiedNotCertified,
        "signature-and-local-preparations-verified-not-certified",
    );

    assert_closed::<ReadbackStatus>(&["native-ref-observations-not-settlement"]);
    assert_wire(
        ReadbackStatus::ObservedNotSettlement,
        "native-ref-observations-not-settlement",
    );

    assert_closed::<PreparationStatus>(&[
        "supported-profile-technical-preparations",
        "local-preparations-inspected-not-certified",
    ]);
    assert_wire(
        PreparationStatus::SupportedProfileTechnical,
        "supported-profile-technical-preparations",
    );
    assert_wire(
        PreparationStatus::InspectedNotCertified,
        "local-preparations-inspected-not-certified",
    );
}
