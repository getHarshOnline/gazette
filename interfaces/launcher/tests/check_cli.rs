//! Direct workspace checking uses the same compiler as the SDK, without native effects.

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{toml, workspace};
use tempfile::tempdir;

const SOURCE: &str = "[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

#[test]
fn explains_an_inherited_publication_field_without_writing() {
    let source = format!(
        "[defaults.publication]\nlanguage='en'\n{SOURCE}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\n"
    );
    let output = run(
        source.as_bytes(),
        &["--why", "language", "--site", "site:one"],
    );
    assert!(output.status.success(), "{output:?}");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["scope"]["kind"], "default");
    assert_eq!(result["value"], "en");
    assert_eq!(
        result["path"],
        serde_json::json!(["defaults", "publication", "language"])
    );
}

#[test]
fn explains_a_local_included_default_and_checks_stdin_bounds_before_decoding() {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("gazette.toml"),
        format!("includes=['defaults.toml']\n{SOURCE}name='Example'\n"),
    )
    .unwrap();
    fs::write(
        root.path().join("defaults.toml"),
        "[defaults.publication]\nlanguage='en'\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "check",
            "--why",
            "language",
            "--site",
            "site:one",
            "--workspace-root",
            root.path().to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["document"], "defaults.toml");
    assert_eq!(result["value"], "en");
    assert_eq!(result["workspace_location"]["found_by"], "request");
    assert_eq!(
        result["workspace_location"]["root"],
        serde_json::json!(root.path().canonicalize().unwrap())
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);

    let output = run(
        &[255; 5],
        &[
            "--why",
            "language",
            "--site",
            "site:one",
            "--max-source-bytes",
            "4",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("toml_limit"));
}

fn run(source: &[u8], args: &[&str]) -> Output {
    let cwd = tempdir().expect("isolated cwd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check")
        .arg("--stdin")
        .args(args)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette executable");
    let written = child.stdin.take().expect("stdin").write_all(source);
    let output = child.wait_with_output().expect("result");
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
        assert!(
            !output.status.success(),
            "successful command must consume test input"
        );
    }
    assert!(
        fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "check writes nothing"
    );
    output
}

#[test]
fn executable_matches_sdk_and_is_deterministic_without_pale_or_workspace_discovery() {
    let output = run(SOURCE.as_bytes(), &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let sdk = workspace::compile(SOURCE, toml::Limits::default()).expect("SDK");
    let mut expected = serde_json::to_vec_pretty(&sdk).expect("JSON");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    assert_eq!(output.stdout, run(SOURCE.as_bytes(), &[]).stdout);
}

#[test]
fn explicit_modular_workspace_load_matches_sdk_without_sibling_or_cwd_discovery() {
    let fixture = tempdir().expect("workspace root");
    fs::create_dir(fixture.path().join("sites")).expect("site directory");
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
    let site = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    fs::write(fixture.path().join("gazette.toml"), root).expect("root document");
    fs::write(fixture.path().join("sites/one.toml"), site).expect("included document");
    fs::write(fixture.path().join("unselected.toml"), b"\xff").expect("invalid unselected sibling");
    let cwd = tempdir().expect("unrelated cwd");
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "check",
            "workspace",
            fixture.path().to_str().expect("UTF-8 fixture path"),
            "--workspace",
            "gazette.toml",
            "--allow-read",
        ])
        .current_dir(cwd.path())
        .output()
        .expect("Gazette executable");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let workspace = "gazette.toml".parse().expect("entry path");
    let sdk = gazette_sdk::configuration::load_workspace(
        fixture.path(),
        &workspace,
        gazette_sdk::configuration::WorkspaceLoadLimits::default(),
    )
    .expect("same SDK operation");
    let mut expected = serde_json::to_vec_pretty(&sdk).expect("SDK JSON");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    assert!(fs::read_dir(cwd.path()).expect("cwd").next().is_none());

    let denied = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "check",
            "workspace",
            fixture.path().join("missing").to_str().expect("UTF-8 path"),
            "--workspace",
            "gazette.toml",
        ])
        .output()
        .expect("Gazette executable");
    assert_eq!(denied.status.code(), Some(1));
    assert!(denied.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("workspace_load_permission"),
        "{denied:?}"
    );
}

#[test]
fn bare_check_selects_only_the_standard_manifest_in_the_current_workspace() {
    let parent = tempdir().expect("workspace parent");
    fs::write(parent.path().join("gazette.toml"), SOURCE).expect("parent decoy");
    let workspace_root = parent
        .path()
        .join("an-arbitrarily-named-publication-workspace");
    fs::create_dir(&workspace_root).expect("workspace directory");
    fs::create_dir(workspace_root.join("sites")).expect("included document directory");
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
    let site = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    fs::write(workspace_root.join("gazette.toml"), root).expect("standard manifest");
    fs::write(workspace_root.join("sites/one.toml"), site).expect("included document");
    fs::write(workspace_root.join("workspace.toml"), b"not = [valid").expect("namesake decoy");

    let explicit = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "check",
            "workspace",
            workspace_root.to_str().expect("UTF-8 fixture path"),
            "--workspace",
            "gazette.toml",
            "--allow-read",
        ])
        .current_dir(parent.path())
        .output()
        .expect("explicit Gazette check");
    assert!(explicit.status.success(), "{explicit:?}");
    assert!(explicit.stderr.is_empty());

    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check")
        .current_dir(&workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("direct Gazette check");
    let input = child.stdin.take().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let completed = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if std::time::Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    drop(input);
    let direct = child.wait_with_output().unwrap();
    assert!(completed, "a local check must not wait for input to close");
    assert!(direct.status.success(), "{direct:?}");
    assert!(direct.stderr.is_empty());

    let direct_report: serde_json::Value = serde_json::from_slice(&direct.stdout).unwrap();
    assert_eq!(
        direct_report["workspace_location"]["found_by"],
        "working_directory"
    );
    assert_eq!(direct_report["checked_sites"], serde_json::json!([]));
    assert_eq!(direct_report["diagnostics"], serde_json::json!([]));

    let child_without_manifest = workspace_root.join("nested-without-a-manifest");
    fs::create_dir(&child_without_manifest).expect("nested directory");
    let missing = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check")
        .current_dir(&child_without_manifest)
        .output()
        .expect("bounded Gazette check");
    assert!(missing.status.success(), "{missing:?}");
    assert_eq!(missing.stdout, direct.stdout);

    let limited = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--max-source-bytes", "2"])
        .current_dir(&child_without_manifest)
        .output()
        .unwrap();
    assert!(!limited.status.success());
    let report: serde_json::Value = serde_json::from_slice(&limited.stdout).unwrap();
    assert!(!report["diagnostics"].as_array().unwrap().is_empty());
    assert!(String::from_utf8_lossy(&limited.stderr).contains("limit"));
}

#[test]
fn brand_repository_settings_resolve_without_creating_or_inferring_a_destination() {
    let root = tempdir().expect("target parent");
    let target = root.path().join("custom-folder");
    let source = format!(
        "{SOURCE}[[brands]]\nid='brand:one'\nname='Publication'\n[[brands.publications]]\nsite='site:one'\ndocumentation_repository={{name='different-repository-name',path={:?}}}\n",
        target.to_str().expect("path")
    );
    let output = run(source.as_bytes(), &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        !target.exists(),
        "pure resolution cannot initialize a documentation repository"
    );
    let sdk = workspace::compile(&source, toml::Limits::default()).expect("shared resolver");
    let mut expected = serde_json::to_vec_pretty(&sdk).expect("JSON");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    let bad = source.replace("site='site:one'", "site='site:missing'");
    let failed = run(bad.as_bytes(), &[]);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(!target.exists());
}

#[test]
fn executable_errors_have_no_partial_workspace_and_never_initialize_state() {
    for (input, args, code) in [
        (b"sites=[]".as_slice(), vec![], "workspace_invalid"),
        (b"sites=[".as_slice(), vec![], "toml_invalid"),
        (b"\xff".as_slice(), vec![], "workspace_utf8"),
        (
            SOURCE.as_bytes(),
            vec!["--max-source-bytes", "2"],
            "toml_limit",
        ),
        (SOURCE.as_bytes(), vec!["--max-nodes", "2"], "toml_limit"),
    ] {
        let output = run(input, &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
    }
    let missing = SOURCE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let output = run(missing.as_bytes(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("workspace_reference"));
}

#[test]
fn origin_queries_use_the_sdk_graph_and_exact_utf8_source_without_native_effects() {
    let source = format!("# 🦀 name='decoy'\r\n{SOURCE}[[brands]]\nid='brand:one'\nname='Rêve'\n");
    let query = r#"{"kind":"brand","identity":"brand:one","field":["name"]}"#;
    let output = run(source.as_bytes(), &["--origin", query]);
    assert!(output.status.success(), "{output:?}");
    let parsed = workspace::ParsedWorkspace::parse(&source, toml::Limits::default()).expect("SDK");
    let origin = parsed
        .origin(&serde_json::from_str(query).expect("shared typed query"))
        .expect("native AST origin");
    assert_eq!(&source[origin.span.start..origin.span.end], "'Rêve'");
    let mut expected = serde_json::to_vec_pretty(&origin).expect("same projection");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    for bad_query in [
        query.replace("brand:one", "brand:missing"),
        query.replace("name", "publications"),
        query.replace("[\"name\"]", "[-1]"),
        query.replace("\"field\"", "\"unknown\""),
    ] {
        let failed = run(source.as_bytes(), &["--origin", &bad_query]);
        assert!(!failed.status.success(), "{failed:?}");
        assert!(failed.stdout.is_empty());
    }
    let invalid = source.replace("publisher='owner:one'", "publisher='owner:missing'");
    let failed = run(invalid.as_bytes(), &["--origin", query]);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("workspace_reference"));
    let incompatible = run(
        source.as_bytes(),
        &["--origin", query, "--acquire-site", "site:one"],
    );
    assert_eq!(incompatible.status.code(), Some(2));
    assert!(incompatible.stdout.is_empty());
}

#[test]
fn malformed_origin_queries_are_shared_contract_failures_without_reports() {
    for query in [
        "not-json",
        "null",
        "[]",
        r#"{"kind":"site","identity":"site:one"}"#,
        r#"{"kind":"unknown","identity":"site:one","field":["id"]}"#,
        r#"{"kind":"site","identity":"one","field":["id"]}"#,
        r#"{"kind":"site","identity":"site:one","field":"id"}"#,
        r#"{"kind":"site","identity":"site:one","field":[true]}"#,
        r#"{"kind":"site","identity":"site:one","field":[{}]}"#,
        r#"{"kind":"site","identity":"site:one","field":[1.5]}"#,
        r#"{"kind":"site","identity":"site:one","field":[18446744073709551616]}"#,
        r#"{"kind":"site","kind":"brand","identity":"site:one","field":["id"]}"#,
        r#"{"kind":"site","\u006bind":"brand","identity":"site:one","field":["id"]}"#,
        r#"{"kind":"site","identity":"site:one","field":["id"]} {}"#,
    ] {
        assert!(
            serde_json::from_str::<workspace::FieldQuery>(query).is_err(),
            "shared query unexpectedly accepted {query}"
        );
        let output = run(SOURCE.as_bytes(), &["--origin", query]);
        assert_eq!(output.status.code(), Some(1), "{query}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("workspace_origin_query"),
            "{output:?}"
        );
    }
}

#[test]
fn origin_wire_spans_preserve_nested_source_indices_and_utf8_bytes() {
    let source = format!(
        "# 🦀 é preceding source, not a UTF-16 offset\r\n{SOURCE}[[brands]]\nid='brand:z'\nname='Rêve 🦀'\npublications=[{{site='site:one',documentation_repository={{name='référence',path='/generated/é'}}}}]\n[[brands]]\nid='brand:a'\nname='Sorts first'\n"
    );
    let parsed =
        workspace::ParsedWorkspace::parse(&source, toml::Limits::default()).expect("SDK graph");
    assert_eq!(
        parsed
            .compilation()
            .workspace
            .brands()
            .next()
            .expect("sorted brand")
            .id
            .as_str(),
        "brand:a"
    );
    for (field, expected, path_suffix) in [
        (
            serde_json::json!(["name"]),
            "'Rêve 🦀'",
            serde_json::json!(["name"]),
        ),
        (
            serde_json::json!(["publications", 0, "documentation_repository", "name"]),
            "'référence'",
            serde_json::json!(["publications", 0, "documentation_repository", "name"]),
        ),
        (
            serde_json::json!(["publications", 0, "documentation_repository", "path"]),
            "'/generated/é'",
            serde_json::json!(["publications", 0, "documentation_repository", "path"]),
        ),
    ] {
        let query = serde_json::json!({"kind":"brand", "identity":"brand:z", "field":field});
        let output = run(source.as_bytes(), &["--origin", &query.to_string()]);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let wire: serde_json::Value = serde_json::from_slice(&output.stdout).expect("origin JSON");
        let start = wire["span"]["start"].as_u64().expect("byte start") as usize;
        let end = wire["span"]["end"].as_u64().expect("byte end") as usize;
        assert_eq!(&source.as_bytes()[start..end], expected.as_bytes());
        assert_eq!(
            start,
            source.find(expected).expect("unique literal location")
        );
        assert_ne!(
            start,
            source[..start].chars().count(),
            "test exercises multibyte prefix"
        );
        assert_eq!(wire["path"][0], "brands");
        assert_eq!(
            wire["path"][1], 0,
            "original source position, not sorted brand index"
        );
        assert_eq!(
            &wire["path"].as_array().expect("path")[2..],
            path_suffix.as_array().expect("suffix")
        );
        let typed: workspace::FieldQuery = serde_json::from_value(query).expect("SDK query");
        let expected = parsed.origin(&typed).expect("SDK source projection");
        assert_eq!(wire, serde_json::to_value(expected).expect("SDK JSON"));
    }
    let string_index =
        r#"{"kind":"brand","identity":"brand:z","field":["publications","0","site"]}"#;
    let missing = run(source.as_bytes(), &["--origin", string_index]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("workspace_origin:"));
}

#[test]
fn origin_transport_cannot_escape_whole_workspace_checks_or_enable_effect_flags() {
    let query = r#"{"kind":"site","identity":"site:one","field":["id"]}"#;
    for source in [
        format!(
            "{SOURCE}[[brands]]\nid='brand:unrelated'\nname='Other'\npublications=[{{site='site:missing'}}]\n"
        ),
        format!(
            "{SOURCE}[[brands]]\nid='brand:duplicate'\nname='A'\n[[brands]]\nid='brand:duplicate'\nname='B'\n"
        ),
        format!("{SOURCE}[[brands]]\nid='brand:other'\nname='Other'\nunknown='rejected'\n"),
        format!("{SOURCE}[[brands"),
    ] {
        assert!(workspace::ParsedWorkspace::parse(&source, toml::Limits::default()).is_err());
        let output = run(source.as_bytes(), &["--origin", query]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    for flags in [
        vec!["--acquire-site", "site:one"],
        vec!["--parse-documents"],
        vec!["--max-placements", "1"],
        vec!["--max-acquired-entries", "1"],
        vec!["--max-acquired-bytes", "1"],
        vec!["--max-corpus-files", "1"],
        vec!["--max-corpus-bytes", "1"],
        vec!["--max-corpus-nodes", "1"],
        vec!["--max-corpus-metadata-nodes", "1"],
    ] {
        let mut args = vec!["--origin", query];
        args.extend(flags);
        let output = run(SOURCE.as_bytes(), &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    for limits in [
        vec!["--max-source-bytes", "1"],
        vec!["--max-nodes", "1"],
        vec!["--max-depth", "0"],
    ] {
        let mut args = vec!["--origin", query];
        args.extend(limits);
        let output = run(SOURCE.as_bytes(), &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("toml_limit"),
            "{output:?}"
        );
    }
}
