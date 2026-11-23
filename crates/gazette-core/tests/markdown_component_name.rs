//! Theme-local component names preserve exact spelling across native and schema boundaries.

use gazette_core::markdown::{ComponentName, InvalidComponentName};
use serde_json::json;

#[test]
fn local_names_preserve_exact_spelling_through_native_and_serde_routes() {
    for value in ["a", "Z", "callout", "Callout", "note-box_2", "A_0-z"] {
        let name: ComponentName = value.parse().expect("local name");
        assert_eq!(name.as_str(), value);
        assert_eq!(name.to_string(), value);
        assert_eq!(ComponentName::try_from(value.to_owned()), Ok(name.clone()));
        assert_eq!(
            serde_json::to_value(&name).expect("serialize"),
            json!(value)
        );
        assert_eq!(
            serde_json::from_value::<ComponentName>(json!(value)).expect("deserialize"),
            name
        );
    }
    assert_ne!(
        "Callout".parse::<ComponentName>().expect("uppercase"),
        "callout".parse::<ComponentName>().expect("lowercase")
    );
}

#[test]
fn invalid_names_cannot_enter_through_native_or_serde_construction() {
    for value in [
        "",
        "1note",
        "_note",
        "-note",
        "component:callout",
        "note.html",
        "a/b",
        "a\\b",
        "callout ",
        " callout",
        "a\tb",
        "callout\n",
        "callout\r\n",
        "a\0",
        "é",
        "a🦀",
        "a\u{2028}",
        "a\u{2029}",
    ] {
        assert_eq!(
            value.parse::<ComponentName>(),
            Err(InvalidComponentName),
            "{value:?}"
        );
        assert_eq!(
            ComponentName::try_from(value.to_owned()),
            Err(InvalidComponentName)
        );
        assert!(
            serde_json::from_value::<ComponentName>(json!(value)).is_err(),
            "{value:?}"
        );
    }
    for value in [
        json!(null),
        json!(1),
        json!(true),
        json!(["callout"]),
        json!({"name":"callout"}),
    ] {
        assert!(serde_json::from_value::<ComponentName>(value).is_err());
    }
}

#[test]
fn schema_uses_exact_ascii_start_and_whole_string_exclusion() {
    let schema = serde_json::to_value(schemars::schema_for!(ComponentName)).expect("schema");
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["pattern"], "^[A-Za-z]");
    assert_eq!(schema["not"], json!({"pattern": "[^A-Za-z0-9_-]"}));

    // Exercise all ASCII code points at the two grammatical positions, through
    // the native constructor and serde. Schema-engine execution belongs to the
    // compiler's existing JSON Schema validator, not a second validator here.
    for byte in 0_u8..=127 {
        let first = char::from(byte).to_string();
        let later = format!("a{}", char::from(byte));
        for (value, accepted) in [
            (first, byte.is_ascii_alphabetic()),
            (
                later,
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'),
            ),
        ] {
            assert_eq!(
                value.parse::<ComponentName>().is_ok(),
                accepted,
                "{value:?}"
            );
            assert_eq!(
                serde_json::from_value::<ComponentName>(json!(value)).is_ok(),
                accepted
            );
        }
    }
}
