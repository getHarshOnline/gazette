//! Distinct manual baselines through the real framed LSP session.

use super::{
    footnotes::{definition, location, position, response},
    *,
};

const DOCUMENTATION_WORKSPACE: &str = r##"
[[principals]]
id = "owner:publisher"
kind = "person"
[[sites]]
id = "site:docs"
publisher = "owner:publisher"
deployable = false
[[projects]]
id = "project:gazette"
lifecycle = "active"
visibility = "public"
availability = "available"
[[sources]]
id = "source:gazette"
location = { kind = "filesystem", root = "/not-opened/gazette" }
[[brands]]
id = "brand:gazette"
name = "Gazette"
[[brands.publications]]
site = "site:docs"
documentation_repository = { name = "gazette_doc", path = "/not-opened/gazette_doc", build = { target = { kind = "mintlify", theme = "mint", primary_color = "#123456" }, modules = [
  { project = "project:gazette", slug = "gazette", title = "Gazette", manual = { source = "source:gazette" }, navigation = [
    { kind = "manual", title = "Manual", pages = [
      { title = "Start", route = "gazette/start", input = { kind = "file", path = "docs/start.md" } }
    ] },
    { kind = "reference", slug = "api-reference", title = "API", input = { kind = "artifact", source = "source:gazette", selection = { kind = "file", path = "generated/api.md" }, format = "markdown" } }
  ] }
] } }
"##;

#[test]
fn manuals_receive_markdown_symbols_diagnostics_and_navigation_without_content_selection() {
    let manual_uri = "file:///selected/manual/index.md";
    let details_uri = "file:///selected/manual/details.md";
    let manual = "# Manual 🦀\r\n\r\n[Details](details.md#details)\r\n";
    let details = "# Details\r\n";
    let mut initialize = selected(&[]);
    initialize["params"]["initializationOptions"]["manuals"] = json!([
        {"source":"source:manual","uri":manual_uri,"path":"manual/index.md","text":manual},
        {"source":"source:manual","uri":details_uri,"path":"manual/details.md","text":details}
    ]);
    let (result, messages) = selected_session(
        initialize,
        [
            request(
                2,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":manual_uri}}),
            ),
            definition(3, manual_uri, position(manual, "[Details]", 0)),
        ],
        Limits::default(),
    );
    result.expect("manual LSP session");
    assert_eq!(response(&messages, 2)["result"][0]["name"], "Manual 🦀");
    assert_eq!(
        response(&messages, 3)["result"],
        location(details_uri, details, "# Details", 0)
    );
    let diagnostics = messages
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == manual_uri
        })
        .expect("manual diagnostics notification");
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));
}

#[test]
fn manual_wire_scopes_equal_capture_paths_by_configured_source() {
    let first_uri = "file:///selected/first/start.md";
    let second_uri = "file:///selected/second/start.md";
    let mut initialize = selected(&[]);
    initialize["params"]["initializationOptions"]["manuals"] = json!([
        {"source":"source:first","uri":first_uri,"path":"docs/start.md","text":"# First\n"},
        {"source":"source:second","uri":second_uri,"path":"docs/start.md","text":"# Second\n"}
    ]);
    let (result, messages) = selected_session(
        initialize,
        [
            request(
                2,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":first_uri}}),
            ),
            request(
                3,
                "textDocument/documentSymbol",
                json!({"textDocument":{"uri":second_uri}}),
            ),
        ],
        Limits::default(),
    );
    result.expect("source-qualified LSP manuals");
    assert_eq!(response(&messages, 2)["result"][0]["name"], "First");
    assert_eq!(response(&messages, 3)["result"][0]["name"], "Second");
}

#[test]
fn resolved_documentation_navigation_reports_origins_and_explicit_closure_unavailability() {
    let manual_uri = "file:///selected/docs/start.md";
    let manual = "# Start\n\n[Local](#start)\n";
    let mut initialize = selected(&[]);
    initialize["params"]["initializationOptions"]["workspace"]["text"] =
        json!(DOCUMENTATION_WORKSPACE);
    initialize["params"]["initializationOptions"]["manuals"] = json!([{
        "source":"source:gazette","uri":manual_uri,"path":"docs/start.md","text":manual,
    }]);
    let route_query = json!({
        "kind":"field",
        "query":{"kind":"brand","identity":"brand:gazette","field":[
            "publications",0,"documentation_repository","build","modules",0,"navigation",0,"pages",0,"route"
        ]}
    });
    let (result, messages) = selected_session(
        initialize,
        [request(
            2,
            "gazette/origin",
            json!({"generation":1,"query":route_query}),
        )],
        Limits::default(),
    );
    result.expect("documentation authoring protocol");
    let diagnostics = messages
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == URI
        })
        .expect("workspace diagnostics")["params"]["diagnostics"]
        .as_array()
        .unwrap();
    let incomplete = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == "documentation_validation_incomplete")
        .unwrap_or_else(|| panic!("explicit incomplete closure: {diagnostics:?}"));
    let view = &incomplete["data"]["documentation"];
    assert_eq!(view["manual_pages"][0]["source"], "source:gazette");
    assert_eq!(view["manual_pages"][0]["route"], "gazette/start");
    assert_eq!(view["manual_pages"][0]["input"]["path"], "docs/start.md");
    assert_eq!(
        view["check"]["build"]["modules"][0]["navigation"][1]["kind"],
        "reference"
    );
    assert_eq!(view["closure"]["status"], "unavailable");
    let origin = &response(&messages, 2)["result"];
    assert_eq!(
        origin["native"]["value"]["origin"]["identity"],
        "brand:gazette"
    );
    assert_eq!(
        origin["native"]["value"]["origin"]["path"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "route"
    );
}

#[test]
fn documentation_manual_closure_defects_are_target_errors() {
    for manuals in [
        json!([]),
        json!([{
            "source":"source:gazette",
            "uri":"file:///selected/docs/start.md",
            "path":"docs/start.md",
            "text":"# Start\n\n[Missing](#missing)\n"
        }]),
    ] {
        let mut initialize = selected(&[]);
        initialize["params"]["initializationOptions"]["workspace"]["text"] =
            json!(DOCUMENTATION_WORKSPACE);
        initialize["params"]["initializationOptions"]["manuals"] = manuals;
        let (result, messages) = selected_session(initialize, [], Limits::default());
        result.expect("documentation closure diagnostics");
        let diagnostics = messages
            .iter()
            .find(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == URI
            })
            .expect("workspace diagnostics")["params"]["diagnostics"]
            .as_array()
            .expect("diagnostic array");
        let failure = diagnostics
            .iter()
            .find(|diagnostic| diagnostic["code"] == "documentation_target_input")
            .unwrap_or_else(|| panic!("real manual closure diagnostic: {diagnostics:?}"));
        assert_eq!(failure["severity"], 1);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| { diagnostic["code"] != "documentation_validation_incomplete" })
        );
    }
}

#[test]
fn documentation_navigation_kind_completion_keeps_manuals_and_references_distinct() {
    let workspace = DOCUMENTATION_WORKSPACE.replacen("kind = \"manual\"", "kind = \"m\"", 1);
    let mut initialize = selected(&[]);
    initialize["params"]["initializationOptions"]["workspace"]["text"] = json!(workspace);
    let offset = workspace.find("\"m\"").unwrap() + 2;
    let before = &workspace[..offset];
    let position = json!({
        "line":before.matches('\n').count(),
        "character":before.rsplit('\n').next().unwrap().encode_utf16().count(),
    });
    let (result, messages) = selected_session(
        initialize,
        [request(
            2,
            "textDocument/completion",
            json!({"textDocument":{"uri":URI},"position":position}),
        )],
        Limits::default(),
    );
    result.expect("documentation schema completion");
    let items = response(&messages, 2)["result"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["manual", "reference"]
    );
}
