//! Typed workspace tools reuse exact SDK associations and held-session lifecycle.

use super::*;
use gazette_cli::sdk::{
    authoring::{PackageBaselineKey, session},
    source::Sha256Digest,
};

const ID: &str = "workspace";
const BASE_THEME: &str = "package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={page='page.html'}\npalettes=[{id='palette:night',browser_theme_colors={light='#111111',dark='#000000'}},{id='palette:day',browser_theme_colors={light='#ffffff',dark='#eeeeee'}}]\n";
const REPLACEMENT_THEME: &str = "package='theme:replacement'\nnamespace='replacement'\ntemplates='templates'\nrender={page='page.html'}\n";
const SEARCH_PLUGIN: &str = "package='search-plugin'\n[resources.'module:search']\ninputs=['search.js']\nbuild={kind='browser-module',entry='search.js'}\n[providers.search]\nimplements=['publication.search/v1']\n";

fn text() -> String {
    "principals=[{name='é🦀 owner:one',id='owner:one',kind='person'}]\r\nsites=[{id='site:one',publisher=\"owner:\\u006fne\",deployable=false}]\r\n".into()
}

fn begin(state: &Sessions, text: &str, broken: bool) -> String {
    let documents = if broken {
        json!([{"id":"bad","path":"bad.md","text":"+++\n[broken\n+++\n"}])
    } else {
        json!([])
    };
    let result = call(
        state,
        Route::Start,
        json!({"workspace":{"id":ID,"path":"workspace.toml","text":text},"documents":documents}),
        1 << 18,
    );
    result["structuredContent"]["session"]
        .as_str()
        .expect("registered handle")
        .into()
}

fn composition_begin(state: &Sessions, text: &str) -> String {
    let result = call(
        state,
        Route::Start,
        json!({
            "workspace":{"id":ID,"path":"workspace.toml","text":text},
            "documents":[{"id":"bad","path":"bad.md","text":"+++\n[broken\n+++\n"}],
            "package_baselines":[
                {"kind":"theme","package":"theme:base","id":"theme-base","path":"packages/theme-base.toml","text":BASE_THEME},
                {"kind":"theme","package":"theme:replacement","id":"theme-replacement","path":"packages/theme-replacement.toml","text":REPLACEMENT_THEME},
                {"kind":"plugin","package":"search-plugin","id":"search-plugin","path":"packages/search-plugin.toml","text":SEARCH_PLUGIN}
            ]
        }),
        1 << 18,
    );
    result["structuredContent"]["session"]
        .as_str()
        .expect("registered composition handle")
        .into()
}

fn composition_package_sources() -> Vec<session::PackageSource> {
    [
        (
            PackageBaselineKey::Theme("theme:base".parse().expect("base Theme package")),
            "theme-base",
            "packages/theme-base.toml",
            BASE_THEME,
        ),
        (
            PackageBaselineKey::Theme(
                "theme:replacement"
                    .parse()
                    .expect("replacement Theme package"),
            ),
            "theme-replacement",
            "packages/theme-replacement.toml",
            REPLACEMENT_THEME,
        ),
        (
            PackageBaselineKey::Plugin("search-plugin".parse().expect("search plugin package")),
            "search-plugin",
            "packages/search-plugin.toml",
            SEARCH_PLUGIN,
        ),
    ]
    .into_iter()
    .map(|(package, id, path, text)| session::PackageSource {
        package,
        input: session::Source {
            id: id.into(),
            path: path.parse().expect("package baseline path"),
            text: text.into(),
        },
    })
    .collect()
}

#[test]
fn selected_package_diagnostics_match_sdk_for_themes_plugins_and_current_overlays() {
    let text = format!(
        "{}\nsources=[{{id='source:theme',location={{kind='filesystem',root='/not-opened/theme'}}}},{{id='source:plugin',location={{kind='filesystem',root='/not-opened/plugin'}}}}]\ntheme_packages=[{{package='theme:base',source='source:theme',manifest='base.toml'}},{{package='theme:replacement',source='source:theme',manifest='replacement.toml'}}]\nplugin_packages=[{{package='search-plugin',source='source:plugin',manifest='plugin.toml'}}]\n",
        text()
    );
    let state = Sessions::default();
    let handle = composition_begin(&state, &text);
    let mut native = session::Session::new_with_package_baselines(
        session::Source {
            id: ID.into(),
            path: "workspace.toml".parse().unwrap(),
            text,
        },
        [session::Source {
            id: "bad".into(),
            path: "bad.md".parse().unwrap(),
            text: "+++\n[broken\n+++\n".into(),
        }],
        composition_package_sources(),
        Default::default(),
    )
    .unwrap();
    let mut generation = 1;
    for source in composition_package_sources() {
        let id = &source.input.id;
        let request = |generation| json!({"session":handle,"generation":generation,"document":id});
        let valid = call(&state, Route::Diagnostics, request(generation), 1 << 18);
        assert_ne!(valid["isError"], true, "{valid}");
        assert_eq!(valid["structuredContent"]["value"], json!([]));

        let broken = "package=[\n";
        assert!(native.open(id, 1, broken.into()).unwrap());
        let opened = call(
            &state,
            Route::Open,
            json!({"session":handle,"generation":generation,"document":id,"version":1,"text":broken}),
            1 << 18,
        );
        assert_eq!(opened["structuredContent"]["changed"], true);
        let previous = generation;
        generation = opened["structuredContent"]["generation"].as_u64().unwrap();
        let document = &native.files()[id];
        let expected = gazette_cli::sdk::authoring::package_diagnostics(
            native.analysis().as_ref().unwrap(),
            &source.package,
            document.path(),
            document.text(),
            1 << 18,
        )
        .unwrap();
        assert!(!expected.is_empty(), "invalid package has real diagnostics");
        let actual = call(&state, Route::Diagnostics, request(generation), 1 << 18);
        assert_ne!(actual["isError"], true, "{actual}");
        assert_eq!(actual["structuredContent"]["value"], json!(expected));
        assert_eq!(actual["structuredContent"]["version"], 1);
        assert_eq!(
            actual["structuredContent"]["source_sha256"],
            json!(Sha256Digest::of(broken.as_bytes()))
        );
        let diagnostic = actual["structuredContent"]["value"][0].clone();
        let offset = diagnostic["location"]["span"]["start"]
            .as_u64()
            .unwrap_or(0);
        let repair_input = json!({
            "session":handle,"generation":generation,"document":id,"version":1,
            "source_sha256":actual["structuredContent"]["source_sha256"],
            "offset":offset,"diagnostic":diagnostic
        });
        let repair = call(&state, Route::Repair, repair_input.clone(), 1 << 18);
        assert_ne!(repair["isError"], true, "{repair}");
        assert_eq!(repair["structuredContent"]["value"], Value::Null);
        let mut outside_source = repair_input.clone();
        outside_source["offset"] = json!(broken.len() + 1);
        assert_eq!(
            call(&state, Route::Repair, outside_source, 1 << 18)["structuredContent"]["code"],
            "authoring_diagnostic_binding"
        );
        assert_eq!(
            call(&state, Route::Diagnostics, request(generation), 1 << 18),
            actual,
            "unassisted review preserves the exact current diagnostic"
        );
        let mut forged = repair_input;
        forged["diagnostic"]["message"] = json!("not the current native diagnostic");
        assert_eq!(
            call(&state, Route::Repair, forged, 1 << 18)["structuredContent"]["code"],
            "authoring_diagnostic_binding"
        );
        assert_eq!(
            call(&state, Route::Diagnostics, request(previous), 1 << 18)["structuredContent"]["code"],
            "authoring_stale_generation"
        );
        assert_eq!(
            call(&state, Route::Diagnostics, request(generation), 128)["isError"],
            true
        );
        assert!(native.close(id).unwrap());
        let closed = call(&state, Route::Close, request(generation), 1 << 18);
        generation = closed["structuredContent"]["generation"].as_u64().unwrap();
        assert_eq!(
            call(&state, Route::Diagnostics, request(generation), 1 << 18)["structuredContent"]["value"],
            json!([])
        );
    }
    assert_eq!(
        call(
            &state,
            Route::End,
            json!({"session":handle,"generation":generation}),
            1 << 18
        )["structuredContent"]["changed"],
        true
    );
}

fn query(
    state: &Sessions,
    handle: &str,
    generation: u64,
    route: Route,
    offset: Option<usize>,
) -> Value {
    let mut input = json!({"session":handle,"generation":generation,"document":ID});
    if let Some(offset) = offset {
        input["offset"] = json!(offset);
    }
    if matches!(route, Route::References) {
        input["include_declaration"] = json!(true);
    }
    call(state, route, input, 1 << 18)
}

#[test]
fn resource_alias_tools_project_native_key_origins_and_quoted_reference_values() {
    let text = "# 🦀\r\n[design_inputs]\r\n\"tok\\u0065ns\"='source:tokens'\r\n\
[[principals]]\r\nid='person:one'\r\nkind='person'\r\n\
[[sites]]\r\nid='site:one'\r\npublisher='person:one'\r\ndeployable=false\r\n\
[[sources]]\r\nid='source:tokens'\r\nlocation={kind='filesystem',root='/not-opened/tokens'}\r\n\
[[theme_packages]]\r\npackage='theme:one'\r\nsource='source:tokens'\r\nmanifest='theme.toml'\r\nresource_inputs={'shared/tokens.css'={input='tokens',entry='dist/tokens.css'}}\r\n";
    let state = Sessions::default();
    let handle = begin(&state, text, false);
    let native = session::Session::new(
        session::Source {
            id: ID.into(),
            path: "workspace.toml".parse().unwrap(),
            text: text.into(),
        },
        [],
        Default::default(),
    )
    .unwrap();
    let analysis = native.analysis().as_ref().unwrap();
    let path = "workspace.toml".parse().unwrap();
    let offset = text.find("input='tokens'").unwrap() + "input=".len();
    let navigation = analysis
        .source_navigation(&path, text, offset)
        .unwrap()
        .unwrap();
    assert_eq!(
        navigation.definition.location.span.slice(text),
        Some("\"tok\\u0065ns\"")
    );
    let definition = query(&state, &handle, 1, Route::Definition, Some(offset));
    assert_eq!(
        definition["structuredContent"]["value"],
        super::selected_location(ID, None, navigation.definition)
    );
    let completion = query(&state, &handle, 1, Route::Completion, Some(offset));
    let expected = analysis
        .complete_reference(&path, text, offset, 1 << 18)
        .unwrap()
        .unwrap();
    assert_eq!(completion["structuredContent"]["value"], json!(expected));
    assert_eq!(
        completion["structuredContent"]["value"]["candidates"][0]["definition"],
        json!(navigation.definition)
    );
    assert_eq!(
        completion["structuredContent"]["value"]["candidates"][0]["replacement"],
        "\"tokens\""
    );
    assert_eq!(
        completion["structuredContent"]["value"]["candidates"][0]["symbol"]["kind"],
        "resource-input"
    );
    let hover = query(&state, &handle, 1, Route::Hover, Some(offset));
    assert_eq!(
        hover["structuredContent"]["value"],
        json!(
            analysis
                .workspace_identity(&path, text, offset)
                .unwrap()
                .unwrap()
        )
    );
    for (route, result) in [
        (Route::Completion, completion),
        (Route::Hover, hover),
        (Route::Definition, definition),
    ] {
        assert_ne!(result["isError"], true);
        let schema = json!(route.descriptor());
        jsonschema::validator_for(&schema["outputSchema"])
            .unwrap()
            .validate(&result["structuredContent"])
            .unwrap();
    }
}

#[test]
fn hover_returns_shared_symbol_precision_schema_and_current_generation_without_stale_fallback() {
    let text = text();
    let state = Sessions::default();
    let handle = begin(&state, &text, true);
    let offset = text.find("owner:\\u006fne").expect("reference");
    let native = session::Session::new(
        session::Source {
            id: ID.into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.clone(),
        },
        [],
        Default::default(),
    )
    .expect("native");
    let expected = native
        .analysis()
        .as_ref()
        .expect("analysis")
        .workspace_identity(&"workspace.toml".parse().expect("path"), &text, offset)
        .expect("source")
        .expect("identity");
    let original = query(&state, &handle, 1, Route::Hover, Some(offset));
    let report = &original["structuredContent"];
    assert_ne!(original["isError"], true);
    assert_eq!(report["value"], json!(expected));
    assert_eq!(report["source_sha256"], json!(expected.source_sha256));
    assert_eq!(report["generation"], 1);
    let descriptor = json!(Route::Hover.descriptor());
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("discovery")
        .validate(report)
        .expect("original native shape");
    let none = query(
        &state,
        &handle,
        1,
        Route::Hover,
        Some(text.find("owner:one").expect("name")),
    );
    assert!(none["structuredContent"]["value"].is_null());
    let tiny = call(
        &state,
        Route::Hover,
        json!({"session":handle,"generation":1,"document":ID,"offset":offset}),
        64,
    );
    assert_eq!(tiny["isError"], true);
    assert!(tiny["structuredContent"].get("value").is_none());
    let changed = text
        .replace("owner:one", "owner:two")
        .replace("owner:\\u006fne", "owner:two");
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":ID,"version":1,"text":changed}),
        1 << 18,
    );
    assert_eq!(
        query(&state, &handle, 1, Route::Hover, Some(offset))["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let current = query(
        &state,
        &handle,
        2,
        Route::Hover,
        Some(changed.rfind("owner:two").expect("changed reference")),
    );
    assert_eq!(
        current["structuredContent"]["value"]["symbol"]["declaration"]["identity"],
        "owner:two"
    );
    call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":2,"document":ID,"version":2,"text":"[broken\n"}),
        1 << 18,
    );
    let failed = query(&state, &handle, 3, Route::Hover, Some(0));
    assert_eq!(failed["isError"], true);
    assert!(failed["structuredContent"].get("value").is_none());
    call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":3,"document":ID}),
        1 << 18,
    );
    let restored = query(&state, &handle, 4, Route::Hover, Some(offset));
    assert_eq!(restored["structuredContent"]["value"], report["value"]);
    assert_eq!(restored["structuredContent"]["generation"], 4);
}

#[test]
fn workspace_tools_return_exact_sdk_constructs_and_both_symbol_schema_roles() {
    let text = text();
    let state = Sessions::default();
    let handle = begin(&state, &text, false);
    let native = session::Session::new(
        session::Source {
            id: ID.into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.clone(),
        },
        [],
        session::Limits::default(),
    )
    .expect("native snapshot");
    let analysis = native.analysis().as_ref().expect("analysis");
    let path = "workspace.toml".parse().expect("selected path");
    let offset = text
        .find("owner:\\u006fne")
        .expect("native escaped reference");
    let expected = analysis
        .source_navigation(&path, &text, offset)
        .expect("current lookup")
        .expect("association");
    let definition = query(&state, &handle, 1, Route::Definition, Some(offset));
    let report = &definition["structuredContent"];
    assert_eq!(
        report["value"],
        super::selected_location(ID, None, expected.definition)
    );
    assert_eq!(
        report["source_sha256"],
        serde_json::to_value(Sha256Digest::of(text.as_bytes())).expect("exact source identity")
    );
    assert_eq!(report["path"], "workspace.toml");
    assert_eq!(report["generation"], 1);
    let references = query(&state, &handle, 1, Route::References, Some(offset));
    let values = references["structuredContent"]["value"]
        .as_array()
        .expect("complete references");
    assert_eq!(values.len(), expected.references.len() + 1);
    for location in expected
        .references
        .iter()
        .chain(std::iter::once(&expected.definition))
    {
        assert!(values.contains(&super::selected_location(ID, None, *location)));
    }
    let symbols = query(&state, &handle, 1, Route::Symbols, None);
    let declarations = analysis
        .workspace_symbols(&path, &text)
        .expect("native symbols")
        .expect("workspace");
    let expected: Vec<_> = declarations
        .iter()
        .map(|declaration| json!({"declaration":declaration,"location":declaration.location()}))
        .collect();
    assert_eq!(symbols["structuredContent"]["value"], json!(expected));
    let decoy = query(
        &state,
        &handle,
        1,
        Route::Definition,
        Some(text.find("owner:one").expect("plain-name decoy")),
    );
    assert!(decoy["structuredContent"]["value"].is_null());
    let descriptor = serde_json::to_value(Route::Symbols.descriptor()).expect("typed discovery");
    let schema = serde_json::to_string(&descriptor["outputSchema"]).expect("schema text");
    assert!(
        schema.contains("declaration") && schema.contains("heading"),
        "both native symbol roles discoverable"
    );
    assert!(
        schema.contains("anyOf"),
        "untagged roles use a schema union"
    );
}

#[test]
fn workspace_queries_keep_phase_independence_and_exact_overlay_generation() {
    let text = text();
    let changed = format!("# changed é🦀\r\n{text}");
    let state = Sessions::default();
    let handle = begin(&state, &text, true);
    let offset = text.find("owner:\\u006fne").expect("reference");
    let changed_offset = changed.find("owner:\\u006fne").expect("moved reference");
    let original = query(&state, &handle, 1, Route::Definition, Some(offset));
    assert_ne!(
        original["isError"], true,
        "broken Markdown does not erase valid workspace navigation"
    );
    assert_ne!(
        query(&state, &handle, 1, Route::Symbols, None)["isError"],
        true
    );
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":ID,"version":1,"text":text}),
        1 << 18,
    );
    call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":2,"document":ID,"version":2,"text":changed}),
        1 << 18,
    );
    let current = query(&state, &handle, 3, Route::Definition, Some(changed_offset));
    assert_ne!(
        current["structuredContent"]["value"],
        original["structuredContent"]["value"]
    );
    let stale = call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":3,"document":ID,"version":1,"text":text}),
        1 << 18,
    );
    assert_eq!(stale["structuredContent"]["changed"], false);
    assert_eq!(
        query(&state, &handle, 3, Route::Definition, Some(changed_offset)),
        current
    );
    call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":3,"document":ID,"version":3,"text":"[broken\n"}),
        1 << 18,
    );
    for route in [Route::Symbols, Route::Definition] {
        let offset = matches!(route, Route::Definition).then_some(0);
        let failure = query(&state, &handle, 4, route, offset);
        assert_eq!(failure["isError"], true);
        assert!(
            failure["structuredContent"].get("value").is_none(),
            "no stale successful result"
        );
    }
    call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":4,"document":ID}),
        1 << 18,
    );
    let restored = query(&state, &handle, 5, Route::Definition, Some(offset));
    assert_eq!(
        restored["structuredContent"]["value"],
        original["structuredContent"]["value"]
    );
    assert_eq!(
        restored["structuredContent"]["source_sha256"],
        original["structuredContent"]["source_sha256"]
    );
    assert_eq!(restored["structuredContent"]["generation"], 5);
    assert_ne!(
        query(&state, &handle, 5, Route::Symbols, None)["isError"],
        true
    );
}

#[test]
fn modular_workspace_tools_share_cross_document_state_and_discovery() {
    let root = "includes=['sites/one.toml']\n[[principals]]\nid='owner:publisher'\nkind='person'\n";
    let site = "[[sites]]\nid='site:one'\npublisher='owner:publisher'\ndeployable=false\n";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"root","path":"gazette.toml","text":root},
            "workspace_documents":[{"id":"site","path":"sites/one.toml","text":site}],
            "documents":[]
        }),
        1 << 18,
    );
    assert_ne!(started["isError"], true, "{started}");
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let offset = site.find("owner:publisher").expect("reference");
    let definition = call(
        &state,
        Route::Definition,
        json!({"session":handle,"generation":1,"document":"site","offset":offset}),
        1 << 18,
    );
    assert_eq!(definition["structuredContent"]["value"]["document"], "root");
    assert_eq!(
        definition["structuredContent"]["value"]["path"],
        "gazette.toml"
    );
    let completion_input =
        json!({"session":handle,"generation":1,"document":"site","offset":offset});
    let completion = call(&state, Route::Completion, completion_input.clone(), 1 << 18);
    assert_ne!(completion["isError"], true, "{completion}");
    let candidates = completion["structuredContent"]["value"]["candidates"]
        .as_array()
        .expect("complete modular candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(completion["structuredContent"]["path"], "sites/one.toml");
    assert_eq!(candidates[0]["definition"]["path"], "gazette.toml");
    assert_eq!(
        candidates[0]["definition"]["source_sha256"],
        json!(Sha256Digest::of(root.as_bytes()))
    );
    assert_eq!(
        candidates[0]["definition"]["location"],
        definition["structuredContent"]["value"]["location"]
    );
    let descriptor = json!(Route::Completion.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("shared completion schema")
        .validate(&completion["structuredContent"])
        .expect("cross-document origin is in the native transport schema");
    let too_small = call(&state, Route::Completion, completion_input, 64);
    assert_eq!(too_small["isError"], true);
    assert!(too_small["structuredContent"].get("value").is_none());
    let origin = call(
        &state,
        Route::Origin,
        json!({"session":handle,"generation":1,"query":{"kind":"field","query":{
            "kind":"site","identity":"site:one","field":["deployable"]
        }}}),
        1 << 18,
    );
    assert_eq!(origin["structuredContent"]["document"], "site");
    assert_eq!(origin["structuredContent"]["path"], "sites/one.toml");

    let changed = site.replace("owner:publisher", "owner:missing");
    let opened = call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"site","version":4,"text":changed}),
        1 << 18,
    );
    assert_eq!(opened["structuredContent"]["generation"], 2);
    let stale = call(
        &state,
        Route::Definition,
        json!({"session":handle,"generation":1,"document":"site","offset":offset}),
        1 << 18,
    );
    assert_eq!(
        stale["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let descriptor = json!(Route::Start.descriptor());
    assert!(
        descriptor["inputSchema"]["properties"]
            .get("workspace_documents")
            .is_some()
    );
}

#[test]
fn site_hover_returns_native_composition_with_origins_schema_bounds_and_current_overlay() {
    let text = r#"principals=[{id='owner:one',kind='person'}]
sources=[{id='source:theme',location={kind='repository',root='/not-opened/theme',commit='0000000000000000000000000000000000000000',subdirectory='theme'}},{id='source:plugin',location={kind='repository',root='/not-opened/plugin',commit='1111111111111111111111111111111111111111',subdirectory='plugin'}}]
theme_packages=[{package='theme:base',source='source:theme',manifest='base.toml'},{package='theme:replacement',source='source:theme',manifest='replacement.toml'}]
plugin_packages=[{package='search-plugin',source='source:plugin',manifest='plugin.toml'}]
brands=[{id='brand:one',name='One',publications=[{site='site:inherited'},{site='site:override'}]}]
[composition]
theme={package='theme:base',palette='palette:night'}
[composition.plugins]
'search-plugin'={state='enabled'}
[composition.capabilities]
'publication.search/v1'={state='enabled',provider={kind='plugin',package='search-plugin',provider='search'},requirement='required'}
[composition.resources]
'module:search'={state='enabled',resource={owner={kind='plugin',package='search-plugin'},resource='module:search'},output='/assets/search.js',use='site'}
[[sites]]
id='site:inherited'
publisher='owner:one'
deployable=false
[[sites]]
id='site:override'
publisher='owner:one'
deployable=false
[sites.composition]
theme={package='theme:replacement'}
[sites.composition.capabilities]
'publication.search/v1'={state='disabled'}
"#;
    let state = Sessions::default();
    let handle = composition_begin(&state, text);
    let native = session::Session::new_with_package_baselines(
        session::Source {
            id: ID.into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.into(),
        },
        [],
        composition_package_sources(),
        Default::default(),
    )
    .expect("native");
    let analysis = native.analysis().as_ref().expect("analysis");
    assert_eq!(analysis.package_baselines().len(), 3);
    assert!(
        analysis.failures().iter().all(|failure| !matches!(
            failure,
            gazette_cli::sdk::authoring::Failure::Package { .. }
        )),
        "valid explicit package baselines add no package diagnostic"
    );
    let descriptor = json!(Route::Hover.descriptor());
    let validator =
        jsonschema::validator_for(&descriptor["outputSchema"]).expect("discoverable schema");
    for site in ["site:inherited", "site:override"] {
        let offsets: Vec<_> = text.match_indices(site).map(|(offset, _)| offset).collect();
        assert_eq!(offsets.len(), 2, "declaration and typed reference");
        let mut previous = None;
        for offset in offsets {
            let expected = analysis
                .workspace_identity(&"workspace.toml".parse().expect("path"), text, offset)
                .expect("native lookup")
                .expect("site");
            let composition = expected.composition.expect("compiled composition");
            let response = query(&state, &handle, 1, Route::Hover, Some(offset));
            assert_ne!(
                response["isError"], true,
                "unrelated invalid Markdown does not remove workspace facts"
            );
            assert_eq!(response["structuredContent"]["value"], json!(expected));
            validator
                .validate(&response["structuredContent"])
                .expect("native composition discoverable");
            if let Some(previous) = previous.replace(json!(composition)) {
                assert_eq!(
                    previous,
                    json!(composition),
                    "same resolution at declaration and reference"
                );
            }
            let theme = composition.theme().expect("theme");
            let search =
                &composition.capabilities()[&"publication.search/v1".parse().expect("capability")];
            let resource = &composition.resources()[&"module:search".parse().expect("resource")];
            let projection =
                serde_json::to_value(composition).expect("native composition projection");
            assert!(projection.get("modules").is_none(), "{projection}");
            assert_eq!(
                projection["resources"]["module:search"]["value"]["use"],
                "site"
            );
            assert!(
                projection["capabilities"]["publication.search/v1"]["value"]
                    .get("artifact")
                    .is_none(),
                "{projection}"
            );
            assert_eq!(
                resource.origin().scope,
                gazette_cli::sdk::workspace::SelectionScope::Workspace
            );
            assert!(matches!(
                resource.value(),
                gazette_cli::sdk::workspace::ResourceSelection::Enabled {
                    resource: gazette_cli::sdk::workspace::ResourceRef {
                        owner: gazette_cli::sdk::workspace::ResourceOwner::Plugin(package),
                        resource,
                    },
                    output,
                    use_: gazette_cli::sdk::workspace::ResourceUse::Site,
                } if package.as_str() == "search-plugin"
                    && resource.as_str() == "module:search"
                    && output.as_str() == "/assets/search.js"
            ));
            if site == "site:inherited" {
                assert_eq!(
                    theme.origin().scope,
                    gazette_cli::sdk::workspace::SelectionScope::Workspace
                );
                assert_eq!(
                    theme.value().palette.as_ref().expect("palette").as_str(),
                    "palette:night"
                );
                assert_eq!(search.origin().scope, theme.origin().scope);
                assert!(matches!(
                    search.value(),
                    gazette_cli::sdk::workspace::CapabilitySelection::Enabled {
                        provider: gazette_cli::sdk::workspace::ProviderRef::Plugin { package, provider },
                        ..
                    } if package.as_str() == "search-plugin"
                        && provider.as_str() == "search"
                ));
            } else {
                assert!(theme.value().palette.is_none());
                assert_eq!(theme.value().package.as_str(), "theme:replacement");
                assert_eq!(
                    theme.origin().scope,
                    gazette_cli::sdk::workspace::SelectionScope::Site(site.parse().expect("key"))
                );
                assert_eq!(search.origin().scope, theme.origin().scope);
                assert!(matches!(
                    search.value(),
                    gazette_cli::sdk::workspace::CapabilitySelection::Disabled {}
                ));
            }
        }
    }
    let offset = text.find("site:inherited").expect("site reference");
    let full = query(&state, &handle, 1, Route::Hover, Some(offset));
    let maximum = serde_json::to_vec(&full["structuredContent"])
        .expect("complete result")
        .len()
        - 1;
    let bounded = call(
        &state,
        Route::Hover,
        json!({"session":handle,"generation":1,"document":ID,"offset":offset}),
        maximum,
    );
    assert_eq!(bounded["isError"], true);
    assert!(
        bounded["structuredContent"].get("value").is_none(),
        "no successful composition prefix"
    );
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":ID,"version":1,"text":text.replace("palette:night", "palette:day")}),
        1 << 18,
    );
    let updated = query(&state, &handle, 2, Route::Hover, Some(offset));
    assert_eq!(
        updated["structuredContent"]["value"]["composition"]["theme"]["value"]["palette"],
        "palette:day"
    );
    call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":2,"document":ID,"version":2,"text":"[broken\n"}),
        1 << 18,
    );
    let invalid = query(&state, &handle, 3, Route::Hover, Some(0));
    assert_eq!(invalid["isError"], true);
    assert!(invalid["structuredContent"].get("value").is_none());
    call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":3,"document":ID}),
        1 << 18,
    );
    assert_eq!(
        query(&state, &handle, 4, Route::Hover, Some(offset))["structuredContent"]["value"],
        full["structuredContent"]["value"]
    );
}
