//! Framed native reference edits preserve UTF-16 coordinates and current diagnostic custody.

use super::{footnotes::response, *};
use gazette_sdk::{authoring::session, source::Sha256Digest};

const ROOT: &str = "includes=['config/identities.toml']\r\n";
const IDENTITIES: &str = "principals=[{id=\"person:o\\u006ee\",kind='person'},{id='org:one',kind='organization'}]\r\nsites=[{id='site:one',publisher='person:one',deployable=false},{id='site:two',publisher='org:one',deployable=false}]\r\n";
const ROUTING: &str = "sources=[{id='source:one',location={kind='filesystem',root='/not-opened/one'}},{id='source:two',location={kind='filesystem',root='/not-opened/two'}}]\r\n[[brands]]\r\nid='brand:one'\r\nname='One'\r\n[[brands.publications]]\r\nsite='site:one'\r\npublic_base_url='https://one.test/mount/'\r\nauthored=[{source='source:one',target='content/one'}]\r\n[[brands.publications]]\r\nsite='site:two'\r\npublic_base_url='https://two.test/mount/'\r\nauthored=[{source='source:two',target='content/two'}]\r\n";
const SOURCE_URI: &str = "untitled:authored-source";
const TARGET_URI: &str = "untitled:authored-target";
const SOURCE: &str = "content/one/source/index.md";
const TARGET: &str = "content/two/target/index.md";

fn page(id: &str, site: &str, relations: &str, route: &str, body: &str) -> String {
    format!(
        "+++\r\n# é🦀 current\r\n{relations}[content]\r\nid='{id}'\r\nsite='{site}'\r\ntitle='Guide'\r\nlifecycle='draft'\r\nroute={{path='{route}'}}\r\nstructure={{kind='page',family={{kind='article'}}}}\r\nattributions=[{{principal='org:one',role='rights-holder'}}]\r\n+++\r\n{body}"
    )
}

fn selection(identities: &str, source: &str, target: &str) -> Value {
    let mut init = selected(&[(SOURCE_URI, SOURCE, source), (TARGET_URI, TARGET, target)]);
    init["params"]["initializationOptions"]["workspace"]["path"] = json!("gazette.toml");
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(ROOT);
    init["params"]["initializationOptions"]["workspace_documents"] =
        json!([{"uri":"untitled:identities","path":"config/identities.toml","text":identities}]);
    init
}

fn native(identities: &str, source: &str, target: &str) -> session::Session {
    let input = |id: &str, path: &str, text: &str| session::Source {
        id: id.into(),
        path: path.parse().unwrap(),
        text: text.into(),
    };
    session::Session::new_with_workspace_documents(
        input(URI, "gazette.toml", ROOT),
        [input(
            "untitled:identities",
            "config/identities.toml",
            identities,
        )],
        [
            input(SOURCE_URI, SOURCE, source),
            input(TARGET_URI, TARGET, target),
        ],
        [],
        Default::default(),
    )
    .unwrap()
}

fn point(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    json!({"line":before.matches('\n').count(),"character":before.rsplit('\n').next().unwrap().encode_utf16().count()})
}

fn completion(id: i32, source: &str, offset: usize) -> Value {
    request(
        id,
        "textDocument/completion",
        json!({"textDocument":{"uri":SOURCE_URI},"position":point(source,offset)}),
    )
}

fn actions(id: i32, source: &str, offset: usize, diagnostic: Value) -> Value {
    let position = point(source, offset);
    request(
        id,
        "textDocument/codeAction",
        json!({"textDocument":{"uri":SOURCE_URI},
        "range":{"start":position,"end":position},"context":{"diagnostics":[diagnostic],"only":["quickfix"]}}),
    )
}

fn diagnostic(messages: &[Value], offset: usize) -> Value {
    messages
        .iter()
        .rev()
        .filter(|row| {
            row["method"] == "textDocument/publishDiagnostics" && row["params"]["uri"] == SOURCE_URI
        })
        .flat_map(|row| row["params"]["diagnostics"].as_array().unwrap())
        .find(|row| {
            row["data"]["location"]["span"]["start"]
                .as_u64()
                .is_some_and(|start| start <= offset as u64)
                && row["data"]["location"]["span"]["end"]
                    .as_u64()
                    .is_some_and(|end| (offset as u64) < end)
        })
        .expect("current exact source-bound native diagnostic")
        .clone()
}

#[test]
fn document_links_use_resolved_local_sources_and_current_target_coordinates() {
    let identities = format!("{IDENTITIES}{ROUTING}");
    let source = page(
        "content:source",
        "site:one",
        "",
        "/source/",
        "# Source 😀\r\n\r\nPréface β [[site:two.test/target/#detail|Visible β label]]\r\n",
    );
    let target = page(
        "content:target",
        "site:two",
        "",
        "/target/",
        "# Target\r\n\r\n## Détail β {#detail}\r\n",
    );
    let moved = target.replace("## Détail", "Text before.\r\n\r\n## Détail");
    let mut init = selection(&identities, &source, &target);
    let local = "file:///foreign%20owner/target/index.md";
    init["params"]["initializationOptions"]["documents"][1]["uri"] = json!(local);
    let links = |id| {
        request(
            id,
            "textDocument/documentLink",
            json!({"textDocument":{"uri":SOURCE_URI}}),
        )
    };
    let position = point(&source, source.find("site:two.test/target/").unwrap());
    let (status, messages) = selected_session(
        init,
        [
            links(2),
            open_content(local, 2, &moved),
            links(3),
            super::footnotes::definition(4, SOURCE_URI, position),
            change(local, 3, "+++\r\ninvalid = [\r\n"),
            links(5),
        ],
        Limits::default(),
    );
    status.expect("native document-link lifecycle");
    let first = &response(&messages, 2)["result"][0];
    assert!(first["target"].as_str().unwrap().starts_with(local));
    assert_eq!(
        first["data"]["definition"]["source_sha256"],
        json!(Sha256Digest::of(target.as_bytes()))
    );
    let next = &response(&messages, 3)["result"][0];
    assert_ne!(
        next["target"], first["target"],
        "new target positions replace the original coordinates"
    );
    assert_eq!(
        next["data"]["definition"]["source_sha256"],
        json!(Sha256Digest::of(moved.as_bytes()))
    );
    assert_eq!(response(&messages, 4)["result"]["uri"], local);
    assert!(
        response(&messages, 5)["error"].is_object(),
        "broken current target must not reuse old clickable links"
    );
}

fn assert_edit(item: &Value, source: &str, value: &Value) {
    let span = &value["location"]["span"];
    let start = span["start"].as_u64().unwrap() as usize;
    let end = span["end"].as_u64().unwrap() as usize;
    assert_eq!(&source[start..end], value["preimage"].as_str().unwrap());
    assert_eq!(item["kind"], 18);
    assert_eq!(item["data"]["generation"], 2);
    assert_eq!(item["data"]["version"], 7);
    assert_eq!(
        item["data"]["source_sha256"],
        json!(Sha256Digest::of(source.as_bytes()))
    );
    assert_eq!(item["data"]["location"], value["location"]);
    assert_eq!(item["data"]["preimage"], value["preimage"]);
    assert_eq!(
        item["textEdit"]["range"],
        json!({"start":point(source,start),"end":point(source,end)})
    );
}

#[test]
fn authored_scalar_completions_match_native_namespaces_and_whole_utf16_scalar_edits() {
    let target = page("content:target", "site:two", "", "/target/", "# Target\r\n");
    for (source, token, field) in [
        (
            page(
                "content:source",
                "site:typo",
                "",
                "/source/",
                "# Source\r\n",
            ),
            "'site:typo'",
            "site",
        ),
        (
            page("content:source", "site:one", "", "/source/", "# Source\r\n")
                .replace("principal='org:one'", "principal='org:typo'"),
            "'org:typo'",
            "attribution_principal",
        ),
        (
            page(
                "content:source",
                "site:one",
                "related=['content:typo']\r\n",
                "/source/",
                "# Source\r\n",
            ),
            "'content:typo'",
            "related_content",
        ),
        (
            "+++\r\n[content]\r\nsite='site:typo'\r\ntitle=\r\n".into(),
            "'site:typo'",
            "site",
        ),
    ] {
        let offset = source.find(token).unwrap() + 2;
        let retained = native(IDENTITIES, &source, &target);
        let expected = retained
            .analysis()
            .as_ref()
            .unwrap()
            .complete_authored_reference(&SOURCE.parse().unwrap(), &source, offset, 1 << 18)
            .unwrap()
            .unwrap();
        let value = json!(expected);
        assert_eq!(value["field"], field);
        let (status, messages) = selected_session(
            selection(IDENTITIES, &source, &target),
            [
                open_content(SOURCE_URI, 7, &source),
                completion(2, &source, offset),
            ],
            Limits::default(),
        );
        status.unwrap();
        let answer = response(&messages, 2);
        assert_eq!(answer["result"]["isIncomplete"], false);
        let items = answer["result"]["items"].as_array().unwrap();
        let candidates = value["candidates"].as_array().unwrap();
        assert_eq!(items.len(), candidates.len());
        for (item, candidate) in items.iter().zip(candidates) {
            assert_edit(item, &source, &value);
            assert_eq!(item["label"], candidate["identity"]);
            assert_eq!(item["textEdit"]["newText"], candidate["replacement"]);
            assert_eq!(item["data"]["candidate"], *candidate);
            assert_eq!(item["data"]["field"], field);
        }
        assert_eq!(
            response(&messages, 90)["result"],
            Value::Null,
            "server closes normally after partial source"
        );
    }
}

#[test]
fn authored_quickfix_is_reviewed_exact_current_edit_and_stale_diagnostics_are_inert() {
    let source = page(
        "content:source",
        "site:typo",
        "",
        "/source/",
        "# Source\r\n",
    );
    let target = page("content:target", "site:two", "", "/target/", "# Target\r\n");
    let offset = source.find("site:typo").unwrap();
    let init = selection(IDENTITIES, &source, &target);
    let (status, initial) = selected_session(
        init.clone(),
        [open_content(SOURCE_URI, 7, &source)],
        Limits::default(),
    );
    status.unwrap();
    let current = diagnostic(&initial, offset);
    let changed = source.replace("site:typo", "site:missing");
    let (status, messages) = selected_session(
        init,
        [
            open_content(SOURCE_URI, 7, &source),
            completion(2, &source, offset),
            actions(3, &source, offset, current.clone()),
            change(SOURCE_URI, 8, &changed),
            actions(4, &changed, offset, current),
        ],
        Limits::default(),
    );
    status.unwrap();
    let items = response(&messages, 2)["result"]["items"]
        .as_array()
        .unwrap();
    let fixes = response(&messages, 3)["result"].as_array().unwrap();
    assert_eq!(fixes.len(), items.len());
    for (fix, item) in fixes.iter().zip(items) {
        assert_eq!(fix["kind"], "quickfix");
        assert_eq!(fix["isPreferred"], false);
        assert_eq!(fix["data"]["reviewRequired"], true);
        assert!(fix.get("command").is_none());
        assert_eq!(
            fix["edit"]["documentChanges"][0]["textDocument"],
            json!({"uri":SOURCE_URI,"version":7})
        );
        assert_eq!(
            fix["edit"]["documentChanges"][0]["edits"],
            json!([item["textEdit"]])
        );
    }
    assert_eq!(response(&messages, 4)["result"], json!([]));
}

#[test]
fn failed_current_included_identity_namespace_never_returns_previous_authored_candidates() {
    let source = page(
        "content:source",
        "site:typo",
        "",
        "/source/",
        "# Source\r\n",
    );
    let target = page("content:target", "site:two", "", "/target/", "# Target\r\n");
    let offset = source.find("site:typo").unwrap();
    let broken = format!("{IDENTITIES}[broken\r\n");
    let (status, messages) = selected_session(
        selection(IDENTITIES, &source, &target),
        [
            open_content(SOURCE_URI, 7, &source),
            completion(2, &source, offset),
            open("untitled:identities", 7, &broken),
            completion(3, &source, offset),
            close("untitled:identities"),
            completion(4, &source, offset),
        ],
        Limits::default(),
    );
    status.unwrap();
    assert!(
        !response(&messages, 2)["result"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(response(&messages, 3)["error"]["code"], -32600);
    assert!(response(&messages, 3).get("result").is_none());
    let before = response(&messages, 2)["result"]["items"]
        .as_array()
        .unwrap();
    let restored = response(&messages, 4)["result"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(
        before.iter().map(|row| &row["label"]).collect::<Vec<_>>(),
        restored.iter().map(|row| &row["label"]).collect::<Vec<_>>()
    );
    assert_eq!(restored[0]["data"]["generation"], 4);
    assert_eq!(response(&messages, 90)["result"], Value::Null);
}

#[test]
fn symbolic_destination_only_completion_and_quickfix_fail_closed_on_current_graph_errors() {
    let identities = format!("{IDENTITIES}{ROUTING}");
    let source = page(
        "content:source",
        "site:one",
        "",
        "/source/",
        "# Source 😀\r\n\r\nPréface β [[site:two.test/ta|Visible β label]]\r\n",
    );
    let target = page(
        "content:target",
        "site:two",
        "",
        "/target/",
        "# Target\r\n\r\n## Détail β {#detail}\r\n",
    );
    let offset = source.find("site:two.test/ta").unwrap();
    let retained = native(&identities, &source, &target);
    let expected = retained
        .analysis()
        .as_ref()
        .unwrap()
        .complete_symbolic_reference(&SOURCE.parse().unwrap(), &source, offset, 1 << 18)
        .unwrap()
        .unwrap();
    let value = json!(expected);
    let init = selection(&identities, &source, &target);
    let (status, initial) = selected_session(
        init.clone(),
        [open_content(SOURCE_URI, 7, &source)],
        Limits::default(),
    );
    status.unwrap();
    let current = diagnostic(&initial, offset);
    assert_eq!(current["code"], "markdown_link_resolution");
    let broken = target.replace("id='content:target'", "id='content:source'");
    let (status, messages) = selected_session(
        init,
        [
            open_content(SOURCE_URI, 7, &source),
            completion(2, &source, offset),
            actions(3, &source, offset, current),
            open_content(TARGET_URI, 7, &broken),
            completion(4, &source, offset),
            close(TARGET_URI),
            completion(5, &source, offset),
            completion(6, &source, source.find("Visible β label").unwrap()),
        ],
        Limits::default(),
    );
    status.unwrap();
    let answer = response(&messages, 2);
    let items = answer["result"]["items"].as_array().unwrap();
    let candidates = value["candidates"].as_array().unwrap();
    assert_eq!(items.len(), candidates.len());
    for (item, candidate) in items.iter().zip(candidates) {
        assert_edit(item, &source, &value);
        assert_eq!(item["label"], candidate["replacement"]);
        assert_eq!(item["data"]["target"], *candidate);
        assert_eq!(item["textEdit"]["newText"], candidate["replacement"]);
    }
    let item = items
        .iter()
        .find(|item| item["label"] == "site:two.test/target/#detail")
        .unwrap();
    let start = value["location"]["span"]["start"].as_u64().unwrap() as usize;
    let end = value["location"]["span"]["end"].as_u64().unwrap() as usize;
    let mut edited = source.clone();
    edited.replace_range(start..end, item["textEdit"]["newText"].as_str().unwrap());
    assert!(edited.contains("[[site:two.test/target/#detail|Visible β label]]"));
    assert_ne!(
        point(&source, offset)["character"].as_u64().unwrap() as usize,
        offset - source[..offset].rfind('\n').unwrap() - 1,
        "UTF-16 is not a byte column"
    );
    let fixes = response(&messages, 3)["result"].as_array().unwrap();
    assert_eq!(fixes.len(), items.len());
    for (fix, item) in fixes.iter().zip(items) {
        assert_eq!(fix["data"]["reviewRequired"], true);
        assert_eq!(
            fix["edit"]["documentChanges"][0]["edits"],
            json!([item["textEdit"]])
        );
    }
    assert_eq!(response(&messages, 4)["error"]["code"], -32600);
    assert!(
        response(&messages, 4).get("result").is_none(),
        "no prior catalog substitutes current failure"
    );
    let restored = response(&messages, 5)["result"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(
        restored.iter().map(|row| &row["label"]).collect::<Vec<_>>(),
        items.iter().map(|row| &row["label"]).collect::<Vec<_>>()
    );
    assert_eq!(restored[0]["data"]["generation"], 4);
    assert!(
        response(&messages, 6)["result"].is_null(),
        "visible label is not a destination edit"
    );
    assert_eq!(response(&messages, 90)["result"], Value::Null);
}
