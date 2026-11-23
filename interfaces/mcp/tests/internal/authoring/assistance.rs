//! Discoverable MCP projection of the same SDK workspace-assistance contract.

use super::*;

#[test]
fn assistance_matches_sdk_under_invalid_current_semantics() {
    let text = "# é🦀\r\n[[sites]]\r\nid='site:one'\r\npublisher='owner:one'\r\ndep";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":text},"documents":[]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let offset = text.len();
    let input = json!({"session":handle,"generation":1,"document":"config","offset":offset});
    let result = call(&state, Route::Assistance, input.clone(), 1 << 18);
    assert_ne!(result["isError"], true, "{result}");

    let native = gazette_cli::sdk::authoring::session::Session::new(
        gazette_cli::sdk::authoring::session::Source {
            id: "config".into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.into(),
        },
        [],
        Default::default(),
    )
    .expect("native session");
    let document = &native.files()["config"];
    let expected = native
        .analysis()
        .as_ref()
        .expect("phase failures retain analysis")
        .assistance(document.path(), document.text(), offset, 1 << 18)
        .expect("native help");
    assert_eq!(result["structuredContent"]["value"], json!(expected));
    assert_eq!(
        result["structuredContent"]["value"]["help"]["candidates"][0]["label"],
        "deployable"
    );

    let descriptor = json!(Route::Assistance.descriptor());
    assert_eq!(descriptor["name"], "authoring_assistance");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["openWorldHint"], false);
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("discoverable output schema")
        .validate(&result["structuredContent"])
        .expect("actual output matches discovery");
    assert_eq!(call(&state, Route::Assistance, input, 128)["isError"], true);
}

#[test]
fn authored_frontmatter_assistance_is_the_same_discoverable_sdk_contract() {
    let workspace = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n";
    let text = "+++\r\n[content]\r\nlifecycle = ";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({
            "workspace":{"id":"config","path":"workspace.toml","text":workspace},
            "documents":[{"id":"article","path":"articles/example.md","text":text}]
        }),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let result = call(
        &state,
        Route::Assistance,
        json!({"session":handle,"generation":1,"document":"article","offset":text.len()}),
        1 << 18,
    );
    assert_ne!(result["isError"], true, "{result}");
    let value = &result["structuredContent"]["value"];
    assert_eq!(value["scope"], "authored_frontmatter");
    assert_eq!(
        value["help"]["candidates"]
            .as_array()
            .expect("candidates")
            .iter()
            .map(|candidate| candidate["label"].as_str().expect("label"))
            .collect::<Vec<_>>(),
        ["archived", "draft", "published"]
    );
}
