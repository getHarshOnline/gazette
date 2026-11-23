use super::*;

#[test]
fn operation_coordinates_are_native_lowercase_exact_and_duplicate_free() {
    let a = source::Sha256Digest::of(b"a");
    let b = source::Sha256Digest::of(b"b");
    let first = parse_operation(&format!("site:one={a}")).expect("native coordinate");
    assert_eq!(
        parse_operation(&format!("site:literal=key={a}"))
            .expect("native key spelling is not narrowed by the delimiter")
            .site
            .as_str(),
        "site:literal=key"
    );
    let second = parse_operation(&format!("site:other={b}")).expect("native coordinate");
    let ids = operation_ids(vec![first.clone(), second]).expect("distinct complete coordinates");
    assert_eq!(ids[&"site:one".parse::<Key>().unwrap()], a);
    assert_eq!(
        operation_ids(vec![first.clone(), first.clone()]).unwrap_err(),
        "materialization_operation_duplicate"
    );
    let other = parse_operation(&format!("site:other={a}")).expect("same operation spelling");
    assert_eq!(
        operation_ids(vec![first, other]).unwrap_err(),
        "materialization_operation_duplicate"
    );
    for invalid in [
        "site:one".to_owned(),
        format!("one={a}"),
        format!("content:one={a}"),
        format!("site:one={}", a.to_string().to_uppercase()),
        "site:one=00".to_owned(),
        format!("site:one={a}=ignored"),
    ] {
        assert!(parse_operation(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn compact_carrier_preserves_coordinates_and_refuses_insufficient_reservation() {
    let operations = operation_ids(vec![
        parse_operation(&format!(
            "site:one={}",
            source::Sha256Digest::of(b"operation")
        ))
        .unwrap(),
    ])
    .unwrap();
    let header = store::JournalHeader {
        schema_version: 1,
        journal_id: [7; 32],
        journal_epoch: 1,
        writer_fence: 2,
    };
    let custody = Custody {
        root: std::path::Path::new("/private/historical-journal"),
        expected_header: header,
        acquired_header: Some(store::JournalHeader {
            writer_fence: 3,
            ..header
        }),
        request_sha256: source::Sha256Digest::of(b"complete private request"),
        operations: &operations,
    };
    assert!(reserve(&custody, 4096).is_ok());
    assert!(
        reserve(&custody, 1).is_err(),
        "native open reserve refuses before effects"
    );
    let frame = Frame {
        event: "delivery_incomplete",
        custody: &custody,
        native_status: "materialization_failed",
        details_complete: false,
        detail: (),
        close: &Close::Failed { code: "store_io" },
    };
    let encoded = encode(&frame, 4096).expect("reserved compact witness");
    assert_eq!(encoded.last(), Some(&b'\n'));
    let value: serde_json::Value = serde_json::from_slice(&encoded).expect("exact JSONL frame");
    assert_eq!(value["custody"]["root"], "/private/historical-journal");
    assert_eq!(
        value["custody"]["expected_header"],
        serde_json::json!(header)
    );
    assert_eq!(value["custody"]["acquired_header"]["writer_fence"], 3);
    assert_eq!(
        value["custody"]["request_sha256"],
        custody.request_sha256.to_string()
    );
    assert_eq!(
        value["custody"]["operations"],
        serde_json::json!(operations)
    );
    assert_eq!(value["details_complete"], false);
    assert_eq!(value["detail"], serde_json::Value::Null);
    assert_eq!(value["close"]["code"], "store_io");
    assert!(encode(&frame, encoded.len() - 1).is_err());
    assert_eq!(
        encode(&frame, encoded.len()).expect("including framing byte"),
        encoded
    );
}
