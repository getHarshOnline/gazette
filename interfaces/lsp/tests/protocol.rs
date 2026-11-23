//! Real framed protocol through the public interface and shared workspace compiler.

#[path = "protocol/authored_references.rs"]
mod authored_references;
#[path = "protocol/code_actions.rs"]
mod code_actions;
#[path = "protocol/completion.rs"]
mod completion;
#[path = "protocol/editor_context.rs"]
mod editor_context;
#[path = "protocol/footnotes.rs"]
mod footnotes;
#[path = "protocol/heading_completion.rs"]
mod heading_completion;
#[path = "protocol/headings.rs"]
mod headings;
#[path = "protocol/hover.rs"]
mod hover;
#[path = "protocol/manuals.rs"]
mod manuals;
#[path = "protocol/native_discovery.rs"]
mod native_discovery;
#[path = "protocol/native_selection.rs"]
mod native_selection;
#[path = "protocol/origin.rs"]
mod origin;
#[path = "protocol/page_packages.rs"]
mod page_packages;
#[path = "protocol/scheduling.rs"]
mod scheduling;
#[path = "protocol/symbols.rs"]
mod symbols;
#[path = "protocol/workspace.rs"]
mod workspace;

use std::io::{self, Cursor, Write};

use gazette_lsp::{Error, Limits, serve};
use serde_json::{Value, json};

const URI: &str = "untitled:gazette-workspace.toml";
const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut frame = format!("Content-Length: {}\r\n\r\n", bytes.len()).into_bytes();
    frame.extend_from_slice(bytes);
    frame
}

fn request(id: i32, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","method":method,"params":params})
}

fn initialize() -> Value {
    request(
        1,
        "initialize",
        json!({"processId":null,"capabilities":{},
            "initializationOptions":{"workspace":{"uri":URI,"path":"workspace.toml","text":WORKSPACE},"documents":[]}
        }),
    )
}

fn open(uri: &str, version: i32, text: &str) -> Value {
    notification(
        "textDocument/didOpen",
        json!({"textDocument":{
            "uri":uri,"languageId":"toml","version":version,"text":text
        }}),
    )
}

fn change(uri: &str, version: i32, text: &str) -> Value {
    notification(
        "textDocument/didChange",
        json!({
            "textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]
        }),
    )
}

fn close(uri: &str) -> Value {
    notification("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
}

fn stream(messages: &[Value]) -> Vec<u8> {
    messages
        .iter()
        .flat_map(|message| frame(&serde_json::to_vec(message).expect("test protocol encoding")))
        .collect()
}

fn session(
    messages: impl IntoIterator<Item = Value>,
    limits: Limits,
) -> (Result<(), Error>, Vec<Value>) {
    selected_session(initialize(), messages, limits)
}

fn selected_session(
    initialize: Value,
    messages: impl IntoIterator<Item = Value>,
    limits: Limits,
) -> (Result<(), Error>, Vec<Value>) {
    let mut input = vec![initialize, notification("initialized", json!({}))];
    input.extend(messages);
    input.push(request(90, "shutdown", Value::Null));
    input.push(notification("exit", Value::Null));
    let mut output = Vec::new();
    let result = serve(Cursor::new(stream(&input)), &mut output, limits);
    (result, decode(&output))
}

fn content(id: &str, edges: &str, structure: &str) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\n[content.structure]\n{structure}\n+++\n# Héllo 😀\n"
    )
}

fn selected(files: &[(&str, &str, &str)]) -> Value {
    let mut value = initialize();
    value["params"]["initializationOptions"]["documents"] = json!(
        files
            .iter()
            .map(|(uri, path, text)| json!({"uri":uri,"path":path,"text":text}))
            .collect::<Vec<_>>()
    );
    value
}

fn native_selection(root: &std::path::Path, workspace: &str, documents: &[&str]) -> Value {
    let root_uri = url::Url::from_directory_path(root)
        .expect("temporary root file URI")
        .to_string();
    request(
        1,
        "initialize",
        json!({
            "processId":null,
            "rootUri":root_uri,
            "workspaceFolders":[{"uri":root_uri,"name":"fixture"}],
            "capabilities":{},
            "initializationOptions":{"native_selection":{
                "allow_read":true,
                "workspace":workspace,
                "documents":documents,
            }}
        }),
    )
}

#[test]
fn explicit_native_selection_reuses_retained_sources_for_diagnostics_and_hover() {
    let root = tempfile::tempdir().expect("native selection root");
    std::fs::create_dir(root.path().join("content")).expect("content directory");
    std::fs::create_dir(root.path().join("sites")).expect("site directory");
    std::fs::write(
        root.path().join("workspace.toml"),
        "includes=['sites/one.toml']\n[[principals]]\nid='owner:one'\nkind='person'\n",
    )
    .expect("workspace root source");
    std::fs::write(
        root.path().join("sites/one.toml"),
        "[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n",
    )
    .expect("declared workspace include");
    let page = content("content:one", "", "kind='page'\nfamily={kind='article'}");
    std::fs::write(root.path().join("content/page.md"), &page).expect("content source");
    std::fs::write(
        root.path().join("unselected-broken.md"),
        "+++\n[broken\n+++",
    )
    .expect("unselected decoy");
    let page_uri = url::Url::from_file_path(root.path().join("content/page.md"))
        .expect("page file URI")
        .to_string();
    let hover_offset = page.find("lifecycle").expect("schema field");
    let before = &page[..hover_offset + 2];
    let position = json!({
        "line":before.matches('\n').count(),
        "character":before.rsplit('\n').next().expect("line").encode_utf16().count(),
    });
    let (result, messages) = selected_session(
        native_selection(root.path(), "workspace.toml", &["content/page.md"]),
        [request(
            2,
            "textDocument/hover",
            json!({
                "textDocument":{"uri":page_uri},
                "position":position,
            }),
        )],
        Limits::default(),
    );
    result.expect("explicit native selection session");
    assert!(messages.iter().any(|message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == page_uri
            && message["params"]["diagnostics"] == json!([])
    }));
    let hover = messages
        .iter()
        .find(|message| message["id"] == 2)
        .expect("native hover response");
    assert!(hover.get("error").is_none(), "{hover}");
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .is_some_and(|value| value.contains("content state")),
        "{hover}"
    );
}

#[test]
fn native_selection_requires_permission_and_never_falls_back_or_scans() {
    let root = tempfile::tempdir().expect("native selection root");
    std::fs::write(root.path().join("workspace.toml"), WORKSPACE).expect("workspace source");
    let mut initialize = native_selection(root.path(), "workspace.toml", &["missing.md"]);
    initialize["params"]["initializationOptions"]["native_selection"]["allow_read"] = json!(false);
    let (result, messages) = selected_session(initialize, [], Limits::default());
    assert!(matches!(
        result,
        Err(Error::Protocol("exit before shutdown"))
    ));
    assert_eq!(messages[0]["error"]["code"], -32602);
    assert!(
        messages[0]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("allow_read=true"))
    );

    let (result, messages) = selected_session(
        native_selection(root.path(), "workspace.toml", &["missing.md"]),
        [],
        Limits::default(),
    );
    assert!(matches!(
        result,
        Err(Error::Protocol("exit before shutdown"))
    ));
    assert_eq!(messages[0]["error"]["code"], -32602);
    assert!(
        messages[0]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("could not be captured"))
    );
}

#[test]
fn native_workspace_failure_preserves_the_loader_reason() {
    let root = tempfile::tempdir().expect("native selection root");
    let workspace = "[broken\n";
    std::fs::write(root.path().join("workspace.toml"), workspace).expect("workspace source");
    let expected = gazette_sdk::configuration::load_workspace(
        root.path(),
        &"workspace.toml".parse().expect("workspace path"),
        gazette_sdk::configuration::WorkspaceLoadLimits::default(),
    )
    .expect_err("invalid native workspace")
    .to_string();

    let (result, messages) = selected_session(
        native_selection(root.path(), "workspace.toml", &[]),
        [],
        Limits::default(),
    );
    assert!(matches!(
        result,
        Err(Error::Protocol("exit before shutdown"))
    ));
    assert_eq!(messages[0]["error"]["code"], -32602);
    assert_eq!(
        messages[0]["error"]["message"],
        format!("selected native workspace could not be loaded: {expected}")
    );
}

fn open_content(uri: &str, version: i32, text: &str) -> Value {
    let mut value = open(uri, version, text);
    value["params"]["textDocument"]["languageId"] = json!("plaintext");
    value
}

#[test]
fn selected_collection_edits_recheck_other_documents_and_close_restores_baseline() {
    let collection = content("collection:guide", "", "kind='collection'");
    let page = content(
        "content:one",
        "collections=['collection:guide']\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let changed = collection.replace("collection:guide", "collection:changed");
    let (result, messages) = selected_session(
        selected(&[
            (
                "file:///selected/collection.md",
                "collection.md",
                &collection,
            ),
            ("file:///selected/page.md", "page.md", &page),
        ]),
        [
            open_content("file:///selected/page.md", 1, &page),
            open_content("file:///selected/collection.md", 7, &collection),
            change("file:///selected/collection.md", 8, &changed),
            change("file:///selected/collection.md", 7, &collection),
            close("file:///selected/collection.md"),
        ],
        Limits::default(),
    );
    result.expect("selected-set session");
    let pages: Vec<_> = messages
        .iter()
        .filter(|m| {
            m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == "file:///selected/page.md"
        })
        .map(|m| &m["params"])
        .collect();
    assert_eq!(pages.len(), 5, "stale edit must not advance analysis");
    assert!(
        pages[0]["diagnostics"]
            .as_array()
            .expect("initial diagnostic array")
            .is_empty()
    );
    let failures = pages[3]["diagnostics"]
        .as_array()
        .expect("current failure diagnostics");
    assert!(!failures.is_empty());
    assert!(
        failures
            .iter()
            .any(|d| d["data"]["source"] == "content:one"
                && d["data"]["target"] == "collection:guide")
    );
    assert_eq!(
        pages[3]["version"], 1,
        "dependent version stays its actual editor version"
    );
    assert_eq!(failures[0]["data"]["generation"], 4);
    assert_eq!(
        failures[0]["data"]["source_sha256"],
        serde_json::to_value(gazette_sdk::source::Sha256Digest::of(page.as_bytes()))
            .expect("source digest encoding")
    );
    assert!(
        pages[4]["diagnostics"]
            .as_array()
            .expect("restored diagnostic array")
            .is_empty()
    );
    let generations: Vec<_> = messages
        .iter()
        .filter(|m| m["method"] == "gazette/analysisPublished")
        .map(|m| {
            m["params"]["generation"]
                .as_u64()
                .expect("numeric analysis generation")
        })
        .collect();
    assert_eq!(generations, [1, 2, 3, 4, 5]);
}

#[test]
fn symbols_use_current_full_source_and_never_reuse_a_previous_valid_corpus() {
    let page =
        content("content:one", "", "kind='page'\nfamily={kind='article'}").replace('\n', "\r\n");
    let uri = "file:///selected/page.md";
    let symbol = |id| {
        request(
            id,
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        )
    };
    let (result, messages) = selected_session(
        selected(&[(uri, "page.md", &page)]),
        [
            symbol(2),
            open_content(uri, 4, "+++\n[broken\n+++\n# Wrong"),
            symbol(3),
            change(uri, 5, &page),
            symbol(4),
            request(
                5,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":"file:///unselected.md"}}),
            ),
        ],
        Limits::default(),
    );
    result.expect("complete current-source symbol session");
    let response = |id| {
        messages
            .iter()
            .find(|m| m["id"] == id)
            .expect("requested symbol response")
    };
    let heading = &response(2)["result"][0];
    assert_eq!(heading["name"], "Héllo 😀");
    let line = page[..page.find("# Héllo").expect("fixture heading")]
        .matches('\n')
        .count();
    assert_eq!(
        heading["range"],
        json!({"start":{"line":line,"character":0},"end":{"line":line,"character":10}})
    );
    assert_eq!(heading["range"], heading["selectionRange"]);
    assert!(
        response(3).get("error").is_some(),
        "invalid current corpus cannot yield stale symbols"
    );
    assert_eq!(response(4)["result"], response(2)["result"]);
    assert_eq!(response(5)["error"]["code"], -32602);
}

#[test]
fn initialization_requires_unambiguous_explicit_roles_and_obeys_selection_bounds() {
    let mut missing = initialize();
    missing["params"]
        .as_object_mut()
        .expect("initialization parameter object")
        .remove("initializationOptions");
    let duplicate_uri = selected(&[(URI, "other.md", "# A")]);
    let duplicate_path = selected(&[("file:///other.md", "workspace.toml", "# A")]);
    let escaped = selected(&[("file:///other.md", "../other.md", "# A")]);
    let mut unknown = initialize();
    unknown["params"]["initializationOptions"]["scanEverything"] = json!(true);
    let mut tuple = initialize();
    tuple["params"]["initializationOptions"]["workspace"] =
        json!([URI, "workspace.toml", WORKSPACE]);
    for initial in [
        missing,
        duplicate_uri,
        duplicate_path,
        escaped,
        unknown,
        tuple,
    ] {
        let (result, messages) = selected_session(initial, [], Limits::default());
        assert!(result.is_err());
        assert_eq!(messages[0]["error"]["code"], -32602);
        assert!(
            !messages
                .iter()
                .any(|m| m["method"] == "textDocument/publishDiagnostics")
        );
    }
    for limits in [
        Limits {
            documents: 0,
            ..Limits::default()
        },
        Limits {
            source_bytes: WORKSPACE.len() - 1,
            ..Limits::default()
        },
        Limits {
            uri_bytes: URI.len() - 1,
            ..Limits::default()
        },
    ] {
        let (result, messages) = selected_session(initialize(), [], limits);
        assert!(result.is_err());
        assert_eq!(messages[0]["error"]["code"], -32602);
    }
}

#[test]
fn retained_overlay_exhaustion_ends_session_without_publishing_a_successful_replacement() {
    let (result, messages) = session(
        [open(URI, 1, WORKSPACE)],
        Limits {
            source_bytes: WORKSPACE.len(),
            ..Limits::default()
        },
    );
    assert!(matches!(result, Err(Error::Limit("retained source bytes"))));
    assert!(
        publications(&messages).is_empty(),
        "only the captured baseline was published"
    );
}

#[test]
fn empty_heading_text_uses_its_native_anchor_for_a_nonempty_editor_symbol() {
    let page = content("content:one", "", "kind='page'\nfamily={kind='article'}")
        .replace("# Héllo 😀", "#");
    let uri = "file:///selected/empty.md";
    let (result, messages) = selected_session(
        selected(&[(uri, "empty.md", &page)]),
        [request(
            2,
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        )],
        Limits::default(),
    );
    result.expect("complete selected-source symbol session");
    let symbols = &messages
        .iter()
        .find(|m| m["id"] == 2)
        .expect("symbol response")["result"];
    assert_eq!(symbols[0]["name"], "section");
    assert_eq!(symbols[0]["range"]["end"]["character"], 1);
}

fn decode(mut bytes: &[u8]) -> Vec<Value> {
    let mut messages = Vec::new();
    while !bytes.is_empty() {
        let end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("complete frame header");
        let header = std::str::from_utf8(&bytes[..end]).expect("ASCII header");
        let length = header
            .strip_prefix("Content-Length: ")
            .expect("one content-length")
            .parse::<usize>()
            .expect("valid output byte count");
        bytes = &bytes[end + 4..];
        messages.push(serde_json::from_slice(&bytes[..length]).expect("complete output JSON"));
        bytes = &bytes[length..];
    }
    messages
}

fn publications(messages: &[Value]) -> Vec<&Value> {
    messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .skip(1) // The separately tested initialization baseline precedes edits.
        .map(|message| &message["params"])
        .collect()
}

#[test]
fn lifecycle_advertises_only_full_sync_and_utf16_then_clears_on_close() {
    let (result, messages) = session([open(URI, 7, WORKSPACE), close(URI)], Limits::default());
    result.expect("complete initialize/open/close/shutdown/exit");
    assert_eq!(
        messages[0]["result"]["capabilities"],
        json!({
            "positionEncoding":"utf-16", "textDocumentSync":{"openClose":true,"change":1,"save":{"includeText":true}},
            "documentSymbolProvider":true,"workspaceSymbolProvider":true,"definitionProvider":true,"referencesProvider":true,"hoverProvider":true,"documentLinkProvider":{"resolveProvider":false},
            "completionProvider":{"resolveProvider":false,"triggerCharacters":["\"","'","=",".","["]},
            "codeActionProvider":{"codeActionKinds":["quickfix"],"resolveProvider":false},
            "experimental":{"gazette":{
                "authoring":"0.1.0",
                "context":{"method":"gazette/authoringContext","generationRequired":true},
                "editorSnapshot":{"manifest":"gazette/editorSnapshot","source":"gazette/editorSource","chunkBytes":65536,"nativeContextFormat":"gazette-editor-context/2"},
                "sourceDiscovery":{"selectSource":"gazette/selectSource"},
                "origin":{
                    "method":"gazette/origin",
                    "generationRequired":true,
                    "nativePositionEncoding":"utf-8",
                    "rangePositionEncoding":"utf-16"
                },
                "pagePackage":{
                    "method":"gazette/pagePackage",
                    "generationRequired":true,
                    "compiledDestinations":"compiler-proven-only"
                }
            }}
        })
    );
    assert_eq!(messages[0]["result"]["serverInfo"]["name"], "gazette");
    let diagnostics = publications(&messages);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics[0],
        &json!({"uri":URI,"version":7,"diagnostics":[]})
    );
    assert_eq!(diagnostics[1], &json!({"uri":URI,"diagnostics":[]}));
    assert_eq!(
        messages.last().expect("shutdown reply")["result"],
        Value::Null
    );
}

#[test]
fn newer_full_buffers_replace_diagnostics_stale_changes_and_duplicate_opens_do_not() {
    let invalid = WORKSPACE.replace("publisher='owner:one'", "publisher='owner:missing'");
    let (result, messages) = session(
        [
            open(URI, 10, &invalid),
            change(URI, 9, WORKSPACE),
            change(URI, 10, WORKSPACE),
            open(URI, 99, WORKSPACE),
            change(URI, 11, WORKSPACE),
            close(URI),
            change(URI, 12, &invalid),
            open(URI, -5, WORKSPACE),
        ],
        Limits::default(),
    );
    result.expect("versioned lifecycle");
    let diagnostics = publications(&messages);
    assert_eq!(diagnostics.len(), 4, "stale/closed changes do not publish");
    assert_eq!(diagnostics[0]["version"], 10);
    assert_eq!(
        diagnostics[0]["diagnostics"][0]["code"],
        "workspace_reference"
    );
    assert_eq!(
        diagnostics[1],
        &json!({"uri":URI,"version":11,"diagnostics":[]})
    );
    assert_eq!(
        diagnostics[3]["version"], -5,
        "reopening starts a new version sequence"
    );
    let warnings: Vec<_> = messages
        .iter()
        .filter_map(|message| message["params"]["message"].as_str())
        .collect();
    assert_eq!(
        warnings
            .iter()
            .filter(|message| message.starts_with("lsp_stale_version"))
            .count(),
        2,
        "superseded pending versions are cancelled before SDK admission"
    );
    assert!(
        warnings
            .iter()
            .any(|message| message.starts_with("lsp_document_open"))
    );
    assert!(
        warnings
            .iter()
            .any(|message| message.starts_with("lsp_stale_version"))
    );
}

#[test]
fn original_syntax_bytes_project_to_utf16_and_crlf_not_byte_columns() {
    let source = "# 🦀前\r\ntitle = \"🦀é\" ?\r\nsites=[]\r\n";
    let error = gazette_sdk::workspace::compile(source, Limits::default().syntax)
        .expect_err("actual TOML trailing token failure");
    let failure = gazette_sdk::workspace::Failure::from(&error);
    let span = failure.span.expect("native parser supplies exact span");
    assert_eq!(span.start, source.find('?').expect("known bad token"));
    let (result, messages) = session([open(URI, 1, source)], Limits::default());
    result.expect("syntax diagnostic is not a protocol failure");
    let diagnostic = &publications(&messages)[0]["diagnostics"][0];
    assert_eq!(diagnostic["code"], failure.code);
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line":1,"character":14})
    );
    assert_eq!(
        diagnostic["data"]["location"]["span"],
        serde_json::to_value(span).expect("span encoding")
    );
    assert_eq!(diagnostic["data"]["location"]["precision"], "Construct");
    assert_ne!(
        span.start - source.find("title").expect("second line"),
        14,
        "fixture distinguishes UTF-8 offsets from UTF-16 columns"
    );
}

#[test]
fn semantic_diagnostics_preserve_sdk_codes_and_logical_fields_without_invented_spans() {
    let source = format!(
        "# 🦀\r\n{}",
        WORKSPACE
            .replace("publisher='owner:one'", "publisher='owner:missing'")
            .replace('\n', "\r\n")
    );
    let error = gazette_sdk::workspace::compile(&source, Limits::default().syntax)
        .expect_err("actual unresolved publisher");
    let failure = gazette_sdk::workspace::Failure::from(&error);
    let native = failure.diagnostics.expect("semantic diagnostics");
    let (result, messages) = session([open(URI, 3, &source)], Limits::default());
    result.expect("logical diagnostics session");
    let publications = publications(&messages);
    let diagnostics = publications[0]["diagnostics"]
        .as_array()
        .expect("diagnostic array");
    assert_eq!(diagnostics.len(), native.len());
    for (actual, expected) in diagnostics.iter().zip(native) {
        assert_eq!(actual["code"], expected.code);
        assert_eq!(actual["data"]["field"], expected.field);
        assert_eq!(
            actual["data"]["key"],
            serde_json::to_value(&expected.key).expect("key encoding")
        );
        assert!(actual["data"]["location"].is_null());
        assert!(actual["data"].get("span").is_none());
        assert_eq!(
            actual["range"],
            json!({"start":{"line":0,"character":0},"end":{"line":8,"character":0}})
        );
    }
}

#[test]
fn markdown_renderability_diagnostics_are_shared_and_projected_to_utf16_before_theme_use() {
    let uri = "file:///selected/render.md";
    let (body, code, reason) = (
        "<script>unsafe()</script>\n",
        "markdown_raw_html_unavailable",
        "raw HTML requires an admitted renderer",
    );
    let page = content("content:render", "", "kind='page'\nfamily={kind='article'}")
        .replace("# Héllo 😀\n", body)
        .replace('\n', "\r\n");
    let (result, messages) = selected_session(
        selected(&[(uri, "render.md", &page)]),
        [open_content(uri, 4, &page)],
        Limits::default(),
    );
    result.expect("shared renderability diagnostic session");
    let page_diagnostics = messages
        .iter()
        .rfind(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == uri
                && message["params"]["version"] == 4
        })
        .expect("current selected page diagnostics");
    let row = page_diagnostics["params"]["diagnostics"]
        .as_array()
        .expect("diagnostic array")
        .iter()
        .find(|row| row["message"] == reason)
        .expect("renderability row");
    assert_eq!(row["code"], code);
    assert_eq!(row["data"]["selection_wide"], false);
    assert_eq!(row["data"]["generation"], 2);
    assert_ne!(row["range"]["start"], row["range"]["end"]);
    assert_eq!(
        row["data"]["source_sha256"],
        serde_json::to_value(gazette_sdk::source::Sha256Digest::of(page.as_bytes()))
            .expect("digest")
    );

    for body in [
        "note[^outer]\n\n[^outer]: nested[^inner]\n\n[^inner]: target\n",
        "![label[^note]](https://example.invalid/image.png)\n\n[^note]: detail\n",
    ] {
        let page = content("content:render", "", "kind='page'\nfamily={kind='article'}")
            .replace("# Héllo 😀\n", body)
            .replace('\n', "\r\n");
        let (result, messages) = selected_session(
            selected(&[(uri, "render.md", &page)]),
            [open_content(uri, 4, &page)],
            Limits::default(),
        );
        result.expect("supported footnote session");
        let page_diagnostics = messages
            .iter()
            .rfind(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == uri
                    && message["params"]["version"] == 4
            })
            .expect("current selected page diagnostics");
        assert!(
            page_diagnostics["params"]["diagnostics"]
                .as_array()
                .expect("diagnostic array")
                .iter()
                .all(|row| row["code"] != "markdown_adapter"),
            "supported footnote structure must not retain the old provider diagnostic"
        );
    }
}

#[test]
fn malformed_changes_are_explicitly_ignored_and_do_not_consume_newer_version() {
    let mut ranged = change(URI, 2, "bad");
    ranged["params"]["contentChanges"][0]["range"] =
        json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}});
    let tuple = notification(
        "textDocument/didChange",
        json!({
            "textDocument":[URI,2],"contentChanges":[{"text":"bad"}]
        }),
    );
    let mut malformed = change(URI, 2, "bad");
    malformed["params"]["textDocument"]["version"] = json!("2");
    let (result, messages) = session(
        [
            open(URI, 1, WORKSPACE),
            ranged,
            tuple,
            malformed,
            notification(
                "textDocument/didChange",
                json!({"textDocument":{"uri":URI,"version":2},"contentChanges":[]}),
            ),
            change(URI, 2, WORKSPACE),
        ],
        Limits::default(),
    );
    result.expect("valid full replacement recovers");
    let diagnostics = publications(&messages);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[1]["version"], 2);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["method"] == "window/logMessage")
            .count(),
        4
    );
}

#[test]
fn compiler_and_document_limits_are_observable_without_silently_reusing_old_diagnostics() {
    let exact = Limits {
        syntax: gazette_sdk::toml::Limits {
            source_bytes: WORKSPACE.len(),
            ..Limits::default().syntax
        },
        ..Limits::default()
    };
    let (result, messages) = session([open(URI, 1, WORKSPACE)], exact);
    result.expect("exact source bytes");
    assert_eq!(publications(&messages)[0]["diagnostics"], json!([]));
    let small = Limits {
        syntax: gazette_sdk::toml::Limits {
            source_bytes: WORKSPACE.len() - 1,
            ..exact.syntax
        },
        ..exact
    };
    let (result, messages) = session([open(URI, 1, WORKSPACE), change(URI, 2, "sites=[]")], small);
    result.expect("source limit is a versioned compiler diagnostic");
    let diagnostics = publications(&messages);
    assert_eq!(diagnostics[0]["diagnostics"][0]["code"], "authoring_limit");
    assert_eq!(diagnostics[1]["version"], 2);
    assert_eq!(diagnostics[1]["diagnostics"][0]["code"], "workspace_empty");
    let other = "untitled:other";
    let (result, messages) = session(
        [
            open(URI, 1, WORKSPACE),
            open(other, 1, WORKSPACE),
            close(URI),
            open(other, 2, WORKSPACE),
        ],
        Limits {
            documents: 1,
            uri_bytes: URI.len(),
            ..Limits::default()
        },
    );
    result.expect("closing restores the selected baseline without admitting other URIs");
    let diagnostics = publications(&messages);
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|d| d["uri"] == URI));
    assert!(messages.iter().any(|message| {
        message["params"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("lsp_document_open"))
    }));
    let (result, messages) = session(
        [open(URI, 1, "sites=[]")],
        Limits {
            diagnostics: 0,
            ..Limits::default()
        },
    );
    assert!(matches!(result, Err(Error::Limit("diagnostics"))));
    assert!(
        publications(&messages).is_empty(),
        "no truncated successful publication"
    );
}

#[test]
fn oversized_change_is_refused_without_ending_the_session() {
    let source_bytes = WORKSPACE.len() * 2;
    let oversized = format!("{WORKSPACE}x");
    let (result, messages) = session(
        [
            open(URI, 1, WORKSPACE),
            change(URI, 2, &oversized),
            request(2, "workspace/symbol", json!({"query":""})),
            change(URI, 3, "sites=[]"),
            request(3, "workspace/symbol", json!({"query":""})),
        ],
        Limits {
            source_bytes,
            ..Limits::default()
        },
    );
    result.expect("oversized edit is a refused mutation, not a terminal session error");
    assert!(messages.iter().any(|message| {
        message["method"] == "window/logMessage"
            && message["params"]["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("lsp_document_change_limit:"))
    }));
    for id in [2, 3] {
        assert!(
            messages
                .iter()
                .find(|message| message["id"] == id)
                .is_some_and(|message| message.get("error").is_none()),
            "session answers request {id} after refused edit"
        );
    }
    assert_eq!(
        publications(&messages).len(),
        2,
        "open and later valid change publish after the filtered initialization baseline; refused edit does not"
    );
}

#[test]
fn malformed_framing_json_ids_and_versions_fail_without_echoing_source() {
    let invalid = [
        b"Content-Length: 2\r\ncontent-length: 2\r\n\r\n{}".to_vec(),
        b"Content-Length: 2\r\nContent-Length: 3\r\n\r\n{}".to_vec(),
        b"Content-Length: +2\r\n\r\n{}".to_vec(),
        b"Content-Length: 2\n\n{}".to_vec(),
        b"Content-Length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=latin1\r\n\r\n{}"
            .to_vec(),
        frame(br#"{"jsonrpc":"2.0","method":"private-canary","method":"exit"}"#),
        frame(br#"{"jsonrpc":"1.0","method":"private-canary"}"#),
        frame(br#"{"jsonrpc":"2.0","id":{},"method":"private-canary"}"#),
        frame(br#"{"jsonrpc":"2.0","id":null,"method":"private-canary"}"#),
        frame(br#"["2.0",1,"private-canary",{}]"#),
        frame(b"not-json-private-canary"),
    ];
    for input in invalid {
        let mut output = Vec::new();
        let error = serve(Cursor::new(input), &mut output, Limits::default())
            .expect_err("invalid transport");
        assert!(!error.to_string().contains("private-canary"));
        assert!(output.is_empty());
    }
}

#[test]
fn transport_limits_precede_body_reads_and_output_frames_are_all_or_nothing() {
    let mut input = Cursor::new(b"Content-Length: 999999999\r\n\r\nprivate-canary".to_vec());
    let mut output = Vec::new();
    let error = serve(
        &mut input,
        &mut output,
        Limits {
            message_bytes: 8,
            ..Limits::default()
        },
    )
    .expect_err("oversized frame");
    assert!(matches!(error, Error::Limit("message bytes")));
    assert!(input.position() < 30, "no body is read or allocated");
    assert!(output.is_empty());
    let error = serve(
        Cursor::new(vec![b'a'; 100]),
        &mut output,
        Limits {
            header_bytes: 8,
            ..Limits::default()
        },
    )
    .expect_err("header cap");
    assert!(matches!(error, Error::Limit("header bytes")));
    let (result, messages) = session([], Limits::default());
    result.expect("baseline wire");
    let exact = serde_json::to_vec(&messages[0])
        .expect("exact response bytes")
        .len();
    session(
        [],
        Limits {
            output_bytes: exact,
            ..Limits::default()
        },
    )
    .0
    .expect("exact response cap");
    let mut output = Vec::new();
    let error = serve(
        Cursor::new(stream(&[initialize()])),
        &mut output,
        Limits {
            output_bytes: exact - 1,
            ..Limits::default()
        },
    )
    .expect_err("expanded response exceeds cap");
    assert!(matches!(error, Error::Limit("output bytes")));
    assert!(
        output.is_empty(),
        "no partial header/body before size validation"
    );
}

#[test]
fn initialization_shutdown_and_terminal_errors_have_explicit_protocol_semantics() {
    let input = stream(&[
        request(0, "unknown", Value::Null),
        request(1, "initialize", json!({"capabilities":[]})),
        initialize(),
        notification("initialized", json!({})),
        initialize(),
        request(3, "textDocument/completion", json!({})),
        request(8, "gazette/unsupported", Value::Null),
        request(4, "shutdown", json!({})),
        request(5, "shutdown", Value::Null),
        request(6, "unknown", Value::Null),
        notification("exit", Value::Null),
    ]);
    let mut output = Vec::new();
    serve(Cursor::new(input), &mut output, Limits::default()).expect("complete lifecycle");
    let messages = decode(&output);
    let responses: Vec<_> = messages.iter().filter(|m| m.get("id").is_some()).collect();
    assert_eq!(responses[0]["error"]["code"], -32002);
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert_eq!(responses[3]["error"]["code"], -32600);
    assert_eq!(responses[4]["id"], 3);
    assert_eq!(responses[4]["error"]["code"], -32602);
    assert_eq!(responses[5]["id"], 8);
    assert_eq!(responses[5]["error"]["code"], -32601);
    assert_eq!(responses[6]["error"]["code"], -32602);
    assert_eq!(responses[7]["result"], Value::Null);
    assert_eq!(responses[8]["error"]["code"], -32600);
    for input in [
        Vec::new(),
        stream(&[notification("exit", Value::Null)]),
        stream(&[initialize()]),
    ] {
        serve(Cursor::new(input), Vec::new(), Limits::default())
            .expect_err("no clean shutdown/exit");
    }
}

struct RefusingWriter;
impl Write for RefusingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "fixture closed receiver",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_failure_is_not_swallowed_as_a_successful_session() {
    let error = serve(
        Cursor::new(stream(&[initialize()])),
        RefusingWriter,
        Limits::default(),
    )
    .expect_err("output cannot be delivered");
    assert!(matches!(error, Error::Io(error) if error.kind() == io::ErrorKind::BrokenPipe));
}
