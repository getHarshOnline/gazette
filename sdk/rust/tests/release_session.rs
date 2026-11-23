//! Shared wire parsing reaches the native option/selection owners without lossy authorization JSON.

use gazette_sdk::{release::session, store};
use serde_json::json;

#[test]
fn public_authority_observation_preserves_the_native_wire_and_schema_shape() {
    use gazette_sdk::process::{Cleanup, Exit, Failure};
    use session::AuthorityObservation as Public;

    let public = [
        Public::ExecutionFailed {
            failure: Failure::Timeout,
            cleanup: Cleanup::Uncertain,
        },
        Public::NonzeroExit {
            exit: Exit { code: Some(9) },
            stdout_bytes: 17,
        },
        Public::InvalidBinding {
            code: "release_binding_shape",
            stdout_bytes: 23,
        },
        Public::Acquired { stdout_bytes: 31 },
    ];
    let native = [
        gazette_adapter_authority::Observation::ExecutionFailed {
            failure: Failure::Timeout,
            cleanup: Cleanup::Uncertain,
        },
        gazette_adapter_authority::Observation::NonzeroExit {
            exit: Exit { code: Some(9) },
            stdout_bytes: 17,
        },
        gazette_adapter_authority::Observation::InvalidBinding {
            code: "release_binding_shape",
            stdout_bytes: 23,
        },
        gazette_adapter_authority::Observation::Acquired { stdout_bytes: 31 },
    ];
    for (public, native) in public.into_iter().zip(native) {
        assert_eq!(
            serde_json::to_value(public).expect("public observation"),
            serde_json::to_value(native).expect("native observation")
        );
    }
    assert_eq!(
        serde_json::to_value(schemars::schema_for!(Public)).expect("public schema"),
        serde_json::to_value(schemars::schema_for!(
            gazette_adapter_authority::Observation
        ))
        .expect("native schema")
    );
}

fn request() -> session::OpenRequest {
    session::OpenRequest {
        coordinator: store::coordinator::LocalCoordinatorSelection {
            anchor_root: "/private/fixture/anchor".into(),
            binding: store::coordinator::LocalCoordinatorBinding {
                coordinator_id: "fixture".into(),
                principal_scope: [1; 32].into(),
                journal_root: "/private/fixture/journal".into(),
                journal_id: [2; 32],
                journal_epoch: 1,
            },
        },
        expected: store::JournalHeader {
            schema_version: 1,
            journal_id: [2; 32],
            journal_epoch: 1,
            writer_fence: 3,
        },
        protected_roots: vec![],
        writer: Default::default(),
        allow_anchor_write: false,
        git: None,
        reviewed: None,
        authority: None,
    }
}

#[test]
fn shared_open_decode_retains_native_defaults_and_rejects_positional_or_duplicate_objects() {
    let complete = serde_json::to_value(request()).expect("native request serialization");
    let bytes = serde_json::to_vec(&complete).expect("native encoded request");
    let decoded = session::parse_open(&bytes).expect("shared parser accepts native request");
    assert!(!decoded.writer.allow_generation_advance);
    let mut defaults = complete.clone();
    defaults.as_object_mut().expect("object").remove("writer");
    defaults
        .as_object_mut()
        .expect("object")
        .remove("allow_anchor_write");
    assert!(
        !session::parse_open(&serde_json::to_vec(&defaults).expect("encode defaults"))
            .expect("missing denied defaults")
            .allow_anchor_write
    );
    for (path, value) in [
        ("writer", json!([false, {"allow_local_reservation": false}])),
        ("expected", json!([1, vec![2; 32], 1, 3])),
        (
            "coordinator",
            json!([
                "/private/fixture/anchor",
                complete["coordinator"]["binding"]
            ]),
        ),
    ] {
        let mut invalid = complete.clone();
        invalid[path] = value;
        assert!(
            session::parse_open(&serde_json::to_vec(&invalid).expect("bad shape bytes")).is_err()
        );
    }
    let text = String::from_utf8(bytes).expect("JSON UTF-8");
    let duplicate = text.replacen(
        "\"writer_fence\":3",
        "\"writer_fence\":3,\"writer_fence\":3",
        1,
    );
    assert_ne!(duplicate, text);
    assert!(session::parse_open(duplicate.as_bytes()).is_err());
    let schema = schemars::schema_for!(session::OpenRequest);
    jsonschema::validator_for(schema.as_value())
        .expect("native open schema")
        .validate(&complete)
        .expect("native request satisfies discovery contract");
}

#[test]
fn shared_operation_decode_preserves_raw_authorization_and_closes_unit_variants() {
    let raw = " {\"duplicate\":1,\"duplicate\":2} ";
    let value = json!({
        "operation":"register", "expected": request().expected,
        "input":{"authorization": raw, "selections":[]}
    });
    let bytes = serde_json::to_vec(&value).expect("outer command bytes");
    let session::Operation::Register { input, options, .. } = session::parse_operation(&bytes)
        .expect("outer parser does not normalize or verify signed inner JSON")
    else {
        panic!("register")
    };
    assert_eq!(input.authorization, raw);
    assert!(!options.record.allow_record);
    for bytes in [
        br#"{"operation":"inspect","extra":null}"#.as_slice(),
        br#"{"operation":"close","extra":[]}"#.as_slice(),
        br#"{"operation":"inspect","operation":"close"}"#.as_slice(),
        br#"["inspect"]"#.as_slice(),
    ] {
        assert!(session::parse_operation(bytes).is_err(), "{bytes:?}");
    }
    assert!(matches!(
        session::parse_operation(br#"{"operation":"inspect"}"#),
        Ok(session::Operation::Inspect)
    ));
    assert!(matches!(
        session::parse_operation(br#"{"operation":"close"}"#),
        Ok(session::Operation::Close)
    ));
    let schema = schemars::schema_for!(session::Operation);
    jsonschema::validator_for(schema.as_value())
        .expect("native operation schema")
        .validate(&value)
        .expect("discovery and native input agree");
}

#[test]
fn explicit_null_and_omitted_optional_selections_share_one_native_wire_contract() {
    let schema = schemars::schema_for!(session::OpenRequest);
    let validator = jsonschema::validator_for(schema.as_value()).expect("native open schema");
    let complete = serde_json::to_value(request()).expect("native request");
    assert_eq!(complete["git"], serde_json::Value::Null);
    assert_eq!(complete["authority"], serde_json::Value::Null);
    for omit_git in [false, true] {
        for omit_authority in [false, true] {
            let mut value = complete.clone();
            if omit_git {
                value.as_object_mut().expect("request").remove("git");
            }
            if omit_authority {
                value.as_object_mut().expect("request").remove("authority");
            }
            validator
                .validate(&value)
                .expect("advertised nullable/default fields");
            let decoded = session::parse_open(&serde_json::to_vec(&value).expect("request bytes"))
                .expect("nullable/default fields decode");
            assert!(decoded.git.is_none());
            assert!(decoded.authority.is_none());
            assert_eq!(
                serde_json::to_value(decoded).expect("canonical request"),
                complete
            );
        }
    }
    let mut with_authority = complete;
    with_authority["authority"] = json!({
        "command":{"program":"/fixture/provider","directory":"/fixture","executable":null},
        "input":"independently selected request", "max_input_bytes":1024,
        "max_output_bytes":4096, "timeout_ms":1000
    });
    let mut canonical = None;
    for omitted in [false, true] {
        let mut value = with_authority.clone();
        if omitted {
            value["authority"]["command"]
                .as_object_mut()
                .expect("command")
                .remove("executable");
        }
        validator
            .validate(&value)
            .expect("native nullable executable schema");
        let decoded = session::parse_open(&serde_json::to_vec(&value).expect("selected command"))
            .expect("nullable executable matches schema");
        assert!(
            decoded
                .authority
                .as_ref()
                .expect("selected authority")
                .command
                .executable
                .is_none()
        );
        let normalized = serde_json::to_value(decoded).expect("native normalized request");
        assert_eq!(
            normalized["authority"]["command"]["executable"],
            serde_json::Value::Null
        );
        if let Some(expected) = &canonical {
            assert_eq!(&normalized, expected);
        }
        canonical = Some(normalized);
    }
}

#[test]
fn nullable_defaults_do_not_admit_unknown_null_members_on_unit_operations() {
    let schema = schemars::schema_for!(session::Operation);
    let validator = jsonschema::validator_for(schema.as_value()).expect("native operation schema");
    for operation in ["inspect", "close"] {
        for field in ["extra", "git", "authority", "options"] {
            let value = json!({"operation":operation, field:null});
            assert!(!validator.is_valid(&value), "{value}");
            assert!(
                session::parse_operation(
                    &serde_json::to_vec(&value).expect("unknown-null request")
                )
                .is_err(),
                "{value}"
            );
        }
    }
}

#[test]
fn native_duration_objects_match_discovery_and_positional_tuples_are_rejected() {
    let schema = schemars::schema_for!(session::Operation);
    let validator = jsonschema::validator_for(schema.as_value()).expect("native operation schema");
    for operation in ["consume", "dispatch", "reconcile"] {
        let mut value = json!({"operation":operation,"expected":request().expected});
        if operation == "reconcile" {
            value["authorization_id"] = json!("record");
            value["number"] = json!(1);
        } else {
            value["input"] = json!({"authorization":"{}","selections":[]});
            if operation == "dispatch" {
                value["ordinal"] = json!(0);
                value["number"] = json!(1);
            }
        }
        let fields: &[&str] = if operation == "dispatch" {
            &["read", "dispatch"]
        } else {
            &["read"]
        };
        for &field in fields {
            for (duration, accepted) in
                [(json!({"secs":1,"nanos":0}), true), (json!([1, 0]), false)]
            {
                let mut selected = value.clone();
                selected["options"] = json!({field:{"timeout":duration}});
                assert_eq!(validator.is_valid(&selected), accepted, "{selected}");
                assert_eq!(
                    session::parse_operation(
                        &serde_json::to_vec(&selected).expect("duration input")
                    )
                    .is_ok(),
                    accepted,
                    "{selected}"
                );
            }
        }
    }
}

#[test]
fn reviewed_operations_keep_confirmation_separate_and_match_the_discovered_schema() {
    let schema = schemars::schema_for!(session::Operation);
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for operation in ["register_reviewed", "consume_reviewed", "dispatch_reviewed"] {
        let mut value = json!({"operation":operation, "expected": request().expected,
            "input":{"approval":" {\"duplicate\":1,\"duplicate\":2} ",
                "confirmed_sha256":"11".repeat(32),"observed_at":12,"selections":[]}});
        if operation == "dispatch_reviewed" {
            value["ordinal"] = json!(0);
            value["number"] = json!(1);
        }
        validator.validate(&value).unwrap();
        assert!(session::parse_operation(&serde_json::to_vec(&value).unwrap()).is_ok());
        value["input"]
            .as_object_mut()
            .unwrap()
            .remove("confirmed_sha256");
        assert!(!validator.is_valid(&value));
        assert!(session::parse_operation(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let value = json!({"operation":"review", "expected":request().expected,
        "input":{"record_id":"review","nonce":"once","approved_at":12,"selections":[]}});
    validator.validate(&value).unwrap();
    assert!(session::parse_operation(&serde_json::to_vec(&value).unwrap()).is_ok());
    let mut null_scope = serde_json::to_value(request()).unwrap();
    null_scope["reviewed"] = serde_json::Value::Null;
    let open_schema = schemars::schema_for!(session::OpenRequest);
    assert!(
        !jsonschema::validator_for(open_schema.as_value())
            .unwrap()
            .is_valid(&null_scope)
    );
    assert!(session::parse_open(&serde_json::to_vec(&null_scope).unwrap()).is_err());
    let signed = serde_json::to_value(request()).unwrap();
    assert!(
        signed.get("reviewed").is_none(),
        "old absent-field wire shape remains unchanged"
    );
}
