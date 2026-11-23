//! Initialized transport projects native authored and symbolic proposals without stale facts.

use super::*;
use gazette_cli::sdk::{authoring::session, source::Sha256Digest};

const ROOT: &str = "includes=['config/identities.toml']\r\n";
const IDENTITIES: &str = "principals=[{id=\"person:o\\u006ee\",kind='person'},{id='org:one',kind='organization'}]\r\nsites=[{id='site:one',publisher='person:one',deployable=false},{id='site:two',publisher='org:one',deployable=false}]\r\n";
const ROUTING: &str = "sources=[{id='source:one',location={kind='filesystem',root='/not-opened/one'}},{id='source:two',location={kind='filesystem',root='/not-opened/two'}}]\r\n[[brands]]\r\nid='brand:one'\r\nname='One'\r\n[[brands.publications]]\r\nsite='site:one'\r\npublic_base_url='https://one.test/mount/'\r\nauthored=[{source='source:one',target='content/one'}]\r\n[[brands.publications]]\r\nsite='site:two'\r\npublic_base_url='https://two.test/mount/'\r\nauthored=[{source='source:two',target='content/two'}]\r\n";
const SOURCE: &str = "content/one/source/index.md";
const TARGET: &str = "content/two/target/index.md";

fn page(id: &str, site: &str, relations: &str, route: &str, body: &str) -> String {
    format!(
        "+++\r\n# é🦀 current\r\n{relations}[content]\r\nid='{id}'\r\nsite='{site}'\r\ntitle='Guide'\r\nlifecycle='draft'\r\nroute={{path='{route}'}}\r\nstructure={{kind='page',family={{kind='article'}}}}\r\nattributions=[{{principal='org:one',role='rights-holder'}}]\r\n+++\r\n{body}"
    )
}

fn selection(identities: &str, source: &str, target: &str) -> Value {
    json!({"workspace":{"id":"config","path":"gazette.toml","text":ROOT},
        "workspace_documents":[{"id":"identities","path":"config/identities.toml","text":identities}],
        "documents":[{"id":"source","path":SOURCE,"text":source},{"id":"target","path":TARGET,"text":target}]})
}

fn native(identities: &str, source: &str, target: &str) -> session::Session {
    let input = |id: &str, path: &str, text: &str| session::Source {
        id: id.into(),
        path: path.parse().unwrap(),
        text: text.into(),
    };
    session::Session::new_with_workspace_documents(
        input("config", "gazette.toml", ROOT),
        [input("identities", "config/identities.toml", identities)],
        [
            input("source", SOURCE, source),
            input("target", TARGET, target),
        ],
        [],
        Default::default(),
    )
    .unwrap()
}

type Reader = BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>;
type Writer = tokio::io::WriteHalf<tokio::io::DuplexStream>;
type Service = tokio::task::JoinHandle<Result<(), crate::Error>>;

async fn rpc(
    reader: &mut Reader,
    writer: &mut Writer,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    writer
        .write_all(format!("{message}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], id, "notifications consume no response");
    assert!(response.get("error").is_none(), "{response}");
    response
}

async fn tool(
    reader: &mut Reader,
    writer: &mut Writer,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    rpc(
        reader,
        writer,
        id,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
    )
    .await
}

async fn initialized() -> (Service, Reader, Writer) {
    let (client, carrier) = tokio::io::duplex(1 << 20);
    let (read, write) = tokio::io::split(carrier);
    let service = tokio::spawn(serve(read, write));
    let (read, mut write) = tokio::io::split(client);
    let mut read = BufReader::new(read);
    let response = rpc(&mut read, &mut write, 1, "initialize", json!({
        "protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"native-authoring-test","version":"1"}
    })).await;
    assert_eq!(response["result"]["protocolVersion"], "2026-07-28");
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    (service, read, write)
}

async fn finish(service: Service, reader: Reader, mut writer: Writer) {
    writer.shutdown().await.unwrap();
    drop((reader, writer));
    tokio::time::timeout(Duration::from_secs(10), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn assert_authoring_alive(reader: &mut Reader, writer: &mut Writer, id: u64) {
    // The admitted revision removed ping. A selected native catalog request
    // proves the next supported response without changing authoring state.
    let response = rpc(
        reader,
        writer,
        id,
        "tools/list",
        json!({"_meta":{"io.rtepl.gazette/discovery":{"capabilities":["authoring"]}}}),
    )
    .await;
    let tools = response["result"]["tools"]
        .as_array()
        .expect("supported authoring catalog response");
    assert!(!tools.is_empty());
    assert!(tools.len() <= 8, "selected catalog retains its page bound");
    assert!(tools.iter().all(|tool| {
        tool["_meta"]["io.rtepl.gazette/capability"] == "authoring"
            && tool["inputSchema"].is_object()
    }));
    assert!(tools.iter().any(|tool| {
        tool["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("authoring_"))
    }));
}

fn validate_completion(response: &Value) {
    let descriptor = json!(crate::authoring::Route::Completion.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .unwrap()
        .validate(result(response))
        .unwrap();
}

#[test]
fn initialized_authored_scalar_completion_and_review_preserve_native_ranges_and_current_namespaces()
{
    runtime().block_on(async {
        let (service, mut reader, mut writer) = initialized().await;
        let target = page("content:target", "site:two", "", "/target/", "# Target\r\n");
        let mut id = 2;
        for (source, token, field, identities) in [
            (page("content:source", "site:typo", "", "/source/", "# Source\r\n"), "'site:typo'", "site", vec!["site:one", "site:two"]),
            (page("content:source", "site:one", "", "/source/", "# Source\r\n").replace("principal='org:one'", "principal='org:typo'"), "'org:typo'", "attribution_principal", vec!["person:one", "org:one"]),
            (page("content:source", "site:one", "related=['content:typo']\r\n", "/source/", "# Source\r\n"), "'content:typo'", "related_content", vec!["content:source", "content:target"]),
            ("+++\r\n[content]\r\nsite='site:typo'\r\ntitle=\r\n".into(), "'site:typo'", "site", vec!["site:one", "site:two"]),
        ] {
            let created = tool(&mut reader, &mut writer, id, "authoring_start", selection(IDENTITIES, &source, &target)).await;
            id += 1;
            let handle = result(&created)["session"].as_str().unwrap();
            let mut generation = 1;
            let offset = source.find(token).unwrap() + 2;
            let query = json!({"session":handle,"generation":1,"document":"source","offset":offset});
            let answer = tool(&mut reader, &mut writer, id, "authoring_complete_reference", query).await;
            id += 1;
            validate_completion(&answer);
            let retained = native(IDENTITIES, &source, &target);
            let expected = retained.analysis().as_ref().unwrap().complete_authored_reference(
                &SOURCE.parse().unwrap(), &source, offset, 1 << 18).unwrap().unwrap();
            let value = &result(&answer)["value"];
            assert_eq!(*value, json!(expected));
            assert_eq!(value["field"], field);
            assert_eq!(value["current"], Value::Null);
            assert_eq!(value["preimage"], token);
            assert_eq!(value["location"]["precision"], "Exact");
            assert_eq!(result(&answer)["source_sha256"], json!(Sha256Digest::of(source.as_bytes())));
            assert_eq!(value["candidates"].as_array().unwrap().iter()
                .map(|row| row["identity"].as_str().unwrap()).collect::<Vec<_>>(), identities);
            if field == "attribution_principal" {
                assert_eq!(value["candidates"][0]["replacement"], "\"person:o\\u006ee\"");
                assert_eq!(value["candidates"][0]["path"], "config/identities.toml");
            }
            if field == "site" && source.contains("id='content:source'") {
                let diagnostics = tool(&mut reader, &mut writer, id, "authoring_diagnostics",
                    json!({"session":handle,"generation":1,"document":"source"})).await;
                id += 1;
                let diagnostic = result(&diagnostics)["value"].as_array().unwrap().iter().find(|row| {
                    row["location"]["span"]["start"].as_u64().is_some_and(|start| start <= offset as u64)
                        && row["location"]["span"]["end"].as_u64().is_some_and(|end| (offset as u64) < end)
                }).expect("actual source-bound authored diagnostic").clone();
                let review = json!({"session":handle,"generation":1,"document":"source","version":null,
                    "source_sha256":result(&diagnostics)["source_sha256"],"offset":offset,"diagnostic":diagnostic});
                let repair = tool(&mut reader, &mut writer, id, "authoring_review_repair", review.clone()).await;
                id += 1;
                assert_eq!(result(&repair)["value"]["review_required"], true);
                assert_eq!(result(&repair)["value"]["proposal"]["kind"], "reference");
                assert_eq!(result(&repair)["value"]["proposal"]["value"], *value);
                let mut stale = review;
                stale["version"] = json!(7);
                let refused = tool(&mut reader, &mut writer, id, "authoring_review_repair", stale).await;
                id += 1;
                assert_eq!(refused["result"]["structuredContent"]["code"], "authoring_stale_source");
                let broken = format!("{IDENTITIES}[broken\r\n");
                let opened = tool(&mut reader, &mut writer, id, "authoring_open_document",
                    json!({"session":handle,"generation":1,"document":"identities","version":7,"text":broken})).await;
                id += 1;
                assert_eq!(result(&opened)["generation"], 2);
                let stale = tool(&mut reader, &mut writer, id, "authoring_complete_reference",
                    json!({"session":handle,"generation":1,"document":"source","offset":offset})).await;
                id += 1;
                assert_eq!(stale["result"]["structuredContent"]["code"], "authoring_stale_generation");
                let failed = tool(&mut reader, &mut writer, id, "authoring_complete_reference",
                    json!({"session":handle,"generation":2,"document":"source","offset":offset})).await;
                id += 1;
                assert_eq!(failed["result"]["isError"], true);
                assert!(failed["result"]["structuredContent"].get("value").is_none());
                let closed = tool(&mut reader, &mut writer, id, "authoring_close_document",
                    json!({"session":handle,"generation":2,"document":"identities"})).await;
                id += 1;
                assert_eq!(result(&closed)["generation"], 3);
                generation = 3;
                let restored = tool(&mut reader, &mut writer, id, "authoring_complete_reference",
                    json!({"session":handle,"generation":3,"document":"source","offset":offset})).await;
                id += 1;
                assert_eq!(result(&restored)["value"], *value);
            }
            let closed = tool(&mut reader, &mut writer, id, "authoring_close_session",
                json!({"session":handle,"generation":generation})).await;
            id += 1;
            assert_eq!(result(&closed)["changed"], true);
        }
        assert_authoring_alive(&mut reader, &mut writer, id).await;
        finish(service, reader, writer).await;
    });
}

#[test]
fn initialized_symbolic_completion_is_destination_only_and_refuses_stale_or_failed_current_targets()
{
    runtime().block_on(async {
        let (service, mut reader, mut writer) = initialized().await;
        let identities = format!("{IDENTITIES}{ROUTING}");
        let source = page("content:source", "site:one", "", "/source/",
            "# Source 😀\r\n\r\nPréface β [[site:two.test/ta|Visible β label]]\r\n");
        let target = page("content:target", "site:two", "", "/target/", "# Target\r\n\r\n## Détail β {#detail}\r\n");
        let created = tool(&mut reader, &mut writer, 2, "authoring_start", selection(&identities, &source, &target)).await;
        let handle = result(&created)["session"].as_str().unwrap();
        let offset = source.find("site:two.test/ta").unwrap();
        let query = json!({"session":handle,"generation":1,"document":"source","offset":offset});
        let answer = tool(&mut reader, &mut writer, 3, "authoring_complete_reference", query.clone()).await;
        validate_completion(&answer);
        let retained = native(&identities, &source, &target);
        let expected = retained.analysis().as_ref().unwrap().complete_symbolic_reference(
            &SOURCE.parse().unwrap(), &source, offset, 1 << 18).unwrap().unwrap();
        let value = &result(&answer)["value"];
        assert_eq!(*value, json!(expected));
        assert_eq!(value["preimage"], "site:two.test/ta");
        let candidate = value["candidates"].as_array().unwrap().iter()
            .find(|row| row["replacement"] == "site:two.test/target/#detail").unwrap();
        assert_eq!(candidate["content"], "content:target");
        assert_eq!(candidate["site"], "site:two");
        assert_eq!(candidate["source_sha256"], json!(Sha256Digest::of(target.as_bytes())));
        let start = value["location"]["span"]["start"].as_u64().unwrap() as usize;
        let end = value["location"]["span"]["end"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], "site:two.test/ta");
        let mut edited = source.clone();
        edited.replace_range(start..end, candidate["replacement"].as_str().unwrap());
        assert!(edited.contains("[[site:two.test/target/#detail|Visible β label]]"));
        let diagnostics = tool(&mut reader, &mut writer, 4, "authoring_diagnostics",
            json!({"session":handle,"generation":1,"document":"source"})).await;
        let diagnostic = result(&diagnostics)["value"].as_array().unwrap().iter()
            .find(|row| row["code"] == "markdown_link_resolution").unwrap().clone();
        let review = json!({"session":handle,"generation":1,"document":"source","version":null,
            "source_sha256":result(&diagnostics)["source_sha256"],"offset":offset,"diagnostic":diagnostic});
        let repair = tool(&mut reader, &mut writer, 5, "authoring_review_repair", review.clone()).await;
        assert_eq!(result(&repair)["value"]["proposal"]["value"], *value);
        assert_eq!(result(&repair)["value"]["review_required"], true);
        let broken = target.replace("id='content:target'", "id='content:source'");
        let opened = tool(&mut reader, &mut writer, 6, "authoring_open_document",
            json!({"session":handle,"generation":1,"document":"target","version":7,"text":broken})).await;
        assert_eq!(result(&opened)["generation"], 2);
        let stale = tool(&mut reader, &mut writer, 7, "authoring_complete_reference", query.clone()).await;
        assert_eq!(stale["result"]["structuredContent"]["code"], "authoring_stale_generation");
        let old_review = tool(&mut reader, &mut writer, 8, "authoring_review_repair", review.clone()).await;
        assert_eq!(old_review["result"]["structuredContent"]["code"], "authoring_stale_generation");
        let mut current = query;
        current["generation"] = json!(2);
        let unavailable = tool(&mut reader, &mut writer, 9, "authoring_complete_reference", current.clone()).await;
        assert_eq!(unavailable["result"]["isError"], true);
        assert!(unavailable["result"]["structuredContent"].get("value").is_none());
        let closed = tool(&mut reader, &mut writer, 10, "authoring_close_document",
            json!({"session":handle,"generation":2,"document":"target"})).await;
        assert_eq!(result(&closed)["generation"], 3);
        current["generation"] = json!(3);
        let restored = tool(&mut reader, &mut writer, 11, "authoring_complete_reference", current.clone()).await;
        assert_eq!(result(&restored)["value"], *value, "close restores exact retained baseline");
        assert_eq!(&edited[..start], &source[..start]);
        assert_eq!(&edited[start + candidate["replacement"].as_str().unwrap().len()..], &source[end..],
            "only the reviewed destination changes; Unicode label and all other bytes remain exact");
        let applied = tool(&mut reader, &mut writer, 12, "authoring_open_document",
            json!({"session":handle,"generation":3,"document":"source","version":8,"text":edited})).await;
        assert_eq!(result(&applied)["generation"], 4);
        let clear = tool(&mut reader, &mut writer, 13, "authoring_diagnostics",
            json!({"session":handle,"generation":4,"document":"source"})).await;
        assert_eq!(result(&clear)["generation"], 4);
        assert_eq!(result(&clear)["version"], 8);
        assert_eq!(result(&clear)["source_sha256"], json!(Sha256Digest::of(edited.as_bytes())));
        assert_eq!(result(&clear)["value"], json!([]), "reanalyzed native link now resolves");
        current["generation"] = json!(4);
        let resolved = tool(&mut reader, &mut writer, 14, "authoring_complete_reference", current.clone()).await;
        validate_completion(&resolved);
        assert_eq!(result(&resolved)["version"], 8);
        assert_eq!(result(&resolved)["value"]["preimage"], "site:two.test/target/#detail");
        assert_eq!(result(&resolved)["value"]["current"], *candidate);
        assert_eq!(result(&resolved)["value"]["source_sha256"], json!(Sha256Digest::of(edited.as_bytes())));
        current["generation"] = json!(3);
        let old_proposal = tool(&mut reader, &mut writer, 15, "authoring_complete_reference", current).await;
        assert_eq!(old_proposal["result"]["structuredContent"]["code"], "authoring_stale_generation");
        let mut old_preimage = review;
        old_preimage["generation"] = json!(4);
        let old_repair = tool(&mut reader, &mut writer, 16, "authoring_review_repair", old_preimage).await;
        assert_eq!(old_repair["result"]["structuredContent"]["code"], "authoring_stale_source",
            "current generation cannot legitimize the prior diagnostic/source preimage");
        let ended = tool(&mut reader, &mut writer, 17, "authoring_close_session",
            json!({"session":handle,"generation":4})).await;
        assert_eq!(result(&ended)["changed"], true);
        assert_authoring_alive(&mut reader, &mut writer, 18).await;
        finish(service, reader, writer).await;
    });
}
