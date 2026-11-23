//! The complete reading model shares one bound across authored and provider pages.

use super::*;

fn document(page: LlmsPage) -> LlmsDocument {
    LlmsDocument {
        title: "Reading".into(),
        description: "A complete reading catalog".into(),
        coverage: LlmsCoverage::AllPages,
        include_human_link: true,
        pages: vec![page],
        sections: vec![],
    }
}

#[test]
fn combined_reading_model_refuses_the_aggregate_without_truncating_entries() {
    let authored = LlmsPage {
        id: Some("content:guide".parse().unwrap()),
        title: "A".repeat(384),
        description: None,
        url: "https://example.test/guide/index.md".into(),
        html_url: "https://example.test/guide/".into(),
        page_number: None,
        page_count: None,
        listing_variant: None,
        listing_direction: None,
    };
    let provider = LlmsPage {
        id: None,
        title: "B".repeat(384),
        url: "https://docs.example.test/reference".into(),
        html_url: "https://docs.example.test/reference".into(),
        ..authored.clone()
    };
    assert!(check_llms_model(&document(authored.clone()), 1024).is_ok());
    assert!(check_llms_model(&document(provider.clone()), 1024).is_ok());
    let mut combined = document(authored);
    combined.pages.push(provider);
    let before = serde_json::to_value(&combined).unwrap();
    assert!(matches!(
        check_llms_model(&combined, 1024),
        Err(render::Error::Limit("documentation llms model bytes"))
    ));
    assert_eq!(serde_json::to_value(&combined).unwrap(), before);
    assert_eq!(combined.pages.len(), 2);
    assert!(check_llms_model(&combined, 4096).is_ok());
}
