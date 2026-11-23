//! Actual native creation results retain their exact identities through bounded delivery.

use gazette_sdk::{
    source::Sha256Digest,
    store::{
        self, OutputLimits,
        output::{Report, Reservation},
    },
};
use serde::Serialize;
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::PermissionsExt,
    },
};

fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private test state")
}

fn read() -> store::ReadOptions {
    store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    }
}

fn protected(root: &std::path::Path) -> store::LifecycleProtectedRoots {
    let roots = vec![root.to_path_buf()];
    store::LifecycleProtectedRoots {
        authored: roots.clone(),
        staged: roots.clone(),
        published: roots.clone(),
        live_journals: roots,
    }
}

fn initialize(output: OutputLimits) -> store::InitializeOptions {
    store::InitializeOptions {
        allow_create: true,
        session: store::SessionOptions {
            allow_local_reservation: true,
            read: read(),
        },
        output,
        ..Default::default()
    }
}

fn encode<T: Serialize + schemars::JsonSchema>(report: &Report<'_, T>, maximum: usize) -> Value {
    let bytes = serde_json::to_vec(report).expect("complete selected JSON");
    assert!(bytes.len() <= maximum);
    let value: Value = serde_json::from_slice(&bytes).expect("selected value");
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Report<'_, T>>();
    jsonschema::validator_for(&serde_json::to_value(schema).expect("schema"))
        .expect("native result schema")
        .validate(&value)
        .expect("result matches actual discovery shape");
    value
}

fn path(value: &Value) -> std::path::PathBuf {
    let bytes: Vec<u8> = serde_json::from_value(value["directory_unix_bytes"].clone())
        .expect("exact retained bytes");
    OsString::from_vec(bytes).into()
}

fn minimum_response(limits: OutputLimits) -> usize {
    let (mut low, mut high) = (1, 16 << 10);
    assert!(Reservation::new(limits, high).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        if Reservation::new(limits, middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

#[test]
fn real_initialize_backup_verify_and_detached_restore_preserve_native_receipts() {
    let root = private();
    let backups = private();
    let restores = private();
    let output = OutputLimits::default();
    let maximum = 256 << 10;
    let reservation = Reservation::new(output, maximum).expect("preflight");
    let initialized = store::initialize(root.path(), initialize(output));
    let report = reservation
        .initialize(&initialized)
        .expect("matched native bounds");
    assert!(!report.is_error());
    let init_json = encode(&report, maximum);
    let initialized = initialized.expect("native initialization");
    assert_eq!(
        init_json,
        serde_json::to_value(&initialized).expect("unchanged receipt")
    );

    let reservation = Reservation::new(output, maximum).expect("backup preflight");
    let backup = store::backup(store::BackupRequest {
        source: &initialized.database,
        output_parent: backups.path(),
        protected: protected(root.path()),
        options: store::BackupOptions {
            allow_create: true,
            read: read(),
            output,
            ..Default::default()
        },
    });
    let report = reservation.backup(&backup).expect("matched native bounds");
    assert!(!report.is_error());
    let backup_json = encode(&report, maximum);
    let backup = backup.expect("native backup");
    assert_eq!(
        backup_json,
        serde_json::to_value(&backup).expect("unchanged backup receipt")
    );
    let verification = store::BackupVerifyOptions {
        read: read(),
        ..Default::default()
    };
    let verified = store::verify_backup(&backup.directory, backup.manifest_sha256, verification)
        .expect("independent retained pin verification");
    assert_eq!(verified.snapshot.sha256, backup.snapshot.sha256);

    let reservation = Reservation::new(output, maximum).expect("restore preflight");
    let restored = store::prepare_restore(store::PrepareRestoreRequest {
        backup: &backup.directory,
        manifest_sha256: backup.manifest_sha256,
        output_parent: restores.path(),
        protected: protected(root.path()),
        options: store::RestoreOptions {
            allow_prepare: true,
            verification,
            output,
        },
    });
    let report = reservation
        .restore(&restored)
        .expect("matched native bounds");
    assert!(!report.is_error());
    let restore_json = encode(&report, maximum);
    let restored = restored.expect("detached copy");
    assert_eq!(
        restore_json,
        serde_json::to_value(&restored).expect("unchanged restore receipt")
    );
    assert_eq!(
        fs::read(restored.directory.join("store.sqlite")).expect("restored bytes"),
        fs::read(backup.directory.join("store.sqlite")).expect("backup bytes")
    );
    assert_eq!(
        store::inspect_journal(&initialized.database, None, read())
            .expect("original intact")
            .header,
        initialized.header
    );
}

#[test]
fn escaped_native_success_overflow_preserves_exact_locator_and_original_independent_pins() {
    let holder = private();
    let root = holder.path().join(format!("store-{}", "\u{1}".repeat(240)));
    let backups = holder
        .path()
        .join(format!("backup-{}", "\u{1}".repeat(240)));
    let restores = holder
        .path()
        .join(format!("restore-{}", "\u{1}".repeat(240)));
    for path in [&root, &backups, &restores] {
        fs::create_dir(path).expect("private child");
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("private permission");
    }
    let output = OutputLimits {
        directory_bytes: restores
            .canonicalize()
            .expect("native parent")
            .as_os_str()
            .as_bytes()
            .len()
            + 64,
    };
    let maximum = minimum_response(output);
    let reservation = Reservation::new(output, maximum).expect("preflight");
    let initialized = store::initialize(&root, initialize(output));
    let report = reservation
        .initialize(&initialized)
        .expect("matched native bounds");
    assert!(
        report.is_error(),
        "native success is not complete receipt delivery"
    );
    let retained = encode(&report, maximum);
    let initialized = initialized.expect("native operation still succeeded");
    assert_eq!(path(&retained), initialized.directory);
    assert_eq!(
        retained["evidence"]["header"],
        serde_json::to_value(initialized.header).expect("header")
    );

    let reservation = Reservation::new(output, maximum).expect("backup reservation");
    let backup = store::backup(store::BackupRequest {
        source: &initialized.database,
        output_parent: &backups,
        protected: protected(&root),
        options: store::BackupOptions {
            allow_create: true,
            read: read(),
            output,
            ..Default::default()
        },
    });
    let report = reservation.backup(&backup).expect("matched native bounds");
    assert!(report.is_error());
    let retained = encode(&report, maximum);
    let original = backup.expect("backup native success");
    assert_eq!(path(&retained), original.directory);
    let pin: Sha256Digest = serde_json::from_value(retained["evidence"]["manifest_sha256"].clone())
        .expect("original independent pin");
    assert_eq!(pin, original.manifest_sha256);
    let verification = store::BackupVerifyOptions {
        read: read(),
        ..Default::default()
    };
    store::verify_backup(&path(&retained), pin, verification)
        .expect("fallback itself enables verification");

    let reservation = Reservation::new(output, maximum).expect("restore reservation");
    let retained_path = path(&retained);
    let restored = store::prepare_restore(store::PrepareRestoreRequest {
        backup: &retained_path,
        manifest_sha256: pin,
        output_parent: &restores,
        protected: protected(&root),
        options: store::RestoreOptions {
            allow_prepare: true,
            verification,
            output,
        },
    });
    let report = reservation
        .restore(&restored)
        .expect("matched native bounds");
    assert!(report.is_error());
    let retained = encode(&report, maximum);
    let original = restored.expect("restoration succeeded natively");
    assert_eq!(path(&retained), original.directory);
    assert_eq!(
        retained["evidence"]["receipt_sha256"],
        serde_json::to_value(original.receipt_sha256).expect("original pin")
    );
}

#[test]
fn refusal_and_partial_initialization_do_not_gain_success_evidence() {
    let root = private();
    let output = OutputLimits::default();
    let maximum = 256 << 10;
    assert!(Reservation::new(output, 20).is_err());
    assert_eq!(fs::read_dir(root.path()).expect("untouched").count(), 0);
    for limit in [0, (1 << 20) + 1] {
        assert!(
            Reservation::new(
                OutputLimits {
                    directory_bytes: limit
                },
                maximum
            )
            .is_err()
        );
    }
    let reservation = Reservation::new(output, maximum).expect("preflight");
    let refused = store::initialize(root.path(), store::InitializeOptions::default());
    let report = reservation
        .initialize(&refused)
        .expect("matched native bounds");
    assert!(report.is_error());
    let value = encode(&report, maximum);
    assert_eq!(value["code"], "store_initialize_permission");
    assert!(value["directory_unix_bytes"].is_null());
    assert!(value.get("evidence").is_none());

    let reservation = Reservation::new(output, maximum).expect("partial preflight");
    let partial = store::initialize(
        root.path(),
        store::InitializeOptions {
            database_bytes: 1,
            ..initialize(output)
        },
    );
    let report = reservation
        .initialize(&partial)
        .expect("matched native bounds");
    assert!(report.is_error());
    let value = encode(&report, maximum);
    let error = partial.expect_err("page bound leaves unverified native output");
    assert!(error.directory.is_some());
    assert_eq!(path(&value), error.directory.expect("native residue"));
    assert!(value.get("evidence").is_none());
}

#[test]
fn compact_budget_is_exact_and_refuses_one_byte_below_reservation() {
    let holder = private();
    let root = holder.path().join(format!("exact-{}", "\u{1}".repeat(240)));
    fs::create_dir(&root).expect("selected private root");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private mode");
    let output = OutputLimits {
        directory_bytes: root
            .canonicalize()
            .expect("native root")
            .as_os_str()
            .as_bytes()
            .len(),
    };
    let required = minimum_response(output);
    assert!(Reservation::new(output, required - 1).is_err());
    let reservation = Reservation::new(output, required).expect("exact reservation");
    let initialized = store::initialize(&root, initialize(output));
    let report = reservation
        .initialize(&initialized)
        .expect("matched native bounds");
    let value = encode(&report, required);
    assert!(report.is_error());
    let native = initialized.expect("native result");
    assert_eq!(path(&value), native.directory);
    assert_eq!(
        value["evidence"]["header"],
        serde_json::to_value(native.header).expect("header")
    );
}

#[test]
fn mismatched_native_limits_and_caller_mutation_are_contract_errors_not_reserved_reports() {
    let root = private();
    let actual = store::initialize(root.path(), initialize(OutputLimits::default()));
    assert!(actual.is_ok());
    let reservation = Reservation::new(OutputLimits { directory_bytes: 1 }, 1024)
        .expect("valid but mismatched preflight");
    let error = reservation
        .initialize(&actual)
        .expect_err("native directory exceeds selected bound");
    assert!(std::ptr::eq(error.result, &actual));
    assert_eq!(error.delivery.code, "store_output_delivery_contract");
    assert_eq!(
        error.delivery.directory_unix_bytes,
        Some(
            actual
                .as_ref()
                .expect("native result retained")
                .directory
                .as_os_str()
                .as_bytes()
        )
    );
    assert!(matches!(
        error.delivery.evidence,
        store::output::Evidence::Initialized { .. }
    ));

    // Public receipt mutation is caller misuse, not native provenance or a filesystem operation.
    let mut changed = actual.expect("returned receipt");
    changed.directory = std::path::PathBuf::from(format!("/{}", "x".repeat(100)));
    let changed = Ok(changed);
    let reservation = Reservation::new(
        OutputLimits {
            directory_bytes: 64,
        },
        1024,
    )
    .expect("reservation");
    let error = reservation
        .initialize(&changed)
        .expect_err("changed public receipt cannot borrow reservation");
    assert!(std::ptr::eq(error.result, &changed));
    assert!(matches!(
        error.delivery.evidence,
        store::output::Evidence::Initialized { .. }
    ));

    // A supplied partial-error locator gets the same guard, never a guessed success pin.
    let failed: Result<store::InitializationReceipt, store::OutputError> =
        Err(store::OutputError {
            directory: Some(std::path::PathBuf::from(format!("/{}", "y".repeat(100)))),
            cause: store::Error::Changed,
        });
    let reservation = Reservation::new(
        OutputLimits {
            directory_bytes: 64,
        },
        1024,
    )
    .expect("reservation");
    let error = reservation
        .initialize(&failed)
        .expect_err("oversized caller-supplied residue");
    assert!(std::ptr::eq(error.result, &failed));
    assert!(matches!(
        error.delivery.evidence,
        store::output::Evidence::Failed
    ));
}

#[test]
fn supplied_non_utf8_result_fields_roundtrip_without_claiming_native_path_qualification() {
    // APFS rejects non-UTF-8 names. This is a pure transport-boundary case, not a native fixture.
    let bytes = b"/private/opaque-\xff\"\n";
    let failed: Result<store::InitializationReceipt, store::OutputError> =
        Err(store::OutputError {
            directory: Some(OsString::from_vec(bytes.to_vec()).into()),
            cause: store::Error::Changed,
        });
    let output = OutputLimits {
        directory_bytes: 128,
    };
    let maximum = minimum_response(output);
    let report = Reservation::new(output, maximum)
        .expect("preflight")
        .initialize(&failed)
        .expect("supplied result fits");
    let value = encode(&report, maximum);
    assert!(report.is_error());
    assert_eq!(path(&value).as_os_str().as_bytes(), bytes);
    assert!(value.get("evidence").is_none());

    let root = private();
    let mut receipt = store::initialize(root.path(), initialize(output)).expect("native origin");
    let header = receipt.header;
    // Deliberate caller-supplied field variation checks serialization only, not filesystem truth.
    receipt.directory = OsString::from_vec(bytes.to_vec()).into();
    let result = Ok(receipt);
    let report = Reservation::new(output, maximum)
        .expect("preflight")
        .initialize(&result)
        .expect("bounded supplied field");
    let value = encode(&report, maximum);
    assert!(report.is_error());
    assert_eq!(value["code"], "store_output_response_unavailable");
    assert_eq!(path(&value).as_os_str().as_bytes(), bytes);
    assert_eq!(
        value["evidence"]["header"],
        serde_json::to_value(header).expect("unchanged header")
    );
    assert_eq!(
        fs::read_dir(root.path())
            .expect("original native state untouched by projection")
            .count(),
        1
    );
}
