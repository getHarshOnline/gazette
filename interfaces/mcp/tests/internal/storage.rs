//! Native storage argument, observation and bounded-response contracts.

use super::*;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

fn arguments(route: Route) -> Value {
    let mut value = json!({"path":"/missing/gazette-inspection.db"});
    if matches!(route, Route::Materialization) {
        value["output_root"] = json!("/historical/gazette-output");
    } else if !matches!(route, Route::Store | Route::Journal | Route::Registry) {
        value["record_id"] = json!("selected-record");
    }
    value
}

fn decode(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named arguments").clone())
}

#[test]
fn discovery_and_decoder_agree_on_independent_permissions_and_named_native_shapes() {
    let header = JournalHeader {
        schema_version: 1,
        journal_id: [7; 32],
        journal_epoch: 0,
        writer_fence: 9,
    };
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
        let input = decode(route, arguments(route)).expect("default input");
        assert!(!input.request.allow_read);
        assert_eq!(json!(input.request.read), json!(ReadOptions::default()));
        assert!(input.request.phase.is_none());
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(descriptor["annotations"]["destructiveHint"], false);
        assert_eq!(descriptor["annotations"]["idempotentHint"], true);
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
        for (field, replacement, accepted) in [
            ("path", json!("relative.db"), false),
            ("path", Value::Null, false),
            ("read", json!([]), false),
            ("read", Value::Null, false),
            ("read", json!({"allow_write":true}), false),
            ("read", json!({}), true),
            (
                "expected_header",
                json!(header),
                !matches!(route, Route::Store),
            ),
            (
                "expected_header",
                Value::Null,
                !matches!(route, Route::Store),
            ),
            ("expected_header", json!([]), false),
            ("expected_header", json!({}), false),
            ("allow_initialize", json!(true), false),
            ("allow_network", json!(true), false),
            ("reserve_local_coordination", json!(true), false),
        ] {
            let mut value = arguments(route);
            value[field] = replacement;
            assert_eq!(
                schema.is_valid(&value),
                accepted,
                "{}: {value}",
                route.name()
            );
            assert_eq!(decode(route, value).is_ok(), accepted);
        }
        if !matches!(route, Route::Store) {
            let mut value = arguments(route);
            value["expected_header"] = json!(header);
            assert_eq!(
                decode(route, value.clone())
                    .expect("exact header")
                    .request
                    .phase,
                Some(header)
            );
            value["expected_header"]["unknown"] = json!(true);
            assert!(!schema.is_valid(&value));
            assert!(decode(route, value).is_err());
        }
        for field in [
            "storage_bytes",
            "busy_timeout_ms",
            "sql_bytes",
            "value_bytes",
            "vm_steps",
        ] {
            for invalid in [
                json!(1.5),
                json!("1"),
                json!(18_446_744_073_709_551_616.0_f64),
            ] {
                let mut value = arguments(route);
                value["read"] = json!({field: invalid});
                assert!(!schema.is_valid(&value), "{field}: {value}");
                assert!(decode(route, value).is_err());
            }
        }
    }
}

#[test]
fn every_read_integer_boundary_matches_native_representation_without_changing_validation() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
        for (field, minimum, maximum, below, above) in [
            (
                "storage_bytes",
                json!(0),
                json!(u64::MAX),
                json!(-1),
                json!(18_446_744_073_709_551_616.0_f64),
            ),
            (
                "vm_steps",
                json!(0),
                json!(u64::MAX),
                json!(-1),
                json!(18_446_744_073_709_551_616.0_f64),
            ),
            (
                "busy_timeout_ms",
                json!(0),
                json!(u32::MAX),
                json!(-1),
                json!(u64::from(u32::MAX) + 1),
            ),
            (
                "sql_bytes",
                json!(i32::MIN),
                json!(i32::MAX),
                json!(i64::from(i32::MIN) - 1),
                json!(i64::from(i32::MAX) + 1),
            ),
            (
                "value_bytes",
                json!(i32::MIN),
                json!(i32::MAX),
                json!(i64::from(i32::MIN) - 1),
                json!(i64::from(i32::MAX) + 1),
            ),
        ] {
            for (value, accepted) in [
                (minimum, true),
                (maximum, true),
                (below, false),
                (above, false),
            ] {
                let mut arguments = arguments(route);
                arguments["read"] = json!({field:value});
                assert_eq!(
                    schema.is_valid(&arguments),
                    accepted,
                    "{field}: {arguments}"
                );
                assert_eq!(decode(route, arguments).is_ok(), accepted, "{field}");
            }
        }
    }
    // Discovery describes the Rust representation. Existing native acceptance
    // still refuses nonpositive SQL/value bounds and unsupported retry durations.
    for read in [
        ReadOptions {
            allow_coordination_files: true,
            sql_bytes: i32::MIN,
            ..Default::default()
        },
        ReadOptions {
            allow_coordination_files: true,
            value_bytes: 0,
            ..Default::default()
        },
        ReadOptions {
            allow_coordination_files: true,
            busy_timeout_ms: u32::MAX,
            ..Default::default()
        },
    ] {
        assert_eq!(
            read.validate()
                .expect_err("existing native semantic bound")
                .code(),
            "store_invalid_limits"
        );
    }
}

#[test]
fn header_integer_and_byte_array_shapes_match_the_native_owner() {
    let header = json!(JournalHeader {
        schema_version: 1,
        journal_id: [0; 32],
        journal_epoch: 0,
        writer_fence: 0,
    });
    for route in [Route::Journal, Route::Registry, Route::Materialization] {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
        let mut cases = Vec::new();
        for (field, minimum, maximum, below, above) in [
            (
                "schema_version",
                json!(0),
                json!(u32::MAX),
                json!(-1),
                json!(u64::from(u32::MAX) + 1),
            ),
            (
                "journal_epoch",
                json!(0),
                json!(u64::MAX),
                json!(-1),
                json!(18_446_744_073_709_551_616.0_f64),
            ),
            (
                "writer_fence",
                json!(0),
                json!(u64::MAX),
                json!(-1),
                json!(18_446_744_073_709_551_616.0_f64),
            ),
        ] {
            for (value, accepted) in [
                (minimum, true),
                (maximum, true),
                (below, false),
                (above, false),
            ] {
                let mut value_header = header.clone();
                value_header[field] = value;
                cases.push((value_header, accepted));
            }
        }
        for (value, accepted) in [
            (json!(vec![0; 32]), true),
            (json!(vec![255; 32]), true),
            (json!(vec![0; 31]), false),
            (json!(vec![0; 33]), false),
            (json!(vec![-1; 32]), false),
            (json!(vec![256; 32]), false),
            (json!(vec![1.5; 32]), false),
            (json!("not an array"), false),
        ] {
            let mut value_header = header.clone();
            value_header["journal_id"] = value;
            cases.push((value_header, accepted));
        }
        for (header, accepted) in cases {
            assert_eq!(
                serde_json::from_value::<JournalHeader>(header.clone()).is_ok(),
                accepted
            );
            let mut value = arguments(route);
            value["expected_header"] = header;
            assert_eq!(schema.is_valid(&value), accepted, "header: {value}");
            assert_eq!(decode(route, value).is_ok(), accepted);
        }
    }
}

#[test]
fn output_numeric_shapes_reject_values_the_native_reports_cannot_represent() {
    let observation = json!(store::Observation {
        provider: "sqlite".into(),
        provider_version: "fixture".into(),
        provider_source_id: "fixture".into(),
        application_id: 0,
        user_version: 0,
        journal_mode: "delete".into(),
        sql_read_only: true,
    });
    let descriptor = json!(Route::Store.descriptor());
    let schema = jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native observation output schema");
    for field in ["application_id", "user_version"] {
        for (number, accepted) in [
            (json!(i32::MIN), true),
            (json!(i32::MAX), true),
            (json!(i64::from(i32::MIN) - 1), false),
            (json!(i64::from(i32::MAX) + 1), false),
        ] {
            let mut value = observation.clone();
            value[field] = number;
            assert_eq!(schema.is_valid(&value), accepted, "{field}: {value}");
            assert_eq!(
                serde_json::from_value::<store::Observation>(value).is_ok(),
                accepted
            );
        }
    }
    let digest = gazette_cli::sdk::source::Sha256Digest::of(b"schema boundary only");
    let registry = json!(store::RegistryInspection {
        journal: store::JournalInspection {
            header: JournalHeader {
                schema_version: 1,
                journal_id: [0; 32],
                journal_epoch: 0,
                writer_fence: 0
            },
            verifier: serde_json::from_value(observation).expect("native observation"),
            schema_sha256: digest,
        },
        registry: Some(store::RegistryHead {
            semantic_sha256: digest,
            configuration_sha256: digest,
            rows_sha256: digest,
            rows: 0
        }),
    });
    let descriptor = json!(Route::Registry.descriptor());
    let schema = jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native registry output schema");
    for (number, accepted) in [
        (json!(0), true),
        (json!(u64::MAX), true),
        (json!(-1), false),
        (json!(18_446_744_073_709_551_616.0_f64), false),
    ] {
        let mut value = registry.clone();
        value["registry"]["rows"] = number;
        assert_eq!(schema.is_valid(&value), accepted, "rows: {value}");
        assert_eq!(
            serde_json::from_value::<store::RegistryHead>(value["registry"].clone()).is_ok(),
            accepted
        );
    }
}

#[test]
fn independent_read_and_coordination_permissions_precede_native_access() {
    let root = tempfile::tempdir().expect("disposable missing database");
    for route in Route::ALL {
        for (allow_read, coordination, code) in [
            (false, false, "storage_permission"),
            (false, true, "storage_permission"),
            (true, false, "store_coordination_permission"),
            (true, true, "store_io"),
        ] {
            let mut value = arguments(route);
            value["path"] = json!(root.path().join("missing.db"));
            value["allow_read"] = json!(allow_read);
            value["read"] = json!({"allow_coordination_files":coordination});
            let result = decode(route, value)
                .expect("typed request")
                .execute(8192)
                .expect("result");
            assert_eq!(result.is_error, Some(true));
            let output = result.structured_content.expect("classified failure");
            assert_eq!(output["code"], code);
            let descriptor = json!(route.descriptor());
            jsonschema::validator_for(&descriptor["outputSchema"])
                .expect("output schema")
                .validate(&output)
                .expect("native failure shape");
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
fn actual_native_reports_validate_and_overflow_does_not_return_a_metadata_prefix() {
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
    .expect("real native initialization");
    let before = fs::read(&initialized.database).expect("initial bytes");
    for route in Route::ALL {
        let mut value = arguments(route);
        value["path"] = json!(initialized.database);
        value["allow_read"] = json!(true);
        value["read"] = json!(read);
        if !matches!(route, Route::Store) {
            value["expected_header"] = json!(initialized.header);
        }
        let expected = match route {
            Route::Store => {
                json!(store::observe(&initialized.database, read).expect("SDK observation"))
            }
            Route::Journal => json!(
                store::inspect_journal(&initialized.database, Some(&initialized.header), read)
                    .expect("SDK journal")
            ),
            Route::Registry => json!(
                store::inspect_registry(&initialized.database, Some(&initialized.header), read)
                    .expect("SDK registry")
            ),
            Route::Materialization => json!(
                store::inspect_materialization(
                    &initialized.database,
                    std::path::Path::new("/historical/gazette-output"),
                    Some(&initialized.header),
                    read
                )
                .expect("SDK materialization history")
            ),
            Route::Authorization => json!(
                store::inspect_release_authorization(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK authorization")
            ),
            Route::Leases => json!(
                store::inspect_release_leases(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK lease history")
            ),
            Route::Consumption => json!(
                store::inspect_release_consumption(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK consumption")
            ),
            Route::Attempts => json!(
                store::inspect_release_attempts(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK attempts")
            ),
            Route::Reconciliations => json!(
                store::inspect_release_reconciliations(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK reconciliations")
            ),
        };
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
        let result = decode(route, value.clone())
            .expect("request")
            .execute(8192)
            .expect("report");
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content, Some(expected.clone()));
        schema.validate(&expected).expect("native success schema");
        let mut corrupt = expected;
        let key = match route {
            Route::Store => "sql_read_only",
            Route::Journal => "header",
            Route::Registry => "registry",
            Route::Materialization => "frontier",
            Route::Authorization | Route::Leases | Route::Consumption => "record",
            Route::Attempts => "attempts",
            Route::Reconciliations => "records",
        };
        corrupt[key] = json!("not the native shape");
        assert!(!schema.is_valid(&corrupt));
        let limited = decode(route, value)
            .expect("request")
            .execute(1)
            .expect("bounded fallback");
        assert_eq!(limited.is_error, Some(true));
        let output = limited.structured_content.expect("fixed failure");
        assert_eq!(output, json!({"code":"mcp_output_limit"}));
        schema.validate(&output).expect("limit schema");
    }
    assert_eq!(
        fs::read(&initialized.database).expect("unchanged bytes"),
        before
    );
    assert_eq!(
        fs::read_dir(root.path())
            .expect("closed native directory")
            .count(),
        1
    );
}

#[test]
fn release_history_requires_exact_record_selection_and_rejects_effect_arguments() {
    for route in [
        Route::Authorization,
        Route::Leases,
        Route::Consumption,
        Route::Attempts,
        Route::Reconciliations,
    ] {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("history schema");
        let mut missing = arguments(route);
        missing.as_object_mut().expect("object").remove("record_id");
        assert!(!schema.is_valid(&missing));
        assert!(decode(route, missing).is_err());
        for invalid in [Value::Null, json!(17), json!([]), json!({})] {
            let mut value = arguments(route);
            value["record_id"] = invalid;
            assert!(!schema.is_valid(&value));
            assert!(decode(route, value).is_err());
        }
        for field in [
            "allow_record",
            "allow_network",
            "advance_writer_generation",
            "allow_migrate",
            "allow_dispatch",
        ] {
            let mut value = arguments(route);
            value[field] = json!(true);
            assert!(!schema.is_valid(&value));
            assert!(decode(route, value).is_err());
        }
    }
}

#[test]
fn materialization_history_requires_an_explicit_absolute_coordinate_and_no_effect_arguments() {
    let route = Route::Materialization;
    let descriptor = json!(route.descriptor());
    assert_eq!(descriptor["name"], "inspect_materialization");
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("history schema");
    let mut missing = arguments(route);
    missing
        .as_object_mut()
        .expect("object")
        .remove("output_root");
    assert!(!schema.is_valid(&missing));
    assert!(decode(route, missing).is_err());
    for invalid in [
        Value::Null,
        json!(17),
        json!([]),
        json!({}),
        json!("relative/output"),
    ] {
        let mut value = arguments(route);
        value["output_root"] = invalid;
        assert!(!schema.is_valid(&value));
        assert!(decode(route, value).is_err());
    }
    for (field, value) in [
        ("record_id", json!("not a materialization selector")),
        ("root", json!("/alternate-coordinate")),
        ("allow_write", json!(true)),
        ("allow_record", json!(true)),
        ("allow_initialize", json!(true)),
        ("advance_writer_generation", json!(true)),
        ("allow_migrate", json!(true)),
    ] {
        let mut arguments = arguments(route);
        arguments[field] = value;
        assert!(!schema.is_valid(&arguments));
        assert!(decode(route, arguments).is_err());
    }
}

#[test]
fn materialization_inspection_checks_all_header_coordinates_without_observing_or_writing_output() {
    let journal = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private journal");
    let read = ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    };
    let initialized = store::initialize(
        journal.path(),
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read,
            },
            ..Default::default()
        },
    )
    .expect("native initialized history");
    let output = tempfile::tempdir().expect("unowned output");
    fs::write(
        output.path().join("user.txt"),
        b"user bytes remain independent",
    )
    .expect("user file");
    fs::create_dir(output.path().join(".git")).expect("unrelated Git administration");
    fs::write(output.path().join(".git/config"), b"unrelated git bytes").expect("Git file");
    let root = fs::canonicalize(output.path()).expect("exact historical coordinate");
    let database_before = fs::read(&initialized.database).expect("initial database bytes");
    let descriptor = json!(Route::Materialization.descriptor());
    let schema =
        jsonschema::validator_for(&descriptor["outputSchema"]).expect("history output schema");
    let request = json!({
        "path": initialized.database,
        "output_root": root,
        "expected_header": initialized.header,
        "allow_read": true,
        "read": read
    });
    // Neither an existing user directory nor a nonexistent historical coordinate
    // is a filesystem observation. Both merely select the stored frontier.
    for selected in [root.clone(), root.join("never-created")] {
        let mut value = request.clone();
        value["output_root"] = json!(selected);
        let result = decode(Route::Materialization, value)
            .expect("coordinate request")
            .execute(8192)
            .expect("native history report");
        assert_eq!(result.is_error, Some(false));
        let report = result.structured_content.expect("whole history");
        schema.validate(&report).expect("native output shape");
        assert_eq!(report["journal"]["header"], json!(initialized.header));
        assert_eq!(
            report["frontier"],
            json!({"root": selected, "head": null, "active": null})
        );
    }
    for field in [
        "schema_version",
        "journal_id",
        "journal_epoch",
        "writer_fence",
    ] {
        let mut value = request.clone();
        if field == "journal_id" {
            value["expected_header"][field][0] = json!(initialized.header.journal_id[0] ^ 1);
        } else {
            let original = value["expected_header"][field]
                .as_u64()
                .expect("integer header");
            value["expected_header"][field] = json!(original + 1);
        }
        let result = decode(Route::Materialization, value)
            .expect("mismatching header")
            .execute(8192)
            .expect("classified refusal");
        assert_eq!(result.is_error, Some(true));
        let report = result.structured_content.expect("failure only");
        assert_eq!(report["code"], "store_journal_mismatch", "{field}");
        assert!(report.get("frontier").is_none());
        schema.validate(&report).expect("native failure shape");
    }
    assert_eq!(
        fs::read(&initialized.database).expect("database readback"),
        database_before
    );
    assert_eq!(
        fs::read(output.path().join("user.txt")).expect("user readback"),
        b"user bytes remain independent"
    );
    assert_eq!(
        fs::read(output.path().join(".git/config")).expect("Git readback"),
        b"unrelated git bytes"
    );
    assert!(!root.join("never-created").exists());
    assert_eq!(
        fs::read_dir(output.path())
            .expect("unchanged output")
            .count(),
        2
    );
    assert_eq!(
        fs::read_dir(journal.path())
            .expect("closed native journal")
            .count(),
        1
    );
}
