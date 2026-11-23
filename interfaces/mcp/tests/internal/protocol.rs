//! Real JSON-RPC framing through the production service and shared SDK.

#[path = "artifact.rs"]
mod artifact;
#[path = "authoring_protocol.rs"]
mod authoring_protocol;
#[path = "commit_write.rs"]
mod commit_write;
#[path = "discovery_protocol.rs"]
mod discovery_protocol;
#[path = "github.rs"]
mod github;
#[path = "journal_commit.rs"]
mod journal_commit;

use std::time::Duration;

use gazette_cli::{
    command::metadata,
    sdk::{toml, workspace},
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;

const BASE: &str = "[[principals]]\nid='owner:one'\nkind='organization'\n[[sites]]\nid='site:z'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=true\n";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("test runtime")
}

fn request(method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{},
    });
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}

async fn exchange_one(message: Value, limits: server::Limits) -> Value {
    let (client, carrier) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(carrier);
    let service = tokio::spawn(serve_with_limits(reader, writer, limits));
    let (reader, mut writer) = tokio::io::split(client);
    let bytes = format!("{message}\n");
    writer
        .write_all(bytes.as_bytes())
        .await
        .expect("send real frame");
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("response deadline")
        .expect("receive frame");
    assert!(!line.is_empty(), "service must return a protocol response");
    writer.shutdown().await.expect("EOF");
    drop(writer);
    drop(reader);
    tokio::time::timeout(Duration::from_secs(10), service)
        .await
        .expect("service deadline")
        .expect("service task")
        .expect("clean carrier");
    serde_json::from_str(&line).expect("one JSON-RPC response, no logging on carrier")
}

async fn initialized_catalog(version: &str, limits: server::Limits) -> (Value, Value) {
    initialized_catalog_for(version, limits, None).await
}

async fn initialized_catalog_for(
    version: &str,
    limits: server::Limits,
    capabilities: Option<&[&str]>,
) -> (Value, Value) {
    let (client, carrier) = tokio::io::duplex(1 << 20);
    let (reader, writer) = tokio::io::split(carrier);
    let service = tokio::spawn(serve_with_limits(reader, writer, limits));
    let (reader, mut writer) = tokio::io::split(client);
    let mut reader = BufReader::new(reader);
    let initialize = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"initialize",
        "params":{
            "protocolVersion":version,
            "capabilities":{},
            "clientInfo":{"name":"gazette-test","version":"1"}
        }
    });
    writer
        .write_all(format!("{initialize}\n").as_bytes())
        .await
        .expect("send initialize");
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("initialize deadline")
        .expect("initialize response");
    let initialized: Value = serde_json::from_str(&line).expect("initialize JSON response");
    let mut catalog_request = request("tools/list", json!({}));
    catalog_request["id"] = json!(2);
    if let Some(capabilities) = capabilities {
        catalog_request["params"]["_meta"]["io.rtepl.gazette/discovery"] =
            json!({"capabilities":capabilities});
    }
    writer
        .write_all(
            format!(
                "{}\n{catalog_request}\n",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .as_bytes(),
        )
        .await
        .expect("send initialized lifecycle and catalog request");
    line.clear();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("catalog deadline")
        .expect("catalog response");
    let mut catalog: Value = serde_json::from_str(&line).expect("catalog JSON response");
    let mut tools = catalog["result"]["tools"]
        .as_array()
        .expect("initialized tool page")
        .clone();
    let mut cursor = catalog["result"]["nextCursor"].as_str().map(str::to_owned);
    let mut seen = std::collections::BTreeSet::new();
    let mut id = 3;
    while let Some(next) = cursor {
        assert!(seen.insert(next.clone()), "initialized cursor loop");
        let request = json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"tools/list",
            "params":{"cursor":next}
        });
        writer
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("request initialized tool page");
        line.clear();
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
            .await
            .expect("initialized page deadline")
            .expect("initialized page response");
        let page: Value = serde_json::from_str(&line).expect("initialized page JSON response");
        assert!(page.get("error").is_none(), "{page}");
        assert_eq!(
            page["result"]["_meta"], catalog["result"]["_meta"],
            "opaque cursor must retain the exact discovery selection"
        );
        tools.extend(
            page["result"]["tools"]
                .as_array()
                .expect("initialized tool page")
                .clone(),
        );
        cursor = page["result"]["nextCursor"].as_str().map(str::to_owned);
        id += 1;
    }
    catalog["result"]["tools"] = Value::Array(tools);
    catalog["result"]
        .as_object_mut()
        .expect("initialized catalog result")
        .remove("nextCursor");
    writer.shutdown().await.expect("EOF");
    drop(writer);
    drop(reader);
    tokio::time::timeout(Duration::from_secs(10), service)
        .await
        .expect("service deadline")
        .expect("service task")
        .expect("clean initialized carrier");
    (initialized, catalog)
}

async fn exchange(mut message: Value, limits: server::Limits) -> Value {
    let paginate = message["method"] == "tools/list" && message["params"].get("cursor").is_none();
    let mut response = exchange_one(message.clone(), limits).await;
    if !paginate || response.get("error").is_some() {
        return response;
    }
    let mut tools = response["result"]["tools"]
        .as_array()
        .expect("tool page")
        .clone();
    let mut cursor = response["result"]["nextCursor"].as_str().map(str::to_owned);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(next) = cursor {
        assert!(seen.insert(next.clone()), "tool cursor loop");
        assert!(seen.len() <= 128, "tool page ceiling");
        message["params"]["cursor"] = json!(next);
        let page = exchange_one(message.clone(), limits).await;
        assert!(page.get("error").is_none(), "{page}");
        assert_eq!(page["result"]["resultType"], "complete");
        assert_eq!(page["result"]["ttlMs"], 0);
        assert_eq!(page["result"]["cacheScope"], "private");
        tools.extend(
            page["result"]["tools"]
                .as_array()
                .expect("tool page")
                .clone(),
        );
        cursor = page["result"]["nextCursor"].as_str().map(str::to_owned);
    }
    response["result"]["tools"] = Value::Array(tools);
    response["result"]
        .as_object_mut()
        .expect("result")
        .remove("nextCursor");
    response
}

fn call(name: &str, arguments: Value) -> Value {
    runtime().block_on(exchange(
        request("tools/call", json!({"name":name,"arguments":arguments})),
        server::Limits::default(),
    ))
}

fn result(response: &Value) -> &Value {
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    let structured = &response["result"]["structuredContent"];
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("readable JSON"),
    )
    .expect("same JSON value");
    assert_eq!(structured, &text);
    structured
}

fn discovery_names() -> Vec<&'static str> {
    let mut operations = metadata::operations().to_vec();
    operations.sort_by_key(|operation| operation.discovery_key());
    operations
        .into_iter()
        .map(|operation| operation.name)
        .collect()
}

#[test]
fn actual_protocol_check_matches_complete_sdk_and_never_opens_declared_source_paths() {
    let source = format!(
        "{BASE}[[sources]]\nid='source:one'\nlocation={{kind='repository',root='/this-path-is-not-opened/gazette-mcp',commit='{}',subdirectory='docs'}}\n",
        "1".repeat(40)
    );
    let response = call("check_workspace", json!({"source":source}));
    let expected = workspace::compile(&source, toml::Limits::default()).expect("native SDK");
    assert_eq!(
        result(&response),
        &serde_json::to_value(expected).expect("SDK JSON")
    );
    assert_eq!(response, call("check_workspace", json!({"source":source})));
}

#[test]
fn actual_protocol_verification_matches_native_digest_evidence_and_redacted_refusal() {
    let authorization = include_str!(
        "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    );
    let binding = include_str!(
        "../../../../crates/gazette-compiler/tests/release_authorization/binding.json"
    );
    let response = call(
        "verify_release_authorization",
        json!({
            "authorization_json":authorization, "binding_json":binding
        }),
    );
    let limits = gazette_cli::sdk::release::Limits::default();
    let parsed = gazette_cli::sdk::release::parse_binding(binding.as_bytes(), limits)
        .expect("fixture binding");
    let expected = gazette_cli::sdk::release::verify(authorization.as_bytes(), &parsed, limits)
        .expect("real signature");
    assert_eq!(result(&response), &json!(expected.report()));
    let failed = call(
        "verify_release_authorization",
        json!({
            "authorization_json":"PRIVATE-malformed", "binding_json":binding
        }),
    );
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(
        failed["result"]["structuredContent"]["code"],
        "release_json"
    );
    assert!(!failed.to_string().contains("PRIVATE"));
}

#[test]
fn actual_protocol_ref_observation_matches_local_sdk_and_rejects_exact_duplicate_bytes() {
    use gazette_cli::sdk::release;
    use std::{fs, os::unix::fs::PermissionsExt};

    let remote = tempfile::tempdir().expect("disposable local destination");
    let repository = git2::Repository::init_bare(remote.path()).expect("real empty bare Git");
    let isolation = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private empty isolation");
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("one native target shape");
    let mut target: release::Target = serde_json::from_value(fixture["ordered_targets"][0].clone())
        .expect("read-only target, no signature dependency");
    assert_eq!(
        target.expected_remote_oid_or_unborn,
        release::RemoteState::Unborn
    );
    target.remote = remote
        .path()
        .to_str()
        .expect("native UTF-8 path")
        .to_owned();
    let targets_json = serde_json::to_string(&[target]).expect("exact target array");
    let targets = release::parse_targets(targets_json.as_bytes(), Default::default())
        .expect("native target validator");
    let expected = release::observe_targets(
        &release::GitSelection {
            program: crate::native::git().to_owned(),
            isolation: isolation.path().to_path_buf(),
            credentials: None,
        },
        &targets,
        release::RemoteReadOptions {
            allow_read: true,
            ..Default::default()
        },
    )
    .expect("native empty advertisement");
    let arguments = json!({"targets_json":targets_json,"git":{"program":crate::native::git(),"isolation":isolation.path()},"allow_read":true});
    let response = call("observe_release_targets", arguments.clone());
    assert_eq!(result(&response), &json!(expected));
    assert_eq!(
        result(&response)["targets"][0]["readback"]["comparison"],
        "prestate"
    );
    assert_eq!(
        result(&response)["targets"][0]["readback"]["advertisement"]["state"],
        "absent",
        "the qualified independent local namespace is empty, not merely unadvertised"
    );
    let mut denied = arguments.clone();
    denied["allow_read"] = json!(false);
    let denied = call("observe_release_targets", denied);
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        denied["result"]["structuredContent"]["code"],
        "release_read_permission"
    );
    let mut duplicate = arguments;
    duplicate["targets_json"] =
        json!(targets_json.replacen("\"ordinal\":0", "\"ordinal\":0,\"ordinal\":0", 1));
    assert_ne!(duplicate["targets_json"], json!(targets_json));
    let refused = call("observe_release_targets", duplicate);
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(
        refused["result"]["structuredContent"]["code"],
        "release_json"
    );
    assert_eq!(
        repository
            .references()
            .expect("native refs unchanged")
            .count(),
        0
    );
    assert!(
        fs::read_dir(isolation.path())
            .expect("isolation unchanged")
            .next()
            .is_none()
    );
}

#[test]
fn actual_protocol_local_read_tools_preserve_permission_and_native_complete_results() {
    let root = tempfile::tempdir().expect("isolated selected tree");
    std::fs::write(root.path().join("workspace.toml"), BASE).expect("synthetic configuration");
    let arguments =
        json!({"root":root.path(),"allow_native_reads":true,"selection":{"kind":"observation"}});
    let response = call("inspect_source", arguments.clone());
    let expected = gazette_cli::sdk::source::inspect(
        root.path(),
        None,
        gazette_cli::sdk::source::Limits::default(),
    )
    .expect("native source");
    assert_eq!(result(&response), &json!(expected));
    let response = call(
        "inspect_toml",
        json!({"root":root.path(),"allow_native_reads":true}),
    );
    let expected = gazette_cli::sdk::configuration::inspect_toml(
        root.path(),
        None,
        gazette_cli::sdk::configuration::Limits::default(),
    )
    .expect("native syntax set");
    assert_eq!(result(&response), &json!(expected));
    let mut denied = arguments;
    denied["allow_native_reads"] = json!(false);
    assert_eq!(call("inspect_source", denied)["error"]["code"], -32602);
    assert_eq!(
        std::fs::read_dir(root.path())
            .expect("unchanged root")
            .count(),
        1
    );
}

#[test]
fn real_protocol_keeps_explicit_authoring_handles_and_current_overlay_evidence() {
    runtime().block_on(async {
        async fn send(
            reader: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
            writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
            name: &str,
            arguments: Value,
        ) -> Value {
            let message = request("tools/call", json!({"name":name,"arguments":arguments}));
            writer.write_all(format!("{message}\n").as_bytes()).await.expect("request");
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await.expect("deadline").expect("response");
            serde_json::from_str(&line).expect("JSON-RPC")
        }
        let (client, carrier) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(carrier);
        let service = tokio::spawn(serve(reader, writer));
        let (reader, mut writer) = tokio::io::split(client);
        let mut reader = BufReader::new(reader);
        let source = "# Original\n\n[link](#original)\n";
        let caller = "😀 [go](../target.md#shared)\r\n[missing](missing.md#shared)\r\n";
        let target = "# Shared 🦀 {#shared}\r\n\r\n😀 [local](#shared)\r\n";
        let changed_target = format!("😀 Unsaved prefix\r\n\r\n{target}");
        let created = send(&mut reader, &mut writer, "authoring_start", json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":BASE},
            "documents":[{"id":"page","path":"page.md","text":source},
                {"id":"untitled:caller","path":"notes/caller.md","text":caller},
                {"id":"untitled:target","path":"target.md","text":target}]
        })).await;
        let handle = result(&created)["session"].as_str().expect("handle");
        let changed = send(&mut reader, &mut writer, "authoring_open_document", json!({"session":handle,"generation":1,"document":"page","version":9,"text":"# Current\n\n[link](#current)\n"})).await;
        assert_eq!(result(&changed)["generation"], 2);
        let completed = send(&mut reader, &mut writer, "authoring_complete_reference", json!({"session":handle,"generation":2,"document":"workspace","offset":BASE.rfind("owner:one").expect("native reference")})).await;
        assert_eq!(result(&completed)["generation"], 2);
        assert_eq!(result(&completed)["value"]["preimage"], "'owner:one'");
        assert_eq!(result(&completed)["value"]["candidates"][0]["symbol"]["declaration"]["identity"], "owner:one");
        let symbols = send(&mut reader, &mut writer, "authoring_document_symbols", json!({"session":handle,"generation":2,"document":"page"})).await;
        assert_eq!(result(&symbols)["value"][0]["heading"]["text"], "Current");
        let searched = send(&mut reader, &mut writer, "authoring_search_symbols", json!({"session":handle,"generation":2,"query":"CURRENT"})).await;
        assert_eq!(result(&searched)["generation"], 2);
        assert_eq!(result(&searched)["symbols"].as_array().expect("complete matches").len(), 1);
        assert_eq!(result(&searched)["symbols"][0]["document"], "page");
        assert_eq!(result(&searched)["symbols"][0]["version"], 9);
        assert_eq!(result(&searched)["symbols"][0]["name"], "Current");
        let stale = send(&mut reader, &mut writer, "authoring_definition", json!({"session":handle,"generation":1,"document":"page","offset":0})).await;
        assert_eq!(stale["result"]["structuredContent"]["code"], "authoring_stale_generation");
        let definition = send(&mut reader, &mut writer, "authoring_definition", json!({"session":handle,"generation":2,"document":"page","offset":14})).await;
        assert_eq!(result(&definition)["value"]["location"]["span"]["start"], 0);
        let markdown = send(&mut reader, &mut writer, "inspect_markdown", json!({"source":"# Current\n","html":true})).await;
        assert!(result(&markdown)["html"].as_str().expect("HTML").contains("Current"));
        let cross = send(&mut reader, &mut writer, "authoring_definition", json!({"session":handle,"generation":2,"document":"untitled:caller","offset":caller.find("[go]").expect("native link")})).await;
        let baseline = result(&cross)["value"].clone();
        assert_eq!(baseline["document"], "untitled:target");
        assert_eq!(baseline["path"], "target.md");
        assert_eq!(baseline["version"], Value::Null);
        assert_eq!(baseline["source_sha256"], json!(gazette_cli::sdk::source::Sha256Digest::of(target.as_bytes())));
        assert_eq!(baseline["location"]["span"]["start"], 0);
        assert_eq!(baseline["location"]["span"]["end"], "# Shared 🦀 {#shared}".len());
        assert_eq!(result(&cross)["document"], "untitled:caller");
        assert_eq!(result(&cross)["source_sha256"], json!(gazette_cli::sdk::source::Sha256Digest::of(caller.as_bytes())));
        let incoming = send(&mut reader, &mut writer, "authoring_references", json!({"session":handle,"generation":2,"document":"untitled:target","offset":0,"include_declaration":true})).await;
        let incoming = result(&incoming)["value"].as_array().expect("complete incoming locations");
        assert_eq!(incoming.len(), 3);
        assert_eq!(incoming[0]["document"], "untitled:caller");
        assert_eq!(incoming[0]["path"], "notes/caller.md");
        assert_eq!(incoming[0]["source_sha256"], json!(gazette_cli::sdk::source::Sha256Digest::of(caller.as_bytes())));
        assert_eq!(incoming[0]["location"]["span"]["start"], caller.find("[go]").expect("UTF-8 offset"));
        assert_eq!(incoming[1], baseline);
        assert_eq!(incoming[2]["document"], "untitled:target");
        assert_eq!(incoming[2]["location"]["span"]["start"], target.find("[local]").expect("target offset"));
        let opened = send(&mut reader, &mut writer, "authoring_open_document", json!({"session":handle,"generation":2,"document":"untitled:target","version":10,"text":target})).await;
        assert_eq!(result(&opened)["generation"], 3);
        let changed = send(&mut reader, &mut writer, "authoring_change_document", json!({"session":handle,"generation":3,"document":"untitled:target","version":11,"text":changed_target})).await;
        assert_eq!(result(&changed)["generation"], 4);
        let cross = send(&mut reader, &mut writer, "authoring_definition", json!({"session":handle,"generation":4,"document":"untitled:caller","offset":caller.find("[go]").expect("native link")})).await;
        let current = &result(&cross)["value"];
        assert_eq!(current["document"], "untitled:target");
        assert_eq!(current["version"], 11);
        assert_eq!(current["source_sha256"], json!(gazette_cli::sdk::source::Sha256Digest::of(changed_target.as_bytes())));
        assert_eq!(current["location"]["span"]["start"], changed_target.find("# Shared").expect("moved target"));
        assert_eq!(result(&cross)["version"], Value::Null, "target overlay never changes caller version");
        let closed = send(&mut reader, &mut writer, "authoring_close_document", json!({"session":handle,"generation":4,"document":"untitled:target"})).await;
        assert_eq!(result(&closed)["generation"], 5);
        let restored = send(&mut reader, &mut writer, "authoring_definition", json!({"session":handle,"generation":5,"document":"untitled:caller","offset":caller.find("[go]").expect("native link")})).await;
        assert_eq!(result(&restored)["value"], baseline, "close restores retained baseline without native reads");
        let unselected = send(&mut reader, &mut writer, "authoring_definition", json!({"session":handle,"generation":5,"document":"untitled:caller","offset":caller.find("[missing]").expect("unselected link")})).await;
        assert_eq!(result(&unselected)["value"], Value::Null);
        let closed = send(&mut reader, &mut writer, "authoring_close_session", json!({"session":handle,"generation":5})).await;
        assert_eq!(result(&closed)["changed"], true);
        let absent = send(&mut reader, &mut writer, "authoring_status", json!({"session":handle})).await;
        assert_eq!(absent["result"]["structuredContent"]["code"], "authoring_unknown_session");

        let invalid = "principals=[{id='owner:one',kind='person'},{id='owner:two',kind='person'}]\nsites=[{id='site:one',publisher='owner:typo',deployable=false}]\n";
        let created = send(&mut reader, &mut writer, "authoring_start", json!({
            "workspace":{"id":"repair","path":"workspace.toml","text":invalid},
            "documents":[]
        })).await;
        let repair_handle = result(&created)["session"].as_str().expect("repair handle");
        let diagnostics = send(&mut reader, &mut writer, "authoring_diagnostics", json!({
            "session":repair_handle,"generation":1,"document":"repair"
        })).await;
        let diagnostic = result(&diagnostics)["value"].as_array().expect("diagnostics")
            .iter().find(|row| row["code"] == "workspace_reference")
            .expect("repairable current diagnostic").clone();
        let repair = send(&mut reader, &mut writer, "authoring_review_repair", json!({
            "session":repair_handle,"generation":1,"document":"repair","version":null,
            "source_sha256":result(&diagnostics)["source_sha256"],
            "offset":invalid.rfind("owner:typo").expect("reference"),"diagnostic":diagnostic
        })).await;
        assert_eq!(result(&repair)["value"]["review_required"], true);
        assert_eq!(result(&repair)["value"]["proposal"]["kind"], "reference");
        assert_eq!(result(&repair)["value"]["proposal"]["value"]["preimage"], "'owner:typo'");
        assert_eq!(result(&repair)["value"]["proposal"]["value"]["candidates"].as_array().expect("candidates").len(), 2);
        assert!(result(&repair)["value"].get("command").is_none());
        assert!(result(&repair)["value"].get("edit").is_none());
        let closed = send(&mut reader, &mut writer, "authoring_close_session", json!({
            "session":repair_handle,"generation":1
        })).await;
        assert_eq!(result(&closed)["changed"], true);
        writer.shutdown().await.expect("EOF");
        drop((writer, reader));
        tokio::time::timeout(Duration::from_secs(10), service).await.expect("shutdown deadline").expect("service task").expect("service exit");
    });
}

#[test]
fn declared_origin_preserves_unicode_crlf_unsorted_sites_and_exact_source_identity() {
    let source =
        format!("# 🦀 é decoy name='wrong'\r\n{BASE}[[brands]]\nid='brand:one'\nname='Rêve'\n");
    let query = json!({"kind":"brand","identity":"brand:one","field":["name"]});
    let parsed = workspace::ParsedWorkspace::parse(&source, toml::Limits::default())
        .expect("valid workspace");
    let expected = parsed
        .origin(&serde_json::from_value(query.clone()).expect("native query"))
        .expect("explicit field origin");
    assert_eq!(&source[expected.span.start..expected.span.end], "'Rêve'");
    let response = call(
        "workspace_field_origin",
        json!({"source":source,"query":query}),
    );
    assert_eq!(
        result(&response),
        &serde_json::to_value(expected).expect("SDK JSON")
    );
}

#[test]
fn composition_origin_explains_workspace_inheritance_and_explicit_site_override() {
    let source = "[composition.capabilities]\n'publication.feeds/v1'={state='disabled'}\n\
        [[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:z'\npublisher='owner:one'\ndeployable=false\n\
        [sites.composition.capabilities]\n'publication.feeds/v1'={state='disabled'}\n\
        [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n";
    let parsed = workspace::ParsedWorkspace::parse(source, toml::Limits::default())
        .expect("valid composition");
    for site in ["site:a", "site:z"] {
        let query = json!({"site":site,"field":{"kind":"capability","capability":"publication.feeds/v1"},"path":["state"]});
        let expected = parsed
            .composition_origin(&serde_json::from_value(query.clone()).expect("native query"))
            .expect("winning composition origin");
        assert_eq!(
            &source[expected.span.start..expected.span.end],
            "'disabled'"
        );
        let response = call(
            "workspace_composition_origin",
            json!({"source":source,"query":query}),
        );
        assert_eq!(
            result(&response),
            &serde_json::to_value(expected).expect("SDK JSON")
        );
    }
}

#[test]
fn native_failures_are_tool_errors_without_partial_workspace_or_invented_ranges() {
    let invalid = BASE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let expected = match workspace::compile(&invalid, toml::Limits::default())
        .expect_err("missing publisher must reject the complete native workspace")
    {
        workspace::Error::Invalid(diagnostics) => {
            serde_json::to_value(diagnostics).expect("native diagnostics")
        }
        other => panic!("expected native topology error: {other:?}"),
    };
    let response = call("check_workspace", json!({"source":invalid}));
    assert_eq!(response["result"]["isError"], true);
    let diagnostic = &response["result"]["structuredContent"];
    assert_eq!(diagnostic["code"], "workspace_invalid");
    assert_eq!(diagnostic["diagnostics"], expected);
    assert!(diagnostic.get("workspace").is_none());
    for (identity, field, expected) in [
        ("site:missing", "id", "origin_unknown_identity"),
        ("site:z", "public_base_url", "origin_no_explicit_field"),
    ] {
        let response = call(
            "workspace_field_origin",
            json!({"source":BASE,"query":{"kind":"site","identity":identity,"field":[field]}}),
        );
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(response["result"]["structuredContent"]["code"], expected);
        assert!(
            response["result"]["structuredContent"]
                .get("span")
                .is_none()
        );
    }
    let response = call("check_workspace", json!({"source":"[broken"}));
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "toml_invalid"
    );
}

#[test]
fn malformed_arguments_unknown_tools_and_invalid_lifecycles_do_not_run_sdk_work() {
    for (name, arguments) in [
        ("unknown", json!({"source":BASE})),
        ("check_workspace", json!({"source":BASE,"path":"/injected"})),
        (
            "workspace_field_origin",
            json!({"source":BASE,"query":{"kind":"brand","identity":"brand:one","field":[],"extra":true}}),
        ),
        (
            "workspace_composition_origin",
            json!({"source":BASE,"query":[]}),
        ),
    ] {
        let response = call(name, arguments);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(
            response["error"]["data"]["io.rtepl.gazette/dispatch"],
            "not-dispatched"
        );
    }
    runtime().block_on(async {
        let missing = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
        assert_eq!(
            exchange(missing, server::Limits::default()).await["error"]["code"],
            -32602
        );
        let mut old = request("tools/list", json!({}));
        old["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2025-11-25");
        assert!(
            exchange(old, server::Limits::default())
                .await
                .get("error")
                .is_some()
        );
        let cursor = request("tools/list", json!({"cursor":"not-a-real-page"}));
        assert_eq!(
            exchange(cursor, server::Limits::default()).await["error"]["code"],
            -32602
        );
    });
}

#[test]
fn initialized_hosts_negotiate_their_revision_without_changing_discovery() {
    runtime().block_on(async {
        let stateless = exchange(request("tools/list", json!({})), server::Limits::default()).await;
        assert!(stateless.get("error").is_none(), "{stateless}");
        for version in ["2025-06-18", "2025-11-25", "2026-07-28"] {
            let (initialized, catalog) =
                initialized_catalog(version, server::Limits::default()).await;
            assert!(initialized.get("error").is_none(), "{initialized}");
            assert_eq!(initialized["result"]["protocolVersion"], version);
            assert_eq!(catalog["result"]["tools"], stateless["result"]["tools"]);
        }
    });
}

#[test]
fn initialized_host_can_page_only_selected_capability_neighborhoods() {
    runtime().block_on(async {
        let (_, complete) = initialized_catalog("2026-07-28", server::Limits::default()).await;
        let selected = ["authoring", "build", "workspace"];
        let (initialized, filtered) =
            initialized_catalog_for("2026-07-28", server::Limits::default(), Some(&selected)).await;
        assert!(initialized.get("error").is_none(), "{initialized}");
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        assert_eq!(
            filtered["result"]["_meta"]["io.rtepl.gazette/discovery"]["capabilities"],
            json!(["authoring", "build", "workspace"])
        );

        let tools = filtered["result"]["tools"]
            .as_array()
            .expect("filtered initialized catalog");
        assert!(!tools.is_empty());
        assert_eq!(
            filtered["result"]["_meta"]["io.rtepl.gazette/discovery"]["tools"],
            tools.len()
        );
        assert!(tools.iter().all(|tool| {
            selected.contains(
                &tool["_meta"]["io.rtepl.gazette/capability"]
                    .as_str()
                    .expect("capability"),
            )
        }));
        assert!(
            tools
                .iter()
                .all(|tool| tool["inputSchema"].is_object() && tool["outputSchema"].is_object()),
            "selected tools retain exact callable schemas"
        );
        assert!(
            tools.iter().all(|tool| !matches!(
                tool["_meta"]["io.rtepl.gazette/capability"].as_str(),
                Some("store" | "release" | "recovery" | "observation")
            )),
            "a Theme-oriented session must not load lifecycle and destination schemas"
        );
        let complete_tools = complete["result"]["tools"]
            .as_array()
            .expect("complete initialized catalog");
        assert!(tools.len() < complete_tools.len());
        assert!(
            serde_json::to_vec(&filtered)
                .expect("filtered response")
                .len()
                < serde_json::to_vec(&complete)
                    .expect("complete response")
                    .len(),
            "bounded discovery must materially reduce the selected decision surface"
        );
    });
}

#[test]
fn discovery_selection_is_typed_and_bound_to_its_cursor() {
    runtime().block_on(async {
        for selection in [
            json!([]),
            json!(["missing"]),
            json!(["build", "build"]),
            json!("build"),
        ] {
            let mut request = request("tools/list", json!({}));
            request["params"]["_meta"]["io.rtepl.gazette/discovery"] =
                json!({"capabilities":selection});
            assert_eq!(
                exchange_one(request, server::Limits::default()).await["error"]["code"],
                -32602
            );
        }

        let mut selected = request("tools/list", json!({}));
        selected["params"]["_meta"]["io.rtepl.gazette/discovery"] =
            json!({"capabilities":["authoring", "build", "workspace"]});
        let first = exchange_one(selected, server::Limits::default()).await;
        let cursor = first["result"]["nextCursor"]
            .as_str()
            .expect("selected Theme-work neighborhood spans pages");
        let mut mismatched = request("tools/list", json!({"cursor":cursor}));
        mismatched["params"]["_meta"]["io.rtepl.gazette/discovery"] =
            json!({"capabilities":["release"]});
        assert_eq!(
            exchange_one(mismatched, server::Limits::default()).await["error"]["code"],
            -32602,
            "a cursor cannot be rebound to another decision surface"
        );
    });
}

#[test]
fn resource_limits_never_return_partial_success_or_queue_unadmitted_work() {
    let source = format!(
        "#{}\n{BASE}",
        "x".repeat(toml::Limits::default().source_bytes)
    );
    let response = call("check_workspace", json!({"source":source}));
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "toml_limit"
    );
    runtime().block_on(async {
        let message = request(
            "tools/call",
            json!({"name":"check_workspace","arguments":{"source":BASE}}),
        );
        let response = exchange(
            message.clone(),
            server::Limits {
                output_bytes: 1,
                ..server::Limits::default()
            },
        )
        .await;
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"],
            json!({"code":"mcp_output_limit"})
        );
        let busy = exchange(
            message,
            server::Limits {
                workers: 0,
                ..server::Limits::default()
            },
        )
        .await;
        assert_eq!(busy["error"]["data"]["code"], "mcp_busy");
        assert_eq!(
            busy["error"]["data"]["io.rtepl.gazette/dispatch"],
            "not-dispatched"
        );
    });
}

#[test]
fn empty_input_terminates_cleanly_and_oversized_frame_fails_the_carrier() {
    runtime().block_on(async {
        serve(std::io::Cursor::new(Vec::<u8>::new()), tokio::io::sink())
            .await
            .expect("ordinary EOF");
        let input = std::io::Cursor::new(vec![b'x'; 33]);
        assert!(
            serve_with_limits(
                input,
                tokio::io::sink(),
                server::Limits {
                    frame_bytes: 32,
                    ..server::Limits::default()
                }
            )
            .await
            .is_err()
        );
    });
}

#[test]
fn observed_read_write_and_zero_write_failures_do_not_become_successful_eof() {
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::ReadBuf;

    struct BrokenRead;
    impl AsyncRead for BrokenRead {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Err(std::io::Error::other("read failed")))
        }
    }
    struct BrokenWrite {
        zero: bool,
    }
    impl AsyncWrite for BrokenWrite {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(if self.zero {
                Ok(0)
            } else {
                Err(std::io::Error::other("write failed"))
            })
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    runtime().block_on(async {
        assert!(serve(BrokenRead, tokio::io::sink()).await.is_err());
        for zero in [false, true] {
            let frame = format!("{}\n", request("tools/list", json!({})));
            let (mut producer, reader) = tokio::io::duplex(4096);
            producer
                .write_all(frame.as_bytes())
                .await
                .expect("request without EOF");
            assert!(
                tokio::time::timeout(Duration::from_secs(10), serve(reader, BrokenWrite { zero }))
                    .await
                    .expect("output failure must stop even while input is open")
                    .is_err()
            );
            drop(producer);
        }
    });
}
