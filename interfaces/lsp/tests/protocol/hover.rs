//! Framed hover over current native workspace identities, never protocol-local semantics.

use super::{footnotes::response, *};

const BASE_THEME: &str = "package='theme:base'\nnamespace='base'\ntemplates='templates'\nrender={page='page.html'}\npalettes=[{id='palette:night',browser_theme_colors={light='#111111',dark='#000000'}},{id='palette:day',browser_theme_colors={light='#ffffff',dark='#eeeeee'}}]\n";
const REPLACEMENT_THEME: &str = "package='theme:replacement'\nnamespace='replacement'\ntemplates='templates'\nrender={page='page.html'}\n";
const SEARCH_PLUGIN: &str = "package='search-plugin'\n[resources.'module:search']\ninputs=['search.js']\nbuild={kind='browser-module',entry='search.js'}\n[providers.search]\nimplements=['publication.search/v1']\n";

fn text() -> String {
    "# é🦀\r\nprincipals=[{id='owner:one',name='owner:one',kind='person'}]\r\nsites=[{id='site:one',publisher=\"owner:\\u006fne\",deployable=false}]\r\n".into()
}
fn point(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    json!({"line":before.matches('\n').count(),"character":before.rsplit('\n').next().expect("line").encode_utf16().count()})
}
fn hover(id: i32, uri: &str, position: Value) -> Value {
    request(
        id,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":position}),
    )
}

#[test]
fn component_hover_projects_only_the_selected_argument_contract() {
    let uri = "untitled:component-hover";
    let page = "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n\n:::callout {tone='note'}\nBody\n:::\n";
    let start = page.find("callout").expect("component");
    let mut initial = super::completion::component_selection(uri, page);
    initial["params"]["capabilities"] =
        json!({"textDocument":{"hover":{"contentFormat":["markdown"]}}});
    let (result, messages) = selected_session(
        initial,
        [
            open_content(uri, 4, page),
            hover(2, uri, point(page, start + 2)),
        ],
        Limits::default(),
    );
    result.expect("component hover protocol");
    let hover = &response(&messages, 2)["result"];
    let text = hover["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("plain component hover: {hover}"));
    assert!(text.contains("Selected Theme component: callout"), "{text}");
    assert_eq!(hover["contents"]["kind"], "markdown");
    assert!(text.contains(r"Template: one\/callout\.html"), "{text}");
    assert!(text.contains("\n```json\n"), "{text}");
    assert!(text.contains("\"tone\""), "{text}");
    assert!(!text.contains("Child sections:"), "{text}");
    assert_eq!(
        hover["range"],
        json!({
            "start":point(page,start),
            "end":point(page,start+"callout".len())
        })
    );
}

#[test]
fn landing_hover_explains_the_native_child_section_choices() {
    let uri = "untitled:landing-hover";
    let page = "format: gazette-landing/v1\ncontent:\n  id: article:one\n  site: site:one\n  title: Landing\n  lifecycle: draft\n  structure: {kind: page, family: {kind: utility}}\nsections:\n  - component: callout\n    arguments: {tone: note}\n    sections:\n      - component: figure\n      - body: Context\n";
    let start = page.find("callout").expect("component");
    let mut initial = super::completion::component_selection(uri, page);
    initial["params"]["initializationOptions"]["documents"][0]["path"] =
        json!("authored/page.landing.yaml");
    let (result, messages) = selected_session(
        initial,
        [
            open_content(uri, 4, page),
            hover(2, uri, point(page, start + 2)),
        ],
        Limits::default(),
    );
    result.expect("landing hover protocol");
    let result = &response(&messages, 2)["result"];
    let text = result["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("plain landing hover: {result}"));
    assert!(
        text.contains("Child sections: 2 selected Theme components"),
        "{text}"
    );
    assert!(
        text.contains("heading, nonempty body, or child section"),
        "{text}"
    );
    assert!(text.contains("\"tone\""), "{text}");
}
fn init(text: &str) -> Value {
    let mut initial = selected(&[("untitled:bad", "bad.md", "+++\n[broken\n+++\n")]);
    initial["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    initial
}

fn composition_selection(text: &str) -> Value {
    let mut initial = init(text);
    initial["params"]["initializationOptions"]["package_baselines"] = json!([
        {
            "kind":"theme",
            "package":"theme:base",
            "uri":"untitled:theme-base.toml",
            "path":"packages/theme-base.toml",
            "text":BASE_THEME
        },
        {
            "kind":"theme",
            "package":"theme:replacement",
            "uri":"untitled:theme-replacement.toml",
            "path":"packages/theme-replacement.toml",
            "text":REPLACEMENT_THEME
        },
        {
            "kind":"plugin",
            "package":"search-plugin",
            "uri":"untitled:search-plugin.toml",
            "path":"packages/search-plugin.toml",
            "text":SEARCH_PLUGIN
        }
    ]);
    initial
}

fn composition_package_sources() -> Vec<gazette_sdk::authoring::session::PackageSource> {
    use gazette_sdk::authoring::{PackageBaselineKey, session};

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
fn authored_frontmatter_hover_uses_schema_description_and_full_file_range() {
    let uri = "untitled:article";
    let text = "+++\r\n# é🦀\r\n[content]\r\nlifecycle = \r\n+++\r\n# Body\r\n";
    let start = text.find("lifecycle").expect("field");
    let (result, messages) = selected_session(
        selected(&[(uri, "articles/example.md", text)]),
        [open(uri, 3, text), hover(2, uri, point(text, start + 2))],
        Limits::default(),
    );
    result.expect("frontmatter hover protocol");
    let hover = &response(&messages, 2)["result"];
    let value = hover["contents"]["value"].as_str().expect("plain hover");
    assert!(value.contains("lifecycle (required)"), "{value}");
    assert!(value.contains("content state"), "{value}");
    assert!(
        value.contains("\nAllowed values:\n- \"archived\":"),
        "{value}"
    );
    assert!(value.contains("\n- \"draft\": Authored work"), "{value}");
    assert!(
        value.contains("\n- \"published\": Owner-declared"),
        "{value}"
    );
    assert!(!value.contains("\"kind\":\"string\""), "{value}");
    assert_eq!(
        hover["range"],
        json!({"start":point(text,start),"end":point(text,start+"lifecycle".len())})
    );
}

#[test]
fn hover_matches_native_identity_and_precision_with_unicode_escaped_values_and_crlf() {
    let text = text();
    let state = gazette_sdk::authoring::session::Session::new(
        gazette_sdk::authoring::session::Source {
            id: URI.into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.clone(),
        },
        [],
        Default::default(),
    )
    .expect("native");
    let offset = text.find("owner:\\u006fne").expect("reference");
    let identity = state
        .analysis()
        .as_ref()
        .expect("analysis")
        .workspace_identity(&"workspace.toml".parse().expect("path"), &text, offset)
        .expect("native lookup")
        .expect("identity");
    let expected = json!({"contents":{"kind":"plaintext","value":"Resolved principal: owner:one"},"range":{"start":point(&text,identity.occurrence.span.start),"end":point(&text,identity.occurrence.span.end)}});
    let (result, messages) = selected_session(
        init(&text),
        [
            hover(2, URI, point(&text, offset)),
            hover(
                3,
                URI,
                point(&text, text.find("name='owner:one'").expect("decoy") + 7),
            ),
            hover(4, "untitled:bad", json!({"line":0,"character":0})),
            hover(5, "untitled:foreign", json!({"line":0,"character":0})),
            hover(6, URI, json!({"line":0,"character":4})),
            request(
                7,
                "textDocument/hover",
                json!({"textDocument":[],"position":[]}),
            ),
        ],
        Limits::default(),
    );
    result.expect("real protocol");
    assert_eq!(
        response(&messages, 1)["result"]["capabilities"]["hoverProvider"],
        true
    );
    assert_eq!(response(&messages, 2)["result"], expected);
    let decoy = response(&messages, 3)["result"]["contents"]["value"]
        .as_str()
        .expect("current schema hover, not an identity inference");
    assert!(!decoy.contains("Resolved"));
    assert!(decoy.contains("Accepts:"), "{decoy}");
    assert!(response(&messages, 4)["result"].is_null());
    for id in [5, 6, 7] {
        assert_eq!(response(&messages, id)["error"]["code"], -32602);
    }
}

#[test]
fn hover_replaces_invalid_current_overlay_and_restores_only_selected_baseline() {
    let text = text();
    let changed = text
        .replace("owner:one", "owner:two")
        .replace("owner:\\u006fne", "owner:two");
    let (result, messages) = selected_session(
        init(&text),
        [
            open(URI, 1, &text),
            change(URI, 2, &changed),
            hover(
                2,
                URI,
                point(
                    &changed,
                    changed.rfind("owner:two").expect("changed reference"),
                ),
            ),
            change(URI, 3, "[broken\n"),
            hover(3, URI, json!({"line":0,"character":0})),
            close(URI),
            hover(
                4,
                URI,
                point(&text, text.find("owner:\\u006fne").expect("baseline")),
            ),
        ],
        Limits::default(),
    );
    result.expect("current protocol");
    assert_eq!(
        response(&messages, 2)["result"]["contents"]["value"],
        "Resolved principal: owner:two"
    );
    assert!(response(&messages, 3)["result"].is_null());
    assert_eq!(
        response(&messages, 4)["result"]["contents"]["value"],
        "Resolved principal: owner:one"
    );
}

#[test]
fn oversized_hover_has_no_successful_text_prefix() {
    let identity = format!("owner:{}", "x".repeat(2048));
    let text = format!(
        "principals=[{{id='{identity}',kind='person'}}]\nsites=[{{id='site:one',publisher='{identity}',deployable=false}}]\n"
    );
    let mut initial = initialize();
    initial["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let call = hover(2, URI, json!({"line":0,"character":18}));
    let (result, complete) = selected_session(initial.clone(), [call.clone()], Limits::default());
    result.expect("valid current identity without unrelated diagnostic exhaustion");
    assert_eq!(
        response(&complete, 2)["result"]["contents"]["value"],
        format!("Resolved principal: {identity}")
    );
    let (result, messages) = selected_session(
        initial,
        [call],
        Limits {
            output_bytes: 1280,
            ..Default::default()
        },
    );
    result.expect("bounded error fits protocol");
    assert_eq!(response(&messages, 2)["error"]["code"], -32600);
    assert!(response(&messages, 2).get("result").is_none());
}

#[test]
fn hover_negotiates_preference_and_preserves_literal_identity_and_utf16_range() {
    let identity = r"owner:*a`b*[c](https://example.test)<&>\d";
    let escaped = r"owner\:\*a\`b\*\[c\]\(https\:\/\/example\.test\)\<\&\>\\d";
    let text = format!(
        "# é🦀\r\nprincipals=[{{id='{identity}',kind='person'}}]\r\nsites=[{{id='site:é🦀',publisher='{identity}',deployable=false}}]\r\n"
    );
    let native = gazette_sdk::authoring::session::Session::new(
        gazette_sdk::authoring::session::Source {
            id: URI.into(),
            path: "workspace.toml".parse().unwrap(),
            text: text.clone(),
        },
        [],
        Default::default(),
    )
    .expect("opaque punctuation is a valid native identity");
    let offset = text.rfind(identity).expect("publisher reference");
    let resolved = native
        .analysis()
        .as_ref()
        .unwrap()
        .workspace_identity(&"workspace.toml".parse().unwrap(), &text, offset)
        .unwrap()
        .expect("exact typed reference");
    let range = json!({"start":point(&text,resolved.occurrence.span.start),"end":point(&text,resolved.occurrence.span.end)});
    for (formats, kind, literal) in [
        (None, "plaintext", identity),
        (Some(json!([])), "plaintext", identity),
        (Some(json!(["plaintext"])), "plaintext", identity),
        (Some(json!(["markdown"])), "markdown", escaped),
        (
            Some(json!(["plaintext", "markdown"])),
            "plaintext",
            identity,
        ),
        (Some(json!(["markdown", "plaintext"])), "markdown", escaped),
    ] {
        let mut initial = init(&text);
        if let Some(formats) = formats {
            initial["params"]["capabilities"] =
                json!({"textDocument":{"hover":{"contentFormat":formats}}});
        }
        let (result, messages) = selected_session(
            initial,
            [hover(2, URI, point(&text, offset))],
            Limits::default(),
        );
        result.expect("negotiated hover");
        assert_eq!(
            response(&messages, 2)["result"],
            json!({
                "contents":{"kind":kind,"value":format!("Resolved principal: {literal}")},
                "range":range
            })
        );
    }
}

#[test]
fn markdown_hover_charges_escaping_without_returning_a_partial_value() {
    let identity = format!("owner:{}", "*".repeat(700));
    let text = format!(
        "principals=[{{id='{identity}',kind='person'}}]\nsites=[{{id='site:one',publisher='{identity}',deployable=false}}]\n"
    );
    for format in ["plaintext", "markdown"] {
        let mut initial = initialize();
        initial["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
        initial["params"]["capabilities"] =
            json!({"textDocument":{"hover":{"contentFormat":[format]}}});
        let (result, messages) = selected_session(
            initial,
            [hover(2, URI, json!({"line":0,"character":18}))],
            Limits {
                output_bytes: 1280,
                ..Default::default()
            },
        );
        result.expect("bounded protocol response");
        if format == "plaintext" {
            assert_eq!(
                response(&messages, 2)["result"]["contents"]["value"],
                format!("Resolved principal: {identity}")
            );
        } else {
            assert_eq!(response(&messages, 2)["error"]["code"], -32600);
            assert!(response(&messages, 2).get("result").is_none());
        }
    }
}

#[test]
fn invalid_strict_workspace_still_has_current_schema_hover_without_prior_success() {
    let text = "# é🦀\r\n[[sites]]\r\ndeployable = ";
    let mut initial = initialize();
    initial["params"]["initializationOptions"]["workspace"]["text"] = json!(text);
    let start = text.find("deployable").expect("key");
    let (result, messages) = selected_session(
        initial,
        [open(URI, 7, text), hover(2, URI, point(text, start + 2))],
        Limits::default(),
    );
    result.expect("schema hover protocol");
    let value = response(&messages, 2)["result"]["contents"]["value"]
        .as_str()
        .expect("plain hover");
    assert!(value.contains("deployable (required)"), "{value}");
    assert!(value.contains("release eligibility"), "{value}");
    assert!(value.contains("true or false"), "{value}");
    assert_eq!(
        response(&messages, 2)["result"]["range"],
        json!({"start":point(text,start),"end":point(text,start+"deployable".len())})
    );
}

fn composition_text() -> &'static str {
    r#"principals=[{id='owner:one',kind='person'}]
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
"#
}

#[test]
fn site_hover_projects_exact_native_composition_at_declarations_and_references() {
    let text = composition_text();
    let state = gazette_sdk::authoring::session::Session::new_with_package_baselines(
        gazette_sdk::authoring::session::Source {
            id: URI.into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.into(),
        },
        [],
        composition_package_sources(),
        Default::default(),
    )
    .expect("native session");
    let analysis = state.analysis().as_ref().expect("analysis");
    assert_eq!(analysis.package_baselines().len(), 3);
    assert!(
        analysis
            .failures()
            .iter()
            .all(|failure| !matches!(failure, gazette_sdk::authoring::Failure::Package { .. })),
        "valid explicit package baselines add no package diagnostic"
    );
    let path = "workspace.toml".parse().expect("path");
    let offsets: Vec<_> = ["site:inherited", "site:override"]
        .into_iter()
        .flat_map(|site| text.match_indices(site).map(|(offset, _)| offset))
        .collect();
    assert_eq!(offsets.len(), 4, "declaration and typed reference per site");
    let (result, messages) = selected_session(
        composition_selection(text),
        offsets.iter().enumerate().map(|(index, offset)| {
            hover(
                i32::try_from(index + 2).expect("request id"),
                URI,
                point(text, *offset),
            )
        }),
        Limits::default(),
    );
    result.expect("real hover despite unrelated malformed Markdown");
    for (index, offset) in offsets.into_iter().enumerate() {
        let identity = analysis
            .workspace_identity(&path, text, offset)
            .expect("native source")
            .expect("site identity");
        let composition = identity.composition.expect("effective site composition");
        let expected = format!(
            "Resolved site: {}\n\nConfigured presentation:\n\n{}",
            identity.symbol.name(),
            serde_json::to_string_pretty(composition).expect("native composition")
        );
        assert_eq!(
            response(&messages, i32::try_from(index + 2).expect("id"))["result"],
            json!({"contents":{"kind":"plaintext","value":expected},"range":{"start":point(text,identity.occurrence.span.start),"end":point(text,identity.occurrence.span.end)}})
        );
        let theme = composition.theme().expect("selected theme");
        let search =
            &composition.capabilities()[&"publication.search/v1".parse().expect("capability")];
        let resource = &composition.resources()[&"module:search".parse().expect("resource")];
        let projection = serde_json::to_value(composition).expect("native composition projection");
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
            gazette_sdk::workspace::SelectionScope::Workspace
        );
        assert!(matches!(
            resource.value(),
            gazette_sdk::workspace::ResourceSelection::Enabled {
                resource: gazette_sdk::workspace::ResourceRef {
                    owner: gazette_sdk::workspace::ResourceOwner::Plugin(package),
                    resource,
                },
                output,
                use_: gazette_sdk::workspace::ResourceUse::Site,
            } if package.as_str() == "search-plugin"
                && resource.as_str() == "module:search"
                && output.as_str() == "/assets/search.js"
        ));
        if identity.symbol.name() == "site:inherited" {
            assert_eq!(
                theme
                    .value()
                    .palette
                    .as_ref()
                    .expect("inherited palette")
                    .as_str(),
                "palette:night"
            );
            assert_eq!(
                theme.origin().scope,
                gazette_sdk::workspace::SelectionScope::Workspace
            );
            assert_eq!(
                search.origin().scope,
                gazette_sdk::workspace::SelectionScope::Workspace
            );
            assert!(matches!(
                search.value(),
                gazette_sdk::workspace::CapabilitySelection::Enabled {
                    provider: gazette_sdk::workspace::ProviderRef::Plugin { package, provider },
                    ..
                } if package.as_str() == "search-plugin"
                    && provider.as_str() == "search"
            ));
        } else {
            assert_eq!(theme.value().package.as_str(), "theme:replacement");
            assert!(
                theme.value().palette.is_none(),
                "whole atom replacement does not inherit the old palette"
            );
            assert_eq!(
                theme.origin().scope,
                gazette_sdk::workspace::SelectionScope::Site("site:override".parse().expect("key"))
            );
            assert_eq!(search.origin().scope, theme.origin().scope);
            assert!(matches!(
                search.value(),
                gazette_sdk::workspace::CapabilitySelection::Disabled {}
            ));
        }
    }
}

#[test]
fn composition_hover_limit_refuses_the_complete_result_without_identity_prefix() {
    let text =
        composition_text().replace("palette:night", &format!("palette:{}", "x".repeat(2048)));
    let initial = composition_selection(&text);
    let call = hover(
        2,
        URI,
        point(&text, text.find("site:inherited").expect("reference")),
    );
    let (result, complete) = selected_session(initial.clone(), [call.clone()], Limits::default());
    result.expect("full composition fits default budget");
    assert!(
        response(&complete, 2)["result"]["contents"]["value"]
            .as_str()
            .expect("hover")
            .len()
            > 2048
    );
    let (result, bounded) = selected_session(
        initial,
        [call],
        Limits {
            output_bytes: 1280,
            ..Default::default()
        },
    );
    result.expect("bounded error response");
    assert_eq!(response(&bounded, 2)["error"]["code"], -32600);
    assert!(
        response(&bounded, 2).get("result").is_none(),
        "no successful short identity substituted for omitted composition"
    );
}
