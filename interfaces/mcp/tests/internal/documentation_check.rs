//! Documentation intent is inspectable without native acquisition or target assembly.

use super::*;
use serde_json::{Value, json};

fn configuration() -> String {
    "[[principals]]\nid='owner:one'\nkind='person'\n\
     [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
     [[projects]]\nid='project:gazette'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
[[sources]]\nid='source:gazette'\nlocation={kind='repository',root='/not-opened/gazette',commit='1111111111111111111111111111111111111111',subdirectory='project'}\n\
     [[brands]]\nid='brand:gazette'\nname='Gazette'\n\
     [[brands.publications]]\nsite='site:one'\n\
     [brands.publications.documentation_repository]\nname='gazette-domain-doc'\npath='/not-opened/gazette-domain-doc'\n\
     [brands.publications.documentation_repository.build]\ntarget={kind='mintlify',theme='mint',primary_color='#123abc'}\n\
     [[brands.publications.documentation_repository.build.modules]]\nproject='project:gazette'\nslug='gazette'\ntitle='Gazette manual'\n\
     manual={source='source:gazette'}\n\
     navigation=[{kind='manual',title='Start',pages=[{title='Overview',route='gazette/overview',input={kind='file',path='docs/overview.md'}}]}]\n"
        .to_owned()
}

fn call(value: Value, maximum: usize) -> Result<CallToolResult, ErrorData> {
    crate::tools::Route::named("check_documentation")
        .expect("discoverable route")
        .decode(value.as_object().expect("named arguments").clone())?
        .execute(
            &crate::authoring::Sessions::default(),
            &crate::release_session::Sessions::default(),
            &crate::recovery_session::Sessions::default(),
            &crate::store_session::Sessions::default(),
            &crate::preview::Sessions::default(),
            maximum,
        )
}

#[test]
fn discovery_is_closed_read_only_and_matches_the_resolved_manual_plan() {
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    assert_eq!(descriptor["name"], "check_documentation");
    assert_eq!(
        descriptor["annotations"],
        json!({
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        })
    );
    let arguments = json!({
        "source": configuration(),
        "site": "site:one",
        "syntax": toml::Limits::default()
    });
    let validator = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
    assert!(validator.is_valid(&arguments));
    let output = call(arguments, 128 << 10).expect("effect-free report");
    assert_eq!(output.is_error, Some(false));
    let value = output.structured_content.expect("structured report");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(&value)
        .expect("report conforms");
    assert_eq!(value["site"], "site:one");
    assert_eq!(value["brand"], "brand:gazette");
    assert_eq!(value["repository_name"], "gazette-domain-doc");
    assert_eq!(
        value["build"]["modules"][0]["navigation"][0]["kind"],
        "manual"
    );
}

#[test]
fn decoder_rejects_unknown_or_positional_syntax_and_native_paths_are_not_opened() {
    for value in [
        json!({"source":configuration(),"site":"site:one","unknown":true}),
        json!({"source":configuration(),"site":"site:one","syntax":[]}),
    ] {
        assert!(decode(value.as_object().expect("arguments").clone()).is_err());
    }
    let output = call(
        json!({"source":configuration(),"site":"site:one"}),
        128 << 10,
    )
    .expect("unopened repository paths remain declarative");
    assert_eq!(output.is_error, Some(false));
}

#[test]
fn invalid_target_or_small_delivery_returns_a_typed_failure_without_effects() {
    let mut source = configuration();
    source = source.replace("theme='mint'", "theme='unknown'");
    let failed =
        call(json!({"source":source,"site":"site:one"}), 128 << 10).expect("typed target refusal");
    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed.structured_content.expect("failure")["code"],
        "documentation_target_input"
    );
    let bounded = call(json!({"source":configuration(),"site":"site:one"}), 8)
        .expect("typed transport limit");
    assert_eq!(bounded.is_error, Some(true));
    assert_eq!(
        bounded.structured_content.expect("bounded failure")["code"],
        "mcp_output_limit"
    );
}
