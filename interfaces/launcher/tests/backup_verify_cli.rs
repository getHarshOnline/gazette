//! Retained backup verification crosses the real creation, SDK and executable paths.

use gazette_sdk::{
    source::Sha256Digest,
    store::{self, BackupOptions, BackupReceipt, BackupVerifyOptions, ReadOptions},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::symlink,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

fn read_options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..ReadOptions::default()
    }
}

fn fixture(source_parent: &Path, backup_parent: &Path) -> BackupReceipt {
    let source = source_parent.join("source.db");
    let writer = Connection::open(&source).expect("disposable source");
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE fixture(value); INSERT INTO fixture VALUES('original'); PRAGMA application_id=23; PRAGMA user_version=17;").expect("committed source");
    let roots = vec![source_parent.to_path_buf()];
    let receipt = store::backup(store::BackupRequest {
        source: &source,
        output_parent: backup_parent,
        protected: store::LifecycleProtectedRoots {
            authored: roots.clone(),
            staged: roots.clone(),
            published: roots.clone(),
            live_journals: roots,
        },
        options: BackupOptions {
            allow_create: true,
            read: read_options(),
            ..BackupOptions::default()
        },
    })
    .expect("production backup");
    drop(writer);
    receipt
}

fn run(directory: &Path, pin: Sha256Digest, extra: &[&str]) -> Output {
    let cwd = tempdir().expect("independent working directory");
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("verify-backup")
        .arg(directory)
        .arg("--manifest-sha256")
        .arg(pin.to_string())
        .arg("--allow-coordination-files")
        .args(extra)
        .current_dir(cwd.path())
        .output()
        .expect("Gazette executable");
    assert!(fs::read_dir(cwd.path()).expect("cwd").next().is_none());
    output
}

fn unchanged(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .expect("backup inventory")
        .map(|entry| {
            let entry = entry.expect("entry");
            (
                entry.file_name().into_string().expect("name"),
                fs::read(entry.path()).expect("bytes"),
            )
        })
        .collect();
    files.sort();
    files
}

fn rejected(output: &Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "no successful subset: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{output:?}"
    );
}

fn replace_manifest(directory: &Path, value: &Value) -> Sha256Digest {
    let bytes = serde_json::to_vec(value).expect("candidate manifest");
    fs::write(directory.join("backup.json"), &bytes).expect("fixture-only mutation");
    Sha256Digest::of(&bytes)
}

#[test]
fn independent_pin_binds_real_sdk_cli_readback_without_changing_backup() {
    let source = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let receipt = fixture(source.path(), backups.path());
    let before = unchanged(&receipt.directory);
    let manifest = fs::read(receipt.directory.join("backup.json")).expect("manifest");
    assert_eq!(receipt.manifest_sha256, Sha256Digest::of(&manifest));
    let sdk = store::verify_backup(
        &receipt.directory,
        receipt.manifest_sha256,
        BackupVerifyOptions {
            read: read_options(),
            ..BackupVerifyOptions::default()
        },
    )
    .expect("native inspection");
    assert_eq!(sdk.snapshot, receipt.snapshot);
    assert_eq!(sdk.verifier.application_id, 23);
    assert_eq!(sdk.verifier.user_version, 17);
    assert_eq!(sdk.verifier.journal_mode, "delete");
    assert!(sdk.verifier.sql_read_only);
    for _ in 0..2 {
        let output = run(&receipt.directory, receipt.manifest_sha256, &[]);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let cli: Value = serde_json::from_slice(&output.stdout).expect("report");
        assert_eq!(cli, serde_json::to_value(&sdk).expect("SDK projection"));
        assert_eq!(before, unchanged(&receipt.directory));
    }
    let help = run(&receipt.directory, receipt.manifest_sha256, &["--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("no restore, adoption or release"));
}

#[test]
fn manifest_pin_and_database_digest_cannot_silently_follow_new_bytes() {
    let source = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let receipt = fixture(source.path(), backups.path());
    rejected(
        &run(
            &receipt.directory,
            Sha256Digest::of(b"different manifest"),
            &[],
        ),
        "store_backup_verification",
    );
    let manifest = fs::read(receipt.directory.join("backup.json")).expect("manifest");
    let mut reformatted = manifest.clone();
    reformatted.push(b'\n');
    fs::write(receipt.directory.join("backup.json"), &reformatted).expect("fixture changed bytes");
    rejected(
        &run(&receipt.directory, receipt.manifest_sha256, &[]),
        "store_backup_verification",
    );
    fs::write(receipt.directory.join("backup.json"), manifest)
        .expect("fixture restore original manifest");
    let writer = Connection::open(receipt.directory.join("store.sqlite")).expect("fixture writer");
    writer
        .execute_batch("UPDATE fixture SET value='changed';")
        .expect("fixture mutation");
    drop(writer);
    let before = unchanged(&receipt.directory);
    rejected(
        &run(&receipt.directory, receipt.manifest_sha256, &[]),
        "store_backup_verification",
    );
    assert_eq!(before, unchanged(&receipt.directory));
}

#[test]
fn malformed_unknown_duplicate_and_path_selecting_manifests_fail_typed_decode() {
    let source = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let receipt = fixture(source.path(), backups.path());
    let original = serde_json::to_value(&receipt.snapshot).expect("original");
    for pointer in ["/source/application_id", "/source/user_version"] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).expect("native header marker") = json!(999);
        let pin = replace_manifest(&receipt.directory, &changed);
        rejected(
            &run(&receipt.directory, pin, &[]),
            "store_backup_verification",
        );
    }
    for (pointer, value) in [
        ("/format", json!("gazette.sqlite-backup.v2")),
        ("/database", json!("../source.db")),
        ("/source/provider", json!("another-provider")),
        ("/source/sql_read_only", json!(false)),
        ("/sha256", json!("A".repeat(64))),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).expect("fixture field") = value;
        let pin = replace_manifest(&receipt.directory, &changed);
        rejected(&run(&receipt.directory, pin, &[]), "store_backup_manifest");
    }
    for nested in [false, true] {
        let mut changed = original.clone();
        if nested {
            changed["source"]["extra"] = json!(true);
        } else {
            changed["extra"] = json!(true);
        }
        let pin = replace_manifest(&receipt.directory, &changed);
        rejected(&run(&receipt.directory, pin, &[]), "store_backup_manifest");
    }
    let encoded = serde_json::to_string(&original).expect("encoded");
    for bytes in [
        b"{".to_vec(),
        format!("{{\"bytes\":1,{}", &encoded[1..]).into_bytes(),
    ] {
        fs::write(receipt.directory.join("backup.json"), &bytes).expect("invalid fixture");
        rejected(
            &run(&receipt.directory, Sha256Digest::of(&bytes), &[]),
            "store_backup_manifest",
        );
    }
}

#[test]
fn matching_declared_hash_does_not_bypass_native_integrity_or_standalone_checks() {
    for header_corruption in [false, true] {
        let source = tempdir().expect("source");
        let backups = tempdir().expect("backups");
        let receipt = fixture(source.path(), backups.path());
        let database = receipt.directory.join("store.sqlite");
        let mut bytes = fs::read(&database).expect("database");
        if header_corruption {
            bytes[18] = 2;
            bytes[19] = 2;
        } else {
            bytes[100] = 0x7f;
        } // Invalid schema B-tree page type, not a header/hash mismatch.
        fs::write(&database, &bytes).expect("fixture corruption");
        let mut manifest = serde_json::to_value(&receipt.snapshot).expect("manifest");
        manifest["sha256"] = json!(Sha256Digest::of(&bytes));
        let pin = replace_manifest(&receipt.directory, &manifest);
        let before = unchanged(&receipt.directory);
        let output = run(&receipt.directory, pin, &[]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            before,
            unchanged(&receipt.directory),
            "verification cannot recover or create sidecars"
        );
    }
}

#[test]
fn missing_permissions_bounds_and_extra_inventory_never_repair_input() {
    let source = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let receipt = fixture(source.path(), backups.path());
    let before = unchanged(&receipt.directory);
    let error = store::verify_backup(
        &receipt.directory,
        receipt.manifest_sha256,
        BackupVerifyOptions::default(),
    )
    .expect_err("missing effects");
    assert_eq!(error.code(), "store_coordination_permission");
    for (flags, code) in [
        (
            vec!["--max-manifest-bytes", "1"],
            "store_backup_manifest_limit",
        ),
        (vec!["--max-manifest-bytes", "0"], "store_invalid_limits"),
        (vec!["--max-storage-bytes", "1"], "store_backup_limit"),
        (vec!["--max-vm-steps", "1"], "store_interrupted"),
    ] {
        rejected(
            &run(&receipt.directory, receipt.manifest_sha256, &flags),
            code,
        );
        assert_eq!(before, unchanged(&receipt.directory));
    }
    fs::write(
        receipt.directory.join("store.sqlite-wal"),
        b"unadmitted sidecar",
    )
    .expect("fixture sidecar");
    let with_sidecar = unchanged(&receipt.directory);
    rejected(
        &run(&receipt.directory, receipt.manifest_sha256, &[]),
        "store_changed",
    );
    assert_eq!(with_sidecar, unchanged(&receipt.directory));
    fs::remove_file(receipt.directory.join("store.sqlite-wal"))
        .expect("remove own fixture sidecar");
    fs::remove_file(receipt.directory.join("backup.json")).expect("incomplete fixture");
    rejected(
        &run(&receipt.directory, receipt.manifest_sha256, &[]),
        "store_changed",
    );
    assert!(!receipt.directory.join("backup.json").exists());
    let absent = backups.path().join("absent");
    rejected(&run(&absent, receipt.manifest_sha256, &[]), "store_io");
    assert!(!absent.exists());
}

#[test]
fn selected_directory_and_file_symlinks_are_rejected_and_cli_requires_exact_pin() {
    let source = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let receipt = fixture(source.path(), backups.path());
    let link = backups.path().join("link");
    symlink(&receipt.directory, &link).expect("directory fixture link");
    rejected(
        &run(&link, receipt.manifest_sha256, &[]),
        "store_unsafe_path",
    );
    rejected(
        &run(&link.join(""), receipt.manifest_sha256, &[]),
        "store_unsafe_path",
    );
    let original = fs::read(receipt.directory.join("backup.json")).expect("manifest");
    let external = backups.path().join("external.json");
    fs::write(&external, &original).expect("outside fixture");
    fs::remove_file(receipt.directory.join("backup.json")).expect("replace fixture manifest");
    symlink(&external, receipt.directory.join("backup.json")).expect("file fixture link");
    rejected(
        &run(&receipt.directory, receipt.manifest_sha256, &[]),
        "store_changed",
    );
    assert_eq!(fs::read(&external).expect("outside unchanged"), original);
    for flags in [vec![], vec!["--manifest-sha256", "abcd"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("verify-backup")
            .arg(&receipt.directory)
            .args(flags)
            .output()
            .expect("CLI usage");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
}
