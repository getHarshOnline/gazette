//! Executable MCP rendering shares native capture, compilation and complete site evidence.

use crate::mcp_carrier as carrier;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use super::{
    Fixture, SITES, Value, acquire_set, build, cli, fs, independent_theme_config, json, options,
    request,
};

fn arguments(recipe: &Value) -> Value {
    json!({"recipe_json":recipe.to_string(),"local":true,"allow_read":true})
}

fn call(arguments: Value, failed: bool) -> Value {
    let responses = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"render_sites","arguments":arguments}}),
    ]);
    let tool = responses[0]["result"]["tools"]
        .as_array()
        .expect("tool discovery")
        .iter()
        .find(|tool| tool["name"] == "render_sites")
        .expect("implemented render tool");
    jsonschema::validator_for(&tool["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("actual named arguments match discovery");
    let recipe: Value =
        serde_json::from_str(arguments["recipe_json"].as_str().expect("recipe string"))
            .expect("fixture recipe JSON");
    jsonschema::validator_for(&tool["inputSchema"]["properties"]["recipe_json"]["contentSchema"])
        .expect("native recipe content schema")
        .validate(&recipe)
        .expect("actual recipe shape");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let result = &response["result"]["structuredContent"];
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(result)
        .expect("actual native result matches discovery");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("readable result"),
    )
    .expect("JSON text result");
    assert_eq!(
        &text, result,
        "structured and text carriers preserve the same evidence"
    );
    result.clone()
}

fn sdk(recipe: &Value, limits: build::PublicationSetRenderLimits) -> (Value, bool) {
    let selected = recipe["sites"]
        .as_array()
        .expect("site requests")
        .iter()
        .map(|site| site["site"].as_str().expect("site key"))
        .collect::<Vec<_>>();
    let inputs = acquire_set(
        recipe["workspace"]["source"]
            .as_str()
            .expect("workspace text"),
        &selected,
    );
    let page = "page.html".parse().expect("page entry");
    let alias = "alias.html".parse().expect("alias entry");
    let model = "model.json".parse().expect("model entry");
    let graph_output = "graph-view.html".parse().expect("graph output");
    let mut selected_options = options(&selected, &page, &alias, &model);
    for site in recipe["sites"].as_array().expect("site requests") {
        if site.get("graph").is_some() {
            selected_options
                .sites
                .get_mut(&super::key(site["site"].as_str().expect("site key")))
                .expect("selected site options")
                .graph = Some(build::GraphRenderOptions {
                output: &graph_output,
                limits: Default::default(),
            });
        }
    }
    if let Some(selection) = recipe.get("selection") {
        selected_options.selection =
            Some(serde_json::from_value(selection.clone()).expect("native selection"));
    }
    let rendered = build::render_publications(&inputs, selected_options, limits)
        .expect("complete shared semantic phase");
    (
        serde_json::to_value(rendered.inspect()).expect("native report"),
        !rendered.is_complete(),
    )
}

#[test]
fn undefined_template_variable_keeps_location_over_mcp_and_cli() {
    let fixture = Fixture::new(&[(
        "theme/templates/page.html",
        b"heading\n{{ missing_value }}\nfooter",
    )]);
    let recipe = request(&fixture.config(), &SITES[..1]);
    let actual = call(arguments(&recipe), true);
    let message = actual["sites"]["site:one"]["message"]
        .as_str()
        .expect("site failure message");
    assert!(
        message.contains("template 'fixture/page.html'"),
        "{message}"
    );
    assert!(message.contains("line 2, column 4"), "{message}");
    assert!(
        message.contains("Variable `missing_value` not found"),
        "{message}"
    );

    let output = cli(&recipe.to_string(), &["render-sites", "--local"]);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete failure report");
    assert_eq!(report, actual);
    assert!(
        output.stderr.is_empty(),
        "structured report owns the failure"
    );
}

fn cli_report(recipe: &Value, extra: &[&str], failed: bool) -> Value {
    let mut flags = vec!["render-sites", "--local"];
    flags.extend_from_slice(extra);
    let response = cli(&recipe.to_string(), &flags);
    assert_eq!(
        response.status.success(),
        !failed,
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    serde_json::from_slice(&response.stdout).expect("complete CLI report")
}

pub(super) fn snapshot(
    root: &Path,
) -> BTreeMap<PathBuf, Option<gazette_sdk::source::Sha256Digest>> {
    fn visit(
        root: &Path,
        path: &Path,
        result: &mut BTreeMap<PathBuf, Option<gazette_sdk::source::Sha256Digest>>,
    ) {
        for entry in fs::read_dir(path).expect("disposable tree") {
            let path = entry.expect("native entry").path();
            let kind = fs::symlink_metadata(&path).expect("native metadata");
            assert!(!kind.file_type().is_symlink(), "fixture has no links");
            let key = path
                .strip_prefix(root)
                .expect("confined fixture entry")
                .to_owned();
            if kind.is_dir() {
                result.insert(key, None);
                visit(root, &path, result);
            } else {
                result.insert(
                    key,
                    Some(gazette_sdk::source::Sha256Digest::of(
                        &fs::read(path).expect("native bytes"),
                    )),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn single_and_multi_site_mcp_cli_sdk_reports_match_without_native_mutation() {
    let fixture = Fixture::new(&[("theme/templates/graph.html", b"<h1>{{ site }}</h1>")]);
    let before = snapshot(fixture.directory.path());
    for selected in [&SITES[..1], SITES.as_slice()] {
        let mut recipe = request(&fixture.config(), selected);
        for site in recipe["sites"].as_array_mut().expect("site requests") {
            site["graph"] = json!({"output":"graph-view.html"});
        }
        let (expected, failed) = sdk(&recipe, Default::default());
        assert!(!failed);
        let actual = call(arguments(&recipe), false);
        assert_eq!(actual, expected);
        assert_eq!(actual, cli_report(&recipe, &[], false));
        for site in selected {
            let inventory = actual["sites"][site]["inventory"]["entries"]
                .as_array()
                .expect("complete native inventory");
            assert!(inventory.iter().any(|entry| entry["path"] == "model.json"));
            assert!(
                inventory
                    .iter()
                    .any(|entry| entry["path"] == "graph-view.html")
            );
            assert_eq!(
                actual["sites"][site]["evidence"]["graph"]["output"],
                "graph-view.html"
            );
            assert!(actual["sites"][site]["evidence"]["graph"]["model_sha256"].is_string());
            assert!(
                !actual["sites"][site]["evidence"]["pages"]
                    .as_object()
                    .expect("rendered page evidence")
                    .is_empty()
            );
        }
    }
    assert_eq!(
        snapshot(fixture.directory.path()),
        before,
        "all source bytes, Git objects, refs and index remain unchanged"
    );
}

#[test]
fn candidate_exclusions_survive_all_three_interfaces_without_becoming_admission() {
    let fixture = Fixture::new(&[("one/private.txt", b"EXCLUDED MCP ASSET")]);
    let before = snapshot(fixture.directory.path());
    let mut recipe = request(&fixture.config(), &SITES);
    recipe["selection"] = json!({"content":{"collection:home":"include","content:one":"exclude","content:other":"include"},"assets":{"one/private.txt":"exclude"}});
    let (expected, failed) = sdk(&recipe, Default::default());
    assert!(!failed);
    let actual = call(arguments(&recipe), false);
    assert_eq!(actual, expected);
    assert_eq!(actual, cli_report(&recipe, &[], false));
    assert_eq!(actual["selection"], recipe["selection"]);
    assert_eq!(actual["status"], "local_set_rendered");
    assert!(
        actual["sites"]["site:one"]["evidence"]["pages"]
            .get("content:one")
            .is_none()
    );
    let inventory = actual["sites"]["site:one"]["inventory"]["entries"]
        .as_array()
        .expect("selected inventory");
    assert!(
        !inventory
            .iter()
            .any(|entry| entry["path"] == "guide/index.html" || entry["path"] == "private.txt")
    );
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn wrong_pin_or_missing_independent_theme_keeps_successful_sibling_and_failed_membership() {
    for missing in [false, true] {
        let fixture = Fixture::new(&[]);
        let other = Fixture::new(&[(
            "theme/theme.toml",
            b"package='theme:other'\nnamespace='other'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n",
        )]);
        let mut config = independent_theme_config(&fixture, &other);
        if missing {
            fs::rename(
                other.directory.path().join("theme"),
                other.directory.path().join("parked-theme"),
            )
            .expect("disposable missing source");
        } else {
            config = config.replace(
                &format!("commit='{}'", other.commit),
                "commit='1111111111111111111111111111111111111111'",
            );
        }
        let before = (
            snapshot(fixture.directory.path()),
            snapshot(other.directory.path()),
        );
        let recipe = request(&config, &SITES);
        let (expected, failed) = sdk(&recipe, Default::default());
        assert!(failed);
        let actual = call(arguments(&recipe), true);
        assert_eq!(actual, expected);
        assert_eq!(actual, cli_report(&recipe, &[], true));
        assert_eq!(
            actual["sites"]
                .as_object()
                .expect("complete membership")
                .len(),
            2
        );
        assert_eq!(actual["sites"]["site:one"]["status"], "rendered");
        assert_eq!(actual["sites"]["site:other"]["status"], "failed");
        assert!(actual["sites"]["site:other"].get("inventory").is_none());
        assert!(actual["source_failures"]["source:other-theme"].is_object());
        assert_eq!(
            (
                snapshot(fixture.directory.path()),
                snapshot(other.directory.path())
            ),
            before
        );
    }
}

#[test]
fn required_shared_source_failure_and_denied_or_unknown_effects_never_return_a_subset() {
    let fixture = Fixture::new(&[]);
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed required content",
    )
    .expect("disposable dirty source");
    let before = snapshot(fixture.directory.path());
    let recipe = request(&fixture.config(), &SITES);
    let error = build::acquire_publications(
        &fixture.config(),
        &SITES.map(super::key),
        Default::default(),
        Default::default(),
    )
    .expect_err("required source failure");
    let actual = call(arguments(&recipe), true);
    assert_eq!(actual["code"], error.code());
    assert!(actual.get("sites").is_none());
    let response = cli(&recipe.to_string(), &["render-sites", "--local"]);
    assert!(!response.status.success());
    assert!(response.stdout.is_empty());
    assert!(String::from_utf8_lossy(&response.stderr).contains(error.code()));
    for field in ["allow_read", "local"] {
        let mut denied = arguments(&recipe);
        denied[field] = json!(false);
        assert_eq!(call(denied, true)["code"], "publication_permission");
    }
    let mut unknown = arguments(&recipe);
    unknown["allow_network"] = json!(true);
    let responses = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"render_sites","arguments":unknown}}),
    ]);
    assert_eq!(responses[0]["error"]["code"], -32602);
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn exact_aggregate_output_budget_and_input_source_ceilings_are_enforced() {
    let fixture = Fixture::new(&[]);
    let before = snapshot(fixture.directory.path());
    let recipe = request(&fixture.config(), &SITES);
    let (baseline, failed) = sdk(&recipe, Default::default());
    assert!(!failed);
    let total = baseline["sites"]
        .as_object()
        .expect("sites")
        .values()
        .map(|site| {
            site["inventory"]["total_bytes"]
                .as_u64()
                .expect("native byte total")
        })
        .sum::<u64>();
    let mut exact = arguments(&recipe);
    exact["max_total_output_bytes"] = json!(total);
    assert_eq!(call(exact, false), baseline);
    let (bounded, failed) = sdk(
        &recipe,
        build::PublicationSetRenderLimits {
            bytes: total - 1,
            ..Default::default()
        },
    );
    assert!(failed);
    let mut limited = arguments(&recipe);
    limited["max_total_output_bytes"] = json!(total - 1);
    assert_eq!(call(limited, true), bounded);
    assert_eq!(
        cli_report(
            &recipe,
            &["--max-total-output-bytes", &(total - 1).to_string()],
            true
        ),
        bounded
    );
    assert_eq!(bounded["sites"]["site:one"], baseline["sites"]["site:one"]);
    assert_eq!(bounded["sites"]["site:other"]["status"], "failed");
    let mut input_limit = arguments(&recipe);
    input_limit["max_input_bytes"] = json!(1);
    assert_eq!(call(input_limit, true)["code"], "publication_set_input");
    let mut limits = build::PublicationSetAcquisitionLimits::default();
    limits.sources.sources = 2;
    let mut source_limit = arguments(&recipe);
    source_limit["acquisition"] = json!(limits);
    let actual = call(source_limit, true);
    assert!(actual.get("sites").is_none());
    let error = build::acquire_publications(
        &fixture.config(),
        &SITES.map(super::key),
        Default::default(),
        limits,
    )
    .expect_err("shared union exceeds two sources");
    assert_eq!(actual["code"], error.code());
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn oversized_native_report_is_an_explicit_error_not_a_successful_prefix() {
    let fixture = Fixture::new(&[]);
    let before = snapshot(fixture.directory.path());
    let config = fixture
        .config()
        .replace("Explicit bindings", &"A".repeat(300_000));
    let recipe = request(&config, &SITES[..1]);
    let native =
        build::LocalPublicationRecipe::parse_json(recipe.to_string().as_bytes(), 1 << 20, 1)
            .expect("bounded native recipe")
            .resolve(gazette_sdk::toml::Limits {
                source_bytes: 1 << 20,
                ..Default::default()
            })
            .expect("explicit invocation resolved against the exact workspace");
    let inputs = build::acquire_publications(
        &config,
        &native.sites().cloned().collect::<Vec<_>>(),
        gazette_sdk::toml::Limits {
            source_bytes: 1 << 20,
            ..Default::default()
        },
        Default::default(),
    )
    .expect("actual configured source acquisition");
    let rendered = build::render_publications(
        &inputs,
        native.options(Default::default()),
        Default::default(),
    )
    .expect("actual complete native render");
    assert!(rendered.is_complete());
    assert!(
        serde_json::to_vec(&rendered.inspect())
            .expect("native report")
            .len()
            > 256 * 1024
    );
    let mut arguments = arguments(&recipe);
    arguments["max_input_bytes"] = json!(1 << 20);
    arguments["syntax"] = json!({"source_bytes":1 << 20});
    let responses = carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"render_sites","arguments":arguments}}),
    ]);
    let result = &responses[1]["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(
        result["structuredContent"],
        json!({"code":"mcp_output_limit","message":"Complete publication inspection exceeds response capacity"})
    );
    let tool = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|t| t["name"] == "render_sites")
        .expect("native tool");
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("schema")
        .validate(&result["structuredContent"])
        .expect("declared failure alternative");
    assert!(result["structuredContent"].get("sites").is_none());
    assert_eq!(snapshot(fixture.directory.path()), before);
}
