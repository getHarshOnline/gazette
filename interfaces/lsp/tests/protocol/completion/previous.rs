use super::*;

fn selected_workspace() -> (String, Value) {
    let source = text();
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(source);
    (source, init)
}

fn incomplete(source: &str, value: &str) -> String {
    source.replace("publisher='owner:one'", &format!("publisher='{value}"))
}

#[test]
fn interrupted_reference_uses_last_parse_but_only_current_edit_coordinates() {
    let (source, init) = selected_workspace();
    let first = incomplete(&source, "owner:o");
    let next = incomplete(&source, "owner:t");
    let start = next.find("publisher=").unwrap() + "publisher=".len();
    let end = start + "'owner:t".len();
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 1, &source),
            change(URI, 2, &first),
            complete(2, point(&first, end)),
            change(URI, 3, &next),
            complete(3, point(&next, end)),
            request(
                4,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":URI}}),
            ),
        ],
        Limits::default(),
    );
    result.unwrap();
    for (id, version, current) in [(2, 2, first.as_str()), (3, 3, next.as_str())] {
        let value = response(&messages, id);
        let items = value["result"]["items"]
            .as_array()
            .unwrap_or_else(|| panic!("{value}"));
        assert_eq!(value["result"]["isIncomplete"], true);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["label"], "owner:one");
        assert_eq!(items[1]["label"], "owner:two");
        for item in items {
            assert_eq!(
                item["textEdit"]["range"],
                json!({"start":point(current,start),"end":point(current,end)})
            );
            assert_eq!(item["data"]["version"], version);
            assert_eq!(item["data"]["preimage"], &current[start..end]);
            assert_eq!(
                item["data"]["source_sha256"],
                json!(gazette_sdk::source::Sha256Digest::of(current.as_bytes()))
            );
            assert_eq!(
                item["data"]["basis_sha256"],
                json!(gazette_sdk::source::Sha256Digest::of(source.as_bytes()))
            );
            assert!(item["data"].get("definition").is_none());
            assert!(item["data"].get("symbol").is_none());
            assert!(
                item["data"]["basis_generation"].as_u64().unwrap()
                    < item["data"]["generation"].as_u64().unwrap()
            );
        }
    }
    assert!(
        response(&messages, 4).get("error").is_some(),
        "navigation must not reuse the parse"
    );
    let diagnostic = messages
        .iter()
        .rev()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == URI
        })
        .unwrap();
    assert_eq!(diagnostic["params"]["version"], 3);
    assert!(
        !diagnostic["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn current_unicode_ranges_are_exact_and_multiline_edits_are_refused() {
    let (source, init) = selected_workspace();
    let unicode = incomplete(&source, "owner:🦀");
    let start = unicode.find("publisher=").unwrap() + "publisher=".len();
    let end = start + "'owner:🦀".len();
    let multiline = incomplete(&source, "owner:\r\nt");
    let offset = start + "'owner:".len();
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 1, &source),
            change(URI, 2, &unicode),
            complete(2, point(&unicode, end)),
            change(URI, 3, &multiline),
            complete(3, point(&multiline, offset)),
        ],
        Limits::default(),
    );
    result.unwrap();
    let reply = response(&messages, 2);
    let items = reply["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{reply}"));
    for item in items {
        assert_eq!(
            item["textEdit"]["range"],
            json!({"start":point(&unicode,start),"end":point(&unicode,end)})
        );
        assert_eq!(item["data"]["preimage"], "'owner:🦀");
    }
    assert!(response(&messages, 3).get("error").is_some());
}

#[test]
fn changed_declarations_or_another_document_discard_previous_candidates() {
    let (source, mut init) = selected_workspace();
    let broken = incomplete(&source, "owner:t");
    let offset = broken.find("publisher=").unwrap() + "publisher='owner:t".len();
    init["params"]["initializationOptions"]["documents"] =
        json!([{"uri":"untitled:page","path":"page.md","text":"# Page\n"}]);
    let changed_declaration = broken.replace("owner:two", "owner:new");
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 1, &source),
            change(URI, 2, &broken),
            complete(2, point(&broken, offset)),
            change(URI, 3, &changed_declaration),
            complete(3, point(&changed_declaration, offset)),
            change(URI, 4, &source),
            complete(4, point(&source, offset)),
            change(URI, 5, &broken),
            complete(5, point(&broken, offset)),
            open_content("untitled:page", 1, "# Changed\n"),
            complete(6, point(&broken, offset)),
        ],
        Limits::default(),
    );
    result.unwrap();
    for id in [2, 5] {
        assert_eq!(
            response(&messages, id)["result"]["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    for id in [3, 6] {
        assert!(response(&messages, id)["result"]["items"].is_null());
    }
    assert!(
        response(&messages, 4)["result"]["items"][0]["data"]
            .get("basis_sha256")
            .is_none()
    );
}

#[test]
fn save_and_close_do_not_keep_previous_completion_history() {
    let (source, init) = selected_workspace();
    let broken = incomplete(&source, "owner:t");
    let offset = broken.find("publisher=").unwrap() + "publisher='owner:t".len();
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 1, &source),
            change(URI, 2, &broken),
            complete(2, point(&broken, offset)),
            notification(
                "textDocument/didSave",
                json!({"textDocument":{"uri":URI},"text":broken}),
            ),
            complete(3, point(&broken, offset)),
            close(URI),
            open(URI, 4, &broken),
            complete(4, point(&broken, offset)),
        ],
        Limits::default(),
    );
    result.unwrap();
    assert!(response(&messages, 2)["result"]["items"].is_array());
    for id in [3, 4] {
        assert!(response(&messages, id)["result"]["items"].is_null());
    }
}

#[test]
fn completion_history_yields_to_the_existing_source_budget() {
    let (source, init) = selected_workspace();
    let broken = incomplete(&source, "owner:t");
    let offset = broken.find("publisher=").unwrap() + "publisher='owner:t".len();
    let limits = Limits {
        source_bytes: source.len() * 2,
        ..Limits::default()
    };
    let (result, messages) = selected_session(
        init,
        [
            open(URI, 1, &source),
            change(URI, 2, &broken),
            complete(2, point(&broken, offset)),
        ],
        limits,
    );
    result.unwrap();
    assert!(response(&messages, 2)["result"]["items"].is_null());
    assert!(messages.iter().any(
        |message| message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["version"] == 2
    ));
}

#[test]
fn native_discovery_keeps_completion_through_an_unsaved_reference_typo() {
    let root = tempfile::tempdir().unwrap();
    let source = text();
    std::fs::write(root.path().join("gazette.toml"), &source).unwrap();
    let root_uri = url::Url::from_directory_path(root.path())
        .unwrap()
        .to_string();
    let uri = url::Url::from_file_path(root.path().join("gazette.toml"))
        .unwrap()
        .to_string();
    let init = request(
        1,
        "initialize",
        json!({"capabilities":{},"rootUri":root_uri,
        "initializationOptions":{"native_selection":{"allow_read":true,"discover":true,"workspace":"gazette.toml"}}}),
    );
    let broken = incomplete(&source, "owner:t");
    let offset = broken.find("publisher=").unwrap() + "publisher='owner:t".len();
    let (result, messages) = selected_session(
        init,
        [
            open(&uri, 1, &source),
            change(&uri, 2, &broken),
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":uri},"position":point(&broken,offset)}),
            ),
        ],
        Limits::default(),
    );
    result.unwrap();
    let reply = response(&messages, 2);
    assert_eq!(
        reply["result"]["items"]
            .as_array()
            .unwrap_or_else(|| panic!("{reply}"))
            .len(),
        2
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("gazette.toml")).unwrap(),
        source
    );
}
