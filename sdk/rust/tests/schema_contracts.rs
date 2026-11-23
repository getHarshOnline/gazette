//! Published schema projections remain structural and preserve layer boundaries.

use gazette_sdk::schema::{self, Contract};

#[test]
fn every_published_contract_schema_is_draft_2020_12_and_structural() {
    for contract in [
        Contract::ThemeManifest,
        Contract::TemplateContext,
        Contract::Workspace,
        Contract::AuthoredContent,
        Contract::LegalSource,
    ] {
        let schema = serde_json::to_value(schema::contract(contract)).expect("schema JSON");
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object(), "{contract:?}");
    }
}

#[test]
fn template_context_schema_keeps_page_and_presentation_layers_distinct() {
    let schema = serde_json::to_value(schema::contract(Contract::TemplateContext))
        .expect("template-context schema JSON");
    let properties = schema["properties"].as_object().expect("root properties");
    assert!(properties.contains_key("presentation"));
    assert!(properties.contains_key("content"));
    assert!(properties.contains_key("links"));
    assert!(properties.contains_key("body_html"));
    assert!(properties.contains_key("json_ld"));
}
