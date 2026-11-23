//! Executable CLI-to-MCP coverage contract over the two native interfaces.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use serde_json::{Value, json};

use gazette_cli::command::metadata;

#[path = "support/mcp.rs"]
mod mcp;

#[path = "editor_context/compact.rs"]
mod compact_editor_context;

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_gazette")
}

fn help(arguments: &[&str]) -> String {
    let output = Command::new(executable())
        .args(arguments)
        .output()
        .expect("run Gazette help");
    assert!(
        output.status.success(),
        "Gazette help failed for {arguments:?}"
    );
    String::from_utf8(output.stdout).expect("Gazette help is UTF-8")
}

fn lsp_frame(message: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(message).expect("encode LSP request");
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend(body);
    frame
}

fn lsp_initialize() -> Value {
    let mut child = Command::new(executable())
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Gazette LSP");
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "processId":null,"capabilities":{},"initializationOptions":{
                "workspace":{"uri":"untitled:gazette-workspace.toml","path":"workspace.toml","text":"[[principals]]\nid='owner:test'\nkind='person'\n"},
                "documents":[]
            }
        }}),
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit","params":null}),
    ];
    let mut stdin = child.stdin.take().expect("LSP stdin");
    for message in messages {
        stdin
            .write_all(&lsp_frame(&message))
            .expect("send LSP request");
    }
    drop(stdin);
    let output = child.wait_with_output().expect("finish Gazette LSP");
    assert!(
        output.status.success(),
        "Gazette LSP failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = output.stdout.as_slice();
    while !bytes.is_empty() {
        let header_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("complete LSP header");
        let header = std::str::from_utf8(&bytes[..header_end]).expect("UTF-8 LSP header");
        let length = header
            .strip_prefix("Content-Length: ")
            .expect("content length")
            .parse::<usize>()
            .expect("numeric content length");
        let start = header_end + 4;
        let end = start + length;
        let message: Value = serde_json::from_slice(&bytes[start..end]).expect("LSP response");
        if message["id"] == 1 {
            return message;
        }
        bytes = &bytes[end..];
    }
    panic!("missing LSP initialize response")
}

fn commands() -> BTreeSet<String> {
    let help = help(&["--help"]);
    let mut inside = false;
    help.lines()
        .filter_map(|line| {
            if line == "Commands:" {
                inside = true;
                return None;
            }
            if inside && line == "Options:" {
                inside = false;
            }
            inside
                .then(|| line.strip_prefix("  "))
                .flatten()
                .filter(|entry| !entry.chars().next().is_some_and(char::is_whitespace))
                .and_then(|entry| entry.split_whitespace().next())
                .filter(|name| !matches!(*name, "mcp" | "lsp" | "help" | "config"))
                .map(str::to_owned)
        })
        .collect()
}

fn catalog() -> BTreeSet<String> {
    let mut child = Command::new(executable())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Gazette MCP");
    let mut stdin = child.stdin.take().expect("MCP stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("MCP stdout"));
    let initialize = json!({
        "jsonrpc":"2.0", "id":0, "method":"initialize",
        "params":{
            "protocolVersion":"2026-07-28", "capabilities":{},
            "clientInfo":{"name":"gazette-interface-coverage","version":"1"}
        }
    });
    writeln!(stdin, "{initialize}").expect("send native initialize");
    stdin.flush().expect("flush native initialize");
    let mut initialized = String::new();
    stdout
        .read_line(&mut initialized)
        .expect("read native initialize response");
    let initialized: Value = serde_json::from_str(&initialized).expect("initialize JSON response");
    assert_eq!(initialized["id"], 0);
    assert!(initialized.get("error").is_none(), "{initialized}");
    assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0", "method":"notifications/initialized"})
    )
    .expect("send initialized notification");
    stdin.flush().expect("flush initialized notification");
    // Negotiation is held by the initialized session. Only the explicit native
    // discovery selection belongs in request metadata; omitted selection is the
    // intentionally unpaged complete catalog, not this bounded paging proof.
    let capabilities = [
        "workspace",
        "authoring",
        "build",
        "preview",
        "documentation",
        "store",
        "release",
        "observation",
        "recovery",
    ];
    let expected_capabilities = capabilities
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut cursor: Option<String> = None;
    let mut cursors = BTreeSet::new();
    let mut tools = BTreeSet::new();
    let mut discovered_capabilities = BTreeSet::new();
    let mut aggregate_bytes = 0;
    let mut pages = 0;
    for page in 0..128 {
        let mut params = json!({"_meta": {
            "io.rtepl.gazette/discovery": {"capabilities":capabilities}
        }});
        if let Some(cursor) = &cursor {
            params["cursor"] = json!(cursor);
        }
        let request = json!({
            "jsonrpc": "2.0",
            "id": page + 1,
            "method": "tools/list",
            "params": params
        });
        writeln!(stdin, "{request}").expect("send tools/list page");
        stdin.flush().expect("flush tools/list page");
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read tools/list page");
        aggregate_bytes += line.len();
        assert!(
            aggregate_bytes <= 16 * 1024 * 1024,
            "catalog aggregate ceiling"
        );
        let response: Value = serde_json::from_str(&line).expect("tools/list response");
        // The initialized notification consumes no response; every page is
        // bound to its exact request ID, not a stateless negotiation substitute.
        assert_eq!(response["id"], request["id"]);
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["resultType"], "complete");
        assert_eq!(response["result"]["ttlMs"], 0);
        assert_eq!(response["result"]["cacheScope"], "private");
        assert_eq!(
            response["result"]["_meta"]["io.rtepl.gazette/discovery"]["capabilities"],
            json!(expected_capabilities)
        );
        assert_eq!(
            response["result"]["_meta"]["io.rtepl.gazette/discovery"]["tools"],
            metadata::operations().len()
        );
        pages += 1;
        let page_tools = response["result"]["tools"].as_array().expect("tool page");
        assert!(!page_tools.is_empty());
        assert!(page_tools.len() <= 8);
        if response["result"]["nextCursor"].is_string() {
            assert_eq!(page_tools.len(), 8, "only the terminal page may be partial");
        }
        for tool in page_tools {
            let name = tool["name"].as_str().expect("named tool").to_owned();
            assert!(tools.insert(name), "duplicate tool across pages");
            discovered_capabilities.insert(
                tool["_meta"]["io.rtepl.gazette/capability"]
                    .as_str()
                    .expect("native capability")
                    .to_owned(),
            );
        }
        assert!(tools.len() <= 1024, "catalog tool ceiling");
        let Some(next) = response["result"]["nextCursor"].as_str() else {
            break;
        };
        assert_ne!(cursor.as_deref(), Some(next), "tool cursor loop");
        assert!(
            cursors.insert(next.to_owned()),
            "tool cursor repeated across any page"
        );
        cursor = Some(next.to_owned());
        assert!(page < 127, "catalog page ceiling");
    }
    assert!(
        pages > 1,
        "initialized native full selection is genuinely paged"
    );
    assert_eq!(discovered_capabilities, expected_capabilities);
    drop(stdin);
    let output = child.wait_with_output().expect("finish Gazette MCP");
    assert!(
        output.status.success(),
        "Gazette MCP failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tools
}

fn vscode_routes() -> BTreeMap<String, BTreeSet<String>> {
    let value: Value = serde_json::from_str(include_str!(
        "../../../integrations/vscode/src/applicationCommandCatalog.json"
    ))
    .expect("VS Code application command catalog");
    value
        .as_array()
        .expect("VS Code command catalog is an array")
        .iter()
        .map(|entry| {
            let command = entry["cli"].as_str().expect("CLI command name").to_owned();
            let tools = entry["tools"]
                .as_array()
                .expect("mapped tools")
                .iter()
                .map(|tool| tool.as_str().expect("mapped tool name").to_owned())
                .collect::<BTreeSet<_>>();
            assert!(!tools.is_empty(), "{command} has no VS Code route");
            (command, tools)
        })
        .collect()
}

#[test]
fn every_cli_application_mode_has_discoverable_mcp_coverage() {
    let actual_commands = commands();
    let declared_commands = metadata::cli_families()
        .iter()
        .map(|family| family.command.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_commands, declared_commands,
        "every non-launcher CLI command must enter the coverage contract"
    );

    let tools = catalog();
    let registered_tools = metadata::operations()
        .iter()
        .map(|operation| operation.name.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tools, registered_tools,
        "canonical registry and live MCP catalog must cover the same operations"
    );
    let vscode = vscode_routes();
    assert_eq!(
        vscode.keys().cloned().collect::<BTreeSet<_>>(),
        declared_commands,
        "every CLI application command must have one named VS Code route"
    );
    for family in metadata::cli_families() {
        assert_eq!(metadata::cli_family(family.command), Some(family));
        let command_help = help(&[family.command, "--help"]);
        let expected_vscode_tools = family
            .operations()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            vscode.get(family.command),
            Some(&expected_vscode_tools),
            "{} VS Code route must preserve every CLI mode's MCP operation",
            family.command
        );
        assert!(
            !family.modes.is_empty(),
            "{} has no coverage",
            family.command
        );
        for mode in family.modes {
            if let Some(selector) = mode.selector {
                assert!(
                    command_help
                        .lines()
                        .any(|line| line.split_whitespace().next() == Some(selector)),
                    "{} no longer exposes material mode {selector}",
                    family.command
                );
            }
            assert!(
                !mode.operations.is_empty(),
                "{} has an empty MCP mapping",
                family.command
            );
            for tool in mode.operations {
                assert!(
                    tools.contains(*tool),
                    "{}{} maps to absent MCP tool {tool}",
                    family.command,
                    mode.selector
                        .map(|value| format!(" {value}"))
                        .unwrap_or_default()
                );
            }
        }
    }
}

#[test]
fn editor_context_cli_inspection_matches_held_mcp_import_and_explicit_closure() {
    use gazette_sdk::{authoring::session, source::Sha256Digest};

    let workspace = json!({
        "id":"file:///not-opened/editor/gazette.toml", "path":"gazette.toml",
        "text":"[[principals]]\nid='owner:fixture'\nkind='person'\n"
    });
    let page = json!({
        "id":"file:///not-opened/editor/page.md", "path":"page.md", "text":"# Café 🦀\n"
    });
    let source = |value: &Value| session::Source {
        id: value["id"].as_str().unwrap().into(),
        path: value["path"].as_str().unwrap().parse().unwrap(),
        text: value["text"].as_str().unwrap().into(),
    };
    let mut native =
        session::Session::new(source(&workspace), vec![source(&page)], Default::default()).unwrap();
    native
        .open(
            page["id"].as_str().unwrap(),
            7,
            page["text"].as_str().unwrap().into(),
        )
        .unwrap();
    let context = json!({
        "format":"gazette-editor-context/1", "workspace":"file:///not-opened/editor",
        "generation":native.generation(),
        "active":{"document":page["id"],"offset":3,"version":7,
            "source_sha256":Sha256Digest::of(page["text"].as_str().unwrap().as_bytes())},
        "manifest":native.snapshot(),
        "start":{"workspace":workspace,"documents":[page]},
        "overlays":[{"id":page["id"],"version":7,"text":page["text"]}]
    });
    let mut child = Command::new(executable())
        .arg("import-editor-context")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), &context).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let inspected: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(inspected["manifest"], context["manifest"]);
    assert_eq!(inspected["active"], context["active"]);
    assert!(
        inspected.get("session").is_none(),
        "CLI inspection retains no handle"
    );

    mcp::initialized_interact("2026-07-28", |_, send| {
        let mut sequence = 0;
        let mut call = |name: &str, arguments: Value| {
            sequence += 1;
            let reply = send(json!({"jsonrpc":"2.0","id":sequence,
                "method":"tools/call","params":{"name":name,"arguments":arguments}}));
            assert!(reply.get("error").is_none(), "{reply}");
            reply["result"].clone()
        };
        let imported = call(
            "authoring_import_editor_context",
            json!({"context":context}),
        );
        assert_eq!(imported["isError"], false, "{imported}");
        let handle = imported["structuredContent"]["session"].clone();
        let generation = imported["structuredContent"]["generation"].clone();
        let status = call("authoring_status", json!({"session":handle}));
        let closed = call(
            "authoring_close_session",
            json!({"session":handle,"generation":generation}),
        );
        let after = call("authoring_status", json!({"session":handle}));
        assert_eq!(closed["isError"], false, "{closed}");
        assert_eq!(closed["structuredContent"]["changed"], true);
        assert_eq!(after["isError"], true);
        assert_eq!(
            after["structuredContent"]["code"],
            "authoring_unknown_session"
        );
        assert_eq!(status["isError"], false, "{status}");
        assert_eq!(status["structuredContent"]["generation"], generation);
        assert_eq!(
            status["structuredContent"]["structurally_valid"],
            inspected["manifest"]["structurally_valid"]
        );
        let files = status["structuredContent"]["files"].as_array().unwrap();
        for source in inspected["manifest"]["sources"].as_array().unwrap() {
            let file = files
                .iter()
                .find(|file| file["document"] == source["id"])
                .unwrap();
            for field in ["path", "version", "source_sha256"] {
                assert_eq!(file[field], source[field]);
            }
        }
        assert_eq!(files.len(), 2);
    });
}

#[test]
fn every_advertised_lsp_semantic_has_a_discoverable_mcp_projection() {
    let response = lsp_initialize();
    assert!(response.get("error").is_none(), "{response}");
    let capabilities = &response["result"]["capabilities"];
    let tools = catalog();
    let required = [
        (
            capabilities["documentSymbolProvider"] == true,
            ["authoring_document_symbols"].as_slice(),
        ),
        (
            capabilities["workspaceSymbolProvider"] == true,
            ["authoring_search_symbols"].as_slice(),
        ),
        (
            capabilities["definitionProvider"] == true,
            ["authoring_definition"].as_slice(),
        ),
        (
            capabilities["referencesProvider"] == true,
            ["authoring_references"].as_slice(),
        ),
        (
            capabilities["hoverProvider"] == true,
            ["authoring_hover", "authoring_component_assistance"].as_slice(),
        ),
        (
            capabilities["completionProvider"].is_object(),
            [
                "authoring_assistance",
                "authoring_component_assistance",
                "authoring_complete_reference",
                "authoring_complete_heading",
            ]
            .as_slice(),
        ),
        (
            capabilities["codeActionProvider"].is_object(),
            ["authoring_review_repair"].as_slice(),
        ),
        (
            capabilities["experimental"]["gazette"]["origin"]["method"] == "gazette/origin",
            ["authoring_origin"].as_slice(),
        ),
    ];
    for (advertised, projected) in required {
        assert!(
            advertised,
            "expected LSP capability is no longer advertised"
        );
        for tool in projected {
            assert!(
                tools.contains(*tool),
                "advertised LSP semantic lacks MCP projection {tool}"
            );
        }
    }
    assert_eq!(capabilities["textDocumentSync"]["openClose"], true);
    assert_eq!(capabilities["textDocumentSync"]["change"], 1);
    for tool in [
        "authoring_start",
        "authoring_status",
        "authoring_open_document",
        "authoring_change_document",
        "authoring_close_document",
        "authoring_close_session",
        "authoring_diagnostics",
    ] {
        assert!(
            tools.contains(tool),
            "LSP session behavior lacks MCP projection {tool}"
        );
    }
}
