//! Diagnostic-gated repair proposals through the real shared authoring session.

use super::*;

fn fixture() -> (&'static str, Sessions, String, Value, Value) {
    let text = "principals=[{id='owner:one',kind='person'},{id='owner:two',kind='person'}]\nsites=[{id='site:one',publisher='owner:typo',deployable=false}]\n";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":text},"documents":[]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session")
        .to_owned();
    call(
        &state,
        Route::Open,
        json!({"session":handle,"generation":1,"document":"config","version":7,"text":text}),
        1 << 18,
    );
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":2,"document":"config"}),
        1 << 18,
    );
    let report = diagnostics["structuredContent"].clone();
    let diagnostic = report["value"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|row| row["code"] == "workspace_reference")
        .expect("reference diagnostic")
        .clone();
    (text, state, handle, report, diagnostic)
}

fn request(text: &str, handle: &str, report: &Value, diagnostic: Value) -> Value {
    json!({
        "session":handle,
        "generation":2,
        "document":"config",
        "version":7,
        "source_sha256":report["source_sha256"],
        "offset":text.rfind("owner:typo").expect("typed reference"),
        "diagnostic":diagnostic,
    })
}

#[test]
fn exact_current_diagnostic_returns_inert_shared_replacements() {
    let (text, state, handle, report, diagnostic) = fixture();
    let input = request(text, &handle, &report, diagnostic.clone());
    let result = call(&state, Route::Repair, input.clone(), 1 << 18);
    assert_ne!(result["isError"], true, "{result}");
    let output = &result["structuredContent"];
    assert_eq!(output["generation"], 2);
    assert_eq!(output["document"], "config");
    assert_eq!(output["version"], 7);
    assert_eq!(output["source_sha256"], report["source_sha256"]);
    assert_eq!(output["value"]["review_required"], true);
    assert_eq!(output["value"]["diagnostic"], diagnostic);
    assert_eq!(output["value"]["proposal"]["kind"], "reference");
    assert_eq!(
        output["value"]["proposal"]["value"]["preimage"],
        "'owner:typo'"
    );
    assert_eq!(
        output["value"]["proposal"]["value"]["candidates"]
            .as_array()
            .expect("complete candidates")
            .iter()
            .map(|candidate| candidate["symbol"]["declaration"]["identity"]
                .as_str()
                .unwrap())
            .collect::<Vec<_>>(),
        ["owner:one", "owner:two"]
    );
    assert!(output["value"].get("command").is_none());
    assert!(output["value"].get("edit").is_none());
    assert_eq!(
        call(
            &state,
            Route::Diagnostics,
            json!({"session":handle,"generation":2,"document":"config"}),
            1 << 18,
        )["structuredContent"],
        report,
        "reviewing a repair does not mutate the held source or diagnostics"
    );

    let descriptor = json!(Route::Repair.descriptor());
    assert_eq!(descriptor["name"], "authoring_review_repair");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["destructiveHint"], false);
    assert_eq!(descriptor["annotations"]["idempotentHint"], true);
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("typed input schema")
        .validate(&input)
        .expect("actual request follows discovery");
}

#[test]
fn stale_invented_and_unassisted_requests_never_receive_a_repair() {
    let (text, state, handle, report, diagnostic) = fixture();
    let mut invented = diagnostic.clone();
    invented["message"] = json!("invented client diagnostic");
    let refusal = call(
        &state,
        Route::Repair,
        request(text, &handle, &report, invented),
        1 << 18,
    );
    assert_eq!(
        refusal["structuredContent"]["code"],
        "authoring_diagnostic_binding"
    );

    let mut stale_source = request(text, &handle, &report, diagnostic.clone());
    stale_source["version"] = json!(6);
    assert_eq!(
        call(&state, Route::Repair, stale_source, 1 << 18)["structuredContent"]["code"],
        "authoring_stale_source"
    );
    let mut stale_digest = request(text, &handle, &report, diagnostic.clone());
    stale_digest["source_sha256"] = json!(gazette_cli::sdk::source::Sha256Digest::of(b"older"));
    assert_eq!(
        call(&state, Route::Repair, stale_digest, 1 << 18)["structuredContent"]["code"],
        "authoring_stale_source"
    );

    let mut unassisted = request(text, &handle, &report, diagnostic);
    unassisted["offset"] = json!(0);
    assert_eq!(
        call(&state, Route::Repair, unassisted, 1 << 18)["structuredContent"]["value"],
        Value::Null
    );

    let mut stale_generation = request(text, &handle, &report, report["value"][0].clone());
    stale_generation["generation"] = json!(1);
    assert_eq!(
        call(&state, Route::Repair, stale_generation, 1 << 18)["structuredContent"]["code"],
        "authoring_stale_generation"
    );
}

#[test]
fn repair_position_is_bound_to_its_exact_half_open_diagnostic_span() {
    let text = "principals=[{id='owner:one',kind='person'},{id='owner:two',kind='person'}]\nsites=[{id='site:one',publisher='owner:first',deployable=false},{id='site:two',publisher='owner:second',deployable=false}]\n";
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
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"config"}),
        1 << 18,
    );
    let report = &diagnostics["structuredContent"];
    let rows = report["value"].as_array().expect("diagnostics");
    let first = rows
        .iter()
        .find(|row| row["key"] == "owner:first")
        .expect("first reference diagnostic")
        .clone();
    let second = rows
        .iter()
        .find(|row| row["key"] == "owner:second")
        .expect("second reference diagnostic")
        .clone();
    let base = |diagnostic: Value, offset: usize| {
        json!({"session":handle,"generation":1,"document":"config","version":null,
            "source_sha256":report["source_sha256"],"offset":offset,"diagnostic":diagnostic})
    };

    let second_offset = text.find("owner:second").expect("second reference");
    let crossed = call(
        &state,
        Route::Repair,
        base(first.clone(), second_offset),
        1 << 18,
    );
    assert_eq!(
        crossed["structuredContent"]["value"],
        Value::Null,
        "a current diagnostic cannot label another diagnostic's proposal"
    );

    let valid = call(&state, Route::Repair, base(second, second_offset), 1 << 18);
    assert_eq!(
        valid["structuredContent"]["value"]["proposal"]["kind"],
        "reference"
    );

    let duplicate = text.replace("owner:second", "owner:first");
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":duplicate},"documents":[]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"config"}),
        1 << 18,
    );
    let report = &diagnostics["structuredContent"];
    let diagnostic = report["value"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|row| row["key"] == "owner:first")
        .expect("duplicate identity diagnostic")
        .clone();
    let ambiguous = call(
        &state,
        Route::Repair,
        json!({"session":handle,"generation":1,"document":"config","version":null,
            "source_sha256":report["source_sha256"],
            "offset":duplicate.rfind("owner:first").expect("second identical reference"),
            "diagnostic":diagnostic}),
        1 << 18,
    );
    assert_eq!(
        ambiguous["structuredContent"]["value"],
        Value::Null,
        "an unlocated diagnostic cannot select between identical reference slots"
    );
}

#[test]
fn repair_proposals_require_a_matching_diagnostic_location() {
    let partial = "# note\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndep";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":partial},"documents":[]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"config"}),
        1 << 18,
    );
    let report = &diagnostics["structuredContent"];
    let diagnostic = report["value"][0].clone();
    let assistance = call(
        &state,
        Route::Repair,
        json!({"session":handle,"generation":1,"document":"config","version":null,
            "source_sha256":report["source_sha256"],"offset":partial.len(),
            "diagnostic":diagnostic}),
        1 << 18,
    );
    assert_eq!(assistance["structuredContent"]["value"], Value::Null);

    let page = "# Target\n\n[repair](#missing)\n";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":WORKSPACE},
            "documents":[{"id":"page","path":"page.md","text":page}]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"page"}),
        1 << 18,
    );
    let report = &diagnostics["structuredContent"];
    let offset = page.find("#missing").expect("destination") + 1;
    let diagnostic = report["value"]
        .as_array()
        .expect("diagnostics")
        .first()
        .expect("current document diagnostic")
        .clone();
    let heading = call(
        &state,
        Route::Repair,
        json!({"session":handle,"generation":1,"document":"page","version":null,
            "source_sha256":report["source_sha256"],"offset":offset,
            "diagnostic":diagnostic}),
        1 << 18,
    );
    let repair = &heading["structuredContent"]["value"];
    assert_eq!(repair["diagnostic"], diagnostic);
    assert_eq!(repair["proposal"]["kind"], "heading");
    assert_eq!(repair["proposal"]["value"]["preimage"], "#missing");
    assert_eq!(
        repair["proposal"]["value"]["candidates"][0]["fragment"],
        "#target"
    );

    let malformed = "+++\n[broken\n+++\n# Body\n";
    let state = Sessions::default();
    let started = call(
        &state,
        Route::Start,
        json!({"workspace":{"id":"config","path":"workspace.toml","text":WORKSPACE},
            "documents":[{"id":"page","path":"page.md","text":malformed}]}),
        1 << 18,
    );
    let handle = started["structuredContent"]["session"]
        .as_str()
        .expect("session");
    let diagnostics = call(
        &state,
        Route::Diagnostics,
        json!({"session":handle,"generation":1,"document":"page"}),
        1 << 18,
    );
    let report = &diagnostics["structuredContent"];
    let diagnostic = report["value"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|row| row["location"].is_object())
        .expect("located malformed-source diagnostic")
        .clone();
    let end = diagnostic["location"]["span"]["end"]
        .as_u64()
        .expect("diagnostic end") as usize;
    let at_exclusive_end = call(
        &state,
        Route::Repair,
        json!({"session":handle,"generation":1,"document":"page","version":null,
            "source_sha256":report["source_sha256"],"offset":end,
            "diagnostic":diagnostic}),
        1 << 18,
    );
    assert_eq!(
        at_exclusive_end["structuredContent"]["code"], "authoring_diagnostic_binding",
        "the diagnostic end is outside its half-open span"
    );
}

#[test]
fn repair_discovery_requires_explicit_version_and_typed_exact_diagnostic() {
    let descriptor = json!(Route::Repair.descriptor());
    let validator = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    let (_, _, handle, report, diagnostic) = fixture();
    let mut valid = request("owner:typo", &handle, &report, diagnostic);
    valid["offset"] = json!(0);
    assert!(validator.is_valid(&valid));
    for field in ["version", "diagnostic", "source_sha256"] {
        let mut invalid = valid.clone();
        invalid.as_object_mut().expect("object").remove(field);
        assert!(!validator.is_valid(&invalid), "missing {field}");
        assert!(
            Route::Repair
                .decode(invalid.as_object().expect("object").clone())
                .is_err()
        );
    }
    let complete = valid.clone();
    let mut invented_shape = valid;
    invented_shape["diagnostic"]["invented"] = json!(true);
    assert!(!validator.is_valid(&invented_shape));
    for field in ["field", "key", "source", "target", "location"] {
        let mut incomplete = complete.clone();
        incomplete["diagnostic"]
            .as_object_mut()
            .expect("diagnostic object")
            .remove(field);
        assert!(
            !validator.is_valid(&incomplete),
            "missing diagnostic {field}"
        );
        assert!(
            Route::Repair
                .decode(incomplete.as_object().expect("object").clone())
                .is_err()
        );
    }
}
