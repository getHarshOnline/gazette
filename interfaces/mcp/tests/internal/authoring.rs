//! Real shared state, native navigation, bounded lifetimes and explicit wire discovery.

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

#[path = "authoring/assistance.rs"]
mod assistance;
#[path = "authoring/completion.rs"]
mod completion;
#[path = "authoring/component_assistance.rs"]
mod component_assistance;
#[path = "authoring/heading_completion.rs"]
mod heading_completion;
#[path = "authoring/origin.rs"]
mod origin;
#[path = "authoring/repair.rs"]
mod repair;
#[path = "authoring/workspace.rs"]
mod workspace;

#[test]
fn search_symbols_has_typed_discovery_current_evidence_and_no_partial_or_stale_success() {
    let state = Sessions::default();
    let handle = start(&state);
    let input = json!({"session":handle,"generation":1,"query":"CAFÉ"});
    let result = call(&state, Route::Search, input.clone(), 1 << 18);
    assert_ne!(result["isError"], true, "{result}");
    let symbols = result["structuredContent"]["symbols"]
        .as_array()
        .expect("complete result");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "Café 🦀");
    assert_eq!(symbols[0]["document"], "page");
    assert_eq!(
        symbols[0]["source"]["source_sha256"],
        json!(gazette_cli::sdk::source::Sha256Digest::of(PAGE.as_bytes()))
    );
    let descriptor = json!(Route::Search.descriptor());
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["openWorldHint"], false);
    let validator = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
    assert!(validator.is_valid(&input));
    for bad in [
        json!({"session":handle,"generation":1}),
        json!({"session":handle,"generation":1,"query":7}),
        json!({"session":handle,"generation":1,"query":"","extra":true}),
    ] {
        assert!(!validator.is_valid(&bad));
        assert!(
            Route::Search
                .decode(bad.as_object().expect("object").clone())
                .is_err()
        );
    }
    assert_eq!(
        call(&state, Route::Search, input.clone(), 256)["isError"],
        true
    );
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"page","version":1,"text":"+++\n[broken\n+++\n# One\n"}),
        1 << 18,
    );
    assert_eq!(
        call(&state, Route::Search, input, 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let failed = call(
        &state,
        Route::Search,
        json!({"session":handle,"generation":2,"query":"owner"}),
        1 << 18,
    );
    assert_eq!(failed["isError"], true);
    assert!(failed["structuredContent"].get("symbols").is_none());
}

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const PAGE: &str = "# Café 🦀\n\n[go](#café-🦀)\n\nNote[^a].\n\n[^a]: Definition\n";

fn selected_location(
    id: &str,
    version: Option<i32>,
    source: gazette_cli::sdk::authoring::SourceLocation<'_>,
) -> Value {
    let mut value = json!(source);
    value["document"] = json!(id);
    value["version"] = json!(version);
    value
}

fn native_session(page: &str) -> gazette_cli::sdk::authoring::session::Session {
    gazette_cli::sdk::authoring::session::Session::new(
        gazette_cli::sdk::authoring::session::Source {
            id: "file:///not-opened/workspace".into(),
            path: "workspace.toml".parse().expect("workspace path"),
            text: WORKSPACE.into(),
        },
        vec![gazette_cli::sdk::authoring::session::Source {
            id: "page".into(),
            path: "page.md".parse().expect("page path"),
            text: page.into(),
        }],
        Default::default(),
    )
    .expect("independent shared SDK selection")
}

#[test]
fn authoring_integer_discovery_matches_generation_offset_and_version_decoding() {
    let beyond_generation: Value = serde_json::from_str(&((u64::MAX as u128) + 1).to_string())
        .expect("out-of-range generation integer");
    let beyond_offset: Value = serde_json::from_str(&((usize::MAX as u128) + 1).to_string())
        .expect("out-of-range offset integer");
    for (route, input) in [
        (
            Route::Open,
            json!({"session":"s","generation":1,"document":"p","version":1,"text":"# A"}),
        ),
        (
            Route::Change,
            json!({"session":"s","generation":1,"document":"p","version":1,"text":"# A"}),
        ),
        (
            Route::Close,
            json!({"session":"s","generation":1,"document":"p"}),
        ),
        (Route::End, json!({"session":"s","generation":1})),
        (
            Route::Diagnostics,
            json!({"session":"s","generation":1,"document":"p"}),
        ),
        (
            Route::PagePackage,
            json!({"session":"s","generation":1,"document":"p"}),
        ),
        (
            Route::Symbols,
            json!({"session":"s","generation":1,"document":"p"}),
        ),
        (
            Route::Search,
            json!({"session":"s","generation":1,"query":""}),
        ),
        (
            Route::Definition,
            json!({"session":"s","generation":1,"document":"p","offset":0}),
        ),
        (
            Route::Hover,
            json!({"session":"s","generation":1,"document":"p","offset":0}),
        ),
        (
            Route::Completion,
            json!({"session":"s","generation":1,"document":"p","offset":0}),
        ),
        (
            Route::Repair,
            json!({"session":"s","generation":1,"document":"p","version":null,
                "source_sha256":"0000000000000000000000000000000000000000000000000000000000000000",
                "offset":0,"diagnostic":{"path":"p.md","source_sha256":"0000000000000000000000000000000000000000000000000000000000000000",
                "code":"x","message":"x","field":null,"key":null,"source":null,"target":null,
                "location":null,"selection_wide":false}}),
        ),
        (
            Route::HeadingCompletion,
            json!({"session":"s","generation":1,"document":"p","offset":0}),
        ),
        (
            Route::References,
            json!({"session":"s","generation":1,"document":"p","offset":0,"include_declaration":true}),
        ),
    ] {
        let schema =
            serde_json::to_value(route.descriptor()).expect("descriptor")["inputSchema"].clone();
        let validator = jsonschema::validator_for(&schema).expect("native numeric discovery");
        let check = |field: &str, value: Value, accepted: bool| {
            let mut input = input.clone();
            input[field] = value;
            assert_eq!(validator.is_valid(&input), accepted, "{input}");
            assert_eq!(
                route
                    .decode(input.as_object().expect("arguments").clone())
                    .is_ok(),
                accepted,
                "{input}"
            );
        };
        check("generation", json!(u64::MAX), true);
        check("generation", beyond_generation.clone(), false);
        if input.get("offset").is_some() {
            check("offset", json!(usize::MAX), true);
            check("offset", beyond_offset.clone(), false);
        }
        if input.get("version").is_some() {
            check("version", json!(i32::MIN), true);
            check("version", json!(i32::MAX), true);
            check("version", json!((i32::MIN as i64) - 1), false);
            check("version", json!((i32::MAX as i64) + 1), false);
        }
    }
}

#[test]
fn failed_current_heading_analysis_is_not_reported_as_empty_success() {
    for source in [
        "+++\n[broken\n+++\n# One\n",
        "# One {#same}\n\n# Two {#same}\n",
    ] {
        let state = Sessions::default();
        let handle = start(&state);
        call(
            &state,
            Route::Open,
            json!({"session":handle,"generation":1,"document":"page","version":1,"text":source}),
            1 << 18,
        );
        let result = call(
            &state,
            Route::Symbols,
            json!({"session":handle,"generation":2,"document":"page"}),
            1 << 18,
        );
        assert_eq!(result["isError"], true, "{result}");
        assert_eq!(
            result["structuredContent"]["code"],
            "authoring_symbols_unavailable"
        );
    }
}

#[test]
fn diagnostics_tool_projects_the_same_pre_theme_markdown_renderability_failures() {
    let (body, code, reason) = (
        "<script>unsafe()</script>\n",
        "markdown_raw_html_unavailable",
        "raw HTML requires an admitted renderer",
    );
    let page = format!(
        "+++\n[content]\nid='content:render'\nsite='site:one'\ntitle='Render'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={{kind='article'}}\n+++\n{body}"
    );
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":WORKSPACE},
            "documents":[{"id":"render","path":"render.md","text":page}]
        }),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("held session");
    let result = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"render"}),
        1 << 18,
    );
    let row = result["structuredContent"]["value"]
        .as_array()
        .expect("complete diagnostic projection")
        .iter()
        .find(|row| row["message"] == reason)
        .expect("shared renderability row");
    assert_eq!(row["code"], code);
    assert_eq!(row["selection_wide"], false);
    assert_eq!(result["structuredContent"]["generation"], 1);
    assert_eq!(result["structuredContent"]["path"], "render.md");
    assert!(
        row["location"]["span"]["end"].as_u64().unwrap()
            > row["location"]["span"]["start"].as_u64().unwrap()
    );

    for body in [
        "note[^outer]\n\n[^outer]: nested[^inner]\n\n[^inner]: target\n",
        "![label[^note]](https://example.invalid/image.png)\n\n[^note]: detail\n",
    ] {
        let page = format!(
            "+++\n[content]\nid='content:render'\nsite='site:one'\ntitle='Render'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={{kind='article'}}\n+++\n{body}"
        );
        let state = Sessions::default();
        let started = call(
            &state,
            Route::Start,
            json!({
                "workspace":{"id":"workspace","path":"workspace.toml","text":WORKSPACE},
                "documents":[{"id":"render","path":"render.md","text":page}]
            }),
            1 << 18,
        );
        let handle = started["structuredContent"]["session"]
            .as_str()
            .expect("held session");
        let result = call(
            &state,
            Route::Diagnostics,
            json!({"session":handle,"generation":1,"document":"render"}),
            1 << 18,
        );
        assert!(
            result["structuredContent"]["value"]
                .as_array()
                .expect("complete diagnostic projection")
                .iter()
                .all(|row| row["code"] != "markdown_adapter"),
            "supported footnote structure must not retain the old provider diagnostic"
        );
    }
}

#[test]
fn selected_baselines_require_named_objects_not_positional_serde_sequences() {
    for value in [
        json!({"workspace":["w","w.toml",WORKSPACE],"documents":[]}),
        json!({"workspace":{"id":"w","path":"w.toml","text":WORKSPACE},"documents":[["p","p.md",PAGE]]}),
        json!({"workspace":{"id":"w","path":"w.toml","text":WORKSPACE},"documents":[],"manuals":[["m","m.md","# Manual"]]}),
    ] {
        assert!(
            Route::Start
                .decode(value.as_object().expect("object").clone())
                .is_err()
        );
    }
}

#[test]
fn manuals_share_the_sdk_markdown_contract_without_becoming_content_documents() {
    let state = Sessions::default();
    let manual = "# Manual 🦀\n\n[Details](details.md#details)\n";
    let details = "# Details\n";
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":WORKSPACE},
            "documents":[],
            "manuals":[
                {"source":"source:manual","id":"details","path":"manual/details.md","text":details},
                {"source":"source:manual","id":"manual","path":"manual/index.md","text":manual}
            ]
        }),
        1 << 18,
    );
    assert_eq!(started["isError"], false, "{started}");
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("manual session");
    let status = call(&state, Route::Status, json!({"session":handle}), 1 << 18);
    assert_eq!(
        status["structuredContent"]["files"]
            .as_array()
            .expect("selected files")
            .iter()
            .find(|file| file["document"] == "manual")
            .expect("manual file")["manual"],
        true
    );
    let symbols = call(
        &state,
        Route::Symbols,
        json!({"session":handle,"generation":1,"document":"manual"}),
        1 << 18,
    );
    assert_eq!(
        symbols["structuredContent"]["value"][0]["heading"]["text"],
        "Manual 🦀"
    );
    let definition = call(
        &state,
        Route::Definition,
        json!({
            "session":handle,
            "generation":1,
            "document":"manual",
            "offset":manual.find("[Details]").expect("manual link")
        }),
        1 << 18,
    );
    assert_eq!(
        definition["structuredContent"]["value"]["path"], "manual/details.md",
        "{definition}"
    );
    let schema = serde_json::to_value(Route::Start.descriptor()).expect("start schema");
    assert!(
        jsonschema::validator_for(&schema["inputSchema"])
            .expect("manual start schema")
            .is_valid(&json!({
                "workspace":{"id":"workspace","path":"workspace.toml","text":WORKSPACE},
                "documents":[],
                "manuals":[{"source":"source:manual","id":"manual","path":"manual/index.md","text":manual}]
            }))
    );
}

#[test]
fn manual_wire_scopes_equal_capture_paths_by_configured_source() {
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":WORKSPACE},
            "documents":[],
            "manuals":[
                {"source":"source:first","id":"first","path":"docs/start.md","text":"# First\n"},
                {"source":"source:second","id":"second","path":"docs/start.md","text":"# Second\n"}
            ]
        }),
        1 << 18,
    );
    assert_eq!(started["isError"], false, "{started}");
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("source-qualified manual session");
    for (document, heading) in [("first", "First"), ("second", "Second")] {
        let symbols = call(
            &state,
            Route::Symbols,
            json!({"session":handle,"generation":1,"document":document}),
            1 << 18,
        );
        assert_eq!(
            symbols["structuredContent"]["value"][0]["heading"]["text"],
            heading
        );
    }
}

#[test]
fn page_package_tool_projects_binary_safe_compiler_ownership() {
    let workspace = r#"
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
    let page = "+++\n[[resources]]\ninputs=['guide/main.ts']\nbuild={kind='browser-module',entry='guide/main.ts'}\noutput='/guide/main.js'\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# Guide\n";
    let bytes = [0_u8, 159, 146, 150, 255];
    let program = b"export const answer = 42;";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":workspace},
            "documents":[{"id":"page","path":"content/guide/index.md","text":page}],
            "page_inputs":[
                {"id":"image","path":"content/guide/image.png","bytes_base64":STANDARD.encode(bytes)},
                {"id":"program","path":"content/guide/main.ts","bytes_base64":STANDARD.encode(program)}
            ]
        }),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("page-package session");
    let owner = call(
        &state,
        Route::PagePackage,
        json!({"session":handle,"generation":1,"document":"page"}),
        1 << 18,
    );
    assert_eq!(owner["isError"], false, "{owner}");
    assert_eq!(
        owner["structuredContent"]["value"]["package"]["document"],
        "content/guide/index.md"
    );
    assert_eq!(
        owner["structuredContent"]["value"]["role"]["kind"],
        "document"
    );
    let companion = call(
        &state,
        Route::PagePackage,
        json!({"session":handle,"generation":1,"document":"image"}),
        1 << 18,
    );
    assert_eq!(
        companion["structuredContent"]["value"]["role"]["kind"],
        "companion"
    );
    let definition = call(
        &state,
        Route::Definition,
        json!({
            "session":handle,
            "generation":1,
            "document":"page",
            "offset":page.find("'guide/main.ts'").expect("resource input") + 2
        }),
        1 << 18,
    );
    assert_eq!(definition["isError"], false, "{definition}");
    assert_eq!(
        definition["structuredContent"]["value"]["document"],
        "program"
    );
    assert_eq!(
        definition["structuredContent"]["value"]["path"],
        "content/guide/main.ts"
    );
    assert_eq!(
        definition["structuredContent"]["value"]["compiled_destinations"][0],
        "/guide/main.js"
    );
    let status = call(&state, Route::Status, json!({"session":handle}), 1 << 18);
    assert_eq!(
        status["structuredContent"]["page_inputs"][0]["document"],
        "image"
    );
    assert_eq!(
        status["structuredContent"]["page_inputs"][0]["source_sha256"],
        gazette_cli::sdk::source::Sha256Digest::of(&bytes).to_string()
    );
    assert_eq!(
        status["structuredContent"]["page_inputs"][1]["document"],
        "program"
    );
}

fn call(state: &Sessions, route: Route, value: Value, maximum: usize) -> Value {
    let result = state
        .execute(
            route
                .decode(value.as_object().expect("arguments").clone())
                .expect("typed arguments"),
            maximum,
        )
        .expect("tool result");
    let result = serde_json::to_value(result).expect("wire result");
    let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
    let validator = jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("discoverable authoring output schema");
    assert!(
        validator.is_valid(&result["structuredContent"]),
        "{} emitted a result outside its actual output contract: {result}; {:?}",
        route.name(),
        validator
            .iter_errors(&result["structuredContent"])
            .collect::<Vec<_>>()
    );
    result
}

#[test]
fn empty_success_and_declaration_inclusion_use_the_same_native_output_contracts() {
    let state = Sessions::default();
    // Use a complete valid heading-reference set; the shared PAGE fixture's
    // unresolved fragment intentionally makes heading navigation unavailable.
    let page = "# Heading\n\n[go](#heading)\n\nNote[^a].\n\n[^a]: Definition\n";
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"file:///not-opened/workspace","path":"workspace.toml","text":WORKSPACE},
            "documents":[{"id":"page","path":"page.md","text":page}]
        }),
        1 << 18,
    );
    assert_eq!(started["isError"], false);
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("explicit session");
    let workspace =
        gazette_cli::sdk::workspace::ParsedWorkspace::parse(WORKSPACE, Default::default())
            .expect("native workspace declarations");
    for route in [Route::Diagnostics, Route::Symbols] {
        let result = call(
            &state,
            route,
            json!({"session":handle,"generation":1,"document":"file:///not-opened/workspace"}),
            1 << 18,
        );
        assert_eq!(result["isError"], false, "{result}");
        let expected = if matches!(route, Route::Symbols) {
            json!(
                workspace
                    .navigation()
                    .symbols()
                    .iter()
                    .map(|declaration| {
                        json!({"declaration":declaration,"location":declaration.location()})
                    })
                    .collect::<Vec<_>>()
            )
        } else {
            json!([])
        };
        assert_eq!(result["structuredContent"]["value"], expected);
    }
    let empty = call(
        &state,
        Route::Definition,
        json!({"session":handle,"generation":1,"document":"page","offset":page.len()}),
        1 << 18,
    );
    assert_eq!(empty["isError"], false, "{empty}");
    assert_eq!(empty["structuredContent"]["value"], Value::Null);
    let empty = call(
        &state,
        Route::References,
        json!({"session":handle,"generation":1,"document":"page","offset":page.len(),"include_declaration":true}),
        1 << 18,
    );
    assert_eq!(empty["structuredContent"]["value"], json!([]));
    let references = call(
        &state,
        Route::References,
        json!({"session":handle,"generation":1,"document":"page","offset":page.find("Note[^a]").expect("reference") + 5,"include_declaration":true}),
        1 << 18,
    );
    let session = native_session(page);
    let document = &session.files()["page"];
    let association = session
        .analysis()
        .as_ref()
        .expect("analysis")
        .source_navigation(
            document.path(),
            page,
            page.find("Note[^a]").expect("reference") + 5,
        )
        .expect("native navigation")
        .expect("association");
    let mut expected = association.references;
    expected.push(association.definition);
    expected.sort_by_key(|location| (location.location.span.start, location.location.span.end));
    expected.dedup();
    assert_eq!(
        references["structuredContent"]["value"],
        json!(
            expected
                .into_iter()
                .map(|location| selected_location("page", None, location))
                .collect::<Vec<_>>()
        )
    );
}

#[test]
fn output_discovery_rejects_corrupted_success_fields_and_untyped_errors() {
    let state = Sessions::default();
    let handle = start(&state);
    for (route, input, pointer, invalid) in [
        (
            Route::Status,
            json!({"session":handle}),
            "/files/0/source_sha256",
            json!("not a digest"),
        ),
        (
            Route::Symbols,
            json!({"session":handle,"generation":1,"document":"page"}),
            "/value/0/location/precision",
            json!("guessed"),
        ),
        (
            Route::Definition,
            json!({"session":handle,"generation":1,"document":"page","offset":PAGE.find("Note[^a]").expect("reference") + 5}),
            "/value/location/span/start",
            json!(-1),
        ),
        (
            Route::Open,
            json!({"session":handle,"generation":1,"document":"unknown","version":1,"text":"unused"}),
            "/changed",
            json!("false"),
        ),
    ] {
        let result = call(&state, route, input, 1 << 18);
        let mut value = result["structuredContent"].clone();
        *value
            .pointer_mut(pointer)
            .expect("actual native output field") = invalid;
        let schema =
            serde_json::to_value(route.descriptor()).expect("descriptor")["outputSchema"].clone();
        let validator = jsonschema::validator_for(&schema).expect("output schema");
        assert!(
            !validator.is_valid(&value),
            "{} accepted {value}",
            route.name()
        );
    }
    for route in Route::ALL {
        let schema =
            serde_json::to_value(route.descriptor()).expect("descriptor")["outputSchema"].clone();
        let validator = jsonschema::validator_for(&schema).expect("output schema");
        for corrupt in [
            json!({}),
            json!({"code":false,"message":123}),
            json!({"session":"s","generation":-1,"changed":true}),
        ] {
            assert!(
                !validator.is_valid(&corrupt),
                "{} accepted {corrupt}",
                route.name()
            );
        }
    }
}

fn start(state: &Sessions) -> String {
    let result = call(
        state,
        Route::Start,
        json!({
            "workspace":{"id":"file:///not-opened/workspace","path":"workspace.toml","text":WORKSPACE},
            "documents":[{"id":"page","path":"page.md","text":PAGE}]
        }),
        1 << 18,
    );
    assert_eq!(result["isError"], false, "{result}");
    result["structuredContent"]["session"]
        .as_str()
        .expect("opaque handle")
        .into()
}

#[test]
fn overlays_preserve_native_current_evidence_and_restore_selected_baseline() {
    let state = Sessions::default();
    let handle = start(&state);
    let open = call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"page","version":5,"text":"# New\n"}),
        1 << 18,
    );
    assert_eq!(open["structuredContent"]["generation"], 2);
    assert_eq!(open["structuredContent"]["changed"], true);
    let stale = call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":1,"document":"page","version":6,"text":"# Wrong"}),
        1 << 18,
    );
    assert_eq!(
        stale["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let stale_version = call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":2,"document":"page","version":5,"text":"# Wrong"}),
        1 << 18,
    );
    assert_eq!(stale_version["structuredContent"]["changed"], false);
    let symbols = call(
        &state,
        Route::Symbols,
        json!({"session":handle,"generation":2,"document":"page"}),
        1 << 18,
    );
    assert_eq!(
        symbols["structuredContent"]["value"][0]["heading"]["text"],
        "New"
    );
    let close = call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":2,"document":"page"}),
        1 << 18,
    );
    assert_eq!(close["structuredContent"]["generation"], 3);
    let symbols = call(
        &state,
        Route::Symbols,
        json!({"session":handle,"generation":3,"document":"page"}),
        1 << 18,
    );
    assert_eq!(
        symbols["structuredContent"]["value"][0]["heading"]["text"],
        "Café 🦀"
    );
    assert_eq!(symbols["structuredContent"]["version"], Value::Null);
    assert_eq!(
        symbols["structuredContent"]["source_sha256"],
        json!(gazette_cli::sdk::source::Sha256Digest::of(PAGE.as_bytes()))
    );
}

#[test]
fn definition_references_and_diagnostics_match_exact_sdk_sources() {
    let state = Sessions::default();
    let handle = start(&state);
    let offset = PAGE.find("Note[^a]").expect("reference") + 5;
    let definitions = call(
        &state,
        Route::Definition,
        json!({"session":handle,"generation":1,"document":"page","offset":offset}),
        1 << 18,
    );
    let references = call(
        &state,
        Route::References,
        json!({"session":handle,"generation":1,"document":"page","offset":offset,"include_declaration":false}),
        1 << 18,
    );
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"page"}),
        1 << 18,
    );
    let session = native_session(PAGE);
    let document = &session.files()["page"];
    let analysis = session.analysis().as_ref().expect("native analysis");
    let expected = analysis
        .source_navigation(document.path(), PAGE, offset)
        .expect("native navigation")
        .expect("association");
    assert_eq!(
        definitions["structuredContent"]["value"],
        selected_location("page", None, expected.definition)
    );
    assert_eq!(
        references["structuredContent"]["value"],
        json!(
            expected
                .references
                .into_iter()
                .map(|location| selected_location("page", None, location))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        diagnostics["structuredContent"]["value"],
        json!(
            gazette_cli::sdk::authoring::diagnostics(analysis, document.path(), PAGE, 1 << 18)
                .expect("native diagnostics")
        )
    );
    let wrong_boundary = PAGE.find('é').expect("Unicode byte") + 1;
    let wrong = call(
        &state,
        Route::Definition,
        json!({"session":handle,"generation":1,"document":"page","offset":wrong_boundary}),
        1 << 18,
    );
    assert_eq!(wrong["structuredContent"]["code"], "authoring_navigation");
}

#[test]
fn current_workspace_failure_is_not_a_stale_success_or_file_read() {
    let state = Sessions::default();
    let handle = start(&state);
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"file:///not-opened/workspace","version":1,"text":"[broken"}),
        1 << 18,
    );
    let result = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":2,"document":"file:///not-opened/workspace"}),
        1 << 18,
    );
    assert_eq!(
        result["structuredContent"]["value"][0]["code"],
        "toml_invalid"
    );
    let status = call(&state, Route::Status, json!({"session":handle}), 1 << 18);
    assert_eq!(status["structuredContent"]["structurally_valid"], false);
    assert!(!status.to_string().contains("[broken"));
}

#[test]
fn bounded_capacity_and_close_have_no_implicit_session_recreation() {
    let state = Sessions::default();
    let handles: Vec<_> = (0..4).map(|_| start(&state)).collect();
    let refused = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"w","path":"w.toml","text":WORKSPACE},"documents":[]}),
        1 << 18,
    );
    assert_eq!(refused["structuredContent"]["code"], "authoring_capacity");
    let ended = call(
        &state,
        Route::End,
        json!({"session":handles[0],"generation":1}),
        1 << 18,
    );
    assert_eq!(ended["structuredContent"]["changed"], true);
    let unknown = call(
        &state,
        Route::Status,
        json!({"session":handles[0]}),
        1 << 18,
    );
    assert_eq!(
        unknown["structuredContent"]["code"],
        "authoring_unknown_session"
    );
    start(&state);
}

#[test]
fn insufficient_acknowledgement_capacity_never_mutates_or_allocates_hidden_state() {
    let state = Sessions::default();
    let failed = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"w","path":"w.toml","text":WORKSPACE},"documents":[]}),
        1,
    );
    assert_eq!(failed["structuredContent"]["code"], "mcp_output_limit");
    let handle = start(&state);
    let failed = call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"page","version":1,"text":"# Changed"}),
        1,
    );
    assert_eq!(failed["structuredContent"]["code"], "mcp_output_limit");
    let status = call(&state, Route::Status, json!({"session":handle}), 1 << 18);
    assert_eq!(status["structuredContent"]["generation"], 1);
}

#[test]
fn typed_discovery_requires_explicit_generation_and_declares_only_admitted_file_import() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
        let validator =
            jsonschema::validator_for(&descriptor["inputSchema"]).expect("valid schema");
        assert!(
            !validator.is_valid(&json!({"unknown":true})),
            "{}",
            route.name()
        );
        assert!(
            route
                .decode(json!({"unknown":true}).as_object().expect("object").clone())
                .is_err()
        );
        assert_eq!(
            descriptor["annotations"]["openWorldHint"],
            matches!(route, Route::Import)
        );
    }
    let descriptor = serde_json::to_value(Route::Change.descriptor()).expect("descriptor");
    let validator = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    assert!(!validator.is_valid(&json!({"session":"x","document":"page","version":1,"text":"x"})));
    assert!(
        validator.is_valid(
            &json!({"session":"x","generation":1,"document":"page","version":1,"text":"x"})
        )
    );
    assert!(!validator.is_valid(
        &json!({"session":"x","generation":1,"document":"page","version":1,"text":"x","open":true})
    ));
}
