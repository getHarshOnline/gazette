//! The executable projects the shared composition resolver and its retained source evidence.

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{toml, workspace};
use serde::Serialize;
use serde_json::{Value, json};
use tempfile::tempdir;

const SOURCE: &str = "# 🦀 é: quoted keys and CRLF are source evidence\r\n\
[composition]\n\
theme = {package='theme:base',palette='palette:light'}\n\
[composition.plugins]\n\
'fixture-capabilities'={state='enabled'}\n\
[composition.capabilities]\n\
'publication.search/v1' = {state='enabled',provider={kind='plugin',package='fixture-capabilities',provider='search'},requirement='required'}\n\
'commerce.catalog/v1' = {state='enabled',provider={kind='plugin',package='fixture-capabilities',provider='catalog'},requirement='optional'}\n\
[[principals]]\n\
id='owner:one'\n\
kind='organization'\n\
[[sources]]\n\
id='source:theme'\n\
location={kind='repository',root='/not-read/é/theme',commit='0000000000000000000000000000000000000000',subdirectory='templates'}\n\
[[theme_packages]]\n\
package='theme:base'\n\
source='source:theme'\n\
manifest='base-theme.toml'\n\
[[theme_packages]]\n\
package='theme:other'\n\
source='source:theme'\n\
manifest='other-theme.toml'\n\
[[plugin_packages]]\n\
package='fixture-capabilities'\n\
source='source:theme'\n\
manifest='plugin.toml'\n\
[[sites]]\n\
id='site:z'\n\
publisher='owner:one'\n\
deployable=false\n\
[sites.composition]\n\
theme={package='theme:other'}\n\
[sites.composition.capabilities]\n\
'publication.search/v1'={state='disabled'}\n\
'commerce.inventory/v1'={state='enabled',provider={kind='plugin',package='fixture-capabilities',provider='inventory'},requirement='optional'}\n\
[[sites]]\n\
id='site:a'\n\
publisher='owner:one'\n\
deployable=true\n";

fn run(source: &str, args: &[&str]) -> Output {
    let cwd = tempdir().expect("isolated native context");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--stdin"])
        .args(args)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real Gazette executable");
    let written = child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes());
    let output = child.wait_with_output().expect("CLI result");
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(!output.status.success());
    }
    assert!(fs::read_dir(cwd.path()).expect("cwd").next().is_none());
    output
}

fn pretty(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("shared serialization");
    bytes.push(b'\n');
    bytes
}

fn query(site: &str, field: Value, path: Value) -> Value {
    json!({"site":site,"field":field,"path":path})
}

#[test]
fn check_uses_the_sdk_composition_without_discovery_or_parallel_defaults() {
    let compiled = workspace::compile(SOURCE, toml::Limits::default()).expect("shared compiler");
    let output = run(SOURCE, &[]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, pretty(&compiled));
    assert_eq!(output.stdout, run(SOURCE, &[]).stdout);
    let inherited = compiled
        .workspace
        .composition_for(&"site:a".parse().expect("key"))
        .expect("known site");
    let overridden = compiled
        .workspace
        .composition_for(&"site:z".parse().expect("key"))
        .expect("known site");
    assert_eq!(
        serde_json::to_value(inherited.theme().expect("inherited theme").value()).expect("value"),
        json!({"package":"theme:base","palette":"palette:light"})
    );
    let theme =
        serde_json::to_value(overridden.theme().expect("site theme").value()).expect("value");
    assert_eq!(theme["package"], "theme:other");
    assert!(
        theme.get("palette").is_none_or(Value::is_null),
        "whole theme override clears palette"
    );
    let search = "publication.search/v1".parse().expect("capability");
    assert_eq!(
        serde_json::to_value(
            overridden
                .capabilities()
                .get(&search)
                .expect("disabled winner")
                .value()
        )
        .expect("value"),
        json!({"state":"disabled"})
    );
    let analytics = "commerce.catalog/v1".parse().expect("capability");
    assert_eq!(
        inherited.capabilities().get(&analytics),
        overridden.capabilities().get(&analytics)
    );
    assert!(
        compiled
            .workspace
            .composition_for(&"site:missing".parse().expect("key"))
            .is_none()
    );
}

#[test]
fn effective_origins_preserve_quoted_keys_original_indices_and_utf8_byte_spans() {
    let parsed =
        workspace::ParsedWorkspace::parse(SOURCE, toml::Limits::default()).expect("shared AST");
    assert_eq!(
        parsed
            .compilation()
            .workspace
            .sites()
            .next()
            .expect("sorted")
            .id
            .as_str(),
        "site:a"
    );
    for (site, field, suffix, literal, path, scope) in [
        (
            "site:a",
            json!({"kind":"theme"}),
            json!(["palette"]),
            "'palette:light'",
            json!(["composition", "theme", "palette"]),
            json!("workspace"),
        ),
        (
            "site:z",
            json!({"kind":"theme"}),
            json!(["package"]),
            "'theme:other'",
            json!(["sites", 0, "composition", "theme", "package"]),
            json!({"site":"site:z"}),
        ),
        (
            "site:z",
            json!({"kind":"capability","capability":"publication.search/v1"}),
            json!(["state"]),
            "'disabled'",
            json!([
                "sites",
                0,
                "composition",
                "capabilities",
                "publication.search/v1",
                "state"
            ]),
            json!({"site":"site:z"}),
        ),
        (
            "site:z",
            json!({"kind":"capability","capability":"commerce.catalog/v1"}),
            json!(["provider", "provider"]),
            "'catalog'",
            json!([
                "composition",
                "capabilities",
                "commerce.catalog/v1",
                "provider",
                "provider"
            ]),
            json!("workspace"),
        ),
        (
            "site:z",
            json!({"kind":"capability","capability":"commerce.inventory/v1"}),
            json!(["provider", "provider"]),
            "'inventory'",
            json!([
                "sites",
                0,
                "composition",
                "capabilities",
                "commerce.inventory/v1",
                "provider",
                "provider"
            ]),
            json!({"site":"site:z"}),
        ),
    ] {
        let wire_query = query(site, field.clone(), suffix);
        let typed: workspace::CompositionOriginQuery =
            serde_json::from_value(wire_query.clone()).expect("shared query");
        let origin = parsed
            .composition_origin(&typed)
            .expect("winning explicit field");
        assert_eq!(&SOURCE[origin.span.start..origin.span.end], literal);
        assert_eq!(origin.source_sha256, parsed.compilation().source_sha256);
        let expected_start = if literal == "'theme:other'" {
            SOURCE.rfind(literal).expect("selected site Theme literal")
        } else {
            SOURCE.find(literal).expect("unique literal")
        };
        assert_eq!(origin.span.start, expected_start);
        assert_ne!(
            origin.span.start,
            SOURCE[..origin.span.start].chars().count()
        );
        let output = run(SOURCE, &["--composition-origin", &wire_query.to_string()]);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, pretty(&origin));
        let wire: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
        assert_eq!(wire["path"], path);
        assert_eq!(wire["selection"]["scope"], scope);
        assert_eq!(wire["selection"]["field"], field);
    }
    // The omitted query path addresses the whole selected atom, not a fabricated scalar.
    let whole =
        json!({"site":"site:z","field":{"kind":"capability","capability":"publication.search/v1"}});
    let typed: workspace::CompositionOriginQuery =
        serde_json::from_value(whole.clone()).expect("default path");
    let origin = parsed.composition_origin(&typed).expect("whole binding");
    assert_eq!(
        &SOURCE[origin.span.start..origin.span.end],
        "{state='disabled'}"
    );
    assert_eq!(
        run(SOURCE, &["--composition-origin", &whole.to_string()]).stdout,
        pretty(&origin)
    );
}

#[test]
fn absent_winners_and_invalid_queries_never_produce_partial_reports() {
    for request in [
        query("site:missing", json!({"kind":"theme"}), json!([])),
        query("site:z", json!({"kind":"theme"}), json!(["palette"])),
        query(
            "site:a",
            json!({"kind":"capability","capability":"commerce.inventory/v1"}),
            json!([]),
        ),
        query(
            "site:z",
            json!({"kind":"capability","capability":"publication.search/v1"}),
            json!(["provider"]),
        ),
        query("site:a", json!({"kind":"theme"}), json!(["unknown"])),
    ] {
        let parsed = workspace::ParsedWorkspace::parse(SOURCE, toml::Limits::default())
            .expect("valid topology");
        let typed = serde_json::from_value(request.clone()).expect("valid query shape");
        assert!(parsed.composition_origin(&typed).is_err());
        let output = run(SOURCE, &["--composition-origin", &request.to_string()]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    for malformed in [
        "not-json",
        "null",
        "[]",
        r#"{"site":"site:a"}"#,
        r#"{"site":"a","field":{"kind":"theme"}}"#,
        r#"{"site":"site:a","field":{"kind":"unknown"}}"#,
        r#"{"site":"site:a","field":{"kind":"theme","unknown":true}}"#,
        r#"{"site":"site:a","field":{"kind":"theme","capability":"publication.search/v1"}}"#,
        r#"{"site":"site:a","field":{"kind":"capability","capability":"publication.search/v1","unknown":true}}"#,
        r#"{"site":"site:a","field":{"kind":"capability"}}"#,
        r#"{"site":"site:a","field":{"kind":"theme"},"path":[-1]}"#,
        r#"{"site":"site:a","field":{"kind":"theme"},"path":[true]}"#,
        r#"{"site":"site:a","field":{"kind":"theme"},"path":"theme"}"#,
        r#"{"site":"site:a","field":{"kind":"theme"},"unknown":true}"#,
        r#"{"site":"site:a","site":"site:z","field":{"kind":"theme"}}"#,
        r#"{"site":"site:a","field":{"kind":"theme"}} {}"#,
    ] {
        assert!(serde_json::from_str::<workspace::CompositionOriginQuery>(malformed).is_err());
        let output = run(SOURCE, &["--composition-origin", malformed]);
        assert_eq!(output.status.code(), Some(1), "{malformed}: {output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn composition_queries_keep_whole_workspace_validation_and_exclude_acquisition() {
    let request = r#"{"site":"site:a","field":{"kind":"theme"}}"#;
    for invalid in [
        SOURCE.replace("publisher='owner:one'", "publisher='owner:missing'"),
        SOURCE.replace("package='theme:base'", "package='theme:base',unknown=true"),
        SOURCE.replace("source='source:theme'", "source='source:missing'"),
        SOURCE.replacen(
            "provider={kind='plugin',package='fixture-capabilities'",
            "provider={kind='plugin',package='missing-capabilities'",
            1,
        ),
        format!("{SOURCE}[[sites"),
    ] {
        assert!(workspace::compile(&invalid, toml::Limits::default()).is_err());
        let output = run(&invalid, &["--composition-origin", request]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    for flags in [
        vec![
            "--origin",
            r#"{"kind":"site","identity":"site:a","field":["id"]}"#,
        ],
        vec!["--acquire-site", "site:a"],
        vec!["--parse-documents"],
        vec!["--max-placements", "1"],
        vec!["--max-acquired-entries", "1"],
        vec!["--max-acquired-bytes", "1"],
        vec!["--max-corpus-files", "1"],
        vec!["--max-corpus-bytes", "1"],
        vec!["--max-corpus-nodes", "1"],
        vec!["--max-corpus-metadata-nodes", "1"],
    ] {
        let mut args = vec!["--composition-origin", request];
        args.extend(flags);
        let output = run(SOURCE, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    for flag in ["--max-source-bytes", "--max-nodes", "--max-depth"] {
        let output = run(SOURCE, &["--composition-origin", request, flag, "1"]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("toml_limit"));
    }
}
