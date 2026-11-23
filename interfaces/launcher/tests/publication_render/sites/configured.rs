//! Configured build intent reaches real CLI and initialized MCP without becoming approval.

use std::collections::BTreeMap;

use crate::mcp_carrier;

use super::{Fixture, Value, build, cli, json};

type OutputBytes = BTreeMap<String, BTreeMap<String, Vec<u8>>>;

pub(super) fn configuration(fixture: &Fixture, policy: &str) -> String {
    format!(
        "[build]\nsites=['site:other','site:one']\nlocal_drafts='{policy}'\n{}",
        fixture
            .config()
            .replace(
                "public_base_url='https://one.example/mount/'",
                "local_port=31511\npublic_base_url='https://one.example/mount/'",
            )
            .replace(
                "public_base_url='https://other.example/'",
                "local_port=31512\npublic_base_url='https://other.example/'",
            )
    )
}

pub(super) fn recipe(config: &str) -> Value {
    json!({
        "format":"gazette-local-publication-set/1",
        "workspace":{"kind":"supplied","source":config},
    })
}

fn arguments(recipe: &Value) -> Value {
    json!({"recipe_json":recipe.to_string(),"local":true,"allow_read":true})
}

fn initialized_calls(calls: &[(Value, bool)]) -> (Value, Vec<Value>) {
    mcp_carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let catalog = send(json!({
            "jsonrpc":"2.0","id":1,"method":"tools/list","params":{}
        }));
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("initialized discovery")
            .iter()
            .find(|tool| tool["name"] == "render_sites")
            .expect("native rendering descriptor")
            .clone();
        let results = calls
            .iter()
            .enumerate()
            .map(|(index, (arguments, failed))| {
                jsonschema::validator_for(&tool["inputSchema"])
                    .expect("named tool schema")
                    .validate(arguments)
                    .expect("actual named request");
                let response = send(json!({
                    "jsonrpc":"2.0","id":index+2,"method":"tools/call",
                    "params":{"name":"render_sites","arguments":arguments}
                }));
                assert!(response.get("error").is_none(), "{response}");
                assert_eq!(response["result"]["isError"], *failed, "{response}");
                let report = response["result"]["structuredContent"].clone();
                let text: Value = serde_json::from_str(
                    response["result"]["content"][0]["text"]
                        .as_str()
                        .expect("readable result"),
                )
                .expect("same structured JSON carrier");
                assert_eq!(text, report);
                if report.get("sites").is_some() {
                    jsonschema::validator_for(&tool["outputSchema"])
                        .expect("native output schema")
                        .validate(&report)
                        .expect("actual complete report schema");
                }
                report
            })
            .collect();
        (tool, results)
    })
}

fn cli_report(recipe: &Value, extra: &[&str], failed: bool) -> Value {
    let mut flags = vec!["render-sites", "--local"];
    flags.extend_from_slice(extra);
    let output = cli(&recipe.to_string(), &flags);
    assert_eq!(output.status.success(), !failed, "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("native complete CLI report")
}

fn expected(recipe: &Value) -> (Value, OutputBytes) {
    build::LocalPublicationRecipe::parse_json(recipe.to_string().as_bytes(), 1 << 20, 2)
        .expect("native strict recipe")
        .with_default_render_year(
            gazette_cli::calendar::resolve(None).expect("same boundary year as CLI and MCP"),
        )
        .with_rendered(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            |rendered| {
                assert!(rendered.is_complete());
                let outputs = rendered
                    .sites()
                    .iter()
                    .map(|(site, result)| {
                        let output = result.as_ref().expect("complete site").site().output();
                        let files = output
                            .inventory()
                            .entries()
                            .iter()
                            .filter_map(|entry| {
                                output
                                    .file(&entry.path)
                                    .map(|bytes| (entry.path.to_string(), bytes.to_vec()))
                            })
                            .collect();
                        (site.to_string(), files)
                    })
                    .collect();
                (json!(rendered.inspect()), outputs)
            },
        )
        .expect("actual native source acquisition and rendering")
}

fn selection(report: &Value, origin: &str, sites: &[&str], policy: &str) {
    assert_eq!(
        report["build_selection"],
        json!({"origin":origin,"sites":sites,"local_drafts":policy}),
    );
    assert_eq!(report["status"], "local_set_rendered");
    for site in report["sites"]
        .as_object()
        .expect("complete site map")
        .values()
    {
        let entries = site["inventory"]["entries"]
            .as_array()
            .expect("exact inventory");
        assert!(entries.iter().all(|entry| entry["path"] != "model.json"));
    }
}

#[test]
fn configured_defaults_preserve_order_drafts_bytes_and_addressing_over_cli_and_initialized_mcp() {
    let fixture = Fixture::new(&[]);
    let before = super::mcp::snapshot(fixture.directory.path());
    for policy in ["include", "exclude"] {
        for addressing in ["public", "local"] {
            let mut recipe = recipe(&configuration(&fixture, policy));
            recipe["addressing"] = json!(addressing);
            let (expected, outputs) = expected(&recipe);
            let (tool, actual) = initialized_calls(&[(arguments(&recipe), false)]);
            let schema = &tool["inputSchema"]["properties"]["recipe_json"]["contentSchema"];
            assert!(
                !schema["required"]
                    .as_array()
                    .expect("named required fields")
                    .iter()
                    .any(|field| field == "sites"),
                "omitted sites remains a discoverable supported intent"
            );
            jsonschema::validator_for(schema)
                .expect("recipe schema")
                .validate(&recipe)
                .expect("configured intent passes discovery");
            assert_eq!(actual[0], expected);
            assert_eq!(cli_report(&recipe, &[], false), expected);
            selection(&actual[0], "workspace", &["site:other", "site:one"], policy);
            assert_eq!(
                outputs["site:one"].contains_key("guide/index.html"),
                policy == "include"
            );
            assert!(!outputs["site:one"].contains_key("model.json"));
            let other = std::str::from_utf8(&outputs["site:other"]["elsewhere/index.html"])
                .expect("real Theme HTML bytes");
            let url = if addressing == "local" {
                "http://127.0.0.1:31512/elsewhere/"
            } else {
                "https://other.example/elsewhere/"
            };
            let attributes =
                crate::browser_carrier::values(other, gazette_sdk::html::TextKind::Attribute);
            assert!(
                attributes.iter().any(|value| value == url),
                "exact decoded canonical link target {url}: {other}"
            );
            if addressing == "local" {
                assert_eq!(
                    actual[0]["preview_urls"]["site:one"],
                    "http://127.0.0.1:31511/mount/"
                );
            } else {
                assert!(actual[0]["preview_urls"].is_null());
            }
        }
    }
    assert_eq!(super::mcp::snapshot(fixture.directory.path()), before);
}

#[test]
fn explicit_site_options_override_defaults_without_relabelling_the_workspace_policy() {
    let fixture = Fixture::new(&[]);
    let mut recipe = recipe(&configuration(&fixture, "exclude"));
    recipe["sites"] = json!([{"site":"site:one"}]);
    let (expected, outputs) = expected(&recipe);
    let (_, actual) = initialized_calls(&[(arguments(&recipe), false)]);
    assert_eq!(actual[0], expected);
    assert_eq!(cli_report(&recipe, &[], false), expected);
    selection(&actual[0], "invocation", &["site:one"], "exclude");
    assert_eq!(actual[0]["sites"].as_object().unwrap().len(), 1);
    assert_eq!(outputs.len(), 1);
    assert!(!outputs["site:one"].contains_key("guide/index.html"));
    assert!(outputs["site:one"].contains_key("index.html"));
}

#[test]
fn omitted_defaults_and_explicit_null_or_empty_selection_refuse_without_source_effects() {
    let fixture = Fixture::new(&[]);
    let before = super::mcp::snapshot(fixture.directory.path());
    let missing_defaults = recipe(&fixture.config());
    let configured = recipe(&configuration(&fixture, "include"));
    let mut calls = vec![(arguments(&missing_defaults), true)];
    for value in [Value::Null, json!([])] {
        let mut invalid = configured.clone();
        invalid["sites"] = value;
        assert!(
            build::LocalPublicationRecipe::parse_json(invalid.to_string().as_bytes(), 1 << 20, 2)
                .is_err()
        );
        let output = cli(&invalid.to_string(), &["render-sites", "--local"]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("publication_set_input"));
        calls.push((arguments(&invalid), true));
    }
    let mut limited = arguments(&configured);
    limited["acquisition"] = json!({"sites":1});
    calls.push((limited, true));
    let (_, actual) = initialized_calls(&calls);
    assert_eq!(actual[0]["code"], "publication_set_selection");
    assert_eq!(actual[1]["code"], "publication_set_input");
    assert_eq!(actual[2]["code"], "publication_set_input");
    assert_eq!(actual[3]["code"], "publication_set_selection");
    let output = cli(&missing_defaults.to_string(), &["render-sites", "--local"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("publication_set_selection"));
    assert_eq!(super::mcp::snapshot(fixture.directory.path()), before);
}

#[test]
fn configured_output_ceiling_keeps_complete_selection_and_permission_precedes_workspace_reads() {
    let fixture = Fixture::new(&[]);
    let before = super::mcp::snapshot(fixture.directory.path());
    let recipe = recipe(&configuration(&fixture, "include"));
    let mut limited = arguments(&recipe);
    limited["max_total_output_bytes"] = json!(1);
    let mut denied_recipe = recipe.clone();
    denied_recipe["workspace"] = json!({
        "kind":"local","root":fixture.directory.path().join("never-opened"),
        "workspace":"gazette.toml"
    });
    let mut denied = arguments(&denied_recipe);
    denied["allow_read"] = json!(false);
    let (_, actual) = initialized_calls(&[(limited, true), (denied, true)]);
    assert_eq!(
        actual[0],
        cli_report(&recipe, &["--max-total-output-bytes", "1"], true)
    );
    assert_eq!(
        actual[0]["build_selection"],
        json!({
            "origin":"workspace","sites":["site:other","site:one"],"local_drafts":"include"
        })
    );
    assert_eq!(actual[0]["sites"].as_object().unwrap().len(), 2);
    assert_eq!(actual[0]["status"], "local_set_incomplete");
    assert_eq!(actual[1]["code"], "publication_permission");
    assert!(!fixture.directory.path().join("never-opened").exists());
    assert_eq!(super::mcp::snapshot(fixture.directory.path()), before);
}

#[test]
fn preview_plan_preserves_unresolved_defaults_and_execution_refuses_wrong_listener_set_before_sources()
 {
    let fixture = Fixture::new(&[]);
    let recipe = recipe(&configuration(&fixture, "include"));
    drop(fixture);
    let plan = cli(
        &recipe.to_string(),
        &[
            "preview-sites",
            "--local",
            "--allow-listeners",
            "--plan",
            "--listen",
            "site:one=127.0.0.1:0",
        ],
    );
    assert!(plan.status.success(), "{plan:?}");
    let evidence: Value = serde_json::from_slice(&plan.stdout).expect("pure plan");
    assert!(
        evidence.get("sites").is_none(),
        "listeners cannot impersonate unresolved build selection"
    );
    assert_eq!(
        evidence["requested_listeners"].as_object().unwrap().len(),
        1
    );
    let output = cli(
        &recipe.to_string(),
        &[
            "preview-sites",
            "--local",
            "--allow-listeners",
            "--listen",
            "site:one=127.0.0.1:0",
        ],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("listener"), "{diagnostic}");
    assert!(
        !diagnostic.contains("repository"),
        "listener mismatch must precede opening deleted sources: {diagnostic}"
    );
}

#[test]
fn workspace_corpus_limit_reaches_cli_mcp_single_site_and_reports_shared_bytes() {
    let fixture = Fixture::new(&[]);
    let config = configuration(&fixture, "include").replace(
        "local_drafts='include'",
        "local_drafts='include'\ncorpus_bytes=65536\nmath=false",
    );
    let request = recipe(&config);
    let expected = cli_report(&request, &[], false);
    let denied = cli(
        &request.to_string(),
        &["render-sites", "--local", "--max-corpus-bytes", "1"],
    );
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("corpus_limit"));
    let mut partial = arguments(&request);
    partial["corpus"] = json!({"files":100});
    let mut small = arguments(&request);
    small["corpus"] = json!({"source_bytes":1});
    let native =
        json!({"workspace":{"kind":"supplied","source":config},"local":true,"allow_read":true});
    let (_, reports) = initialized_calls(&[(partial, false), (small, true), (native, false)]);
    assert_eq!(reports[0], expected);
    assert_eq!(reports[2], expected);
    assert_eq!(reports[1]["code"], "corpus_limit");
    let bytes = &expected["source_bytes"];
    assert!(bytes["shared_content"].as_u64().unwrap() > 0);
    assert_eq!(
        bytes["captured_total"],
        bytes["sites"]["site:one"]["captured"]
    );
    assert_eq!(bytes["sites"]["site:one"], bytes["sites"]["site:other"]);
    assert_eq!(bytes["sites"]["site:one"]["missing_sources"], json!([]));
    assert_eq!(
        bytes["sites"]["site:one"]["captured"].as_u64().unwrap(),
        bytes["shared_content"].as_u64().unwrap()
            + bytes["sites"]["site:one"]["presentation"].as_u64().unwrap()
    );

    let denied = cli(
        &config.replace("corpus_bytes=65536", "corpus_bytes=1"),
        &["render-site", "--local", "--site", "site:one"],
    );
    assert!(!denied.status.success());
    let allowed = cli(
        &config.replace("corpus_bytes=65536", "corpus_bytes=1"),
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--max-corpus-bytes",
            "65536",
        ],
    );
    assert!(allowed.status.success(), "{allowed:?}");

    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("gazette.toml"), &config).unwrap();
    let native_cli = cli(
        "",
        &[
            "render-sites",
            "--local",
            "--workspace-root",
            root.path().to_str().unwrap(),
        ],
    );
    assert!(native_cli.status.success(), "{native_cli:?}");
    let native_report: Value = serde_json::from_slice(&native_cli.stdout).unwrap();
    assert_eq!(native_report["source_bytes"], expected["source_bytes"]);
}

#[test]
fn configured_preview_limits_reach_pinned_and_watch_capture() {
    let fixture = Fixture::new(&[]);
    let config = configuration(&fixture, "include").replace(
        "local_drafts='include'",
        "local_drafts='include'\ncorpus_bytes=1",
    );
    let request = recipe(&config);
    for extra in [vec![], vec!["--watch"]] {
        let mut flags = vec![
            "preview-sites",
            "--local",
            "--allow-listeners",
            "--listen",
            "site:one=127.0.0.1:0",
            "--listen",
            "site:other=127.0.0.1:0",
        ];
        flags.extend(extra.iter().copied());
        let denied = cli(&request.to_string(), &flags);
        assert!(!denied.status.success(), "{denied:?}");
        let mut allowed = extra;
        allowed.extend(["--max-corpus-bytes", "65536"]);
        let mut process = super::preview::Process::with_arguments(&request, &allowed);
        let ready = process.event();
        assert_eq!(ready["event"], "ready", "{ready}");
        process.close();
    }
}

#[test]
fn partial_capture_reports_retained_sizes_and_missing_sources_without_rereading() {
    let fixture = Fixture::new(&[]);
    let other = Fixture::new(&[("theme/theme.toml", b"package='theme:other'\nnamespace='other'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n")]);
    let config = super::independent_theme_config(&fixture, &other);
    std::fs::rename(
        other.directory.path().join("theme"),
        other.directory.path().join("held-theme"),
    )
    .unwrap();
    let inputs = super::acquire_set(&config, &super::SITES);
    let captured: u64 = inputs
        .sources()
        .values()
        .map(|source| source.inventory().total_bytes())
        .sum();
    let content: u64 = ["source:one", "source:other"]
        .iter()
        .map(|key| inputs.sources()[&super::key(key)].inventory().total_bytes())
        .sum();
    std::fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed after capture",
    )
    .unwrap();
    let rendered = build::render_publications(
        &inputs,
        super::options(
            &super::SITES,
            &"page.html".parse().unwrap(),
            &"alias.html".parse().unwrap(),
            &"model.json".parse().unwrap(),
        ),
        Default::default(),
    )
    .unwrap();
    assert!(!rendered.is_complete());
    let report = json!(rendered.inspect());
    assert_eq!(report["source_bytes"]["captured_total"], captured);
    assert_eq!(report["source_bytes"]["shared_content"], content);
    assert_eq!(
        report["source_bytes"]["sites"]["site:other"]["captured"],
        content
    );
    assert_eq!(
        report["source_bytes"]["sites"]["site:other"]["missing_sources"],
        json!(["source:other-theme"])
    );
    assert_eq!(
        report["source_bytes"]["sites"]["site:one"]["missing_sources"],
        json!([])
    );
}
