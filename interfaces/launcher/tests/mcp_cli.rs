//! The single Gazette executable launches the real SDK-backed MCP transport.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use gazette_cli::command::metadata;
use gazette_sdk::{configuration, source, toml, workspace};
use serde_json::{Value, json};
use tempfile::tempdir;

const SOURCE: &str = "# 🦀\r\n[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

#[path = "support/mcp.rs"]
mod carrier;
use carrier::{Process, exchange};

#[path = "mcp_cli/store_lifecycle.rs"]
mod store_lifecycle;

#[test]
fn direct_mcp_lists_only_real_tools_and_returns_the_same_workspace_and_origin_as_sdk() {
    let fixture = tempdir().expect("modular workspace root");
    fs::create_dir(fixture.path().join("sites")).expect("site directory");
    fs::write(
        fixture.path().join("gazette.toml"),
        "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='organization'\n",
    )
    .expect("workspace root");
    fs::write(
        fixture.path().join("sites/one.toml"),
        "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
    )
    .expect("workspace module");
    fs::write(fixture.path().join("unselected.toml"), b"\xff").expect("invalid unselected sibling");
    let query = json!({"kind":"site","identity":"site:one","field":["publisher"]});
    let responses = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check_workspace","arguments":{"source":SOURCE}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workspace_field_origin","arguments":{"source":SOURCE,"query":query}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"check_workspace","arguments":{"source":"sites=[]"}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"load_workspace","arguments":{"allow_native_reads":true,"root":fixture.path(),"workspace":"gazette.toml"}}}),
    ]);
    let names: Vec<_> = responses[0]["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name"))
        .collect();
    assert!(!names.is_empty(), "native MCP catalog must not be empty");
    assert_eq!(
        names.iter().copied().collect::<BTreeSet<_>>().len(),
        names.len(),
        "the native catalog page must contain unique names"
    );
    let mut expected = metadata::operations().iter().collect::<Vec<_>>();
    expected.sort_by_key(|operation| operation.discovery_key());
    assert_eq!(
        names,
        expected
            .into_iter()
            .take(names.len())
            .map(|operation| operation.name)
            .collect::<Vec<_>>(),
        "the executable catalog must preserve canonical progressive discovery order"
    );
    for tool in responses[0]["result"]["tools"].as_array().expect("catalog") {
        assert!(tool["inputSchema"].is_object(), "{tool}");
        assert!(tool["outputSchema"].is_object(), "{tool}");
        assert_eq!(tool["outputSchema"]["type"], "object", "{}", tool["name"]);
    }
    let parsed = workspace::ParsedWorkspace::parse(SOURCE, toml::Limits::default()).expect("SDK");
    assert_eq!(
        responses[1]["result"]["structuredContent"],
        json!(parsed.compilation())
    );
    let query = serde_json::from_value(query).expect("SDK query");
    assert_eq!(
        responses[2]["result"]["structuredContent"],
        json!(parsed.origin(&query).expect("origin"))
    );
    let failure = workspace::compile("sites=[]", toml::Limits::default())
        .expect_err("complete invalid workspace");
    assert_eq!(responses[3]["result"]["isError"], true);
    assert_eq!(
        responses[3]["result"]["structuredContent"],
        json!(workspace::Failure::from(&failure))
    );
    let path: source::EntryPath = "gazette.toml".parse().expect("entry path");
    let loaded = configuration::load_workspace(
        fixture.path(),
        &path,
        configuration::WorkspaceLoadLimits::default(),
    )
    .expect("same SDK operation");
    assert_eq!(responses[4]["result"]["structuredContent"], json!(loaded));
}

#[test]
fn direct_cli_and_mcp_catalogs_obey_the_explicit_interface_coverage_law() {
    let help = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("--help")
        .output()
        .expect("Gazette help");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("UTF-8 help");
    let commands: BTreeSet<_> = help
        .split_once("Commands:\n")
        .expect("command section")
        .1
        .split_once("\nOptions:")
        .expect("option section")
        .0
        .lines()
        .filter_map(|line| {
            let entry = line.strip_prefix("  ")?;
            (!entry.starts_with(char::is_whitespace))
                .then(|| entry.split_whitespace().next().expect("command name"))
        })
        .collect();

    // The canonical interface registry owns every CLI-to-operation projection.
    // This executable test compares that one source of truth to both live surfaces
    // instead of maintaining a second hand-written coverage table.
    let coverage: BTreeMap<_, BTreeSet<_>> = metadata::cli_families()
        .iter()
        .map(|family| (family.command, family.operations().collect()))
        .collect();

    let application_commands: BTreeSet<_> = commands
        .difference(&BTreeSet::from(["help", "lsp", "mcp", "config"]))
        .copied()
        .collect();
    assert_eq!(
        coverage.keys().copied().collect::<BTreeSet<_>>(),
        application_commands
    );

    let listed = carrier::interact(|send| {
        let mut tools = Vec::new();
        let mut params = json!({});
        let mut cursors = BTreeSet::new();
        for id in 1..=32 {
            let response =
                send(json!({"jsonrpc":"2.0","id":id,"method":"tools/list","params":params}));
            tools.extend(
                response["result"]["tools"]
                    .as_array()
                    .expect("live catalog page")
                    .iter()
                    .cloned(),
            );
            let Some(cursor) = response["result"]["nextCursor"].as_str() else {
                return tools;
            };
            assert!(cursors.insert(cursor.to_owned()), "catalog cursor repeated");
            params = json!({"cursor":cursor});
        }
        panic!("catalog did not finish within 32 pages");
    });
    for tool in &listed {
        assert_eq!(tool["outputSchema"]["type"], "object", "{}", tool["name"]);
    }
    let tools: BTreeSet<_> = listed
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        tools.len(),
        listed.len(),
        "catalog names must not repeat across pages"
    );
    let projected: BTreeSet<_> = coverage
        .values()
        .flat_map(|names| names.iter().copied())
        .collect();
    assert!(
        projected.is_subset(&tools),
        "CLI operations missing from MCP: {:?}",
        projected.difference(&tools).collect::<Vec<_>>()
    );
    assert!(
        tools
            .difference(&projected)
            .all(|name| name.starts_with("authoring_")),
        "only editor authoring operations may be MCP-only"
    );
    assert!(tools.iter().any(|name| name.starts_with("authoring_")));
    assert!(
        !tools.contains("mcp") && !tools.contains("lsp"),
        "launchers are not recursive tools"
    );
}

#[test]
fn direct_mcp_verification_preserves_native_evidence_and_does_not_consume_the_input() {
    let authorization = include_str!(
        "../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    );
    let binding =
        include_str!("../../../crates/gazette-compiler/tests/release_authorization/binding.json");
    let responses = exchange(
        [authorization, "PRIVATE-malformed", authorization]
            .into_iter()
            .enumerate()
            .map(|(id, input)| {
                json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                    "name":"verify_release_authorization",
                    "arguments":{"authorization_json":input,"binding_json":binding}
                }})
            })
            .collect(),
    );
    let limits = gazette_sdk::release::Limits::default();
    let binding = gazette_sdk::release::parse_binding(binding.as_bytes(), limits)
        .expect("independently supplied fixture binding");
    let verified = gazette_sdk::release::verify(authorization.as_bytes(), &binding, limits)
        .expect("frozen real signature");
    for index in [0, 2] {
        let result = &responses[index]["result"];
        assert_eq!(result["isError"], false, "{result}");
        assert_eq!(result["structuredContent"], json!(verified.report()));
        let readable: Value = serde_json::from_str(
            result["content"][0]["text"]
                .as_str()
                .expect("readable JSON"),
        )
        .expect("complete report");
        assert_eq!(readable, result["structuredContent"]);
    }
    let failed = &responses[1]["result"];
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["structuredContent"]["code"], "release_json");
    assert!(
        failed["structuredContent"]
            .get("authorization_sha256")
            .is_none()
    );
    assert!(!responses[1].to_string().contains("PRIVATE"));
}

#[test]
fn direct_help_exposes_one_mcp_command_with_explicit_native_reads() {
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["mcp", "--help"])
        .output()
        .expect("help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("gazette mcp"));
    assert!(help.contains("no publication"));
    assert!(help.contains("explicit native-read permissions"));
    assert!(help.contains("discoverable SDK and authoring tools"));
}

#[test]
fn failed_protocol_output_is_not_reported_as_clean_completion() {
    let mut child = Process(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(std::fs::File::open("/dev/null").expect("read-only output descriptor"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("MCP executable"),
    );
    let mut input = child.0.stdin.take().expect("stdin");
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}
        }}})
    )
    .expect("request");
    input.flush().expect("flush request");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("process status") {
            assert!(!status.success(), "lost response must fail");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "failed output did not stop service"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(input);
}
