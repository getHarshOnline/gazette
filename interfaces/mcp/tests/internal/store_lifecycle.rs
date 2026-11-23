//! Real native store lifecycle through the discoverable projection, never automatic recovery.

use super::*;
use serde_json::json;
use std::{
    ffi::OsStr,
    fs,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::Path,
};

const MAXIMUM: usize = 1 << 20;

fn decode(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named request").clone())
}

fn call(route: Route, value: Value, maximum: usize) -> Result<CallToolResult, ErrorData> {
    crate::tools::Route::named(route.name())
        .expect("wired discovery route")
        .decode(value.as_object().expect("named input").clone())?
        .execute(
            &crate::authoring::Sessions::default(),
            &crate::release_session::Sessions::default(),
            &crate::recovery_session::Sessions::default(),
            &crate::store_session::Sessions::default(),
            &crate::preview::Sessions::default(),
            maximum,
        )
}

fn report(route: Route, response: CallToolResult, failed: bool) -> Value {
    assert_eq!(response.is_error, Some(failed));
    let value = response.structured_content.expect("complete native report");
    let descriptor = json!(route.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native output schema")
        .validate(&value)
        .expect("actual native response matches discovery");
    value
}

fn private(parent: &Path, name: &OsStr) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir(&path).expect("private child");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("private mode");
    path
}

fn read_options() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn initialize_options() -> store::InitializeOptions {
    store::InitializeOptions {
        allow_create: true,
        session: store::SessionOptions {
            allow_local_reservation: true,
            read: read_options(),
        },
        ..Default::default()
    }
}

fn arguments(route: Route) -> Value {
    let pin = Sha256Digest::of(b"independently retained fixture pin");
    match route {
        Route::Initialize => json!({"directory":"/missing/store"}),
        Route::Backup => json!({
            "source":"/missing/store.sqlite","parent":"/missing/backups",
            "authored_roots":["/protected/authored"],"staged_roots":["/protected/staged"],
            "published_roots":["/protected/published"],"live_journal_roots":["/protected/journal"]
        }),
        Route::Verify => json!({"directory":"/missing/backup","manifest_sha256":pin}),
        Route::Restore => {
            json!({
                "backup":"/missing/backup","parent":"/missing/restores","manifest_sha256":pin,
                "authored_roots":["/protected/authored"],"staged_roots":["/protected/staged"],
                "published_roots":["/protected/published"],"live_journal_roots":["/protected/journal"]
            })
        }
    }
}

fn lifecycle_roots(mut value: Value, root: &Path) -> Value {
    for field in [
        "authored_roots",
        "staged_roots",
        "published_roots",
        "live_journal_roots",
    ] {
        value[field] = json!([root]);
    }
    value
}

#[test]
fn discovery_and_decoding_preserve_closed_named_native_options_and_exact_pins() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
        assert!(schema.is_valid(&arguments(route)));
        assert!(decode(route, arguments(route)).is_ok());
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(
            descriptor["annotations"]["idempotentHint"],
            matches!(route, Route::Verify)
        );
        assert_eq!(
            descriptor["annotations"]["destructiveHint"],
            !matches!(route, Route::Verify)
        );
        for options in [
            Value::Null,
            json!([]),
            json!({"unknown":true}),
            json!({"output":[]}),
            json!({"read":[]}),
            json!({"session":{"read":[]}}),
            json!({"verification":{"read":[]}}),
        ] {
            let mut value = arguments(route);
            value["options"] = options;
            assert!(!schema.is_valid(&value), "{} {value}", route.name());
            assert!(decode(route, value).is_err());
        }
        let path = match route {
            Route::Backup => "source",
            Route::Restore => "backup",
            _ => "directory",
        };
        let mut relative = arguments(route);
        relative[path] = json!("relative");
        assert!(!schema.is_valid(&relative));
        assert!(decode(route, relative).is_err());
        if matches!(route, Route::Backup | Route::Restore) {
            for field in [
                "authored_roots",
                "staged_roots",
                "published_roots",
                "live_journal_roots",
            ] {
                for invalid in [json!([]), json!(["relative"])] {
                    let mut value = arguments(route);
                    value[field] = invalid;
                    assert!(!schema.is_valid(&value));
                    assert!(decode(route, value).is_err());
                }
            }
        }
        if matches!(route, Route::Verify | Route::Restore) {
            for bad in [json!(vec![0; 32]), json!("short"), json!(13), Value::Null] {
                let mut value = arguments(route);
                value["manifest_sha256"] = bad;
                assert!(!schema.is_valid(&value));
                assert!(decode(route, value).is_err());
            }
        }
    }
}

#[test]
fn actual_initialize_backup_pinned_verification_and_detached_restore_share_native_receipts() {
    let root = tempfile::tempdir().expect("disposable fixture");
    let base = root.path().canonicalize().expect("exact native parent");
    let journal = private(&base, OsStr::new("journal"));
    let backups = private(&base, OsStr::new("backups"));
    let restores = private(&base, OsStr::new("restores"));
    let init = json!({"directory":journal,"options":initialize_options()});
    let initialized = report(
        Route::Initialize,
        call(Route::Initialize, init.clone(), MAXIMUM).expect("initialize tool"),
        false,
    );
    let database = PathBuf::from(initialized["database"].as_str().expect("native database"));
    let before = fs::read(&database).expect("original bytes");
    let original_header = initialized["header"].clone();
    assert_eq!(
        json!(
            store::inspect_journal(&database, None, read_options())
                .expect("independent header")
                .header
        ),
        original_header
    );
    report(
        Route::Initialize,
        call(Route::Initialize, init, MAXIMUM).expect("repeat refusal"),
        true,
    );
    assert_eq!(fs::read(&database).expect("preserved original"), before);
    let backup = report(
        Route::Backup,
        call(
            Route::Backup,
            lifecycle_roots(
                json!({
                    "source":database,"parent":backups,"allow_read":true,
                    "options":{"allow_create":true,"read":read_options()}
                }),
                &journal,
            ),
            MAXIMUM,
        )
        .expect("backup tool"),
        false,
    );
    let directory = PathBuf::from(backup["directory"].as_str().expect("created backup"));
    let pin: Sha256Digest = serde_json::from_value(backup["manifest_sha256"].clone())
        .expect("original independent pin");
    assert_eq!(
        Sha256Digest::of(&fs::read(directory.join("backup.json")).expect("actual manifest")),
        pin
    );
    let verify = json!({"directory":directory,"manifest_sha256":pin,"allow_read":true,"options":{"read":read_options()}});
    let verified = report(
        Route::Verify,
        call(Route::Verify, verify.clone(), MAXIMUM).expect("verify tool"),
        false,
    );
    assert_eq!(
        verified,
        json!(
            store::verify_backup(
                &directory,
                pin,
                store::BackupVerifyOptions {
                    read: read_options(),
                    ..Default::default()
                }
            )
            .expect("same native verification")
        )
    );
    assert_eq!(verified["snapshot"], backup["snapshot"]);
    let mut tampered = verify;
    tampered["manifest_sha256"] = json!(Sha256Digest::of(b"not the original manifest"));
    report(
        Route::Verify,
        call(Route::Verify, tampered, MAXIMUM).expect("wrong pin refusal"),
        true,
    );
    let restored = report(
        Route::Restore,
        call(
            Route::Restore,
            lifecycle_roots(
                json!({
                    "backup":directory,"parent":restores,"manifest_sha256":pin,"allow_read":true,
                    "options":{"allow_prepare":true,"verification":{"read":read_options()}}
                }),
                &journal,
            ),
            MAXIMUM,
        )
        .expect("detached restore tool"),
        false,
    );
    let output = PathBuf::from(restored["directory"].as_str().expect("detached output"));
    assert_ne!(output, journal);
    assert_eq!(
        restored["preparation"]["source_manifest_sha256"],
        json!(pin)
    );
    assert_eq!(
        restored["receipt_sha256"],
        json!(Sha256Digest::of(
            &fs::read(output.join("restore.json")).expect("retained original restore receipt")
        ))
    );
    assert_eq!(
        fs::read(output.join("store.sqlite")).expect("detached bytes"),
        fs::read(directory.join("store.sqlite")).expect("backup bytes")
    );
    assert_eq!(fs::read(&database).expect("live source unchanged"), before);
    assert_eq!(fs::read_dir(backups).expect("one backup").count(), 1);
    assert_eq!(
        fs::read_dir(restores).expect("one detached output").count(),
        1
    );
}

#[test]
fn permissions_and_response_reservation_refuse_before_native_output() {
    let root = tempfile::tempdir().expect("private fixture");
    let journal = private(
        &root.path().canonicalize().expect("native path"),
        OsStr::new("empty"),
    );
    let denied = report(
        Route::Initialize,
        call(Route::Initialize, json!({"directory":journal}), MAXIMUM)
            .expect("native denied permission"),
        true,
    );
    assert_eq!(denied["code"], "store_initialize_permission");
    assert!(
        call(
            Route::Initialize,
            json!({"directory":journal,"options":initialize_options()}),
            1
        )
        .is_err()
    );
    assert!(
        fs::read_dir(&journal)
            .expect("still empty")
            .next()
            .is_none()
    );
    for route in [Route::Backup, Route::Verify, Route::Restore] {
        let denied = report(
            route,
            call(route, arguments(route), MAXIMUM).expect("independent permission refusal"),
            true,
        );
        assert_eq!(denied["code"], "storage_permission");
        let mut value = arguments(route);
        value["allow_read"] = json!(true);
        report(
            route,
            call(route, value, MAXIMUM).expect("native denied permissions"),
            true,
        );
    }
    let mut bounded = initialize_options();
    bounded.output.directory_bytes = 1;
    let response = report(
        Route::Initialize,
        call(
            Route::Initialize,
            json!({"directory":journal,"options":bounded}),
            MAXIMUM,
        )
        .expect("actual path ceiling"),
        true,
    );
    assert!(response["directory_unix_bytes"].is_null());
    assert!(
        fs::read_dir(journal)
            .expect("no output on path bound")
            .next()
            .is_none()
    );
}

fn minimum_response(limits: store::OutputLimits) -> usize {
    let (mut low, mut high) = (1, MAXIMUM);
    assert!(output::Reservation::new(limits, high).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        if output::Reservation::new(limits, middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(output::Reservation::new(limits, low - 1).is_err());
    low
}

#[test]
fn real_escaped_native_output_overflows_with_original_pin_and_lossless_locator() {
    let root = tempfile::tempdir().expect("native oversized-receipt fixture");
    let base = root.path().canonicalize().expect("native parent");
    let journal = private(&base, OsStr::new("journal"));
    let initialized = store::initialize(&journal, initialize_options()).expect("actual source");
    // Valid UTF-8 control bytes expand sixfold in a JSON string, but remain exact
    // native path bytes. Nested components stay below filesystem name limits.
    let name = "\u{1}".repeat(200);
    let first = private(&base, OsStr::new(&name));
    let parent = private(&first, OsStr::new(&name));
    let limits = store::OutputLimits {
        directory_bytes: parent.join("gazette-backup-").as_os_str().as_bytes().len() + 6,
    };
    let maximum = minimum_response(limits);
    let response = call(
        Route::Backup,
        lifecycle_roots(
            json!({
                "source":initialized.database,"parent":parent,"allow_read":true,
                "options":{"allow_create":true,"read":read_options(),"output":limits}
            }),
            &journal,
        ),
        maximum,
    )
    .expect("real decoded backup output selection");
    let fallback = report(Route::Backup, response, true);
    assert_eq!(fallback["code"], "store_output_response_unavailable");
    assert_eq!(fallback["evidence"]["kind"], "backup");
    let bytes: Vec<u8> =
        serde_json::from_value(fallback["directory_unix_bytes"].clone()).expect("exact Unix bytes");
    let directory = PathBuf::from(OsStr::from_bytes(&bytes));
    assert_eq!(directory.parent(), Some(parent.as_path()));
    assert_eq!(bytes.len(), limits.directory_bytes);
    assert!(bytes.contains(&1));
    let pin: Sha256Digest = serde_json::from_value(fallback["evidence"]["manifest_sha256"].clone())
        .expect("original pin retained");
    assert_eq!(
        Sha256Digest::of(&fs::read(directory.join("backup.json")).expect("native manifest")),
        pin
    );
    let snapshot: store::BackupSnapshot = serde_json::from_slice(
        &fs::read(directory.join("backup.json")).expect("exact stored payload"),
    )
    .expect("native backup payload");
    let full = store::BackupReceipt {
        directory: directory.clone(),
        snapshot,
        manifest_sha256: pin,
    };
    assert!(
        serde_json::to_vec(&full)
            .expect("complete native receipt encoding")
            .len()
            > maximum
    );
    store::verify_backup(
        &directory,
        pin,
        store::BackupVerifyOptions {
            read: read_options(),
            ..Default::default()
        },
    )
    .expect("retained pin independently verifies output");
    assert_eq!(
        fs::read_dir(parent)
            .expect("single native invocation")
            .count(),
        1
    );
}

#[test]
fn late_native_initialization_failure_retains_residue_without_repair_or_retry() {
    let root = tempfile::tempdir().expect("bounded creation fixture");
    let journal = private(
        &root.path().canonicalize().expect("exact parent"),
        OsStr::new("journal"),
    );
    let mut options = initialize_options();
    options.database_bytes = 4096;
    let failed = report(
        Route::Initialize,
        call(
            Route::Initialize,
            json!({"directory":journal,"options":options}),
            MAXIMUM,
        )
        .expect("native bounded failure"),
        true,
    );
    let bytes: Vec<u8> = serde_json::from_value(failed["directory_unix_bytes"].clone())
        .expect("actual retained residue directory");
    assert_eq!(bytes, journal.as_os_str().as_bytes());
    assert!(journal.join("store.sqlite").exists());
    let before = fs::read(journal.join("store.sqlite")).expect("retained failed bytes");
    report(
        Route::Initialize,
        call(
            Route::Initialize,
            json!({"directory":journal,"options":initialize_options()}),
            MAXIMUM,
        )
        .expect("no reuse after failure"),
        true,
    );
    assert_eq!(
        fs::read(journal.join("store.sqlite")).expect("residue preserved"),
        before
    );
}
