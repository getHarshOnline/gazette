//! Real framed completion is a projection of current SDK reference replacement.

use super::{footnotes::response, *};

fn text() -> String {
    "# é🦀\r\nprincipals=[{id=\"owner:\\u006fne\",kind='person'},{id='owner:two',kind='person'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n".into()
}
fn point(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    json!({"line":before.matches('\n').count(),"character":before.rsplit('\n').next().expect("line").encode_utf16().count()})
}
fn complete(id: i32, position: Value) -> Value {
    request(
        id,
        "textDocument/completion",
        json!({"textDocument":{"uri":URI},"position":position}),
    )
}

pub(super) fn component_selection(uri: &str, page: &str) -> Value {
    let workspace = "[composition]\ntheme={package='theme:one'}\n\
[[principals]]\nid='owner:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[sources]]\nid='source:theme'\nlocation={kind='repository',root='/not-opened/theme',commit='0000000000000000000000000000000000000000',subdirectory='theme'}\n\
[[sources]]\nid='source:content'\nlocation={kind='repository',root='/not-opened/content',commit='0000000000000000000000000000000000000000',subdirectory='content'}\n\
[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n\
[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:content',target='authored'}]\n";
    let theme = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n\
[components.callout]\ntemplate='callout.html'\n\
arguments={type='object',required=['tone'],properties={caption={type='string'},tone={type='string',enum=['note','warning']}},additionalProperties=false}\n\
[components.figure]\ntemplate='figure.html'\n";
    let mut value = selected(&[(uri, "authored/page.md", page)]);
    value["params"]["initializationOptions"]["workspace"]["text"] = json!(workspace);
    value["params"]["initializationOptions"]["package_baselines"] = json!([{
        "kind":"theme",
        "package":"theme:one",
        "uri":"untitled:theme.toml",
        "path":"themes/main/theme.toml",
        "text":theme
    }]);
    value
}

#[test]
fn component_argument_completion_uses_theme_schema_and_exact_utf16_edits() {
    let uri = "untitled:component-arguments";
    let property_page = "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n\n:::callout {caption='🦀', to='n'}\nBody\n:::\n";
    let property_start = property_page.find("to='n'").unwrap();
    let (result, messages) = selected_session(
        component_selection(uri, property_page),
        [
            open_content(uri, 11, property_page),
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":uri},"position":point(property_page,property_start+2)}),
            ),
        ],
        Limits::default(),
    );
    result.expect("component property completion protocol");
    let items = response(&messages, 2)["result"]["items"]
        .as_array()
        .expect("property candidates")
        .clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "tone");
    assert_eq!(items[0]["filterText"], "tone");
    assert_eq!(items[0]["kind"], 5);
    assert_eq!(items[0]["data"]["preimage"], "to");
    assert_eq!(
        items[0]["textEdit"]["range"],
        json!({"start":point(property_page,property_start),"end":point(property_page,property_start+2)})
    );

    let value_page = property_page.replace("to='n'", "tone='n'");
    let value_start = value_page.find("'n'").unwrap();
    let (result, messages) = selected_session(
        component_selection(uri, &value_page),
        [
            open_content(uri, 12, &value_page),
            request(
                3,
                "textDocument/completion",
                json!({"textDocument":{"uri":uri},"position":point(&value_page,value_start+2)}),
            ),
        ],
        Limits::default(),
    );
    result.expect("component value completion protocol");
    let items = response(&messages, 3)["result"]["items"]
        .as_array()
        .expect("closed value candidates")
        .clone();
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["note", "warning"]
    );
    for item in items {
        assert_eq!(item["kind"], 12);
        assert_eq!(
            item["filterText"],
            format!("'{}", item["label"].as_str().unwrap())
        );
        assert_eq!(item["data"]["preimage"], "'n'");
        assert_eq!(item["data"]["version"], 12);
        assert_eq!(
            item["textEdit"]["range"],
            json!({"start":point(&value_page,value_start),"end":point(&value_page,value_start+3)})
        );
    }
}

#[test]
fn component_completion_projects_the_selected_theme_catalog_with_exact_edit_range() {
    let uri = "untitled:component";
    let page = "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n\n:::call {tone='note'}\nBody\n:::\n";
    let start = page.find(":::call").expect("component") + 3;
    let offset = start + "call".len();
    let (result, messages) = selected_session(
        component_selection(uri, page),
        [
            open_content(uri, 7, page),
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":uri},"position":point(page,offset)}),
            ),
        ],
        Limits::default(),
    );
    result.expect("component completion protocol");
    let completion = response(&messages, 2);
    let items = completion["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("component catalog: {completion}"));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().expect("label"))
            .collect::<Vec<_>>(),
        ["callout", "figure"]
    );
    for item in items {
        assert_eq!(item["kind"], 7);
        assert_eq!(item["data"]["preimage"], "call");
        assert_eq!(item["data"]["version"], 7);
        assert_eq!(
            item["textEdit"]["range"],
            json!({"start":point(page,start),"end":point(page,offset)})
        );
    }
}

#[test]
fn authored_frontmatter_completion_uses_current_schema_and_full_file_utf16_ranges() {
    let uri = "untitled:article";
    let text = "+++\n# é🦀\n[content]\nlifecycle = \n+++\n# Body\n";
    let init = selected(&[(uri, "articles/example.md", text)]);
    let (result, messages) = selected_session(
        init,
        [
            open(uri, 4, text),
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":uri},"position":{"line":3,"character":11}}),
            ),
        ],
        Limits::default(),
    );
    result.expect("frontmatter completion protocol");
    let completion = response(&messages, 2);
    let items = completion["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("closed lifecycle literals: {completion}"));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().expect("label"))
            .collect::<Vec<_>>(),
        ["archived", "draft", "published"]
    );
    for item in items {
        assert_eq!(item["data"]["scope"], "authored_frontmatter");
        assert_eq!(item["data"]["version"], 4);
        let start = usize::try_from(item["data"]["location"]["span"]["start"].as_u64().unwrap())
            .expect("start");
        let end = usize::try_from(item["data"]["location"]["span"]["end"].as_u64().unwrap())
            .expect("end");
        assert_eq!(item["data"]["preimage"], &text[start..end]);
        assert_eq!(
            item["textEdit"]["range"],
            json!({"start":point(text,start),"end":point(text,end)})
        );
    }
}

#[test]
fn framed_unresolved_reference_edit_repairs_current_diagnostics_without_namespace_guessing() {
    let text = text().replace("publisher='owner:one'", "publisher='owner:typo'")
        + "projects=[{id='owner:decoy',lifecycle='active',visibility='public',availability='available'}]\r\n";
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let offset = text.rfind("owner:typo").unwrap();
    let query = complete(2, point(&text, offset));
    let (result, messages) = selected_session(
        init.clone(),
        [open(URI, 7, &text), query.clone()],
        Limits::default(),
    );
    result.expect("framed current typo session");
    let items = response(&messages, 2)["result"]["items"]
        .as_array()
        .expect("unresolved principal candidates");
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["owner:one", "owner:two"]
    );
    let item = &items[0];
    assert_eq!(item["data"]["version"], 7);
    assert_eq!(item["data"]["preimage"], "'owner:typo'");
    assert_eq!(
        item["data"]["source_sha256"],
        json!(gazette_sdk::source::Sha256Digest::of(text.as_bytes()))
    );
    assert_eq!(
        item["textEdit"]["range"],
        json!({"start":point(&text,offset-1),"end":point(&text,offset+"owner:typo'".len())})
    );
    let start =
        usize::try_from(item["data"]["location"]["span"]["start"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(item["data"]["location"]["span"]["end"].as_u64().unwrap()).unwrap();
    assert_eq!(&text[start..end], "'owner:typo'");
    let mut repaired = text.clone();
    repaired.replace_range(start..end, item["textEdit"]["newText"].as_str().unwrap());
    // Replay the same held protocol sequence, applying the edit actually returned above.
    let (result, repaired_messages) = selected_session(
        init,
        [
            open(URI, 7, &text),
            query,
            change(URI, 8, &repaired),
            complete(3, point(&repaired, start + 1)),
        ],
        Limits::default(),
    );
    result.expect("framed repair lifecycle");
    assert_eq!(
        response(&repaired_messages, 2)["result"],
        response(&messages, 2)["result"]
    );
    assert_eq!(
        response(&repaired_messages, 3)["result"]["items"][0]["data"]["version"],
        8
    );
    for (version, expected_error) in [(7, true), (8, false)] {
        let diagnostic = repaired_messages
            .iter()
            .find(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == URI
                    && message["params"]["version"] == version
            })
            .expect("current version diagnostics");
        let rows = diagnostic["params"]["diagnostics"].as_array().unwrap();
        if expected_error {
            assert!(rows.iter().any(|row| row["code"] == "workspace_reference"));
        } else {
            assert!(rows.is_empty(), "{diagnostic}");
        }
    }
}

#[test]
fn framed_complete_replacement_has_exact_preimage_namespace_and_current_version() {
    let text = text();
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let offset = text.rfind("owner:one").expect("reference");
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 7, &text),
            complete(2, point(&text, offset)),
            complete(
                3,
                point(&text, text.find("owner:\\u006fne").expect("declaration")),
            ),
            complete(4, json!({"line":0,"character":4})),
            request(
                5,
                "textDocument/completion",
                json!({"textDocument":[],"position":[]}),
            ),
            change(URI, 8, "[broken\n"),
            complete(6, json!({"line":0,"character":0})),
            change(URI, 7, &text),
            complete(7, json!({"line":0,"character":0})),
            close(URI),
            complete(8, point(&text, offset)),
        ],
        Limits::default(),
    );
    result.expect("framed lifecycle");
    assert_eq!(
        response(&messages, 1)["result"]["capabilities"]["completionProvider"]["resolveProvider"],
        false
    );
    let list = &response(&messages, 2)["result"];
    assert_eq!(list["isIncomplete"], false);
    let items = list["items"].as_array().expect("complete list");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["label"], "owner:one");
    assert_eq!(items[1]["label"], "owner:two");
    for (item, replacement) in items.iter().zip(["\"owner:\\u006fne\"", "'owner:two'"]) {
        assert_eq!(item["textEdit"]["newText"], replacement);
        assert_eq!(
            item["filterText"],
            format!("'{}", item["label"].as_str().unwrap())
        );
        assert_eq!(
            item["textEdit"]["range"],
            json!({"start":point(&text,offset-1),"end":point(&text,offset+"owner:one'".len())})
        );
        assert_eq!(item["data"]["preimage"], "'owner:one'");
        assert_eq!(item["data"]["version"], 7);
        assert_eq!(
            item["data"]["source_sha256"],
            json!(gazette_sdk::source::Sha256Digest::of(text.as_bytes()))
        );
    }
    assert!(response(&messages, 3)["result"].is_null());
    for id in [4, 5] {
        assert_eq!(response(&messages, id)["error"]["code"], -32602);
    }
    for id in [6, 7] {
        let current = &response(&messages, id)["result"];
        if !current.is_null() {
            let items = current["items"]
                .as_array()
                .expect("current schema completions");
            assert!(!items.is_empty());
            for item in items {
                assert!(item["data"].get("symbol").is_none());
                assert_eq!(item["data"]["version"], 8);
                assert_eq!(
                    item["data"]["source_sha256"],
                    json!(gazette_sdk::source::Sha256Digest::of(b"[broken\n"))
                );
            }
        }
    }
    assert_eq!(
        response(&messages, 8)["result"]["items"][0]["data"]["version"],
        Value::Null
    );
}

#[test]
fn replacement_response_ceiling_refuses_the_whole_list_not_a_successful_prefix() {
    let identity = format!("owner:{}", "x".repeat(2048));
    let text = text().replace("owner:two", &identity);
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let request = complete(2, point(&text, text.rfind("owner:one").expect("reference")));
    let (result, full) = selected_session(init.clone(), [request.clone()], Limits::default());
    result.expect("full native list");
    assert_eq!(
        response(&full, 2)["result"]["items"]
            .as_array()
            .expect("items")
            .len(),
        2
    );
    let (result, bounded) = selected_session(
        init,
        [request],
        Limits {
            output_bytes: 1280,
            ..Default::default()
        },
    );
    result.expect("bounded refusal still completes protocol");
    assert_eq!(response(&bounded, 2)["error"]["code"], -32600);
    assert!(response(&bounded, 2).get("result").is_none());
}

#[test]
fn multiline_native_reference_remains_an_exact_sdk_proposal_not_an_illegal_lsp_edit() {
    let text = text().replace("publisher='owner:one'", "publisher=\"\"\"\nowner:one\"\"\"");
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let offset = text.rfind("owner:one").expect("multiline scalar");
    let native = gazette_sdk::authoring::analyze(
        gazette_sdk::authoring::Input {
            path: "workspace.toml".parse().expect("path"),
            bytes: text.as_bytes(),
        },
        [],
        Default::default(),
    )
    .expect("shared analysis");
    assert!(
        native
            .complete_reference(
                &"workspace.toml".parse().expect("path"),
                &text,
                offset,
                1 << 18
            )
            .expect("native scalar")
            .is_some()
    );
    let (result, messages) =
        selected_session(init, [complete(2, point(&text, offset))], Limits::default());
    result.expect("legal protocol refusal");
    assert_eq!(response(&messages, 2)["error"]["code"], -32600);
    assert!(response(&messages, 2).get("result").is_none());
}

#[test]
fn incomplete_workspace_uses_schema_completion_with_exact_utf16_edits_and_no_stale_model() {
    let text = "# é🦀\r\n[[sites]]\r\nid='site:one'\r\npublisher='owner:one'\r\ndep";
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let start = text.rfind("dep").expect("partial key");
    let value = "# é🦀\r\n[[sites]]\r\ndeployable = ";
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 1, text),
            complete(2, point(text, text.len())),
            change(URI, 2, value),
            complete(3, point(value, value.len())),
            change(URI, 3, "[[sites]]\nunknown = "),
            complete(4, json!({"line":1,"character":10})),
        ],
        Limits::default(),
    );
    result.expect("schema assistance protocol");
    let key = &response(&messages, 2)["result"]["items"];
    assert_eq!(key.as_array().expect("key candidates").len(), 1);
    assert_eq!(key[0]["label"], "deployable");
    assert_eq!(key[0]["detail"], "required field");
    assert_eq!(key[0]["data"]["version"], 1);
    assert_eq!(key[0]["data"]["preimage"], "dep");
    assert_eq!(
        key[0]["textEdit"]["range"],
        json!({"start":point(text,start),"end":point(text,text.len())})
    );
    let literal = response(&messages, 3)["result"]["items"]
        .as_array()
        .expect("literal candidates");
    assert_eq!(
        literal
            .iter()
            .map(|item| item["label"].as_str().expect("label"))
            .collect::<Vec<_>>(),
        ["false", "true"]
    );
    assert!(literal.iter().all(|item| item["data"]["version"] == 2));
    assert_eq!(response(&messages, 4)["error"]["code"], -32600);
}

#[path = "completion/previous.rs"]
mod previous;
