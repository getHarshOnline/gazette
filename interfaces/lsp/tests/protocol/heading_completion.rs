//! Real framed heading edits use compiler anchors, current text and UTF-16 ranges.

use super::{
    footnotes::{position, response},
    *,
};

const PAGE: &str = "untitled:heading-completion.md";

fn query(id: i32, source: &str, needle: &str) -> Value {
    request(
        id,
        "textDocument/completion",
        json!({"textDocument":{"uri":PAGE},"position":position(source,needle,0)}),
    )
}

#[test]
fn heading_edit_preserves_unicode_preimage_and_resolves_after_current_overlay_change() {
    let source = format!(
        "{}\n# Repeat\n\n# Repeat\n\n## Custom {{#\"résumé\"}}\n\n😀 [go](#missing)\n",
        content("content:one", "", "kind='page'\nfamily={kind='article'}")
    )
    .replace('\n', "\r\n");
    let init = selected(&[(PAGE, "page.md", &source)]);
    let (result, messages) = selected_session(
        init.clone(),
        [
            open_content(PAGE, 7, &source),
            query(2, &source, "#missing"),
        ],
        Limits::default(),
    );
    result.unwrap();
    let list = &response(&messages, 2)["result"];
    assert_eq!(list["isIncomplete"], false);
    let items = list["items"].as_array().unwrap();
    let item = items
        .iter()
        .find(|item| item["label"] == "#r%C3%A9sum%C3%A9")
        .unwrap();
    assert_eq!(item["data"]["preimage"], "#missing");
    assert_eq!(item["data"]["version"], 7);
    assert_eq!(
        item["data"]["source_sha256"],
        json!(gazette_sdk::source::Sha256Digest::of(source.as_bytes()))
    );
    assert_eq!(
        item["textEdit"]["range"]["start"],
        position(&source, "#missing", 0)
    );
    let start = item["data"]["location"]["span"]["start"].as_u64().unwrap() as usize;
    let end = item["data"]["location"]["span"]["end"].as_u64().unwrap() as usize;
    assert_eq!(&source[start..end], "#missing");
    let mut changed = source.clone();
    changed.replace_range(start..end, item["textEdit"]["newText"].as_str().unwrap());
    let (result, messages) = selected_session(
        init,
        [
            open_content(PAGE, 7, &source),
            change(PAGE, 8, &changed),
            query(3, &changed, "#r%C3%A9sum%C3%A9"),
            footnotes::definition(4, PAGE, position(&changed, "[go]", 0)),
            change(PAGE, 7, &source),
            query(5, &changed, "#r%C3%A9sum%C3%A9"),
        ],
        Limits::default(),
    );
    result.unwrap();
    assert_eq!(
        response(&messages, 3)["result"]["items"][0]["data"]["version"],
        8
    );
    assert_eq!(
        response(&messages, 4)["result"]["range"]["start"],
        position(&changed, "## Custom", 0)
    );
    assert_eq!(
        response(&messages, 3)["result"],
        response(&messages, 5)["result"],
        "stale editor version cannot replace current proposals"
    );
}

#[test]
fn heading_completion_refuses_partial_response_and_unsupported_or_invalid_positions() {
    let source = format!(
        "# {}\n\n😀 [go](#)\n[title](#a \"title\")\n![image](#a)\n[external](other.md#a)\n",
        "A".repeat(2048)
    );
    let init = selected(&[(PAGE, "page.md", &source)]);
    let (result, messages) = selected_session(
        init.clone(),
        [
            query(2, &source, "#)"),
            query(3, &source, "#a \"title\""),
            query(4, &source, "#a)"),
            query(5, &source, "other.md#a"),
            request(
                6,
                "textDocument/completion",
                json!({"textDocument":{"uri":PAGE},"position":{"line":2,"character":1}}),
            ),
        ],
        Limits::default(),
    );
    result.unwrap();
    assert_eq!(
        response(&messages, 2)["result"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for id in [3, 4, 5] {
        assert!(response(&messages, id)["result"].is_null());
    }
    assert_eq!(response(&messages, 6)["error"]["code"], -32602);
    let (result, messages) = selected_session(
        init,
        [query(2, &source, "#)")],
        Limits {
            output_bytes: 1280,
            ..Default::default()
        },
    );
    result.unwrap();
    assert_eq!(response(&messages, 2)["error"]["code"], -32600);
    assert!(response(&messages, 2).get("result").is_none());
}
