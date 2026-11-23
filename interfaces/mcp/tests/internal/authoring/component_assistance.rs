//! Discoverable MCP projection of source-bound Theme component assistance.

use super::*;

const WORKSPACE: &str = "[composition]\ntheme={package='theme:one'}\n\
[[principals]]\nid='owner:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[sources]]\nid='source:theme'\nlocation={kind='repository',root='/not-opened/theme',commit='0000000000000000000000000000000000000000',subdirectory='theme'}\n\
[[sources]]\nid='source:content'\nlocation={kind='repository',root='/not-opened/content',commit='0000000000000000000000000000000000000000',subdirectory='content'}\n\
[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n\
[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\nauthored=[{source='source:content',target='authored'}]\n";

const THEME: &str = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n\
[components.callout]\ntemplate='callout.html'\n\
arguments={type='object',required=['tone'],properties={tone={type='string',enum=['note','warning']}},additionalProperties=false}\n\
[components.figure]\ntemplate='figure.html'\n";

fn page(component: &str) -> String {
    format!(
        "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\n\
[content.structure]\nkind='page'\nfamily={{kind='article'}}\n+++\n\n:::{component} {{tone='note'}}\nBody\n:::\n"
    )
}

#[test]
fn component_assistance_matches_shared_sdk_and_discovery_without_native_reads() {
    let page = page("callout");
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":WORKSPACE},
            "documents":[{"id":"page","path":"authored/page.md","text":page}],
            "package_baselines":[{
                "kind":"theme",
                "package":"theme:one",
                "id":"theme-package",
                "path":"themes/main/theme.toml",
                "text":THEME
            }]
        }),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let offset = page.find("callout").expect("component") + 2;
    let input = json!({
        "session":handle,
        "generation":1,
        "document":"page",
        "offset":offset
    });
    let result = call(&state, Route::ComponentAssistance, input.clone(), 1 << 18);
    assert_ne!(result["isError"], true, "{result}");
    let value = &result["structuredContent"]["value"];
    assert_eq!(value["completion"]["kind"], "name");
    assert_eq!(value["completion"]["preimage"], "callout");
    assert_eq!(value["hover"]["name"], "callout");
    assert_eq!(
        value["hover"]["arguments"]["properties"]["tone"]["enum"][1],
        "warning"
    );
    assert_eq!(
        value["completion"]["candidates"]
            .as_array()
            .expect("complete catalog")
            .iter()
            .map(|candidate| candidate["name"].as_str().expect("name"))
            .collect::<Vec<_>>(),
        ["callout", "figure"]
    );

    let descriptor = json!(Route::ComponentAssistance.descriptor());
    assert_eq!(descriptor["name"], "authoring_component_assistance");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["openWorldHint"], false);
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("input schema")
        .validate(&input)
        .expect("actual input matches discovery");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&result["structuredContent"])
        .expect("actual output matches discovery");

    let native = gazette_cli::sdk::authoring::session::Session::new_with_package_baselines(
        gazette_cli::sdk::authoring::session::Source {
            id: "workspace".into(),
            path: "workspace.toml".parse().expect("path"),
            text: WORKSPACE.into(),
        },
        [gazette_cli::sdk::authoring::session::Source {
            id: "page".into(),
            path: "authored/page.md".parse().expect("path"),
            text: page.clone(),
        }],
        [gazette_cli::sdk::authoring::session::PackageSource {
            package: gazette_cli::sdk::authoring::PackageBaselineKey::Theme(
                "theme:one".parse().expect("Theme package"),
            ),
            input: gazette_cli::sdk::authoring::session::Source {
                id: "theme-package".into(),
                path: "themes/main/theme.toml".parse().expect("path"),
                text: THEME.into(),
            },
        }],
        Default::default(),
    )
    .expect("native session");
    let document = &native.files()["page"];
    let expected = native
        .analysis()
        .as_ref()
        .expect("analysis")
        .component_assistance(document.path(), document.text(), offset, 1 << 18)
        .expect("native result");
    assert_eq!(value, &json!(expected));

    let argument = call(
        &state,
        Route::ComponentAssistance,
        json!({
            "session":handle,
            "generation":1,
            "document":"page",
            "offset":page.find("'note'").unwrap()+2
        }),
        1 << 18,
    );
    assert_eq!(
        argument["structuredContent"]["value"]["completion"]["kind"],
        "argument"
    );
    assert_eq!(
        argument["structuredContent"]["value"]["completion"]["completion_kind"],
        "literal"
    );
    assert_eq!(
        argument["structuredContent"]["value"]["completion"]["candidates"][1]["replacement"],
        "\"warning\""
    );

    let changed = page.replace("'note'", "'warning'");
    let opened = call(
        &state,
        Route::Open,
        json!({
            "session":handle,
            "generation":1,
            "document":"page",
            "version":2,
            "text":changed
        }),
        1 << 18,
    );
    assert_eq!(opened["structuredContent"]["generation"], 2);
    assert_eq!(
        call(&state, Route::ComponentAssistance, input, 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );

    assert_eq!(
        call(
            &state,
            Route::ComponentAssistance,
            json!({
                "session":handle,
                "generation":2,
                "document":"page",
                "offset":offset
            }),
            256,
        )["isError"],
        true
    );
}
