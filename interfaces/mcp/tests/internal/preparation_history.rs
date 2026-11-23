//! Real native inspection, permission and protocol-shape boundaries.

use super::*;
use gazette_cli::sdk::store;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

fn arguments(route: Route) -> Value {
    let mut value = json!({"path":"/missing/gazette.db"});
    match route {
        Route::Checkpoint => value["checkpoint_id"] = json!(Sha256Digest::of(b"checkpoint")),
        Route::Worktree => {
            value["preparation_id"] = json!(Sha256Digest::of(b"preparation"));
            value["commit"] = json!("0123456789012345678901234567890123456789");
            value["protected_roots"] = json!([]);
        }
    }
    value
}

fn decode(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named arguments").clone())
}

#[test]
fn discovery_and_decoder_preserve_exact_coordinates_named_objects_and_separate_permissions() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
        let base = arguments(route);
        assert!(schema.is_valid(&base));
        assert!(decode(route, base.clone()).is_ok());
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(descriptor["annotations"]["destructiveHint"], false);
        assert_eq!(descriptor["annotations"]["idempotentHint"], true);
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
        for (field, replacement, accepted) in [
            ("path", json!("relative.db"), false),
            ("read", json!([]), false),
            ("read", Value::Null, false),
            ("read", json!({}), true),
            ("read", json!({"allow_write":true}), false),
            ("expected_header", Value::Null, true),
            ("expected_header", json!([]), false),
            ("expected_header", json!({}), false),
            ("allow_read", json!(true), true),
            ("allow_read", json!("true"), false),
            ("allow_initialize", json!(true), false),
            ("allow_network", json!(true), false),
            ("advance_writer_generation", json!(true), false),
            ("record_preparation", json!(true), false),
            ("synchronize_worktree", json!(true), false),
        ] {
            let mut value = base.clone();
            value[field] = replacement;
            assert_eq!(
                schema.is_valid(&value),
                accepted,
                "{}: {value}",
                route.name()
            );
            assert_eq!(decode(route, value).is_ok(), accepted);
        }
        let coordinate = match route {
            Route::Checkpoint => "checkpoint_id",
            Route::Worktree => "preparation_id",
        };
        for invalid in [json!("latest"), json!([]), json!(null)] {
            let mut value = base.clone();
            value[coordinate] = invalid;
            assert!(!schema.is_valid(&value));
            assert!(decode(route, value).is_err());
        }
        let mut value = base;
        value["expected_header"] = json!(JournalHeader {
            schema_version: 1,
            journal_id: [8; 32],
            journal_epoch: 0,
            writer_fence: 2,
        });
        assert!(schema.is_valid(&value));
        assert!(decode(route, value.clone()).is_ok());
        value["expected_header"]["extra"] = json!(true);
        assert!(!schema.is_valid(&value));
        assert!(decode(route, value).is_err());
    }
}

#[test]
fn worktree_options_do_not_accept_positional_objects_unknown_fields_or_relative_roots() {
    let descriptor = json!(Route::Worktree.descriptor());
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    for (field, replacement, accepted) in [
        ("worktree_limits", json!({}), true),
        ("worktree_limits", json!([]), false),
        ("worktree_limits", json!({"output":[]}), false),
        ("worktree_limits", json!({"output":{"content":[]}}), false),
        ("worktree_limits", json!({"history":[]}), false),
        ("worktree_limits", json!({"unknown":1}), false),
        ("worktree_limits", json!({"history":{"unknown":1}}), false),
        ("worktree_limits", json!({"history":{"objects":1.5}}), false),
        ("worktree_limits", json!({"history":{"bytes":-1}}), false),
        (
            "worktree_limits",
            json!({"history":{"object_bytes":"4"}}),
            false,
        ),
        ("protected_roots", json!(["relative"]), false),
        ("protected_roots", json!(["/configured/source"]), true),
        ("protected_roots", Value::Null, false),
        ("commit", json!("HEAD"), false),
        ("commit", json!([]), false),
    ] {
        let mut value = arguments(Route::Worktree);
        value[field] = replacement;
        assert_eq!(schema.is_valid(&value), accepted, "{value}");
        assert_eq!(decode(Route::Worktree, value).is_ok(), accepted);
    }
    let mut value = arguments(Route::Worktree);
    value
        .as_object_mut()
        .expect("arguments")
        .remove("protected_roots");
    assert!(!schema.is_valid(&value));
    assert!(decode(Route::Worktree, value).is_err());
}

#[test]
fn denied_permissions_and_native_invalid_limits_precede_database_access() {
    let root = tempfile::tempdir().expect("disposable missing database");
    for route in Route::ALL {
        for (allowed, read, code) in [
            (false, ReadOptions::default(), "storage_permission"),
            (
                true,
                ReadOptions::default(),
                "store_coordination_permission",
            ),
            (
                true,
                ReadOptions {
                    allow_coordination_files: true,
                    value_bytes: 0,
                    ..Default::default()
                },
                "store_invalid_limits",
            ),
        ] {
            let mut value = arguments(route);
            value["path"] = json!(root.path().join("absent.db"));
            value["allow_read"] = json!(allowed);
            value["read"] = json!(read);
            let result = decode(route, value)
                .expect("typed request")
                .execute(8192)
                .expect("bounded failure");
            assert_eq!(result.is_error, Some(true));
            let output = result.structured_content.expect("failure");
            assert_eq!(output["code"], code);
            let descriptor = json!(route.descriptor());
            jsonschema::validator_for(&descriptor["outputSchema"])
                .expect("output schema")
                .validate(&output)
                .expect("failure schema");
        }
    }
    assert!(
        fs::read_dir(root.path())
            .expect("unchanged directory")
            .next()
            .is_none()
    );
}

#[test]
fn real_missing_history_matches_sdk_and_does_not_synthesize_preparation_or_advance_generation() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private journal");
    let read = ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    };
    let initialized = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read,
            },
            ..Default::default()
        },
    )
    .expect("native initialization");
    let before = fs::read(&initialized.database).expect("initial database");
    for route in Route::ALL {
        let mut value = arguments(route);
        value["path"] = json!(initialized.database);
        value["allow_read"] = json!(true);
        value["read"] = json!(read);
        value["expected_header"] = json!(initialized.header);
        let result = decode(route, value.clone())
            .expect("typed request")
            .execute(65536)
            .expect("native result");
        let output = result.structured_content.expect("complete result");
        match route {
            Route::Checkpoint => {
                let expected = release_commit::inspect_checkpoint(
                    &initialized.database,
                    Sha256Digest::of(b"checkpoint"),
                    Some(&initialized.header),
                    read,
                )
                .expect("SDK absent checkpoint");
                assert!(expected.checkpoint.is_none());
                assert_eq!(result.is_error, Some(false));
                assert_eq!(output, json!(expected));
            }
            Route::Worktree => {
                assert_eq!(result.is_error, Some(true));
                assert_eq!(output["code"], "preparation_missing");
            }
        }
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
        schema.validate(&output).expect("native output schema");
        let limited = decode(route, value.clone())
            .expect("typed request")
            .execute(1)
            .expect("bounded fallback");
        assert_eq!(
            limited.structured_content,
            Some(json!({"code":"mcp_output_limit"}))
        );
        let mut stale = initialized.header;
        stale.writer_fence += 1;
        value["expected_header"] = json!(stale);
        let refused = decode(route, value)
            .expect("typed request")
            .execute(8192)
            .expect("native refusal");
        assert_eq!(
            refused.structured_content.expect("refusal")["code"],
            "store_journal_mismatch"
        );
    }
    let after = store::inspect_journal(&initialized.database, Some(&initialized.header), read)
        .expect("unchanged header");
    assert_eq!(after.header, initialized.header);
    assert_eq!(
        fs::read(&initialized.database).expect("database after reads"),
        before
    );
}
