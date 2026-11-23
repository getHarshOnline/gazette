//! Capture recipes are bounded data, not caller-supplied native commands.

use super::parse;
use gazette_compiler::{theme::SocialCardMediaKind, theme_package::Error};

const RECIPE: &str = "template='card.html',width=1200,height=630,media_selector='[data-card-media]',media_kind='image'";

fn manifest(recipe: &str) -> String {
    format!(
        "package='theme:card'\nnamespace='card'\ntemplates='templates'\nrender={{page='page.html',social_card={{{recipe}}}}}\n"
    )
}

#[test]
fn social_card_recipe_retains_exact_capture_fields_and_template_entry() {
    for (kind, seek) in [
        ("image", ""),
        ("video", ""),
        ("video", ",video_time_ms=600000"),
    ] {
        let source = manifest(&format!(
            "{}{}",
            RECIPE.replace("media_kind='image'", &format!("media_kind='{kind}'")),
            seek
        ));
        let parsed = parse(&source).unwrap();
        let render = parsed.manifest().render.as_ref().unwrap();
        let recipe = render.social_card.as_ref().unwrap();
        assert_eq!((recipe.width, recipe.height), (1200, 630));
        assert_eq!(recipe.media_selector, "[data-card-media]");
        assert_eq!(
            recipe.media_kind,
            if kind == "image" {
                SocialCardMediaKind::Image
            } else {
                SocialCardMediaKind::Video
            }
        );
        assert_eq!(
            recipe.video_time_ms,
            if seek.is_empty() { None } else { Some(600000) }
        );
        assert!(render.entries().any(|entry| entry == &recipe.template));
        let schema = serde_json::to_value(schemars::schema_for!(
            gazette_compiler::theme::SocialCardRecipe
        ))
        .unwrap();
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&serde_json::to_value(recipe).unwrap())
            .unwrap();
    }
    let parsed = parse("package='theme:none'\nnamespace='none'\ntemplates='templates'\nrender={page='page.html'}\n").unwrap();
    let render = parsed.manifest().render.as_ref().unwrap();
    assert!(render.social_card.is_none());
    assert!(
        serde_json::to_value(render)
            .unwrap()
            .get("social_card")
            .is_none()
    );
}

#[test]
fn social_card_dimensions_selector_and_seek_enforce_exact_bounds() {
    let maximum_selector = "a".repeat(1024);
    for recipe in [
        RECIPE.replace("width=1200,height=630", "width=1,height=4096"),
        RECIPE.replace("width=1200,height=630", "width=4096,height=1"),
        RECIPE.replace("[data-card-media]", &maximum_selector),
        format!(
            "{},video_time_ms=0",
            RECIPE.replace("media_kind='image'", "media_kind='video'")
        ),
    ] {
        assert!(parse(&manifest(&recipe)).is_ok(), "{recipe}");
    }
    for (recipe, expected) in [
        (RECIPE.replace("width=1200", "width=0"), "width"),
        (RECIPE.replace("width=1200", "width=4097"), "width"),
        (RECIPE.replace("height=630", "height=0"), "height"),
        (RECIPE.replace("height=630", "height=4097"), "height"),
        (RECIPE.replace("[data-card-media]", ""), "media_selector"),
        (
            RECIPE.replace("[data-card-media]", " img"),
            "media_selector",
        ),
        (
            RECIPE.replace("[data-card-media]", "img "),
            "media_selector",
        ),
        (
            RECIPE.replace("[data-card-media]", &"x".repeat(1025)),
            "media_selector",
        ),
        (
            RECIPE.replace("[data-card-media]", &"é".repeat(513)),
            "media_selector",
        ),
        (
            RECIPE.replace(
                "media_selector='[data-card-media]'",
                "media_selector=\"img\\n\"",
            ),
            "media_selector",
        ),
        (format!("{RECIPE},video_time_ms=0"), "video_time_ms"),
        (
            format!(
                "{},video_time_ms=600001",
                RECIPE.replace("media_kind='image'", "media_kind='video'")
            ),
            "video_time_ms",
        ),
    ] {
        match parse(&manifest(&recipe)) {
            Err(Error::Declaration { field, .. }) => {
                assert_eq!(field, format!("render.social_card.{expected}"))
            }
            _ => panic!("expected bounded recipe refusal for {expected}"),
        }
    }
}

#[test]
fn social_card_required_fields_unknown_controls_and_unsafe_entries_are_refused() {
    let positional = "package='theme:card'\nnamespace='card'\ntemplates='templates'\nrender={page='page.html',social_card=['card.html',1200,630,'img','image']}\n";
    assert!(parse(positional).is_err());
    for recipe in [
        RECIPE.replace("width=1200,", ""),
        RECIPE.replace("height=630,", ""),
        RECIPE.replace("media_selector='[data-card-media]',", ""),
        RECIPE.replace(",media_kind='image'", ""),
        RECIPE.replace("width=1200", "width=-1"),
        RECIPE.replace("height=630", "height=2.5"),
        RECIPE.replace("media_kind='image'", "media_kind='IMAGE'"),
        RECIPE.replace("card.html", "../card.html"),
        format!("{RECIPE},arguments=['--remote-debugging-port=9']"),
        format!("{RECIPE},environment={{DISPLAY='inherited'}}"),
        format!("{RECIPE},source='source:helper'"),
        format!("{RECIPE},video_time_ms=-1"),
    ] {
        assert!(parse(&manifest(&recipe)).is_err(), "{recipe}");
    }
}

#[test]
fn card_resources_are_ordered_typed_and_unique() {
    let source = manifest(&format!(
        "{RECIPE},resource_requirements=['stylesheet:card','module:media']"
    ));
    let parsed = parse(&source).unwrap();
    let recipe = parsed
        .manifest()
        .render
        .as_ref()
        .unwrap()
        .social_card
        .as_ref()
        .unwrap();
    assert_eq!(
        recipe
            .resource_requirements
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["stylesheet:card", "module:media"]
    );
    for requirements in ["['module:card','module:card']", "['bad']"] {
        assert!(
            parse(&manifest(&format!(
                "{RECIPE},resource_requirements={requirements}"
            )))
            .is_err()
        );
    }
}
