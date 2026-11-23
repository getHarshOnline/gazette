//! Behavioral proof through CLI → SDK → application → filesystem adapter.

use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::tempdir;

fn inspect(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-source")
        .arg(root)
        .args(arguments)
        .output()
        .expect("run Gazette")
}

fn assert_failure(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "failure emitted partial inventory"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{:?}",
        output
    );
}

#[test]
fn observes_bytes_empty_directories_and_hidden_files_without_writing() {
    let source = tempdir().expect("source");
    fs::create_dir(source.path().join("empty")).expect("directory");
    fs::write(source.path().join(".hidden"), b"abc").expect("file");
    let before = fs::metadata(source.path().join(".hidden"))
        .expect("metadata")
        .modified()
        .expect("mtime");
    let output = inspect(source.path(), &[]);
    assert!(output.status.success(), "{:?}", output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(
        report,
        json!({"entries": [
            {"path": ".hidden", "kind": "file", "bytes": 3, "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},
            {"path": "empty", "kind": "directory"}
        ], "total_bytes": 3, "identity": {
            "format": "gazette.inventory.v1",
            "sha256": "513140c1e6c293bc9ca780f7a31a6bcacabc1b7940e79c0662c1fb09fd78aca8"
        }})
    );
    assert_eq!(fs::read_dir(source.path()).expect("entries").count(), 2);
    assert_eq!(
        fs::read(source.path().join(".hidden")).expect("bytes"),
        b"abc"
    );
    assert_eq!(
        fs::metadata(source.path().join(".hidden"))
            .expect("metadata")
            .modified()
            .expect("mtime"),
        before
    );
}

#[test]
fn output_is_independent_of_location_creation_order_and_same_size_old_bytes() {
    let first = tempdir().expect("first");
    let second = tempdir().expect("second");
    for name in ["z.md", "a.md"] {
        fs::write(first.path().join(name), b"abc").expect("file");
    }
    for name in ["a.md", "z.md"] {
        fs::write(second.path().join(name), b"abc").expect("file");
    }
    let a = inspect(first.path(), &[]);
    let b = inspect(second.path(), &[]);
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout);
    fs::File::open(second.path().join("a.md"))
        .expect("fixture file")
        .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
        .expect("different fixture timestamp");
    let relocated = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .current_dir(second.path())
        .arg("inspect-source")
        .arg(second.path())
        .output()
        .expect("different working directory");
    assert!(relocated.status.success(), "{relocated:?}");
    assert_eq!(a.stdout, relocated.stdout);
    fs::write(second.path().join("a.md"), b"xyz").expect("changed bytes");
    let changed = inspect(second.path(), &[]);
    assert!(changed.status.success());
    assert_ne!(a.stdout, changed.stdout);
    let before: Value = serde_json::from_slice(&a.stdout).expect("before JSON");
    let after: Value = serde_json::from_slice(&changed.stdout).expect("after JSON");
    assert_ne!(before["identity"], after["identity"]);
}

#[test]
fn rejects_file_directory_and_dangling_symlinks_without_partial_output() {
    for target in ["outside.txt", "outside", "missing"] {
        let fixture = tempdir().expect("fixture");
        let root = fixture.path().join("source");
        fs::create_dir(&root).expect("root");
        fs::create_dir(fixture.path().join("outside")).expect("outside directory");
        fs::write(fixture.path().join("outside.txt"), b"private").expect("outside file");
        symlink(fixture.path().join(target), root.join("escape")).expect("link");
        assert_failure(inspect(&root, &[]), "source_unsupported");
    }
}

#[test]
fn rejects_special_files() {
    let source = tempdir().expect("source");
    let _socket = UnixListener::bind(source.path().join("socket")).expect("socket");
    assert_failure(inspect(source.path(), &[]), "source_unsupported");
}

#[test]
fn each_resource_ceiling_is_enforced_through_cli() {
    let source = tempdir().expect("source");
    fs::create_dir(source.path().join("nested")).expect("directory");
    fs::write(source.path().join("nested/first"), b"abc").expect("file");
    fs::write(source.path().join("second"), b"abc").expect("file");
    for arguments in [
        ["--max-entries", "2"],
        ["--max-depth", "1"],
        ["--max-file-bytes", "2"],
        ["--max-total-bytes", "5"],
    ] {
        assert_failure(inspect(source.path(), &arguments), "source_limit");
    }
    assert!(
        inspect(
            source.path(),
            &[
                "--max-entries",
                "3",
                "--max-depth",
                "2",
                "--max-file-bytes",
                "3",
                "--max-total-bytes",
                "6"
            ]
        )
        .status
        .success()
    );
}

#[test]
fn missing_or_symlinked_root_fails() {
    let fixture = tempdir().expect("fixture");
    assert_failure(inspect(&fixture.path().join("missing"), &[]), "source_io");
    fs::create_dir(fixture.path().join("real")).expect("root");
    symlink(fixture.path().join("real"), fixture.path().join("link")).expect("link");
    assert_failure(inspect(&fixture.path().join("link"), &[]), "source_io");
    assert_failure(inspect(&fixture.path().join("link/"), &[]), "source_io");
    assert_failure(inspect(&fixture.path().join("link/."), &[]), "source_io");
}

#[test]
fn empty_source_accepts_zero_ceilings() {
    let source = tempdir().expect("source");
    let output = inspect(
        source.path(),
        &[
            "--max-entries",
            "0",
            "--max-depth",
            "0",
            "--max-file-bytes",
            "0",
            "--max-total-bytes",
            "0",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(
        report,
        json!({"entries": [], "total_bytes": 0, "identity": {
            "format": "gazette.inventory.v1",
            "sha256": "7cf91ba08b23337630ced184a081ba7d19733bb06c4679726f60db9bdd6214e1"
        }})
    );
}

#[test]
fn direct_cli_reports_identity_and_rejects_unimplemented_commands() {
    let version = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("--version")
        .output()
        .expect("version");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).expect("UTF-8 version"),
        format!(
            "gazette {}\n",
            gazette_sdk::build::runtime_version_coordinate()
        )
    );
    let absent = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("deploy")
        .output()
        .expect("invalid command");
    assert_eq!(absent.status.code(), Some(2));
    assert!(absent.stdout.is_empty());
}

#[test]
fn sdk_and_cli_share_results_and_failure_classes() {
    use gazette_sdk::source::{self, Limits};

    let root = tempdir().expect("source");
    fs::create_dir(root.path().join("empty")).expect("empty directory");
    fs::write(root.path().join(".hidden"), b"abc").expect("file");
    let sdk = source::inspect(root.path(), None, Limits::default()).expect("SDK observation");
    let cli = inspect(root.path(), &[]);
    assert!(cli.status.success(), "{cli:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&cli.stdout).expect("CLI JSON"),
        serde_json::to_value(sdk).expect("SDK projection")
    );

    symlink("missing", root.path().join("link")).expect("unsupported link");
    let sdk_error = source::inspect(root.path(), None, Limits::default()).expect_err("SDK failure");
    assert_eq!(sdk_error.code(), "source_unsupported");
    assert_failure(inspect(root.path(), &[]), sdk_error.code());
}

#[test]
fn control_and_backslash_names_are_rejected() {
    for name in ["bad\n", "bad\\name"] {
        let source = tempdir().expect("source");
        fs::write(source.path().join(name), b"abc").expect("file");
        assert_failure(inspect(source.path(), &[]), "source_name");
    }
}

#[test]
fn selects_only_the_requested_subdirectory_with_relative_limits() {
    let source = tempdir().expect("source");
    let selected = source.path().join("docs/reference");
    fs::create_dir_all(&selected).expect("selected directory");
    fs::write(selected.join("readme.md"), b"abc").expect("documentation");
    fs::write(source.path().join("private.txt"), b"not selected").expect("sibling");
    // Unselected unsupported content must not be inspected either.
    symlink("missing", source.path().join("docs/unselected")).expect("sibling link");
    let output = inspect(
        source.path(),
        &[
            "--subdir",
            "docs/reference",
            "--max-entries",
            "1",
            "--max-depth",
            "1",
            "--max-total-bytes",
            "3",
        ],
    );
    let direct = inspect(&selected, &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(direct.status.success(), "{direct:?}");
    assert_eq!(output.stdout, direct.stdout);
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(report["entries"][0]["path"], "readme.md");
    assert_eq!(report["total_bytes"], 3);
    assert_eq!(
        fs::read(selected.join("readme.md")).expect("source bytes"),
        b"abc"
    );
    assert_eq!(
        fs::read(source.path().join("private.txt")).expect("sibling bytes"),
        b"not selected"
    );
}

#[test]
fn subdirectory_never_follows_intermediate_or_final_symlinks() {
    let fixture = tempdir().expect("fixture");
    let root = fixture.path().join("source");
    let outside = fixture.path().join("outside");
    fs::create_dir_all(root.join("docs")).expect("source directory");
    fs::create_dir_all(outside.join("reference")).expect("outside directory");
    fs::write(outside.join("reference/private"), b"not admitted").expect("private bytes");
    symlink(&outside, root.join("escape")).expect("intermediate link");
    symlink(outside.join("reference"), root.join("docs/escape")).expect("final link");
    symlink("missing", root.join("dangling")).expect("dangling link");
    for selection in ["escape/reference", "docs/escape", "dangling"] {
        assert_failure(inspect(&root, &["--subdir", selection]), "source_io");
    }
}

#[test]
fn invalid_subdirectory_is_rejected_before_source_access() {
    let fixture = tempdir().expect("fixture");
    let missing = fixture.path().join("does-not-exist");
    for selection in [
        "",
        "/docs",
        "docs/",
        "docs//reference",
        ".",
        "..",
        "docs/./reference",
        "docs/../private",
        "C:/docs",
        "C:docs",
        "docs\\private",
        "//server/share",
        "docs\nprivate",
        "docs\u{0080}",
        "%2e",
        "%2E%2e",
        ".%2e",
        "%2e.",
        "docs/%2e%2e/private",
        "docs%2freference",
        "docs%2Freference",
        "docs%5cprivate",
        "C%3Adocs",
        "docs%00private",
        "docs%0aprivate",
        "docs%7f",
        "docs%C2%80",
        "%252e%252e",
        "%25252f",
        "%25",
        "%ff",
    ] {
        let output = inspect(&missing, &["--subdir", selection]);
        assert_eq!(output.status.code(), Some(2), "{selection:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{selection:?}: partial output");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("invalid value"),
            "{selection:?}: {diagnostic}"
        );
        assert!(
            !diagnostic.contains("source_io"),
            "source was opened before validation"
        );
    }
}

#[test]
fn source_selection_preserves_literal_percent_unicode_and_spaces() {
    let source = tempdir().expect("source");
    for name in [
        "release%20notes",
        "release notes",
        "研究",
        "100%",
        "partial%2",
        "raw%zz",
    ] {
        fs::create_dir(source.path().join(name)).expect("literal directory");
        fs::write(source.path().join(name).join("name"), name).expect("distinct bytes");
    }
    for name in [
        "release%20notes",
        "release notes",
        "研究",
        "100%",
        "partial%2",
        "raw%zz",
    ] {
        let output = inspect(source.path(), &["--subdir", name]);
        let direct = inspect(&source.path().join(name), &[]);
        assert!(output.status.success(), "{name}: {output:?}");
        assert!(direct.status.success(), "{name}: {direct:?}");
        assert_eq!(
            output.stdout, direct.stdout,
            "selector changed literal name"
        );
    }
    assert_ne!(
        inspect(source.path(), &["--subdir", "release%20notes"]).stdout,
        inspect(source.path(), &["--subdir", "release notes"]).stdout,
        "percent encoding silently selected the space-named directory"
    );
}

#[test]
fn absent_or_non_directory_selection_fails_without_inventory() {
    let source = tempdir().expect("source");
    fs::write(source.path().join("file"), b"abc").expect("file");
    for selection in ["missing", "file", "file/reference"] {
        assert_failure(
            inspect(source.path(), &["--subdir", selection]),
            "source_io",
        );
    }
}

// macOS rejects these names at file creation. The production decoder is tested
// on all Unix hosts; Linux also exercises the native filesystem/CLI boundary.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filename_fails_through_cli() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let source = tempdir().expect("source");
    fs::write(source.path().join(OsStr::from_bytes(b"bad\xff")), b"abc").expect("file");
    assert_failure(inspect(source.path(), &[]), "source_name");
}
