//! One canonical digest spelling shared by serde and direct invocation.

use gazette_core::source::Sha256Digest;

#[test]
fn fixed_digest_parses_and_round_trips_through_json() {
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let parsed: Sha256Digest = expected.parse().expect("known SHA-256");
    assert_eq!(parsed, Sha256Digest::of(b"abc"));
    let json = serde_json::to_string(&parsed).expect("serialize");
    assert_eq!(json, format!("\"{expected}\""));
    assert_eq!(
        serde_json::from_str::<Sha256Digest>(&json).expect("deserialize"),
        parsed
    );
}

#[test]
fn noncanonical_digest_spellings_are_never_normalized() {
    for invalid in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        "é".repeat(32),
        format!(" {}", "a".repeat(64)),
    ] {
        assert!(invalid.parse::<Sha256Digest>().is_err(), "{invalid:?}");
        let json = serde_json::to_string(&invalid).expect("fixture");
        assert!(
            serde_json::from_str::<Sha256Digest>(&json).is_err(),
            "{invalid:?}"
        );
    }
    for json in ["null", "0", "[]", "{}"] {
        assert!(serde_json::from_str::<Sha256Digest>(json).is_err());
    }
}
