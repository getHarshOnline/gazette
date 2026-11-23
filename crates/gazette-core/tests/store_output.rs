//! Native output bounds and schema closure are shared across store interfaces.

use gazette_core::store::{
    BackupInspection, BackupOptions, BackupReceipt, BackupVerifyOptions, InitializationReceipt,
    InitializeOptions, OutputLimits, RestoreOptions, RestoreReceipt,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[test]
fn directory_limits_accept_exact_bytes_and_refuse_unbounded_or_exhausted_selections() {
    assert_eq!(OutputLimits::default().directory_bytes, 16 * 1024);
    for bytes in [1, 16 * 1024, 1024 * 1024] {
        let limits = OutputLimits {
            directory_bytes: bytes,
        };
        limits.validate().expect("bounded selection");
        limits.check_directory_bytes(bytes).expect("exact bound");
        assert!(limits.check_directory_bytes(bytes + 1).is_err());
    }
    for bytes in [0, 1024 * 1024 + 1, usize::MAX] {
        let limits = OutputLimits {
            directory_bytes: bytes,
        };
        assert!(limits.validate().is_err());
        assert!(limits.check_directory_bytes(1).is_err());
    }
}

fn closed_options<T: Default + Serialize + DeserializeOwned + schemars::JsonSchema>() -> Value {
    let default: T = serde_json::from_value(json!({})).expect("native defaults");
    assert_eq!(
        serde_json::to_value(default).expect("decoded defaults"),
        serde_json::to_value(T::default()).expect("native defaults")
    );
    assert!(serde_json::from_value::<T>(json!({"extra": true})).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(T)).expect("native schema");
    assert_eq!(schema["additionalProperties"], false);
    schema
}

#[test]
fn store_option_schemas_close_nested_shapes_and_preserve_native_integer_ranges() {
    let output = closed_options::<OutputLimits>();
    assert_eq!(output["properties"]["directory_bytes"]["minimum"], 1);
    assert_eq!(
        output["properties"]["directory_bytes"]["maximum"],
        1_048_576
    );

    let initialize = closed_options::<InitializeOptions>();
    let backup = closed_options::<BackupOptions>();
    let restore = closed_options::<RestoreOptions>();
    let verify = closed_options::<BackupVerifyOptions>();
    for schema in [&initialize, &backup, &restore] {
        assert_eq!(
            schema["$defs"]["OutputLimits"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema["$defs"]["OutputLimits"]["properties"]["directory_bytes"]["maximum"],
            1_048_576
        );
    }
    for schema in [&initialize, &backup] {
        assert_eq!(schema["properties"]["database_bytes"]["minimum"], 1);
        assert_eq!(schema["properties"]["database_bytes"]["maximum"], u64::MAX);
    }
    assert_eq!(backup["properties"]["pages_per_step"]["minimum"], 1);
    assert_eq!(backup["properties"]["pages_per_step"]["maximum"], i32::MAX);
    assert_eq!(backup["properties"]["steps"]["maximum"], u32::MAX);
    assert_eq!(
        verify["properties"]["manifest_bytes"]["maximum"],
        u64::MAX - 1
    );

    assert!(serde_json::from_value::<InitializeOptions>(json!({"output":{"extra":1}})).is_err());
    assert!(serde_json::from_value::<BackupOptions>(json!({"output":{"extra":1}})).is_err());
    assert!(serde_json::from_value::<RestoreOptions>(json!({"output":{"extra":1}})).is_err());
    assert!(serde_json::from_value::<BackupVerifyOptions>(json!({"read":{"extra":1}})).is_err());
    assert!(!InitializeOptions::default().allow_create);
    assert!(!BackupOptions::default().allow_create);
    assert!(!RestoreOptions::default().allow_prepare);
    assert!(!BackupVerifyOptions::default().read.allow_coordination_files);
}

#[test]
fn complete_store_receipts_have_closed_native_schemas() {
    for schema in [
        schemars::schema_for!(InitializationReceipt),
        schemars::schema_for!(BackupReceipt),
        schemars::schema_for!(BackupInspection),
        schemars::schema_for!(RestoreReceipt),
    ] {
        let schema = serde_json::to_value(schema).expect("native receipt schema");
        assert_eq!(schema["additionalProperties"], false);
        for nested in schema["$defs"]
            .as_object()
            .expect("nested native contracts")
            .values()
        {
            if nested["type"] == "object" {
                assert_eq!(nested["additionalProperties"], false);
            }
        }
    }
}
