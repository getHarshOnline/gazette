//! Native Markdown encoding for semantic Theme metadata, including resource bounds.
use super::*;

#[test]
fn metadata_cannot_inject_markdown_structure_or_change_destinations() {
    let source = captured(&[("llms.txt", b"# {{ title | markdown_text }}\n[{{ title | markdown_text }}]({{ url | markdown_destination }}): {{ description | markdown_text }}")]);
    let set = TemplateSet::compile(&source, Default::default()).unwrap();
    let actual = output(
        &set,
        "llms.txt",
        json!({
            "title":"A [title](not-a-link) <html>",
            "url":"https://writer.test/a(1)?x=1&y=2",
            "description":"A purpose.\n\n## Injected [item](bad)"
        }),
    )
    .unwrap();
    assert_eq!(
        actual,
        "# A \\[title\\](not-a-link) \\<html\\>\n[A \\[title\\](not-a-link) \\<html\\>](https://writer.test/a\\(1\\)?x=1\\&y=2): A purpose. \\#\\# Injected \\[item\\](bad)"
    );
    assert!(!actual.contains("\n## Injected"));
}

#[test]
fn theme_llms_guide_keeps_prose_punctuation_readable() {
    let source = captured(&[(
        "llms.txt",
        b"# {{ llms.title | markdown_text }}\n\n> {{ llms.description | markdown_text }}\n\n{% for page in llms.pages %}- [{{ page.title | markdown_text }}]({{ page.url | markdown_destination }}): {{ page.description | markdown_text }}\n{% endfor %}",
    )]);
    let set = TemplateSet::compile(&source, Default::default()).unwrap();
    let actual = output(
        &set,
        "llms.txt",
        json!({"llms": {
            "title": "Sleepwalker",
            "description": "A science-fiction story of seven children, in different worlds.",
            "pages": [
                {"title": "DAO", "url": "https://writer.test/dao/index.md",
                 "description": "Composed applications, coordinated response."},
                {"title": "Services", "url": "https://writer.test/services/index.md",
                 "description": "A paid audit, an agreed build, and hands-on engineering (the team's work)."}
            ]
        }}),
    ).unwrap();
    assert_eq!(
        actual,
        "# Sleepwalker\n\n> A science-fiction story of seven children, in different worlds.\n\n- [DAO](https://writer.test/dao/index.md): Composed applications, coordinated response.\n- [Services](https://writer.test/services/index.md): A paid audit, an agreed build, and hands-on engineering (the team's work).\n"
    );
}

#[test]
fn literal_metadata_keeps_structure_entities_and_whitespace_bounded() {
    let source = captured(&[("text.txt", b"{{ value | markdown_text }}")]);
    let set = TemplateSet::compile(&source, Default::default()).unwrap();
    for (value, expected) in [
        ("\n  - item\n+ other\t", r"\- item + other"),
        ("+ item", r"\+ item"),
        ("1. item", r"1\. item"),
        ("123) item", r"123\) item"),
        ("---", r"\---"),
        ("===", r"\==="),
        (": definition", r"\: definition"),
        ("![alt](image)", r"!\[alt\](image)"),
        (
            "<tag> `code` *bold* _em_ ~~gone~~",
            r"\<tag\> \`code\` \*bold\* \_em\_ \~\~gone\~\~",
        ),
        ("&amp; &#65;", r"\&amp; \&\#65;"),
        ("{value} | $value \\end", r"\{value\} \| \$value \\end"),
        (
            "café, नमस्ते. 1.2 / 3: 'quoted' (aside)!",
            "café, नमस्ते. 1.2 / 3: 'quoted' (aside)!",
        ),
    ] {
        assert_eq!(
            output(&set, "text.txt", json!({"value":value})).unwrap(),
            expected
        );
    }
}

#[test]
fn markdown_encoder_refuses_wrong_shapes_arguments_controls_and_excess() {
    let source = captured(&[
        ("text.txt", b"{{ value | markdown_text }}"),
        ("url.txt", b"{{ value | markdown_destination }}"),
        ("argument.txt", b"{{ value | markdown_text(extra=true) }}"),
        ("hidden.txt", b"{% set encoded = value | markdown_text %}ok"),
    ]);
    let set = TemplateSet::compile(&source, Default::default()).unwrap();
    for (name, value) in [
        ("text.txt", json!(12)),
        ("text.txt", json!({})),
        ("argument.txt", json!("text")),
        ("url.txt", json!("https://x/\n- injected")),
        ("text.txt", json!("a\u{0}b")),
    ] {
        assert!(output(&set, name, json!({"value":value})).is_err());
    }
    let set = TemplateSet::compile(
        &source,
        Limits {
            output_bytes: 8,
            ..Default::default()
        },
    )
    .unwrap();
    for name in ["text.txt", "hidden.txt"] {
        assert!(matches!(
            output(&set, name, json!({"value":"********"})),
            Err(Error::Limit("output bytes"))
        ));
    }
    assert_eq!(
        output(&set, "text.txt", json!({"value":"abcd"})).unwrap(),
        "abcd"
    );
}
