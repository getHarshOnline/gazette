//! CSS raw-style URL transport is decoded independently of its encoder.
use super::{captured, compile_port, has_native_tera_cause, output};
use gazette_adapter_tera::{Error, Limits, TemplateSet};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

fn decoded_url(value: &str) -> String {
    let mut input = cssparser::ParserInput::new(value);
    let mut parser = cssparser::Parser::new(&mut input);
    let value = parser.expect_url().expect("one native CSS URL").to_string();
    parser.expect_exhausted().expect("no injected CSS tokens");
    value
}

#[test]
fn native_css_url_preserves_complete_mounted_percent_encoded_url() {
    let source = captured(&[("value.html", b"{{ value | css_url }}")]);
    let direct = TemplateSet::compile(&source, Limits::default()).expect("captured templates");
    let dynamic = compile_port(&source, Limits::default()).expect("dynamic templates");
    drop(source);
    let entry = "value.html".parse().expect("entry");
    let value = "https://one.test/mounted/a%20b.webp?x=1&y=%2f#fragment";
    let context = json!({"value":value});
    let bytes = direct
        .render(&entry, &context)
        .expect("native css_url filter");
    let encoded = std::str::from_utf8(&bytes).expect("UTF-8 CSS");
    assert!(encoded.starts_with("url(\"") && encoded.ends_with("\")"));
    assert_eq!(decoded_url(encoded), value);
    assert_eq!(dynamic.render(&entry, &context).expect("same port"), bytes);
}

#[test]
fn css_parser_recovers_sensitive_controls_hex_adjacency_and_unicode_exactly() {
    let source = captured(&[("value.html", b"{{ value | css_url }}")]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("captured templates");
    for value in [
        "",
        "https://one.test/a\") ; color:red; background:url(\"x",
        "</StYlE><script>alert('&x')</script><!--",
        "\"'\\\n\r\t\u{c}\u{1}\u{7f}\u{85}\u{2028}\u{2029}",
        "<a0 >f0 \u{1}a \u{1} ",
        "雪😀 space#fragment?x=%20&y=%2f",
        "literal \\3c escape and &#x3c; entity",
    ] {
        let encoded = output(&set, "value.html", json!({"value":value})).expect("CSS transport");
        assert_eq!(decoded_url(&encoded), value);
        assert!(!encoded.contains('<') && !encoded.contains('>'));
        assert!(!encoded.contains("&quot;") && !encoded.contains("&#x2F;"));
        assert_eq!(
            output(&set, "value.html", json!({"value":value})).expect("repeat"),
            encoded
        );
    }
}

#[derive(Default)]
struct StyleSink {
    in_style: Cell<bool>,
    tags: RefCell<Vec<String>>,
    text: RefCell<String>,
}

impl html5ever::tokenizer::TokenSink for StyleSink {
    type Handle = ();
    fn process_token(
        &self,
        token: html5ever::tokenizer::Token,
        _: u64,
    ) -> html5ever::tokenizer::TokenSinkResult<()> {
        use html5ever::tokenizer::{TagKind, Token, TokenSinkResult, states::RawKind};
        match token {
            Token::TagToken(tag) => {
                self.tags.borrow_mut().push(format!(
                    "{}{}",
                    if tag.kind == TagKind::EndTag { "/" } else { "" },
                    tag.name
                ));
                if tag.name.as_ref() == "style" {
                    let start = tag.kind == TagKind::StartTag;
                    self.in_style.set(start);
                    if start {
                        return TokenSinkResult::RawData(RawKind::Rawtext);
                    }
                }
            }
            Token::CharacterTokens(value) if self.in_style.get() => {
                self.text.borrow_mut().push_str(&value)
            }
            _ => (),
        }
        TokenSinkResult::Continue
    }
}

#[test]
fn html_tokenizer_keeps_mixed_case_style_end_tags_and_injection_as_one_css_string() {
    use html5ever::{
        tendril::StrTendril,
        tokenizer::{BufferQueue, Tokenizer},
    };
    let source = captured(&[(
        "value.html",
        b"<style>.x{background:{{ value | css_url }}}</style><p>after</p>",
    )]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("style template");
    let value = "https://one.test/</StYlE\t><img src=x onerror='alert(1)'>\";}/*<!--雪\\a";
    let rendered = output(&set, "value.html", json!({"value":value})).expect("raw-style transport");
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(&rendered));
    let tokenizer = Tokenizer::new(StyleSink::default(), Default::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    assert_eq!(
        *tokenizer.sink.tags.borrow(),
        ["style", "/style", "p", "/p"]
    );
    let style = tokenizer.sink.text.borrow();
    let token = style
        .strip_prefix(".x{background:")
        .and_then(|text| text.strip_suffix('}'))
        .expect("unchanged CSS wrapper");
    assert_eq!(decoded_url(token), value);
}

#[test]
fn exact_expansion_intermediate_and_aggregate_budgets_refuse_without_partial_success() {
    let source = captured(&[
        ("value.html", b"{{ value | css_url }}"),
        ("hidden.html", b"{% set encoded = value | css_url %}ok"),
        ("twice.html", b"{{ value | css_url }}{{ value | css_url }}"),
    ]);
    let expected = b"url(\"\\3c \")";
    let limits = Limits {
        output_bytes: expected.len(),
        ..Limits::default()
    };
    let set = TemplateSet::compile(&source, limits).expect("exact ceiling");
    let dynamic = compile_port(&source, limits).expect("dynamic ceiling");
    let entry = "value.html".parse().expect("entry");
    let context = json!({"value":"<"});
    assert_eq!(
        set.render(&entry, &context).expect("exact expansion"),
        expected
    );
    assert_eq!(
        dynamic
            .render(&entry, &context)
            .expect("exact dynamic expansion"),
        expected
    );
    assert_eq!(
        output(&set, "hidden.html", context.clone()).expect("bounded hidden buffer"),
        "ok"
    );
    assert!(matches!(
        output(&set, "twice.html", context.clone()),
        Err(Error::Limit("output bytes"))
    ));
    let smaller = TemplateSet::compile(
        &source,
        Limits {
            output_bytes: expected.len() - 1,
            ..limits
        },
    )
    .expect("one below ceiling");
    let smaller_dynamic = compile_port(
        &source,
        Limits {
            output_bytes: expected.len() - 1,
            ..limits
        },
    )
    .expect("one below dynamic ceiling");
    for name in ["value.html", "hidden.html"] {
        assert!(matches!(
            output(&smaller, name, context.clone()),
            Err(Error::Limit("output bytes"))
        ));
        assert!(matches!(
            smaller_dynamic.render(&name.parse().expect("entry"), &context),
            Err(Error::Limit("output bytes"))
        ));
        let recovery = json!({"value":"a"});
        let expected = if name == "hidden.html" {
            b"ok".as_slice()
        } else {
            b"url(\"a\")".as_slice()
        };
        assert_eq!(
            smaller
                .render(&name.parse().expect("entry"), &recovery)
                .expect("no poisoned state"),
            expected
        );
    }
    assert!(matches!(
        output(&smaller, "hidden.html", json!({"value":"<".repeat(4096)})),
        Err(Error::Limit("output bytes"))
    ));
}

#[test]
fn non_strings_nul_arguments_and_missing_values_keep_native_generic_errors() {
    let source = captured(&[
        ("value.html", b"{{ value | css_url }}"),
        (
            "argument.html",
            b"{{ value | css_url(context='attribute') }}",
        ),
        ("missing.html", b"{{ absent | css_url }}"),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("filter templates");
    for (name, value) in [
        ("value.html", Value::Null),
        ("value.html", json!(true)),
        ("value.html", json!(1)),
        ("value.html", json!([])),
        ("value.html", json!({"private-input-canary":true})),
        ("value.html", json!("private-input-canary\0")),
        ("argument.html", json!("private-input-canary")),
        ("missing.html", json!("private-input-canary")),
    ] {
        let Err(Error::Template(error)) = output(&set, name, json!({"value":value})) else {
            panic!("ordinary native template refusal");
        };
        assert!(has_native_tera_cause(error.as_ref()));
        assert!(!error.to_string().contains("private-input-canary"));
        assert_eq!(
            output(&set, "value.html", json!({"value":"a"})).expect("recovery"),
            "url(\"a\")"
        );
    }
}
