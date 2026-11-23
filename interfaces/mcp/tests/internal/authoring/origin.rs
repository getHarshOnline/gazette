//! Discoverable current-buffer origins exercise the real retained SDK session.
use super::*;

const BASE: &str = "principals=[{id='owner:one',kind='person',name='é🦀'}]\r\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n[composition.capabilities]\r\n'publication.search/v1'={state='disabled'}\r\n[composition.resources]\r\n'module:search'={state='disabled'}\r\n";
fn query() -> Value {
    json!({"kind":"composition","query":{"site":"site:one","field":{"kind":"capability","capability":"publication.search/v1"},"path":["state"]}})
}
fn resource_query() -> Value {
    json!({"kind":"composition","query":{"site":"site:one","field":{"kind":"resource","resource":"module:search"},"path":["state"]}})
}

#[test]
fn origin_schema_and_operation_preserve_overlays_failures_and_exact_native_evidence() {
    let state = Sessions::default();
    let result = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"workspace","path":"workspace.toml","text":BASE},"documents":[{"id":"bad","path":"bad.md","text":"+++\n[broken\n+++\n"}]}),
        1 << 18,
    );
    let handle = result["structuredContent"]["session"]
        .as_str()
        .expect("handle");
    let input = |generation| json!({"session":handle,"generation":generation,"query":query()});
    let descriptor = json!(Route::Origin.descriptor());
    assert_eq!(descriptor["name"], "authoring_origin");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["idempotentHint"], true);
    let inputs = jsonschema::validator_for(&descriptor["inputSchema"]).expect("typed discovery");
    assert!(inputs.is_valid(&input(1)));
    assert!(!inputs.is_valid(&json!({"session":handle,"query":query()})));
    assert!(inputs.is_valid(&json!({"session":handle,"generation":1,"query":resource_query()})));
    assert!(!inputs.is_valid(&json!({"session":handle,"generation":1,"query":{"kind":"composition","query":{"site":"site:one","field":{"kind":"module","module":"module:search"},"path":["state"]}}})));
    let output =
        jsonschema::validator_for(&descriptor["outputSchema"]).expect("native output schema");
    let baseline = call(&state, Route::Origin, input(1), 1 << 18);
    assert_ne!(baseline["isError"], true, "{baseline}");
    output
        .validate(&baseline["structuredContent"])
        .expect("actual discovery parity");
    let span = &baseline["structuredContent"]["value"]["origin"]["span"];
    assert_eq!(
        &BASE[span["start"].as_u64().expect("start") as usize
            ..span["end"].as_u64().expect("end") as usize],
        "'disabled'"
    );
    let resource = call(
        &state,
        Route::Origin,
        json!({"session":handle,"generation":1,"query":resource_query()}),
        1 << 18,
    );
    output
        .validate(&resource["structuredContent"])
        .expect("resource origin output schema");
    assert_eq!(
        baseline["structuredContent"]["value"]["origin"]["selection"]["field"]["kind"],
        "capability"
    );
    assert_eq!(
        resource["structuredContent"]["value"]["origin"]["selection"]["field"]["kind"],
        "resource"
    );
    let changed = BASE.replace(
        "deployable=false}",
        "deployable=false,composition={capabilities={'publication.search/v1'={state=\"disabled\"}}}}",
    );
    let opened = call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"workspace","version":5,"text":changed}),
        1 << 18,
    );
    assert_eq!(opened["structuredContent"]["generation"], 2);
    assert_eq!(
        call(&state, Route::Origin, input(1), 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );
    let current = call(&state, Route::Origin, input(2), 1 << 18);
    output
        .validate(&current["structuredContent"])
        .expect("overlay output schema");
    assert_ne!(
        current["structuredContent"]["value"],
        baseline["structuredContent"]["value"]
    );
    assert_eq!(current["structuredContent"]["version"], 5);
    assert_eq!(
        call(&state, Route::Origin, input(2), 1)["isError"],
        true,
        "no partial output"
    );
    call(
        &state,
        Route::Change,
        json!({"session":handle,"generation":2,"document":"workspace","version":6,"text":"[broken\n"}),
        1 << 18,
    );
    assert_eq!(
        call(&state, Route::Origin, input(3), 1 << 18)["structuredContent"]["code"],
        "authoring_origin_workspace"
    );
    call(
        &state,
        Route::Close,
        json!({"session":handle,"generation":3,"document":"workspace"}),
        1 << 18,
    );
    assert_eq!(
        call(&state, Route::Origin, input(4), 1 << 18)["structuredContent"]["value"],
        baseline["structuredContent"]["value"]
    );
}
