//! Executable private publication tools preserve the native CLI/SDK projections.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde_json::json;

use super::{Fixture, PAGE, Value, acquire, build, cli, fs, source};

const TOOLS: [(&str, &[&str]); 3] = [
    ("inspect_publication", &[]),
    ("review_publication", &["--review"]),
    ("project_registry", &["--registry"]),
];

fn arguments(config: &str) -> Value {
    json!({"source":config,"site":"site:one","allow_read":true})
}

fn call(tool: &str, arguments: Value, failed: bool) -> Value {
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("tool catalog")
        .iter()
        .find(|entry| entry["name"] == tool)
        .expect("implemented native tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("native input schema")
        .validate(&arguments)
        .expect("actual request matches discovery");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let value = &response["result"]["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native output schema")
        .validate(value)
        .expect("actual SDK projection matches discovery");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("readable result");
    if value["code"] == "mcp_output_limit" {
        assert!(text.contains("output ceiling"));
    } else {
        assert_eq!(
            serde_json::from_str::<Value>(text).expect("complete readable JSON"),
            *value
        );
    }
    value.clone()
}

fn cli_projection(config: &str, flags: &[&str]) -> Value {
    let mut args = vec!["check", "--publication", "site:one"];
    args.extend_from_slice(flags);
    let result = cli(config, &args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).expect("complete CLI projection")
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<source::Sha256Digest>> {
    fn visit(root: &Path, path: &Path, map: &mut BTreeMap<PathBuf, Option<source::Sha256Digest>>) {
        for entry in fs::read_dir(path).expect("disposable source tree") {
            let path = entry.expect("native entry").path();
            let metadata = fs::symlink_metadata(&path).expect("entry kind");
            assert!(!metadata.file_type().is_symlink(), "fixture has no links");
            let key = path
                .strip_prefix(root)
                .expect("fixture-relative entry")
                .to_owned();
            if metadata.is_dir() {
                map.insert(key, None);
                visit(root, &path, map);
            } else {
                map.insert(
                    key,
                    Some(source::Sha256Digest::of(
                        &fs::read(path).expect("native bytes"),
                    )),
                );
            }
        }
    }
    let mut map = BTreeMap::new();
    visit(root, root, &mut map);
    map
}

fn catalogs(config: String) -> String {
    format!(
        "{config}\n[[principals]]\nid='person:unused'\nkind='person'\nname='PRIVATE UNUSED PRINCIPAL'\n[[sites]]\nid='site:empty'\npublisher='owner:one'\ndeployable=false\n"
    )
}

#[test]
fn all_three_tools_match_native_cli_sdk_with_drafts_foreign_content_and_nested_uses() {
    let page = PAGE.replace("[content]\n", "[content]\npublished='2025-02-03'\nmodified='2025-02-04T05:06:07.123+05:30'\nattributions=[{principal='owner:one',role='author'}]\n");
    let bundled = PAGE
        .replace("content:one", "content:bundle")
        .replace("One <escaped>", "Bundled page")
        .replace("/guide/", "/bundle/")
        .replace("aliases=['/old.html']", "aliases=[]");
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/bundle/index.md", bundled.as_bytes()),
        (
            "one/bundle/main.ts",
            b"export const retained: boolean = true;\n",
        ),
    ]);
    let config = catalogs(fixture.config());
    let before = snapshot(fixture.directory.path());
    let inputs = acquire(&config);
    let parsed = build::parse_publication(&inputs, Default::default()).expect("native corpus");
    let resolved = parsed
        .resolve(Default::default())
        .expect("complete source-bound model");
    let review = build::review(&resolved, Default::default()).expect("native nested review");
    let registry = resolved
        .registry(Default::default())
        .expect("complete registry");
    let expected = [json!(resolved.inspect()), json!(review), json!(registry)];
    for ((tool, flags), expected) in TOOLS.into_iter().zip(&expected) {
        let actual = call(tool, arguments(&config), false);
        assert_eq!(&actual, expected);
        assert_eq!(actual, cli_projection(&config, flags));
    }
    let graph = &expected[0]["graph"];
    assert_eq!(graph["nodes"]["content:one"]["lifecycle"], "draft");
    assert_eq!(graph["nodes"]["content:other"]["site"], "site:other");
    assert_eq!(
        graph["nodes"]["content:one"]["modified"],
        "2025-02-04T05:06:07.123+05:30"
    );
    assert_eq!(
        expected[0]["acquisition"]["sources"]
            .as_object()
            .expect("exact source evidence")
            .len(),
        3
    );
    assert_eq!(
        expected[0]["page_packages"]["content:bundle"]["root"],
        "bundle"
    );
    assert_eq!(
        expected[0]["page_packages"]["content:bundle"]["companions"][0]["relative"],
        "main.ts"
    );
    assert_eq!(
        expected[0]["page_packages"]["content:bundle"]["companions"][0]["kind"],
        "program"
    );
    let mut found_nested_foreign = false;
    for (index, surface) in review.request().surfaces().iter().enumerate() {
        let actual_surface = &expected[1]["request"]["surfaces"][index]["value"];
        for usage in surface.uses() {
            assert_eq!(
                actual_surface.pointer(&usage.pointer),
                surface.value().pointer(&usage.pointer)
            );
            if usage.pointer.contains("/related/")
                && actual_surface.pointer(&usage.pointer) == Some(&json!("content:other"))
            {
                found_nested_foreign = true;
            }
        }
    }
    assert!(
        found_nested_foreign,
        "review must retain nested foreign uses rather than top-level IDs only"
    );
    assert_eq!(
        expected[2]["model"]["principals"]["person:unused"]["name"],
        "PRIVATE UNUSED PRINCIPAL"
    );
    assert!(expected[2]["model"]["sites"]["site:empty"].is_object());
    assert_eq!(
        expected[2]["model"]["provenance"]["content:other"]["source"],
        "source:other"
    );
    assert_eq!(expected[2]["status"], "resolved-unadmitted");
    assert_eq!(
        snapshot(fixture.directory.path()),
        before,
        "source files and all Git metadata unchanged"
    );
}

#[test]
fn empty_registry_retains_complete_catalogs_instead_of_skipping_projection() {
    let fixture = Fixture::new(&[
        ("one/assets/input.bin", b"one"),
        ("other/assets/input.bin", b"other"),
    ]);
    let config = catalogs(fixture.config())
        .replace("subdirectory='one'", "subdirectory='one/assets'")
        .replace("subdirectory='other'", "subdirectory='other/assets'");
    let before = snapshot(fixture.directory.path());
    let inputs = acquire(&config);
    let parsed =
        build::parse_publication(&inputs, Default::default()).expect("empty content corpus");
    let resolved = parsed
        .resolve(Default::default())
        .expect("empty graph with catalogs");
    let native = json!(
        resolved
            .registry(Default::default())
            .expect("complete empty registry")
    );
    let actual = call("project_registry", arguments(&config), false);
    assert_eq!(actual, native);
    assert_eq!(actual, cli_projection(&config, &["--registry"]));
    assert_eq!(actual["model"]["content"]["nodes"], json!({}));
    assert_eq!(actual["model"]["provenance"], json!({}));
    assert_eq!(
        actual["model"]["sites"]
            .as_object()
            .expect("all sites")
            .len(),
        3
    );
    assert_eq!(
        actual["model"]["principals"]
            .as_object()
            .expect("all principals")
            .len(),
        2
    );
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn denied_reads_and_native_source_failures_never_return_partial_projections() {
    let fixture = Fixture::new(&[]);
    let before = snapshot(fixture.directory.path());
    let config = fixture.config();
    let stale = config.replace(
        &fixture.commit.to_string(),
        "1111111111111111111111111111111111111111",
    );
    let missing = config.replace("subdirectory='one'", "subdirectory='absent'");
    for source in [stale.as_str(), missing.as_str(), "[broken TOML"] {
        let native = build::acquire_publication(
            source,
            &"site:one".parse().expect("site"),
            Default::default(),
            build::PublicationSetAcquisitionLimits::default().sources,
        )
        .expect_err("actual native acquisition failure");
        for (tool, flags) in TOOLS {
            let mut denied = arguments(source);
            denied["allow_read"] = json!(false);
            assert_eq!(call(tool, denied, true)["code"], "publication_permission");
            let actual = call(tool, arguments(source), true);
            assert_eq!(actual["code"], native.code());
            assert!(
                actual.get("graph").is_none()
                    && actual.get("request").is_none()
                    && actual.get("model").is_none()
            );
            let mut args = vec!["check", "--publication", "site:one"];
            args.extend_from_slice(flags);
            let result = cli(source, &args);
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
            assert!(String::from_utf8_lossy(&result.stderr).contains(native.code()));
        }
    }
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn exact_review_and_registry_bounds_and_earlier_corpus_limits_fail_without_subsets() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let before = snapshot(fixture.directory.path());
    let inputs = acquire(&config);
    let parsed = build::parse_publication(&inputs, Default::default()).expect("native corpus");
    let resolved = parsed.resolve(Default::default()).expect("native graph");
    let review = build::review(&resolved, Default::default()).expect("complete review");
    let uses = review
        .request()
        .surfaces()
        .iter()
        .map(|surface| surface.uses().count())
        .sum::<usize>();
    let pointers = review
        .request()
        .surfaces()
        .iter()
        .flat_map(|surface| surface.uses())
        .map(|usage| usage.pointer.len())
        .sum::<usize>();
    let bytes = serde_json::to_vec(review.request())
        .expect("compact review request")
        .len()
        .max(pointers);
    let mut exact = arguments(&config);
    exact["review"] = json!({"bytes":bytes,"uses":uses});
    assert_eq!(
        call("review_publication", exact.clone(), false),
        json!(review)
    );
    exact["review"]["uses"] = json!(uses - 1);
    assert_eq!(
        call("review_publication", exact, true)["code"],
        "publication_review_limit"
    );
    assert_eq!(
        cli_projection(
            &config,
            &[
                "--review",
                "--max-review-bytes",
                &bytes.to_string(),
                "--max-review-uses",
                &uses.to_string()
            ]
        ),
        json!(review)
    );
    let registry = resolved
        .registry(Default::default())
        .expect("complete registry");
    let bytes = serde_json::to_vec(&registry)
        .expect("compact registry")
        .len();
    let mut exact = arguments(&config);
    exact["registry"] = json!({"bytes":bytes});
    assert_eq!(
        call("project_registry", exact.clone(), false),
        json!(registry)
    );
    exact["registry"]["bytes"] = json!(bytes - 1);
    assert_eq!(
        call("project_registry", exact, true)["code"],
        "registry_encoding"
    );
    let corpus = gazette_sdk::corpus::Limits {
        files: 0,
        ..Default::default()
    };
    let native =
        build::parse_publication(&inputs, corpus).expect_err("whole corpus exceeds zero files");
    for (tool, _) in TOOLS {
        let mut arguments = arguments(&config);
        arguments["corpus"] = json!(corpus);
        let actual = call(tool, arguments, true);
        assert_eq!(actual["code"], native.code());
        assert!(
            actual.get("graph").is_none()
                && actual.get("request").is_none()
                && actual.get("model").is_none()
        );
    }
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn native_projection_exceeding_carrier_budget_is_an_error_without_a_successful_prefix() {
    let fixture = Fixture::new(&[]);
    let config = fixture
        .config()
        .replace("Explicit bindings", &"A".repeat(300_000));
    let before = snapshot(fixture.directory.path());
    let syntax = gazette_sdk::toml::Limits {
        source_bytes: 1 << 20,
        ..Default::default()
    };
    let inputs = build::acquire_publication(
        &config,
        &"site:one".parse().expect("site"),
        syntax,
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("bounded actual native capture");
    let parsed = build::parse_publication(&inputs, Default::default()).expect("native corpus");
    let resolved = parsed.resolve(Default::default()).expect("native model");
    let values = [
        ("inspect_publication", json!(resolved.inspect())),
        (
            "project_registry",
            json!(
                resolved
                    .registry(Default::default())
                    .expect("native registry")
            ),
        ),
    ];
    for (tool, native) in values {
        assert!(
            serde_json::to_vec(&native)
                .expect("native encoded output")
                .len()
                > 256 * 1024
        );
        let mut arguments = arguments(&config);
        arguments["syntax"] = json!(syntax);
        assert_eq!(
            call(tool, arguments, true),
            json!({"code":"mcp_output_limit"})
        );
    }
    assert_eq!(snapshot(fixture.directory.path()), before);
}
