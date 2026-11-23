use super::*;

#[test]
fn native_manifest_distinguishes_clean_open_text_from_unsaved_text_and_pins_configuration() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("gazette.toml"), WORKSPACE).unwrap();
    let root_uri = url::Url::from_directory_path(root.path()).unwrap();
    let uri = root_uri.join("gazette.toml").unwrap().to_string();
    let init = request(
        1,
        "initialize",
        json!({"processId":null,"capabilities":{},
        "rootUri":root_uri.to_string(),"initializationOptions":{"native_selection":{
            "allow_read":true,"discover":true,"workspace":"gazette.toml"}}}),
    );
    let changed = format!("{WORKSPACE}\n# unsaved café 🦀\n");
    let (result, output) = selected_session(
        init,
        [
            open(&uri, 1, WORKSPACE),
            request(2, "gazette/editorSnapshot", json!({})),
            change(&uri, 2, &changed),
            request(3, "gazette/editorSnapshot", json!({})),
        ],
        Limits::default(),
    );
    result.unwrap();
    let snapshot = |id| &output.iter().find(|value| value["id"] == id).unwrap()["result"];
    let baseline = json!(gazette_sdk::source::Sha256Digest::of(WORKSPACE.as_bytes()));
    let clean = snapshot(2);
    assert_eq!(clean["sources"][0]["version"], 1);
    assert_eq!(clean["sources"][0]["baseline_sha256"], baseline);
    assert_eq!(clean["sources"][0]["source_sha256"], baseline);
    let dirty = snapshot(3);
    assert_eq!(dirty["sources"][0]["baseline_sha256"], baseline);
    assert_ne!(dirty["sources"][0]["source_sha256"], baseline);
    assert_eq!(
        dirty["native_selection"]["configuration"]["gazette.toml"],
        baseline
    );
    assert_eq!(dirty["native_selection"]["workspace"], "gazette.toml");
}

#[test]
fn context_snapshot_and_save_follow_the_current_generation() {
    let changed = WORKSPACE.replace("owner:one", "owner:renamed");
    let (result, output) = session(
        [
            request(2, "gazette/editorSnapshot", json!({})),
            request(
                3,
                "gazette/authoringContext",
                json!({"generation":1,"textDocument":{"uri":URI},"position":{"line":1,"character":2}}),
            ),
            open(URI, 3, &changed),
            request(
                4,
                "gazette/editorSource",
                json!({"generation":1,"document":URI,"offset":0,"length":65536}),
            ),
            request(
                5,
                "gazette/editorSource",
                json!({"generation":2,"document":URI,"offset":0,"length":65536}),
            ),
            notification(
                "textDocument/didSave",
                json!({"textDocument":{"uri":URI},"text":changed}),
            ),
            close(URI),
            request(6, "gazette/editorSnapshot", json!({})),
            request(
                7,
                "gazette/editorSource",
                json!({"generation":4,"document":URI,"offset":0,"length":65536}),
            ),
        ],
        Limits::default(),
    );
    result.expect("protocol");
    let reply = |id| output.iter().find(|v| v["id"] == id).expect("reply");
    assert_eq!(reply(2)["result"]["generation"], 1);
    assert_eq!(
        reply(2)["result"]["sources"][0]["role"]["kind"],
        "workspace"
    );
    assert!(
        reply(3)["result"]["fields"]
            .as_array()
            .is_some_and(|v| !v.is_empty()),
        "{}",
        reply(3)
    );
    assert!(reply(4).get("error").is_some());
    use base64::Engine as _;
    let decode = |id| {
        base64::engine::general_purpose::STANDARD
            .decode(
                reply(id)["result"]["bytes_base64"]
                    .as_str()
                    .expect("source"),
            )
            .expect("base64")
    };
    assert_eq!(decode(5), changed.as_bytes());
    assert_eq!(reply(6)["result"]["generation"], 4);
    assert!(reply(6)["result"]["sources"][0]["version"].is_null());
    assert_eq!(
        decode(7),
        changed.as_bytes(),
        "closing saved file must retain the saved baseline"
    );
}

#[test]
fn frontmatter_context_keeps_schema_and_ranges_in_the_full_markdown_source() {
    let page = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='A 🦀 page'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# Heading\n";
    let page_uri = "untitled:page.md";
    let mut setup = initialize();
    setup["params"]["initializationOptions"]["documents"] =
        json!([{"uri":page_uri,"path":"content/page.md","text":page}]);
    let (result, output) = selected_session(
        setup,
        [request(
            2,
            "gazette/authoringContext",
            json!({"generation":1,"textDocument":{"uri":page_uri},"position":{"line":5,"character":2}}),
        )],
        Limits::default(),
    );
    result.expect("protocol");
    let context = &output.iter().find(|v| v["id"] == 2).expect("context")["result"];
    let fields = context["fields"].as_array().expect("frontmatter fields");
    let lifecycle = fields
        .iter()
        .find(|field| field["field"]["name"] == "lifecycle")
        .expect("lifecycle");
    assert_eq!(lifecycle["range"]["start"]["line"], 5);
    assert!(
        !lifecycle["field"]["schema"]["allowed_values"]
            .as_array()
            .expect("schema literals")
            .is_empty()
    );
    assert_eq!(context["assistance"]["status"], "available");
    assert_eq!(
        context["source_sha256"],
        json!(gazette_sdk::source::Sha256Digest::of(page.as_bytes()))
    );
}

#[test]
fn schema_errors_in_one_module_never_rebind_ranges_to_sibling_documents_or_stop_queries() {
    let module_uri = "untitled:sites.toml";
    let root = format!(
        "includes=['sites.toml']\n{}[[principals]]\nid='owner:one'\nkind='person'\n",
        "# padding keeps the source range owner observable\n".repeat(100)
    );
    let module = "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
    let bad_root = format!("{root}[build]\nlocal_draftz='include'\n");
    let bad_module = format!(
        "{module}{}unknown_field=true\n",
        "# module-local padding\n".repeat(300)
    );
    let mut init = initialize();
    init["params"]["initializationOptions"]["workspace"]["text"] = json!(root);
    init["params"]["initializationOptions"]["workspace_documents"] =
        json!([{"uri":module_uri,"path":"sites.toml","text":module}]);
    let (result, output) = selected_session(
        init,
        [
            open(URI, 1, &bad_root),
            request(2, "gazette/editorSnapshot", json!({})),
            request(
                3,
                "gazette/authoringContext",
                json!({"generation":2,"textDocument":{"uri":URI},"position":{"line":0,"character":0}}),
            ),
            change(URI, 2, &root),
            open(module_uri, 1, &bad_module),
            request(4, "gazette/editorSnapshot", json!({})),
            request(
                5,
                "gazette/authoringContext",
                json!({"generation":4,"textDocument":{"uri":module_uri},"position":{"line":0,"character":0}}),
            ),
            change(module_uri, 2, module),
            request(6, "gazette/editorSnapshot", json!({})),
        ],
        Limits::default(),
    );
    result.expect("invalid edits must not terminate the selected-source session");
    let reply = |id| {
        output
            .iter()
            .find(|value| value["id"] == id)
            .expect("query response")
    };
    assert_eq!(reply(2)["result"]["generation"], 2);
    assert_eq!(reply(4)["result"]["generation"], 4);
    for id in [3, 5] {
        assert!(
            reply(id)["result"]["diagnostics"]
                .as_array()
                .is_some_and(|items| !items.is_empty()),
            "{}",
            reply(id)
        );
    }
    assert_eq!(reply(6)["result"]["structurally_valid"], true);
    for uri in [URI, module_uri] {
        let restored = output
            .iter()
            .rev()
            .find(|v| v["method"] == "textDocument/publishDiagnostics" && v["params"]["uri"] == uri)
            .expect("restored diagnostics");
        assert_eq!(restored["params"]["diagnostics"], json!([]));
    }
}

#[test]
fn invalid_modular_reference_keeps_exact_owner_and_cross_module_review_proposals() {
    let module_uri = "untitled:site.toml";
    let root = "includes=['site.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n";
    let module = "[[sites]]\nid='site:one'\npublisher='owner:missing'\ndeployable=false\n";
    let setup = || {
        let mut init = initialize();
        init["params"]["initializationOptions"]["workspace"]["text"] = json!(root);
        init["params"]["initializationOptions"]["workspace_documents"] =
            json!([{"uri":module_uri,"path":"site.toml","text":module}]);
        init
    };
    let (result, output) =
        selected_session(setup(), [open(module_uri, 7, module)], Limits::default());
    result.expect("current invalid typed graph remains inspectable");
    let diagnostic = output
        .iter()
        .rev()
        .find(|m| {
            m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == module_uri
        })
        .expect("module diagnostics")["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "workspace_reference")
        .expect("reference belongs to module")
        .clone();
    assert_eq!(diagnostic["range"]["start"]["line"], 2);
    assert_eq!(diagnostic["data"]["selection_wide"], false);
    let cursor = json!({"line":2,"character":13});
    let (result, output) = selected_session(
        setup(),
        [
            open(module_uri, 7, module),
            request(
                2,
                "textDocument/codeAction",
                json!({"textDocument":{"uri":module_uri},"range":{"start":cursor,"end":cursor},"context":{"diagnostics":[diagnostic],"only":["quickfix"]}}),
            ),
        ],
        Limits::default(),
    );
    result.expect("review proposals over invalid modular reference");
    let response = output.iter().find(|m| m["id"] == 2).unwrap();
    let actions = response["result"].as_array().expect("actions");
    assert_eq!(actions.len(), 1, "{response}");
    assert_eq!(
        actions[0]["edit"]["documentChanges"][0]["textDocument"],
        json!({"uri":module_uri,"version":7})
    );
    assert_eq!(actions[0]["title"], "Review Gazette replacement: owner:one");
}
