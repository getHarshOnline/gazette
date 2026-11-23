//! Complete native reference proposals under exact held-session generations.

use super::*;

#[test]
fn unresolved_reference_repair_retains_diagnostics_and_nullable_discoverable_target() {
    let text = "principals=[{id='owner:one',kind='person'},{id='owner:two',kind='person'}]\nprojects=[{id='owner:decoy',lifecycle='active',visibility='public',availability='available'}]\nsites=[{id='site:one',publisher='owner:typo',deployable=false}]\n";
    let state = Sessions::default();
    let start = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":text},"documents":[]}),
        1 << 18,
    );
    let handle = start["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let opened = call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"config","version":7,"text":text}),
        1 << 18,
    );
    assert_ne!(opened["isError"], true);
    let diagnostics_input = json!({"session":handle,"generation":2,"document":"config"});
    let before = call(
        &state,
        Route::Diagnostics,
        diagnostics_input.clone(),
        1 << 18,
    );
    assert!(
        before["structuredContent"]["value"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|row| row["code"] == "workspace_reference")
    );
    let input = json!({"session":handle,"generation":2,"document":"config","offset":text.rfind("owner:typo").unwrap()});
    let result = call(&state, Route::Completion, input.clone(), 1 << 18);
    assert_ne!(result["isError"], true, "{result}");
    let report = &result["structuredContent"];
    assert_eq!(report["generation"], 2);
    assert_eq!(report["version"], 7);
    let value = &report["value"];
    assert_eq!(value.get("current"), Some(&Value::Null));
    assert_eq!(value["preimage"], "'owner:typo'");
    assert_eq!(
        value["source_sha256"],
        json!(gazette_cli::sdk::source::Sha256Digest::of(text.as_bytes()))
    );
    let candidates = value["candidates"].as_array().expect("complete candidates");
    assert_eq!(
        candidates
            .iter()
            .map(|item| item["symbol"]["declaration"]["identity"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["owner:one", "owner:two"]
    );
    let descriptor = json!(Route::Completion.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("discovered schema")
        .validate(report)
        .expect("unresolved current:null is truthful discovery");
    assert_eq!(
        call(&state, Route::Diagnostics, diagnostics_input, 1 << 18)["structuredContent"]["value"],
        before["structuredContent"]["value"]
    );
    let start = usize::try_from(value["location"]["span"]["start"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(value["location"]["span"]["end"].as_u64().unwrap()).unwrap();
    assert_eq!(&text[start..end], "'owner:typo'");
    let mut repaired = text.to_owned();
    repaired.replace_range(start..end, candidates[0]["replacement"].as_str().unwrap());
    let changed = call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":2,"document":"config","version":8,"text":repaired}),
        1 << 18,
    );
    assert_ne!(changed["isError"], true);
    assert_eq!(
        call(&state, Route::Completion, input.clone(), 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let clear = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":3,"document":"config"}),
        1 << 18,
    );
    assert_eq!(clear["structuredContent"]["value"], json!([]));
    let mut current = input;
    current["generation"] = json!(3);
    let resolved = call(&state, Route::Completion, current, 1 << 18);
    assert_eq!(
        resolved["structuredContent"]["value"]["current"]["declaration"]["identity"],
        "owner:one"
    );
}

#[test]
fn completion_matches_sdk_and_discovery_and_refuses_stale_or_oversized_results() {
    let text = "principals=[{id=\"owner:\\u006fne\",kind='person'},{id='owner:two',kind='person'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n";
    let state = Sessions::default();
    let start = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":text},"documents":[]}),
        1 << 18,
    );
    let handle = start["structuredContent"]["session"]
        .as_str()
        .expect("handle");
    let offset = text.rfind("owner:one").expect("reference");
    let input = json!({"session":handle,"generation":1,"document":"config","offset":offset});
    let result = call(&state, Route::Completion, input.clone(), 1 << 18);
    assert_ne!(result["isError"], true);
    let native = gazette_cli::sdk::authoring::session::Session::new(
        gazette_cli::sdk::authoring::session::Source {
            id: "config".into(),
            path: "workspace.toml".parse().expect("path"),
            text: text.into(),
        },
        [],
        Default::default(),
    )
    .expect("shared native session");
    let expected = native
        .analysis()
        .as_ref()
        .expect("analysis")
        .complete_reference(
            &"workspace.toml".parse().expect("path"),
            text,
            offset,
            1 << 18,
        )
        .expect("query")
        .expect("slot");
    assert_eq!(result["structuredContent"]["value"], json!(expected));
    assert_eq!(result["structuredContent"]["generation"], 1);
    assert!(result["structuredContent"]["version"].is_null());
    let descriptor = json!(Route::Completion.descriptor());
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native schema")
        .validate(&result["structuredContent"])
        .expect("actual report");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["openWorldHint"], false);
    let tiny = call(&state, Route::Completion, input.clone(), 256);
    assert_eq!(tiny["isError"], true);
    assert!(tiny["structuredContent"].get("value").is_none());
    let mut invalid = input.clone();
    invalid["offset"] = json!([offset]);
    assert!(
        Route::Completion
            .decode(invalid.as_object().expect("object").clone())
            .is_err()
    );
    invalid = input.clone();
    invalid["unknown"] = json!("PRIVATE");
    assert!(
        Route::Completion
            .decode(invalid.as_object().expect("object").clone())
            .is_err()
    );
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"config","version":2,"text":"[broken\n"}),
        1 << 18,
    );
    assert_eq!(
        call(&state, Route::Completion, input.clone(), 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let mut current = input.clone();
    current["generation"] = json!(2);
    current["offset"] = json!(0);
    let failed = call(&state, Route::Completion, current, 1 << 18);
    assert_eq!(failed["isError"], true);
    assert!(failed["structuredContent"].get("value").is_none());
    call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":2,"document":"config"}),
        1 << 18,
    );
    let mut restored = input;
    restored["generation"] = json!(3);
    assert_eq!(
        call(&state, Route::Completion, restored, 1 << 18)["structuredContent"]["value"],
        result["structuredContent"]["value"]
    );
}
