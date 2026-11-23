use super::*;
#[test]
fn encodes_values_without_creating_query_separators_or_markup() {
    let filter = UrlQuery { output_bytes: 1024 };
    for (input, expected) in [
        ("a b+c&x=y#z?", "a%20b%2Bc%26x%3Dy%23z%3F"),
        ("雪💡", "%E9%9B%AA%F0%9F%92%A1"),
        ("</script>\"'\r\n\0", "%3C%2Fscript%3E%22%27%0D%0A%00"),
        ("a-Z_9.~", "a-Z_9.~"),
    ] {
        assert_eq!(
            filter
                .filter(&Value::String(input.into()), &HashMap::new())
                .unwrap(),
            expected
        );
    }
}
#[test]
fn refuses_types_arguments_and_expansion_without_partial_output() {
    let exact = UrlQuery { output_bytes: 9 };
    assert_eq!(
        exact
            .filter(&Value::String("雪".into()), &HashMap::new())
            .unwrap(),
        "%E9%9B%AA"
    );
    let short = UrlQuery { output_bytes: 8 };
    assert!(
        short
            .filter(&Value::String("雪".into()), &HashMap::new())
            .is_err()
    );
    for value in [Value::Null, Value::Bool(false), serde_json::json!(["x"])] {
        assert!(exact.filter(&value, &HashMap::new()).is_err());
    }
    assert!(
        exact
            .filter(
                &Value::String("x".into()),
                &HashMap::from([("mode".into(), Value::Bool(true))])
            )
            .is_err()
    );
    assert_eq!(
        UrlQuery { output_bytes: 0 }
            .filter(&Value::String(String::new()), &HashMap::new())
            .unwrap(),
        ""
    );
}
