//! Heading proposals share the compiler model and exact held source generation.

use super::*;

#[test]
fn heading_completion_matches_sdk_schema_and_preserves_exact_overlay_lifecycle() {
    let text =
        "# Repeat\r\n\r\n# Repeat\r\n\r\n## Custom {#\"résumé\"}\r\n\r\n😀 [go](#missing)\r\n";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":WORKSPACE},"documents":[{"id":"page","path":"page.md","text":text}]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"].as_str().unwrap();
    let offset = text.find("#missing").unwrap() + 2;
    let input = json!({"session":handle,"generation":1,"document":"page","offset":offset});
    let answer = call(&state, Route::HeadingCompletion, input.clone(), 1 << 18);
    assert_ne!(answer["isError"], true, "{answer}");
    let native = gazette_cli::sdk::authoring::session::Session::new(
        gazette_cli::sdk::authoring::session::Source {
            id: "config".into(),
            path: "workspace.toml".parse().unwrap(),
            text: WORKSPACE.into(),
        },
        [gazette_cli::sdk::authoring::session::Source {
            id: "page".into(),
            path: "page.md".parse().unwrap(),
            text: text.into(),
        }],
        Default::default(),
    )
    .unwrap();
    let expected = native
        .analysis()
        .as_ref()
        .unwrap()
        .complete_heading(&"page.md".parse().unwrap(), text, offset, 1 << 18)
        .unwrap()
        .unwrap();
    let report = &answer["structuredContent"];
    assert_eq!(report["value"], json!(expected));
    assert_eq!(report["generation"], 1);
    assert!(report["version"].is_null());
    assert_eq!(report["value"]["preimage"], "#missing");
    assert_eq!(
        report["value"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["fragment"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["#repeat", "#repeat-1", "#r%C3%A9sum%C3%A9"]
    );
    let descriptor = json!(Route::HeadingCompletion.descriptor());
    assert_eq!(descriptor["name"], "authoring_complete_heading");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["openWorldHint"], false);
    jsonschema::validator_for(&descriptor["outputSchema"])
        .unwrap()
        .validate(report)
        .unwrap();
    assert_eq!(
        call(&state, Route::HeadingCompletion, input.clone(), 256)["isError"],
        true
    );
    let changed = text.replace("# Repeat", "# New");
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"page","version":7,"text":changed}),
        1 << 18,
    );
    assert_eq!(
        call(&state, Route::HeadingCompletion, input.clone(), 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let current = json!({"session":handle,"generation":2,"document":"page","offset":changed.find("#missing").unwrap()});
    let fresh = call(&state, Route::HeadingCompletion, current, 1 << 18);
    assert_eq!(fresh["structuredContent"]["version"], 7);
    assert_eq!(
        fresh["structuredContent"]["value"]["candidates"][0]["fragment"],
        "#new"
    );
    call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":2,"document":"page"}),
        1 << 18,
    );
    let mut restored = input;
    restored["generation"] = json!(3);
    assert_eq!(
        call(&state, Route::HeadingCompletion, restored, 1 << 18)["structuredContent"]["value"],
        report["value"]
    );
}

#[test]
fn unsupported_heading_destinations_are_null_not_guessed_and_invalid_offsets_fail() {
    let state = Sessions::default();
    let text = "# Here\n\n[empty](#)\n[title](#here \"title\")\n![image](#here)\n[external](other.md#here)\n[reference][target]\n\n[target]: #here\n\n[incomplete](#here\n";
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":WORKSPACE},"documents":[{"id":"page","path":"page.md","text":text}]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"].as_str().unwrap();
    for (needle, shift) in [
        ("#here \"title\"", 0),
        ("![image](#here)", 9),
        ("other.md#here", 8),
        ("[reference]", 2),
        ("[target]: #here", 10),
        ("[incomplete](#here", 13),
    ] {
        let result = call(
            &state,
            Route::HeadingCompletion,
            json!({"session":handle,"generation":1,"document":"page","offset":text.find(needle).unwrap()+shift}),
            1 << 18,
        );
        assert_ne!(result["isError"], true, "{needle}: {result}");
        assert!(
            result["structuredContent"]["value"].is_null(),
            "{needle}: {result}"
        );
    }
    let empty = call(
        &state,
        Route::HeadingCompletion,
        json!({"session":handle,"generation":1,"document":"page","offset":text.find("(#)").unwrap()+2}),
        1 << 18,
    );
    assert_eq!(empty["structuredContent"]["value"]["preimage"], "#");
    let invalid = call(
        &state,
        Route::HeadingCompletion,
        json!({"session":handle,"generation":1,"document":"page","offset":text.len()+1}),
        1 << 18,
    );
    assert_eq!(invalid["isError"], true);
}
