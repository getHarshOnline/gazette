//! Executable MCP rendering submodes preserve the native producer and retained output.

use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn publication(config: &str) -> Value {
    json!({"recipe_json":json!({"format":"gazette-local-publication-set/1","render_year":2040,"workspace":{"kind":"supplied","source":config},"sites":[{"site":"site:one","model_output":"model.data"}]}).to_string(),"local":true,"allow_read":true})
}
fn call(name: &str, args: Value, failed: bool) -> Value {
    let responses = mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}}),
    ]);
    let tool = responses[0]["result"]["tools"]
        .as_array()
        .expect("discovery")
        .iter()
        .find(|t| t["name"] == name)
        .expect("implemented tool");
    jsonschema::validator_for(&tool["inputSchema"])
        .expect("schema")
        .validate(&args)
        .expect("actual input");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let value = response["result"]["structuredContent"].clone();
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(&value)
        .expect("native wire evidence");
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("JSON");
    assert_eq!(text, value);
    value
}
fn snapshot(path: &std::path::Path) -> source::TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("exact native tree")
        .identity()
}

#[test]
fn stage_rendered_site_matches_cli_retained_bytes_and_original_evidence() {
    let fixture = Fixture::new(&[("one/static.txt", &[0, 255, 13, 10])]);
    let before = snapshot(fixture.directory.path());
    let config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]",
    );
    let parent = tempdir().expect("MCP parent");
    let mcp = call(
        "stage_rendered_site",
        json!({"publication":publication(&config),"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024}),
        false,
    );
    let cli_parent = tempdir().expect("CLI parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[5] = "model.data";
    arguments.extend([
        "--output-parent",
        cli_parent.path().to_str().expect("path"),
        "--directory-bytes",
        "1024",
    ]);
    let response = cli(&config, &arguments);
    assert!(response.status.success(), "{response:?}");
    let native: Value = serde_json::from_slice(&response.stdout).expect("native CLI report");
    assert_eq!(mcp["inventory"], native["inventory"]);
    assert_eq!(mcp["evidence"], native["evidence"]);
    assert_eq!(mcp["inputs"], native["inputs"]);
    let mcp_path = std::path::Path::new(mcp["directory"].as_str().expect("MCP path"));
    let cli_path = std::path::Path::new(native["directory"].as_str().expect("CLI path"));
    assert_eq!(snapshot(mcp_path), snapshot(cli_path));
    assert_eq!(snapshot(fixture.directory.path()), before);
    let short = cli(
        &config,
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--model-output",
            "model.data",
            "--output-parent",
            cli_parent.path().to_str().expect("path"),
            "--directory-bytes",
            "1",
        ],
    );
    assert!(!short.status.success());
    assert_eq!(
        fs::read_dir(cli_parent.path())
            .expect("no extra stage")
            .count(),
        1
    );
}

#[test]
fn probe_rendered_site_matches_cli_and_uses_generated_media_not_imported_suffixes() {
    let fixture = Fixture::new(&[(
        "theme/templates/page.html",
        b"<p>Decoded&#32;Secret</p>{{ body_html | safe }}",
    )]);
    let config = fixture.config();
    let before = snapshot(fixture.directory.path());
    let rules = vec![
        json!({"id":"rule:private","literal":"Decoded Secret"}),
        json!({"id":"rule:title","literal":"One <escaped>"}),
    ];
    let observed = call(
        "probe_rendered_site",
        json!({"publication":publication(&config),"rules_json":json!({"format":"gazette-disclosure-output-input/1","rules":rules}).to_string()}),
        false,
    );
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[5] = "model.data";
    arguments.push("--probe-disclosure");
    let response = cli(
        &json!({"format":"gazette-disclosure-probe-input/1","workspace":config,"rules":rules})
            .to_string(),
        &arguments,
    );
    assert!(response.status.success(), "{response:?}");
    assert_eq!(
        observed,
        serde_json::from_slice::<Value>(&response.stdout).expect("CLI probe")
    );
    let inputs = acquire(&config);
    let rendered = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            model_output: Some(&"model.data".parse().expect("model")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("original native producer");
    let rules = BTreeMap::from([
        (
            "rule:private".parse().expect("rule"),
            "Decoded Secret".to_owned(),
        ),
        (
            "rule:title".parse().expect("rule"),
            "One <escaped>".to_owned(),
        ),
    ]);
    assert_eq!(
        observed,
        json!(
            build::probe_disclosure(
                rendered.site().output(),
                rendered.site().media_types(),
                &rules,
                Default::default()
            )
            .expect("SDK producer probe")
        )
    );
    assert_ne!(
        observed,
        json!(
            build::probe_disclosure(
                rendered.site().output(),
                &BTreeMap::new(),
                &rules,
                Default::default()
            )
            .expect("imported empty-map contrast")
        )
    );
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn rendered_markdown_disclosure_uses_shared_sdk_inspectors_in_cli_and_mcp() {
    let page = PAGE.replace("**Retained** body", "Folded **Secret**");
    let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let before = snapshot(fixture.directory.path());
    let config = fixture.config().replace(
        "site='site:one'\npublic_base_url=",
        "site='site:one'\nmachine={markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\npublic_base_url=",
    );
    let rules = vec![json!({"id":"rule:decoded","literal":"Folded Secret"})];
    let mut selected = publication(&config);
    selected["recipe_json"] = json!(
        selected["recipe_json"]
            .as_str()
            .unwrap()
            .replace("model.data", "model.json")
    );
    let mcp = call(
        "probe_rendered_site",
        json!({"publication":selected,"rules_json":json!({"format":"gazette-disclosure-output-input/1","rules":rules}).to_string()}),
        false,
    );
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.push("--probe-disclosure");
    let response = cli(
        &json!({"format":"gazette-disclosure-probe-input/1","workspace":config,"rules":rules})
            .to_string(),
        &arguments,
    );
    assert!(response.status.success(), "{response:?}");
    assert_eq!(
        mcp,
        serde_json::from_slice::<Value>(&response.stdout).unwrap()
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).unwrap();
    let rules = BTreeMap::from([("rule:decoded".parse().unwrap(), "Folded Secret".into())]);
    let expected = build::probe_disclosure(
        rendered.site().output(),
        rendered.site().media_types(),
        &rules,
        Default::default(),
    )
    .unwrap();
    assert_eq!(mcp, json!(expected));
    assert!(mcp["findings"].as_array().unwrap().iter().any(|finding| {
        finding["path"] == "guide/index.md" && finding["rule"] == "rule:decoded"
    }));
    assert_eq!(snapshot(fixture.directory.path()), before);
}

#[test]
fn rendered_tools_do_not_stage_after_read_selection_or_output_failure() {
    let fixture = Fixture::new(&[]);
    let parent = tempdir().expect("output parent");
    let mut args = json!({"publication":publication(&fixture.config()),"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024});
    args["publication"]["allow_read"] = json!(false);
    assert_eq!(
        call("stage_rendered_site", args.clone(), true)["code"],
        "publication_permission"
    );
    args["publication"]["allow_read"] = json!(true);
    args["publication"]["output"] = json!({"total_bytes":1});
    call("stage_rendered_site", args, true);
    assert_eq!(fs::read_dir(parent.path()).expect("no outputs").count(), 0);
}

#[test]
fn lost_mcp_stage_delivery_preserves_one_exact_native_retention_without_retry() {
    use std::time::{Duration, Instant};
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let before = snapshot(fixture.directory.path());
    let parent = tempdir().expect("private output parent");
    let args = json!({"publication":publication(&config),"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024});
    let mut child = mcp_carrier::Process(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(fs::File::open("/dev/null").expect("read-only receipt descriptor"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("real MCP executable"),
    );
    let mut input = child.0.stdin.take().expect("stdin");
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":args,"_meta":{
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{}
    }}})).expect("effect request");
    input.flush().expect("request flush");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("process status") {
            assert!(
                !status.success(),
                "lost effect receipt is not clean completion"
            );
            break;
        }
        assert!(Instant::now() < deadline, "failed delivery must terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(input);
    let paths = fs::read_dir(parent.path())
        .expect("original custody remains")
        .map(|entry| entry.expect("entry").path())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1, "no implicit retry or cleanup");
    let inputs = acquire(&config);
    let rendered = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            model_output: Some(&"model.data".parse().expect("model")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("independent original producer");
    let recovered = source::capture_pinned(
        &paths[0],
        rendered.site().output().inventory().identity(),
        Default::default(),
    )
    .expect("retained output is still exact after carrier loss");
    for entry in recovered.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                recovered.file(&entry.path),
                rendered.site().output().file(&entry.path)
            );
        }
    }
    assert_eq!(snapshot(fixture.directory.path()), before);
}
