//! Exact numeric values and unique decoded object members through the production decoder.

use gazette_core::json;
use serde_json::Value;

#[test]
fn numeric_profile_sees_raw_literals_once_and_never_string_contents() {
    let seen = std::cell::RefCell::new(Vec::new());
    let parsed = json::parse_with_number_validation(
        br#"{"-0":["1e0",0,-0,1e0,1.0,9007199254740993]}"#,
        &|number| {
            seen.borrow_mut().push(number.to_owned());
            true
        },
    )
    .expect("valid native grammar independent from the caller profile");
    assert!(parsed.is_object());
    assert_eq!(
        *seen.borrow(),
        ["0", "-0", "1e0", "1.0", "9007199254740993"]
    );
    assert!(json::parse_with_number_validation(br#"{"value":-0}"#, &|n| n != "-0").is_err());
    assert!(json::parse_with_number_validation(br#"{"value":"-0"}"#, &|_| false).is_ok());
    assert!(json::parse_with_number_validation(br#"{"a":0,"a":1}"#, &|_| true).is_err());
    assert!(json::parse_with_number_validation(b"01", &|_| true).is_err());
    assert!(json::parse_with_number_validation(b"0 trailing", &|_| true).is_err());
}

#[test]
fn numbers_keep_exact_literals_and_private_tokens_remain_ordinary_object_keys() {
    for (literal, number_text) in [
        ("18446744073709551617", "18446744073709551617"),
        ("-18446744073709551617", "-18446744073709551617"),
        (
            "0.12345678901234567890123456789",
            "0.12345678901234567890123456789",
        ),
        // Serde normalizes the positive exponent sign, not its numeric value.
        ("1e400", "1e+400"),
    ] {
        let source = format!(r#"{{"number":{literal},"nested":[{literal}]}}"#);
        let value = json::parse(source.as_bytes()).expect("exact JSON numbers");
        for number in [&value["number"], &value["nested"][0]] {
            assert!(number.is_number());
            assert_eq!(number.to_string(), number_text);
        }
    }
    for token in [
        "$serde_json::private::Number",
        "$serde_json::private::RawValue",
    ] {
        let source = format!(r#"{{"{token}":"123","ordinary":true}}"#);
        let value = json::parse(source.as_bytes()).expect("ordinary object");
        assert_eq!(
            value.as_object().expect("not an impersonated scalar").len(),
            2
        );
        assert_eq!(value[token], "123");
        assert_eq!(value["ordinary"], true);
    }
}

#[test]
fn duplicate_decoded_keys_trailing_values_and_excessive_depth_fail() {
    for input in [
        r#"{"a":1,"\u0061":2}"#,
        r#"[{"a":{"key":1,"k\u0065y":2}}]"#,
        "{} []",
        "[0,]",
        "NaN",
    ] {
        assert!(json::parse(input.as_bytes()).is_err(), "{input}");
    }
    let exact = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    assert!(json::parse(exact.as_bytes()).is_ok());
    let excessive = format!("[{exact}]");
    assert!(json::parse(excessive.as_bytes()).is_err());
    assert_eq!(json::parse(b" true \n").expect("scalar"), Value::Bool(true));
}
