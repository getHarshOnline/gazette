//! Complete selected-source search through real framed LSP requests.

use super::{
    footnotes::{location, response},
    *,
};

fn search(id: i32, query: &str) -> Value {
    request(id, "workspace/symbol", json!({"query":query}))
}

#[test]
fn workspace_search_projects_current_selected_declarations_and_unicode_heading_ranges() {
    let uri = "file:///not-opened-by-gazette/r%C3%A9sum%C3%A9.md";
    uri.parse::<lsp_types::Uri>()
        .expect("standard encoded fixture URI");
    let other_uri = "untitled:other.md";
    let page = content("content:one", "", "kind='page'\nfamily={kind='article'}")
        .replace("# Héllo 😀", "# RÉSUMÉ 🦀")
        .replace('\n', "\r\n");
    let other = content("content:two", "", "kind='page'\nfamily={kind='article'}")
        .replace("# Héllo 😀", "# Other");
    let changed = page.replace("# RÉSUMÉ 🦀", "\r\n# RÉSUMÉ 😀 edited");
    let (result, messages) = selected_session(
        selected(&[(uri, "résumé.md", &page), (other_uri, "other.md", &other)]),
        [
            search(2, "résumé"),
            search(3, "OWNER:ONE"),
            search(4, ""),
            open_content(uri, 7, &changed),
            search(5, "résumé"),
            change(uri, 6, &page),
            search(6, "résumé"),
            close(uri),
            search(7, "résumé"),
            open_content("file:///not-selected.md", 1, &changed),
            search(8, "edited"),
        ],
        Limits::default(),
    );
    result.expect("real framed selected workspace search");
    assert_eq!(
        response(&messages, 1)["result"]["capabilities"]["workspaceSymbolProvider"],
        true
    );
    let expected =
        json!([{"name":"RÉSUMÉ 🦀","kind":3,"location":location(uri, &page, "# RÉSUMÉ 🦀", 0)}]);
    assert_eq!(response(&messages, 2)["result"], expected);
    assert_eq!(
        response(&messages, 3)["result"],
        json!([{
            "name":"owner:one","kind":19,"location":location(URI, WORKSPACE, "'owner:one'", 0)
        }])
    );
    assert_eq!(
        response(&messages, 4)["result"]
            .as_array()
            .expect("complete selected symbols")
            .len(),
        4
    );
    assert_eq!(
        response(&messages, 5)["result"],
        json!([{
            "name":"RÉSUMÉ 😀 edited","kind":3,"location":location(uri, &changed, "# RÉSUMÉ 😀 edited", 0)
        }])
    );
    assert_eq!(
        response(&messages, 6)["result"],
        response(&messages, 5)["result"],
        "stale overlay ignored"
    );
    assert_eq!(
        response(&messages, 7)["result"],
        expected,
        "close restores captured baseline, not disk"
    );
    assert_eq!(
        response(&messages, 8)["result"],
        json!([]),
        "unselected open never enters search"
    );
    for id in 2..=8 {
        assert!(response(&messages, id).get("error").is_none());
    }
}

#[test]
fn invalid_current_selected_source_refuses_whole_search_without_reusing_previous_matches() {
    let uri = "untitled:page.md";
    let page = content("content:one", "", "kind='page'\nfamily={kind='article'}");
    let (result, messages) = selected_session(
        selected(&[(uri, "page.md", &page)]),
        [
            search(2, "owner"),
            open_content(uri, 1, "+++\n[broken\n+++\n"),
            search(3, "owner"),
            close(uri),
            search(4, "owner"),
            request(5, "workspace/symbol", json!({})),
            request(6, "workspace/symbol", json!({"query":null})),
            request(7, "workspace/symbol", json!({"query":1})),
            request(8, "workspaceSymbol/resolve", json!({})),
        ],
        Limits::default(),
    );
    result.expect("failed current search is explicit protocol response");
    assert!(response(&messages, 2)["result"].is_array());
    assert_eq!(response(&messages, 3)["error"]["code"], -32600);
    assert!(response(&messages, 3).get("result").is_none());
    assert_eq!(
        response(&messages, 4)["result"],
        response(&messages, 2)["result"]
    );
    for id in 5..=7 {
        assert_eq!(response(&messages, id)["error"]["code"], -32602);
    }
    assert_eq!(
        response(&messages, 8)["error"]["code"],
        -32601,
        "no lazy resolution advertised or implemented"
    );
}

#[test]
fn workspace_search_overflow_returns_error_not_a_successful_subset() {
    let mut page = content("content:one", "", "kind='page'\nfamily={kind='article'}");
    for index in 0..100 {
        page.push_str(&format!("\n# Result {index}\n"));
    }
    let (result, messages) = selected_session(
        selected(&[("untitled:many.md", "many.md", &page)]),
        [search(2, "Result")],
        Limits {
            output_bytes: 2048,
            ..Limits::default()
        },
    );
    result.expect("small complete-result error fits normal framing");
    assert_eq!(response(&messages, 2)["error"]["code"], -32600);
    assert!(response(&messages, 2).get("result").is_none());
}
