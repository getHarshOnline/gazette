//! MCP projects actual captured composition phases without conflating their evidence.

use super::{
    Fixture, PLUGIN_MANIFEST, Value, acquire, build, json, rejected, render, run, source, toml,
};

const TOOLS: [(&str, &[&str]); 3] = [
    ("inspect_composition", &[]),
    ("check_composition", &["--check-composition"]),
    ("compile_templates", &["--compile-templates"]),
];

#[test]
fn initialized_session_refuses_recursive_include_then_compiles_healthy_templates() {
    let recursive = Fixture::new(&[(
        "presentation/templates/page.html",
        b"{% include 'base/page.html' %}",
    )]);
    let healthy = Fixture::new(&[]);
    let before = (recursive.snapshot(), healthy.snapshot());
    super::mcp_carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let request = |id, config: &str| {
            json!({
                "jsonrpc":"2.0", "id":id, "method":"tools/call",
                "params":{"name":"compile_templates", "arguments":arguments(config)}
            })
        };
        let failed = send(request(1, &recursive.config()));
        assert!(failed.get("error").is_none(), "{failed}");
        assert_eq!(failed["result"]["isError"], true, "{failed}");
        let error = failed["result"]["structuredContent"].to_string();
        assert!(error.contains("render_template"), "{error}");
        assert!(error.contains("recursive template include"), "{error}");
        assert!(error.contains("page.html"), "{error}");
        // A supported current request on the same initialized process proves
        // refusal did not terminate the server; no obsolete ping is used.
        let succeeded = send(request(2, &healthy.config()));
        assert!(succeeded.get("error").is_none(), "{succeeded}");
        assert_eq!(succeeded["result"]["isError"], false, "{succeeded}");
        let config = healthy.config();
        let acquired = acquire(&config);
        let metadata = acquired
            .check_metadata(toml::Limits::default())
            .expect("healthy metadata");
        let compiled = build::compile_templates(&metadata, render::Limits::default())
            .expect("healthy native templates");
        assert_eq!(
            succeeded["result"]["structuredContent"],
            json!(compiled.evidence())
        );
    });
    assert_eq!((recursive.snapshot(), healthy.snapshot()), before);
}

fn arguments(config: &str) -> Value {
    json!({"source":config,"site":"site:one","allow_read":true})
}

fn call(tool: &str, arguments: Value, failed: bool) -> Value {
    let responses = super::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|entry| entry["name"] == tool)
        .expect("named tool");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&arguments)
        .expect("actual native input matches discovery");
    let response = &responses[1];
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], failed, "{response}");
    let value = &response["result"]["structuredContent"];
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(value)
        .expect("native phase evidence matches discovery");
    assert_eq!(
        serde_json::from_str::<Value>(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("readable JSON")
        )
        .expect("complete JSON"),
        *value
    );
    value.clone()
}

fn cli(config: &str, flags: &[&str], failed: bool) -> Option<Value> {
    let mut args = vec!["--acquire-composition", "site:one"];
    args.extend_from_slice(flags);
    let output = run(config, &args);
    assert_eq!(
        !output.status.success(),
        failed,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if failed {
        assert!(output.stdout.is_empty(), "no successful prefix");
        None
    } else {
        Some(serde_json::from_slice(&output.stdout).expect("native CLI result"))
    }
}

#[test]
fn all_three_composition_phases_match_cli_sdk_and_leave_native_inputs_unchanged() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let before = fixture.snapshot();
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("native metadata");
    let compiled =
        build::compile_templates(&metadata, render::Limits::default()).expect("native Tera");
    let values = [
        json!(acquired.inspect()),
        json!(metadata.inspect()),
        json!(compiled.evidence()),
    ];
    for ((tool, flags), expected) in TOOLS.into_iter().zip(values) {
        let actual = call(tool, arguments(&config), false);
        assert_eq!(actual, expected);
        assert_eq!(Some(actual), cli(&config, flags, false));
    }
    // The shared fixture has invalid template-like JS and an invalid neighboring
    // directory: only the selected template subtree is parsed, and no JS executes.
    assert_ne!(
        compiled.evidence().template_tree,
        compiled.evidence().packages[0].source_tree
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn invalid_unused_template_stops_compilation_but_not_prior_phases() {
    let fixture = Fixture::new(&[("presentation/templates/unused.txt", b"{% if %}")]);
    let config = fixture.config();
    let before = fixture.snapshot();
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("valid declarations");
    let error = rejected(build::compile_templates(&metadata, Default::default()));
    for (tool, flags) in TOOLS {
        let failed = tool == "compile_templates";
        let actual = call(tool, arguments(&config), failed);
        if failed {
            assert_eq!(actual["code"], error.code());
            assert!(
                actual["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("template 'base/unused.txt'")),
                "{actual}"
            );
            assert!(
                actual["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("line 1, column")),
                "{actual}"
            );
        } else {
            assert_eq!(Some(actual), cli(&config, flags, false));
        }
        if failed {
            cli(&config, flags, true);
        }
    }
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn missing_required_metadata_inputs_fail_even_for_optional_capabilities() {
    let manifest = PLUGIN_MANIFEST.replace("inputs=['search.js']", "inputs=['absent.js']");
    let fixture = Fixture::new(&[("meta/plugin.toml", manifest.as_bytes())]);
    let config = fixture.config();
    let before = fixture.snapshot();
    let acquired = acquire(&config);
    let error = acquired
        .check_metadata(Default::default())
        .expect_err("required declared file");
    assert_eq!(
        call("inspect_composition", arguments(&config), false),
        json!(acquired.inspect())
    );
    for (tool, flags) in &TOOLS[1..] {
        let actual = call(tool, arguments(&config), true);
        assert_eq!(actual["code"], error.code());
        assert!(actual.get("manifests").is_none() && actual.get("tool").is_none());
        cli(&config, flags, true);
    }
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn permissions_stale_pins_and_each_phase_bound_preserve_complete_native_failures() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let before = fixture.snapshot();
    let stale = config.replace(
        &fixture.commit.to_string(),
        "1111111111111111111111111111111111111111",
    );
    let native = build::acquire_composition(
        &stale,
        &"site:one".parse().expect("site"),
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect_err("unavailable exact pin");
    for (tool, _) in TOOLS {
        let mut denied = arguments(&stale);
        denied["allow_read"] = json!(false);
        let refusal = call(tool, denied, true);
        assert_ne!(
            refusal["code"],
            native.code(),
            "permission must precede acquisition"
        );
        assert_eq!(call(tool, arguments(&stale), true)["code"], native.code());
        let mut bounded = arguments(&config);
        let mut limits = build::PublicationSetAcquisitionLimits::default().sources;
        limits.sources = 0;
        bounded["acquisition"] = json!(limits);
        let error = build::acquire_composition(
            &config,
            &"site:one".parse().expect("site"),
            Default::default(),
            limits,
        )
        .expect_err("zero selected source budget");
        assert_eq!(call(tool, bounded, true)["code"], error.code());
        let mut bounded = arguments(&config);
        bounded["syntax"] = json!({"source_bytes":0});
        let error = build::acquire_composition(
            &config,
            &"site:one".parse().expect("site"),
            toml::Limits {
                source_bytes: 0,
                ..Default::default()
            },
            build::PublicationSetAcquisitionLimits::default().sources,
        )
        .expect_err("zero source bytes");
        assert_eq!(call(tool, bounded, true)["code"], error.code());
    }
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("metadata");
    let mut limits = render::Limits::default();
    limits.templates.total_bytes = source::inspect(
        &fixture.root.path().join("selected/presentation/templates"),
        None,
        Default::default(),
    )
    .expect("independent selected subtree")
    .total_bytes();
    let mut exact = arguments(&config);
    exact["templates"] = json!(limits);
    assert_eq!(
        call("compile_templates", exact, false),
        json!(
            build::compile_templates(&metadata, limits)
                .expect("exact byte budget")
                .evidence()
        )
    );
    limits.templates.total_bytes -= 1;
    let mut bounded = arguments(&config);
    bounded["templates"] = json!(limits);
    let error = rejected(build::compile_templates(&metadata, limits));
    assert_eq!(
        call("compile_templates", bounded, true)["code"],
        error.code()
    );
    assert_eq!(fixture.snapshot(), before);
}
