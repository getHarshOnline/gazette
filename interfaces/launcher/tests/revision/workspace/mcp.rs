//! Actual MCP site-documentation phases share CLI/SDK captures and complete reports.

use super::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const TOOLS: [&str; 2] = ["inspect_site", "parse_site_documents"];

fn arguments(config: &str) -> Value {
    json!({"source":config,"site":"site:one","allow_read":true})
}

pub(super) fn call(tool: &str, arguments: Value, failed: bool) -> Value {
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool_info| tool_info["name"] == tool)
        .expect("named native operation");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("real input agrees with discovery");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let value = &response["result"]["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(value)
        .expect("actual native report agrees with discovery");
    if value["code"] == "mcp_output_limit" {
        assert_eq!(value, &json!({"code":"mcp_output_limit"}));
    } else {
        assert_eq!(
            serde_json::from_str::<Value>(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .expect("readable report")
            )
            .expect("complete JSON"),
            *value
        );
    }
    value.clone()
}

fn capture(config: &str) -> build::AcquiredSite {
    build::acquire_site(
        config,
        &"site:one".parse().expect("site"),
        Default::default(),
        limits(),
    )
    .expect("exact native capture")
}

fn cli(config: &str, tool: &str, extra: &[&str]) -> Output {
    let mut flags = vec!["--acquire-site", "site:one"];
    if tool == "parse_site_documents" {
        flags.push("--parse-documents");
    }
    flags.extend_from_slice(extra);
    run(config, &flags)
}

fn parity(config: &str, tool: &str, expected: Value) -> Value {
    let actual = call(tool, arguments(config), false);
    assert_eq!(actual, expected);
    let output = cli(config, tool, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI JSON"),
        actual
    );
    actual
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<source::Sha256Digest>> {
    fn visit(
        root: &Path,
        path: &Path,
        entries: &mut BTreeMap<PathBuf, Option<source::Sha256Digest>>,
    ) {
        for entry in fs::read_dir(path).expect("fixture directory") {
            let path = entry.expect("fixture entry").path();
            let metadata = fs::symlink_metadata(&path).expect("fixture metadata");
            assert!(!metadata.file_type().is_symlink(), "no fixture links");
            let relative = path
                .strip_prefix(root)
                .expect("confined fixture")
                .to_owned();
            if metadata.is_dir() {
                entries.insert(relative, None);
                visit(root, &path, entries);
            } else {
                assert!(metadata.is_file());
                entries.insert(
                    relative,
                    Some(source::Sha256Digest::of(
                        &fs::read(path).expect("fixture bytes"),
                    )),
                );
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn fixture(files: &[(&str, &[u8])]) -> Fixture {
    let mut fixture = Fixture::new();
    let repository = Repository::open(fixture.root.path()).expect("native repository");
    let mut entries = vec![(
        "readme.md",
        repository.blob(b"abc").expect("existing Markdown"),
        0o100644,
    )];
    for (name, bytes) in files {
        fs::write(fixture.root.path().join("docs").join(name), bytes).expect("fixture payload");
        entries.push((
            *name,
            repository.blob(bytes).expect("native object"),
            0o100644,
        ));
    }
    fixture.commit = commit_docs(&repository, &entries, "MCP documentation fixture");
    fixture
}

#[test]
fn mixed_document_notebook_asset_reports_match_cli_sdk_and_preserve_exact_source() {
    let document = "+++\r\ntitle='Café'\r\n+++\r\n# Héllo 😀\r\n[README](readme.md)\r\n";
    let notebook = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"id":"one","cell_type":"code","metadata":{},"source":"raise RuntimeError('must never execute')","execution_count":null,"outputs":[]}]}"#;
    let fixture = fixture(&[
        ("guide.md", document.as_bytes()),
        ("image.bin", &[255, 128, 0]),
        ("calculation.ipynb", notebook),
    ]);
    let config = config(&fixture);
    let before = snapshot(fixture.root.path());
    let acquired = capture(&config);
    parity(&config, TOOLS[0], json!(acquired.inspect()));
    let parsed = build::parse_documents(&acquired, Default::default()).expect("native corpus");
    let expected = json!(parsed.inspect().expect("complete inspection"));
    let actual = parity(&config, TOOLS[1], expected.clone());
    let files = &actual["corpus"]["files"];
    assert_eq!(files.as_object().expect("complete corpus").len(), 4);
    assert_eq!(files["docs/tool/readme.md"]["kind"], "markdown");
    assert_eq!(files["docs/tool/guide.md"]["kind"], "document");
    assert_eq!(files["docs/tool/calculation.ipynb"]["syntax"]["cells"], 1);
    assert_eq!(
        files["docs/tool/image.bin"]["syntax"]["sha256"],
        json!(source::Sha256Digest::of(&[255, 128, 0]))
    );
    assert_eq!(snapshot(fixture.root.path()), before);
    fs::write(
        fixture.root.path().join("docs/readme.md"),
        "different live bytes",
    )
    .expect("deliberate source change");
    assert_eq!(
        json!(
            build::parse_documents(&acquired, Default::default())
                .expect("retained parse")
                .inspect()
                .expect("retained report")
        ),
        expected
    );
    for tool in TOOLS {
        assert_eq!(
            call(tool, arguments(&config), true)["code"],
            "workspace_source"
        );
    }
}

#[test]
fn denied_reads_and_unselected_placements_do_not_acquire_missing_repositories() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.root.path());
    let missing = fixture.root.path().join("absent");
    let config = config(&fixture).replace(
        fixture.root.path().to_str().expect("root"),
        missing.to_str().expect("missing root"),
    );
    for tool in TOOLS {
        for source in [config.as_str(), "not valid TOML"] {
            let mut denied = arguments(source);
            denied["allow_read"] = json!(false);
            assert_eq!(call(tool, denied, true)["code"], "site_permission");
        }
        assert_eq!(
            call(tool, arguments(&config), true)["code"],
            "workspace_source"
        );
    }
    let disabled = config.replace("render=true", "render=false");
    let other =
        format!("{config}[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n");
    for (source, site) in [(&disabled, "site:one"), (&other, "site:two")] {
        let acquired = build::acquire_site(
            source,
            &site.parse().expect("site"),
            Default::default(),
            limits(),
        )
        .expect("unselected source remains unopened");
        for tool in TOOLS {
            let mut args = arguments(source);
            args["site"] = json!(site);
            let actual = call(tool, args, false);
            let expected = if tool == TOOLS[0] {
                json!(acquired.inspect())
            } else {
                json!(
                    build::parse_documents(&acquired, Default::default())
                        .expect("empty corpus")
                        .inspect()
                        .expect("empty report")
                )
            };
            assert_eq!(actual, expected);
            let flags = if tool == TOOLS[0] {
                vec!["--acquire-site", site]
            } else {
                vec!["--acquire-site", site, "--parse-documents"]
            };
            let output = run(source, &flags);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).expect("CLI report"),
                actual
            );
        }
    }
    assert!(!missing.exists());
    assert_eq!(snapshot(fixture.root.path()), before);
}

#[test]
fn later_malformed_document_or_notebook_never_returns_successful_corpus_prefix() {
    for (name, bytes) in [("z.md", b"+++\nbroken = [\n+++\nbody".as_slice()), ("z.ipynb", br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"markdown","metadata":{},"source":"missing ID"}]}"#.as_slice())] {
        let fixture = fixture(&[(name, bytes)]);
        let config = config(&fixture);
        let before = snapshot(fixture.root.path());
        let acquired = capture(&config);
        parity(&config, TOOLS[0], json!(acquired.inspect()));
        let error = build::parse_documents(&acquired, Default::default()).expect_err("native late syntax failure");
        let actual = call(TOOLS[1], arguments(&config), true);
        assert_eq!(actual["code"], error.code());
        assert!(actual.get("corpus").is_none());
        let output = cli(&config, TOOLS[1], &[]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(error.code()));
        assert_eq!(snapshot(fixture.root.path()), before);
    }
}

#[test]
fn native_capture_and_corpus_exact_limits_match_and_exhaustion_has_no_subset() {
    let fixture = Fixture::new();
    let config = config(&fixture);
    let before = snapshot(fixture.root.path());
    let acquired = capture(&config);
    let corpus = corpus::Limits {
        files: 1,
        source_bytes: 3,
        path_bytes: "docs/tool/readme.md".len(),
        ..Default::default()
    };
    let expected = json!(
        build::parse_documents(&acquired, corpus)
            .expect("exact source/path/file budgets")
            .inspect()
            .expect("native report")
    );
    let mut args = arguments(&config);
    args["corpus"] = json!(corpus);
    assert_eq!(call(TOOLS[1], args.clone(), false), expected);
    let output = cli(
        &config,
        TOOLS[1],
        &["--max-corpus-files", "1", "--max-corpus-bytes", "3"],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI report"),
        expected
    );
    for field in ["files", "source_bytes", "path_bytes", "markdown_nodes"] {
        let mut limited = json!(corpus);
        limited[field] = json!(0);
        let native: corpus::Limits =
            serde_json::from_value(limited.clone()).expect("native bounds");
        let error = build::parse_documents(&acquired, native).expect_err("complete corpus limit");
        args["corpus"] = limited;
        assert_eq!(call(TOOLS[1], args.clone(), true)["code"], error.code());
    }
    for tool in TOOLS {
        let mut acquisition = limits();
        acquisition.bytes = 0;
        let error = build::acquire_site(
            &config,
            &"site:one".parse().expect("site"),
            Default::default(),
            acquisition,
        )
        .expect_err("native capture ceiling");
        let mut args = arguments(&config);
        args["acquisition"] = json!(acquisition);
        assert_eq!(call(tool, args, true)["code"], error.code());
        let output = cli(&config, tool, &["--max-acquired-bytes", "0"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(error.code()));
    }
    assert_eq!(snapshot(fixture.root.path()), before);
}

#[test]
fn real_native_reports_over_carrier_ceiling_return_only_explicit_failure() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.root.path());
    let config = config(&fixture).replace(
        "kind='person'",
        &format!("kind='person'\nname='{}'", "A".repeat(300_000)),
    );
    let syntax = toml::Limits {
        source_bytes: 1 << 20,
        ..Default::default()
    };
    let acquired = build::acquire_site(
        &config,
        &"site:one".parse().expect("site"),
        syntax,
        limits(),
    )
    .expect("bounded large native configuration");
    let parsed = build::parse_documents(&acquired, Default::default()).expect("native corpus");
    for (tool, expected) in TOOLS.into_iter().zip([
        json!(acquired.inspect()),
        json!(parsed.inspect().expect("native inspection")),
    ]) {
        assert!(
            serde_json::to_vec(&expected)
                .expect("actual full report")
                .len()
                > 256 * 1024
        );
        let mut args = arguments(&config);
        args["syntax"] = json!(syntax);
        assert_eq!(call(tool, args, true), json!({"code":"mcp_output_limit"}));
    }
    assert_eq!(snapshot(fixture.root.path()), before);
}
