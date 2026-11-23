//! Executable release-history observations retain native SDK semantics and permission gates.

use gazette_sdk::store::{self, ReadOptions};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

const COMMANDS: [&str; 5] = [
    "inspect-release-authorization",
    "inspect-release-leases",
    "inspect-release-consumption",
    "inspect-release-attempts",
    "inspect-release-reconciliations",
];

fn invoke(command: &str, path: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg(command)
        .arg(path)
        .arg("selected-record")
        .args(extra)
        .output()
        .expect("native CLI process")
}

#[test]
fn history_permissions_precede_database_access_and_never_create_missing_state() {
    let root = tempfile::tempdir().expect("disposable absent store");
    let path = root.path().join("missing.db");
    for command in COMMANDS {
        for (flags, code) in [
            (vec![], "storage_permission"),
            (vec!["--allow-coordination-files"], "storage_permission"),
            (vec!["--allow-read"], "store_coordination_permission"),
            (
                vec!["--allow-read", "--allow-coordination-files"],
                "store_io",
            ),
        ] {
            let result = invoke(command, &path, &flags);
            assert!(!result.status.success(), "{command}");
            assert!(result.stdout.is_empty(), "no partial history");
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(code),
                "{command}: {:?}",
                result.stderr
            );
        }
    }
    assert_eq!(
        fs::read_dir(root.path()).expect("unchanged root").count(),
        0
    );
}

#[test]
fn fresh_history_matches_sdk_and_retained_header_mismatch_is_not_observation() {
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
    .expect("native fresh initialization");
    let before = fs::read(&initialized.database).expect("initial database bytes");
    let header = serde_json::to_string(&initialized.header).expect("exact header argument");
    let mut wrong = initialized.header;
    wrong.writer_fence += 1;
    let wrong_header = serde_json::to_string(&wrong).expect("independent wrong header");
    for command in COMMANDS {
        let expected = match command {
            "inspect-release-authorization" => json!(
                store::inspect_release_authorization(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK authorization")
            ),
            "inspect-release-leases" => json!(
                store::inspect_release_leases(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK leases")
            ),
            "inspect-release-consumption" => json!(
                store::inspect_release_consumption(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK consumption")
            ),
            "inspect-release-attempts" => json!(
                store::inspect_release_attempts(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK attempts")
            ),
            "inspect-release-reconciliations" => json!(
                store::inspect_release_reconciliations(
                    &initialized.database,
                    "selected-record",
                    Some(&initialized.header),
                    read
                )
                .expect("SDK reconciliation")
            ),
            _ => panic!("enumerated history command"),
        };
        for expected_header in [None, Some(header.as_str())] {
            let mut flags = vec!["--allow-read", "--allow-coordination-files"];
            if let Some(header) = expected_header {
                flags.extend(["--expected-header", header]);
            }
            let result = invoke(command, &initialized.database, &flags);
            assert!(
                result.status.success(),
                "{command}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&result.stdout).expect("complete native report"),
                expected
            );
        }
        let mismatch = invoke(
            command,
            &initialized.database,
            &[
                "--allow-read",
                "--allow-coordination-files",
                "--expected-header",
                &wrong_header,
            ],
        );
        assert!(!mismatch.status.success());
        assert!(mismatch.stdout.is_empty());
        let native_error = store::inspect_release_authorization(
            &initialized.database,
            "selected-record",
            Some(&wrong),
            read,
        )
        .expect_err("native expected mismatch");
        assert!(String::from_utf8_lossy(&mismatch.stderr).contains(native_error.code()));
    }
    assert_eq!(
        fs::read(&initialized.database).expect("unchanged database"),
        before
    );
}
