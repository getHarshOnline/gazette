//! Framed code actions remain current, reviewable projections of shared repairs.

use super::{footnotes::response, *};

fn point(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    json!({
        "line":before.matches('\n').count(),
        "character":before.rsplit('\n').next().expect("line").encode_utf16().count()
    })
}

fn code_action(id: i32, range: Value, diagnostics: Value, only: Option<&str>) -> Value {
    let mut context = json!({"diagnostics": diagnostics});
    if let Some(kind) = only {
        context["only"] = json!([kind]);
    }
    request(
        id,
        "textDocument/codeAction",
        json!({"textDocument":{"uri":URI},"range":range,"context":context}),
    )
}

fn cursor(text: &str, offset: usize) -> Value {
    let position = point(text, offset);
    json!({"start":position,"end":position})
}

fn current_diagnostic(messages: &[Value], version: i32) -> Value {
    messages
        .iter()
        .rfind(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == URI
                && message["params"]["version"] == version
        })
        .expect("current publication")["params"]["diagnostics"]
        .as_array()
        .expect("diagnostic array")
        .iter()
        .find(|row| row["code"] == "workspace_reference")
        .expect("unresolved reference")
        .clone()
}

#[test]
fn current_diagnostic_produces_versioned_review_actions_without_applying_them() {
    let invalid = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'")
        + "[[principals]]\nid='owner:two'\nkind='person'\n";
    let offset = invalid.find("owner:missing").expect("typed reference");
    let (result, first) = session([open(URI, 7, &invalid)], Limits::default());
    result.expect("current diagnostic fixture");
    let diagnostic = current_diagnostic(&first, 7);
    let (result, messages) = session(
        [
            open(URI, 7, &invalid),
            code_action(
                2,
                cursor(&invalid, offset + 2),
                json!([diagnostic]),
                Some("quickfix"),
            ),
        ],
        Limits::default(),
    );
    result.expect("review action protocol");
    let actions = response(&messages, 2)["result"]
        .as_array()
        .expect("review actions");
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions
            .iter()
            .map(|action| action["title"].as_str().expect("title"))
            .collect::<Vec<_>>(),
        [
            "Review Gazette replacement: owner:one",
            "Review Gazette replacement: owner:two"
        ]
    );
    for action in actions {
        assert_eq!(action["kind"], "quickfix");
        assert_eq!(action["isPreferred"], false);
        assert_eq!(action["data"]["reviewRequired"], true);
        assert_eq!(action["data"]["generation"], 2);
        assert!(action.get("command").is_none());
        assert_eq!(action["diagnostics"][0]["code"], "workspace_reference");
        let edit = &action["edit"]["documentChanges"][0];
        assert_eq!(edit["textDocument"], json!({"uri":URI,"version":7}));
        assert_eq!(edit["edits"].as_array().expect("one edit").len(), 1);
        assert_eq!(
            edit["edits"][0]["range"],
            json!({
                "start":point(&invalid,offset-1),
                "end":point(&invalid,offset+"owner:missing".len()+1)
            })
        );
    }
    assert_eq!(
        messages
            .iter()
            .filter(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["version"] == 7
            })
            .count(),
        1,
        "returning review actions does not change the selected source"
    );
}

#[test]
fn stale_invented_unrelated_and_unassisted_diagnostics_offer_no_action() {
    let invalid = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let offset = invalid.find("owner:missing").expect("typed reference");
    let (result, first) = session([open(URI, 7, &invalid)], Limits::default());
    result.expect("diagnostic fixture");
    let diagnostic = current_diagnostic(&first, 7);
    let mut invented = diagnostic.clone();
    invented["message"] = json!("invented client message");
    let (result, messages) = session(
        [
            open(URI, 7, &invalid),
            code_action(
                2,
                cursor(&invalid, offset + 2),
                json!([diagnostic.clone()]),
                Some("source"),
            ),
            change(URI, 8, WORKSPACE),
            code_action(3, cursor(WORKSPACE, 0), json!([diagnostic]), None),
            change(URI, 9, &invalid),
            code_action(4, cursor(&invalid, offset + 2), json!([invented]), None),
            code_action(5, cursor(&invalid, offset + 2), json!([]), None),
        ],
        Limits::default(),
    );
    result.expect("bounded refusals");
    for id in [2, 3, 4, 5] {
        assert_eq!(response(&messages, id)["result"], json!([]));
    }
}

#[test]
fn review_actions_exclude_the_diagnostic_end_and_later_positions() {
    let invalid = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let (result, first) = session([open(URI, 7, &invalid)], Limits::default());
    result.expect("diagnostic fixture");
    let diagnostic = current_diagnostic(&first, 7);
    let end = diagnostic["range"]["end"].clone();
    let mut after = end.clone();
    after["character"] = json!(end["character"].as_u64().expect("end character") + 1);
    let point_range = |position: Value| json!({"start":position,"end":position});
    let (result, messages) = session(
        [
            open(URI, 7, &invalid),
            code_action(2, point_range(end), json!([diagnostic.clone()]), None),
            code_action(3, point_range(after), json!([diagnostic]), None),
        ],
        Limits::default(),
    );
    result.expect("half-open range requests");
    assert_eq!(response(&messages, 2)["result"], json!([]));
    assert_eq!(response(&messages, 3)["result"], json!([]));
}

#[test]
fn broad_selection_cannot_bind_an_unrelated_later_diagnostic_to_a_valid_reference() {
    let page_uri = "file:///selected/one.md";
    let source = content("content:one", "", "kind='page'\nfamily={kind='article'}")
        .replace("lifecycle='draft'", "lifecycle='typo'")
        .replace('\n', "\r\n");
    let site_offset = source.find("'site:one'").expect("valid native site scalar") + 2;
    let lifecycle_offset = source.find("'typo'").expect("invalid closed literal") + 2;
    let initialize = selected(&[(page_uri, "one.md", &source)]);
    let (result, first) = selected_session(initialize.clone(), [], Limits::default());
    result.expect("actual current authored diagnostic fixture");
    let diagnostic = first
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == page_uri
        })
        .expect("selected page diagnostics")["params"]["diagnostics"]
        .as_array()
        .expect("native diagnostic array")
        .iter()
        .find(|diagnostic| {
            diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("typo"))
        })
        .expect("native unknown lifecycle diagnostic")
        .clone();
    assert!(diagnostic["data"]["location"].is_object());
    assert!(
        diagnostic["data"]["location"]["span"]["start"]
            .as_u64()
            .expect("native diagnostic start")
            > site_offset as u64,
        "the diagnostic belongs to the later lifecycle, not the valid site"
    );
    let action = |id, range| {
        let mut call = code_action(id, range, json!([diagnostic.clone()]), None);
        call["params"]["textDocument"]["uri"] = json!(page_uri);
        call
    };
    let (result, messages) = selected_session(
        initialize,
        [
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":page_uri},"position":point(&source, site_offset)}),
            ),
            action(
                3,
                json!({"start":point(&source, site_offset),"end":point(&source, source.len())}),
            ),
            action(4, cursor(&source, lifecycle_offset)),
        ],
        Limits::default(),
    );
    result.expect("framed source-bound repair checks retain protocol lifetime");
    assert!(
        !response(&messages, 2)["result"]["items"]
            .as_array()
            .expect("valid site still has native candidates")
            .is_empty()
    );
    assert_eq!(response(&messages, 3)["result"], json!([]));
    let actions = response(&messages, 4)["result"]
        .as_array()
        .expect("actual lifecycle repair actions");
    assert!(
        !actions.is_empty(),
        "the correctly positioned repair survives"
    );
    for action in actions {
        assert_eq!(action["diagnostics"], json!([diagnostic.clone()]));
        assert_eq!(action["data"]["reviewRequired"], true);
        assert!(action.get("command").is_none());
    }
}

#[test]
fn unlocated_workspace_findings_bind_only_one_exact_native_reference_identity() {
    let one = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    for (text, queried) in [
        (
            format!("{one}[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n"),
            "publisher='owner:one'",
        ),
        (
            format!("{one}[[sites]]\nid='site:two'\npublisher='owner:missing'\ndeployable=false\n"),
            "publisher='owner:missing'",
        ),
    ] {
        let (result, first) = session([open(URI, 7, &text)], Limits::default());
        result.expect("native unlocated diagnostic fixture");
        let diagnostic = current_diagnostic(&first, 7);
        assert!(diagnostic["data"]["location"].is_null());
        assert_eq!(diagnostic["data"]["selection_wide"], false);
        let offset = text.find(queried).unwrap() + "publisher='".len();
        let (result, messages) = session(
            [
                open(URI, 7, &text),
                request(
                    2,
                    "textDocument/completion",
                    json!({"textDocument":{"uri":URI},"position":point(&text,offset)}),
                ),
                code_action(3, cursor(&text, offset), json!([diagnostic]), None),
            ],
            Limits::default(),
        );
        result.expect("unlocated identity refusal preserves framed lifetime");
        assert!(
            !response(&messages, 2)["result"]["items"]
                .as_array()
                .expect("ordinary native completion remains available")
                .is_empty()
        );
        assert_eq!(response(&messages, 3)["result"], json!([]));
    }
}

#[test]
fn ambiguous_modular_finding_never_becomes_a_local_source_quickfix() {
    let root = format!("includes=['sites/other.toml']\n{WORKSPACE}");
    let included = "[[sites]]\nid='site:other'\npublisher='owner:missing'\ndeployable=false\n[[sites]]\nid='site:third'\npublisher='owner:missing'\ndeployable=false\n";
    let mut initialize = initialize();
    initialize["params"]["initializationOptions"]["workspace"]["text"] = json!(root);
    initialize["params"]["initializationOptions"]["workspace_documents"] = json!([{
        "uri":"untitled:other-site","path":"sites/other.toml","text":included
    }]);
    let (result, first) = selected_session(initialize.clone(), [], Limits::default());
    result.expect("native selected modular finding");
    let diagnostic = first
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == URI
        })
        .expect("root diagnostics")["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "workspace_reference")
        .expect("native cross-document finding")
        .clone();
    assert_eq!(diagnostic["data"]["selection_wide"], true);
    assert!(diagnostic["data"]["location"].is_null());
    let offset = root.find("publisher='owner:one'").unwrap() + "publisher='".len();
    let (result, messages) = selected_session(
        initialize,
        [
            request(
                2,
                "textDocument/completion",
                json!({"textDocument":{"uri":URI},"position":point(&root,offset)}),
            ),
            code_action(3, cursor(&root, offset), json!([diagnostic]), None),
        ],
        Limits::default(),
    );
    result.expect("selection-wide repair refusal preserves framed lifetime");
    // Current typed facts remain useful, but an ambiguous missing identity
    // cannot turn a selection-wide diagnostic into an unrelated local repair.
    assert!(response(&messages, 2)["result"]["items"].is_array());
    assert_eq!(response(&messages, 3)["result"], json!([]));
}

#[test]
fn malformed_or_unselected_requests_are_refused() {
    let malformed = request(
        2,
        "textDocument/codeAction",
        json!({"textDocument":[],"range":[],"context":[]}),
    );
    let unselected = request(
        3,
        "textDocument/codeAction",
        json!({
            "textDocument":{"uri":"untitled:other"},
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
            "context":{"diagnostics":[]}
        }),
    );
    let (result, messages) = session([malformed, unselected], Limits::default());
    result.expect("request refusals");
    assert_eq!(response(&messages, 2)["error"]["code"], -32602);
    assert_eq!(response(&messages, 3)["error"]["code"], -32602);
}
