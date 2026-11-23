//! Real configuration-directory capture and executable/SDK parity without mutations.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use gazette_sdk::{configuration, source};
use serde_json::Value;
use tempfile::tempdir;

fn run(root: &Path, args: &[&str]) -> Output {
    let cwd = tempdir().expect("independent current directory");
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-toml")
        .arg(root)
        .args(args)
        .current_dir(cwd.path())
        .output()
        .expect("Gazette");
    assert!(fs::read_dir(cwd.path()).expect("cwd").next().is_none());
    output
}

#[test]
fn executable_captures_every_selected_file_and_matches_the_sdk_without_writes() {
    let root = tempdir().expect("source");
    fs::create_dir_all(root.path().join("config/nested")).expect("directories");
    fs::write(root.path().join("outside.txt"), "not TOML!").expect("unselected source");
    fs::write(root.path().join("config/z.toml"), "title='Z'\n").expect("source");
    fs::write(
        root.path().join("config/nested/.hidden"),
        "future={value=1}\n",
    )
    .expect("hidden source");
    fs::write(root.path().join("config/empty"), "").expect("empty source");
    let before = source::inspect(root.path(), None, source::Limits::default()).expect("before");
    let output = run(root.path(), &["--subdir", "config"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let sdk = configuration::inspect_toml(
        root.path(),
        Some(&"config".parse().expect("selection")),
        configuration::Limits::default(),
    )
    .expect("SDK");
    assert_eq!(report, serde_json::to_value(&sdk).expect("SDK JSON"));
    let paths: Vec<_> = sdk.documents.iter().map(|doc| doc.path.as_str()).collect();
    assert_eq!(paths, ["empty", "nested/.hidden", "z.toml"]);
    assert_eq!(sdk.documents[1].syntax.keys, ["future"]);
    assert_eq!(
        run(root.path(), &["--subdir", "config"]).stdout,
        output.stdout
    );
    assert_eq!(
        source::inspect(root.path(), None, source::Limits::default()).expect("after"),
        before
    );

    fs::remove_file(root.path().join("config/z.toml")).expect("fixture removal");
    let reduced = run(root.path(), &["--subdir", "config"]);
    assert!(reduced.status.success());
    let reduced: Value = serde_json::from_slice(&reduced.stdout).expect("reduced JSON");
    assert_eq!(
        reduced["documents"].as_array().expect("complete set").len(),
        2
    );
}

#[test]
fn malformed_hidden_or_late_documents_fail_without_partial_reports_or_source_changes() {
    let root = tempdir().expect("source");
    fs::write(root.path().join("a.toml"), "valid=true\n").expect("valid source");
    for (name, bytes, code) in [
        ("z.toml", b"broken=[".as_slice(), "toml_invalid"),
        (".hidden", b"not TOML!".as_slice(), "toml_invalid"),
        ("z.toml", b"x='\xff'".as_slice(), "toml_set_utf8"),
    ] {
        let path = root.path().join(name);
        fs::write(&path, bytes).expect("bad selected source");
        let before = source::inspect(root.path(), None, source::Limits::default()).expect("before");
        let output = run(root.path(), &[]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains(code) && diagnostic.contains(name),
            "{output:?}"
        );
        assert_eq!(
            source::inspect(root.path(), None, source::Limits::default()).expect("after"),
            before
        );
        fs::remove_file(path).expect("remove only disposable bad fixture");
    }
}

#[test]
fn executable_reports_all_independent_failures_from_the_shared_compiler() {
    let root = tempdir().expect("source");
    for (name, bytes) in [
        ("z.toml", b"bad=[".as_slice()),
        ("valid.toml", b"valid=true".as_slice()),
        ("b.toml", b"x='\xff'".as_slice()),
        ("a.toml", b"bad={".as_slice()),
    ] {
        fs::write(root.path().join(name), bytes).expect("source");
    }
    let before = source::inspect(root.path(), None, source::Limits::default()).expect("before");
    let output = run(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "no successful subset");
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic");
    let positions: Vec<_> = ["a.toml", "b.toml", "z.toml"]
        .into_iter()
        .map(|name| {
            diagnostic
                .find(name)
                .unwrap_or_else(|| panic!("missing {name}: {diagnostic}"))
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    let sdk = configuration::inspect_toml(root.path(), None, configuration::Limits::default())
        .expect_err("same shared failure");
    assert_eq!(
        diagnostic,
        format!(
            "gazette: {}: {sdk}; no configuration report produced\n",
            sdk.code()
        )
    );
    assert_eq!(
        source::inspect(root.path(), None, source::Limits::default()).expect("after"),
        before
    );
}

#[test]
fn native_and_parser_bounds_are_effective_at_the_executable_boundary() {
    let root = tempdir().expect("source");
    fs::create_dir(root.path().join("nested")).expect("nested");
    fs::write(root.path().join("a.toml"), "x=1").expect("source");
    fs::write(root.path().join("nested/b.toml"), "x=2").expect("source");
    for (flag, value, code) in [
        ("--max-entries", "1", "source_limit"),
        ("--max-path-depth", "1", "source_limit"),
        ("--max-document-bytes", "2", "source_limit"),
        ("--max-total-bytes", "5", "source_limit"),
        ("--max-documents", "1", "toml_set_limit"),
        ("--max-path-bytes", "1", "toml_set_limit"),
        ("--max-document-nodes", "1", "toml_limit"),
        ("--max-total-nodes", "3", "toml_set_limit"),
        ("--max-depth", "0", "toml_limit"),
    ] {
        let output = run(root.path(), &[flag, value]);
        assert_eq!(output.status.code(), Some(1), "{flag}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{flag}: {output:?}"
        );
    }
    assert_eq!(
        run(root.path(), &["--subdir", "../escape"]).status.code(),
        Some(2)
    );
    assert_eq!(run(root.path(), &["--publish"]).status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn selected_links_and_special_files_cannot_become_configuration_sources() {
    use std::os::unix::{fs::symlink, net::UnixListener};
    let root = tempdir().expect("source");
    let outside = tempdir().expect("outside");
    fs::write(outside.path().join("secret.toml"), "value='outside'").expect("outside source");
    symlink(
        outside.path().join("secret.toml"),
        root.path().join("link.toml"),
    )
    .expect("link");
    let output = run(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source_unsupported"));
    fs::remove_file(root.path().join("link.toml")).expect("disposable link only");
    let socket = root.path().join("socket");
    let _listener = UnixListener::bind(&socket).expect("special fixture");
    assert_eq!(run(root.path(), &[]).status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(outside.path().join("secret.toml")).expect("outside preserved"),
        "value='outside'"
    );
}

#[test]
fn empty_syntax_set_is_explicit_and_help_discloses_non_effects() {
    let root = tempdir().expect("empty selected root");
    let output = run(root.path(), &[]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("empty evidence");
    assert_eq!(report["documents"], serde_json::json!([]));
    assert_eq!(report["node_count"], 0);
    let help = run(root.path(), &["--help"]);
    let help = String::from_utf8(help.stdout).expect("help");
    assert!(help.contains("no writes, imports, reconciliation or release"));
    assert!(help.contains("including hidden files"));
}
