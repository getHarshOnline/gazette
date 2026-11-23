//! Card projection preserves the exact canonical page context across resumption.

use super::{restore_card_context, take_social_card};
use serde_json::{Value, json};

fn page(card: bool) -> Value {
    let mut model = json!({
        "title": "Retained title",
        "presentation": {"surface": "page", "theme": "base", "webmcp": {}},
        "items": [3, 1, 2]
    });
    if card {
        model["presentation"]["social_card"] = json!({"url": "card.png", "width": 1200});
    }
    model.sort_all_objects();
    model
}

#[test]
fn resumed_card_has_the_same_pre_card_bytes_and_retains_changed_input_detection() {
    let mut first = page(false);
    let mut resumed = page(true);
    assert!(take_social_card(&mut first).unwrap().is_none());
    assert!(take_social_card(&mut resumed).unwrap().is_some());
    let expected = serde_json::to_vec(&first).unwrap();
    assert_eq!(expected, serde_json::to_vec(&resumed).unwrap());
    resumed["title"] = json!("Changed title");
    assert_ne!(expected, serde_json::to_vec(&resumed).unwrap());
}

#[test]
fn temporary_card_context_restores_the_original_canonical_bytes() {
    for has_card in [false, true] {
        let mut model = page(has_card);
        let original = model.clone();
        let expected = serde_json::to_vec(&model).unwrap();
        let previous = take_social_card(&mut model).unwrap();
        let presentation = model["presentation"].clone();
        model["card"] = json!({"width": 1200, "height": 630});
        model["presentation"]["browser_modules"] = json!({"preloads": ["card.js"]});
        restore_card_context(&mut model, presentation, previous).unwrap();
        assert_eq!(model, original);
        assert_eq!(serde_json::to_vec(&model).unwrap(), expected);
    }
}
