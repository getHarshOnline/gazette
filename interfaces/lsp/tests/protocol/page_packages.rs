//! Page-package diagnostics project the compiler's retained-input result.

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;

const PAGE_URI: &str = "file:///selected/content/guide/index.md";
const WORKSPACE_WITH_CONTENT: &str = r#"
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='Example'
[[brands.publications]]
site='site:one'
authored=[{source='source:content',target='content'}]
[[sources]]
id='source:content'
location={kind='filesystem',root='/not-opened/content'}
"#;

#[test]
fn ownership_is_context_while_unhandled_companions_remain_diagnostics() {
    let page = content("content:guide", "", "kind='page'\nfamily={kind='article'}");
    let mut initialize = selected(&[(PAGE_URI, "content/guide/index.md", &page)]);
    initialize["params"]["initializationOptions"]["workspace"]["text"] =
        json!(WORKSPACE_WITH_CONTENT);
    initialize["params"]["initializationOptions"]["page_inputs"] = json!([{
        "uri":"file:///selected/content/guide/private.bin",
        "path":"content/guide/private.bin",
        "bytes_base64":STANDARD.encode([0_u8, 159, 146, 150, 255]),
    }]);

    let (result, messages) = selected_session(
        initialize,
        [request(
            2,
            "gazette/authoringContext",
            json!({"generation":1,"textDocument":{"uri":PAGE_URI},"position":{"line":0,"character":0}}),
        )],
        Limits::default(),
    );
    result.expect("page-package LSP session");
    let diagnostics = messages
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == PAGE_URI
        })
        .expect("owning index diagnostics")["params"]["diagnostics"]
        .as_array()
        .expect("diagnostic list");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["code"] != "page_package_owner")
    );
    let owner = &super::footnotes::response(&messages, 2)["result"]["page_package"];
    assert_eq!(owner["status"], "available");
    assert_eq!(
        owner["value"]["package"]["document"],
        "content/guide/index.md"
    );
    assert_eq!(
        owner["value"]["package"]["companions"][0]["source_sha256"],
        gazette_sdk::source::Sha256Digest::of(&[0_u8, 159, 146, 150, 255]).to_string()
    );
    let unhandled = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == "page_package_unhandled_companion")
        .expect("unhandled companion warning");
    assert_eq!(unhandled["severity"], 2);
    assert_eq!(unhandled["data"]["companion"]["relative"], "private.bin");
}

#[test]
fn resource_completion_and_package_inspection_use_compiler_owned_paths_and_outputs() {
    let page = content(
        "content:guide",
        "[[resources]]\ninputs=['guide/main.ts','guide/lib/dep.ts']\nbuild={kind='browser-module',entry='guide/main.ts'}\noutput='/custom/page.js'\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let mut initialize = selected(&[(PAGE_URI, "content/guide/index.md", &page)]);
    initialize["params"]["initializationOptions"]["workspace"]["text"] =
        json!(WORKSPACE_WITH_CONTENT);
    initialize["params"]["initializationOptions"]["page_inputs"] = json!([
        {"uri":"file:///selected/content/guide/main.ts","path":"content/guide/main.ts","bytes_base64":STANDARD.encode(b"import './lib/dep.ts';")},
        {"uri":"file:///selected/content/guide/lib/dep.ts","path":"content/guide/lib/dep.ts","bytes_base64":STANDARD.encode(b"export const answer=42;")},
    ]);
    let offset = page.find("'guide/main.ts'").expect("resource input") + 2;
    let before = &page[..offset];
    let position = json!({
        "line":before.matches('\n').count(),
        "character":before.rsplit('\n').next().expect("line").encode_utf16().count(),
    });
    let (result, messages) = selected_session(
        initialize,
        [
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":PAGE_URI},"position":position}),
            ),
            request(
                3,
                "gazette/pagePackage",
                json!({"generation":1,"document":"file:///selected/content/guide/main.ts"}),
            ),
            request(
                4,
                "textDocument/definition",
                json!({"textDocument":{"uri":PAGE_URI},"position":position}),
            ),
        ],
        Limits::default(),
    );
    result.expect("page resource authoring protocol");
    let completion = super::footnotes::response(&messages, 2);
    let items = completion["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("resource candidates: {completion}"));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["guide/lib/dep.ts", "guide/main.ts"]
    );
    assert_eq!(
        items[0]["data"]["compiled_destinations"][0],
        "/custom/page.js"
    );
    let inspection = &super::footnotes::response(&messages, 3)["result"];
    assert_eq!(inspection["retained_source"]["uri"], PAGE_URI);
    assert_eq!(inspection["ownership"]["role"]["kind"], "companion");
    assert_eq!(
        inspection["compiled_destinations"][0]["output"],
        "/custom/page.js"
    );
    let definition = &super::footnotes::response(&messages, 4)["result"];
    assert_eq!(definition["uri"], "file:///selected/content/guide/main.ts");
    assert_eq!(
        definition["range"]["start"],
        json!({"line":0,"character":0})
    );
    assert_eq!(definition["range"]["end"], json!({"line":0,"character":0}));
}

#[test]
fn duplicate_resource_outputs_remain_structured_native_diagnostics() {
    let page = content(
        "content:guide",
        "[[resources]]\ninputs=['guide/one.ts']\nbuild={kind='browser-module',entry='guide/one.ts'}\noutput='/same.js'\n[[resources]]\ninputs=['guide/two.ts']\nbuild={kind='browser-module',entry='guide/two.ts'}\noutput='/same.js'\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let mut initialize = selected(&[(PAGE_URI, "content/guide/index.md", &page)]);
    initialize["params"]["initializationOptions"]["workspace"]["text"] =
        json!(WORKSPACE_WITH_CONTENT);
    initialize["params"]["initializationOptions"]["page_inputs"] = json!([
        {"uri":"file:///selected/content/guide/one.ts","path":"content/guide/one.ts","bytes_base64":STANDARD.encode(b"one")},
        {"uri":"file:///selected/content/guide/two.ts","path":"content/guide/two.ts","bytes_base64":STANDARD.encode(b"two")},
    ]);
    let (result, messages) = selected_session(initialize, [], Limits::default());
    result.expect("resource collision protocol");
    let diagnostics = messages
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == PAGE_URI
        })
        .expect("page diagnostics")["params"]["diagnostics"]
        .as_array()
        .unwrap();
    let collision = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("same output"))
        })
        .expect("native resource collision");
    assert!(collision["code"].is_string());
    assert_eq!(collision["data"]["generation"], 1);
    assert_eq!(
        collision["data"]["source_sha256"],
        json!(gazette_sdk::source::Sha256Digest::of(page.as_bytes()))
    );
}
