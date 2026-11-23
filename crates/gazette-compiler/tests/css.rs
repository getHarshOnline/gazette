//! Native CSS token decoding and strict inspection boundaries.

use std::convert::Infallible;

use gazette_compiler::css::{self, Error, Input, Limits, TextKind};

fn text(source: &[u8], limits: Limits) -> Result<Vec<(TextKind, String)>, Error<Infallible>> {
    let mut output = Vec::new();
    css::inspect(Input::Stylesheet(source), limits, |kind, value| {
        output.push((kind, value.to_owned()));
        Ok(())
    })?;
    Ok(output)
}

#[test]
fn native_escapes_cover_identifier_families_strings_urls_and_nested_functions() {
    let source = br#"@m\65 dia #\73 ecret .\74 ag { w:12p\78; x:f\6f o(["s\65 cret"],url(https\3a //example.test/a)); y:url("quoted\2f path"); }"#;
    let found = text(source, Limits::default()).expect("complete native tokens");
    for (kind, value) in [
        (TextKind::Identifier, "media"),
        (TextKind::Identifier, "secret"),
        (TextKind::Identifier, "tag"),
        (TextKind::Identifier, "px"),
        (TextKind::Identifier, "foo"),
        (TextKind::String, "secret"),
        (TextKind::Url, "https://example.test/a"),
        (TextKind::String, "quoted/path"),
    ] {
        assert!(
            found.contains(&(kind, value.to_owned())),
            "missing decoded {kind:?}"
        );
    }
}

#[test]
fn tokens_are_not_concatenated_or_url_decoded_and_comments_are_not_text() {
    let found = text(
        br#""sec" "ret" /* private */ url(a%2fb) url(data:x;base64,c2VjcmV0)"#,
        Limits::default(),
    )
    .expect("tokens");
    assert_eq!(
        found,
        vec![
            (TextKind::String, "sec".into()),
            (TextKind::String, "ret".into()),
            (TextKind::Url, "a%2fb".into()),
            (TextKind::Url, "data:x;base64,c2VjcmV0".into()),
        ]
    );
    assert_eq!(
        text(b"/* */", Limits::default()).expect("closed comment"),
        vec![]
    );
}

#[test]
fn repaired_eof_and_bad_tokens_never_produce_success() {
    for source in [
        "\"",
        "'unterminated",
        "\"escaped\\\"",
        "\"trailing\\",
        "/*",
        "/*/",
        "url(",
        "url(x",
        "url(x\\)",
        "url(x y)",
        "url(x\"y)",
        "{",
        "((x)",
        "[x)",
        "{x]}",
        "}",
        ")",
        "]",
        "a\\",
        "\"line\nbreak\"",
    ] {
        assert!(
            matches!(
                text(source.as_bytes(), Limits::default()),
                Err(Error::Malformed)
            ),
            "accepted malformed {source:?}"
        );
    }
    for source in [
        "()",
        "([])",
        "{}",
        "url()",
        "url(x\\))",
        "\"escaped\\\"\"",
        "a\\\\",
        "\"line\\\nbreak\"",
    ] {
        text(source.as_bytes(), Limits::default()).expect("real terminators and escapes");
    }
}

#[test]
fn later_failure_keeps_earlier_callbacks_speculative_and_errors_redacted() {
    let mut seen = Vec::new();
    let result: Result<(), Error<Infallible>> = css::inspect(
        Input::Stylesheet(b"first \"broken"),
        Limits::default(),
        |_, value| {
            seen.push(value.to_owned());
            Ok(())
        },
    );
    assert_eq!(seen, ["first"]);
    assert_eq!(result, Err(Error::Malformed));
    let error = css::inspect(
        Input::Stylesheet(b"first second"),
        Limits::default(),
        |_, _| Err("private consumer payload"),
    )
    .expect_err("consumer refusal");
    assert_eq!(error, Error::Consumer("private consumer payload"));
    assert!(!error.to_string().contains("private"));
    let borrowed = String::from("borrowed consumer failure");
    for input in [Input::Stylesheet(b"f(nested)"), Input::Decoded("f(nested)")] {
        assert_eq!(
            css::inspect(input, Limits::default(), |kind, value| {
                if kind == TextKind::Identifier && value == "nested" {
                    Err(borrowed.as_str())
                } else {
                    Ok(())
                }
            }),
            Err(Error::Consumer(borrowed.as_str()))
        );
    }
}

#[test]
fn decoded_text_preserves_bom_character_while_stylesheet_bytes_consume_signature() {
    let source = "\u{feff}\\73 ecret{}";
    for (input, expected, decoded_bytes) in [
        (Input::Stylesheet(source.as_bytes()), "secret", 6),
        (Input::Decoded(source), "\u{feff}secret", 9),
    ] {
        let limits = Limits {
            input_bytes: source.len(),
            tokens: 3,
            depth: 1,
            decoded_bytes,
        };
        let mut found = Vec::new();
        css::inspect(input, limits, |kind, value| {
            found.push((kind, value.to_owned()));
            Ok::<_, Infallible>(())
        })
        .expect("input-kind-specific native identifier");
        assert_eq!(found, [(TextKind::Identifier, expected.to_owned())]);
        for (limits, boundary) in [
            (
                Limits {
                    input_bytes: source.len() - 1,
                    ..limits
                },
                "input bytes",
            ),
            (
                Limits {
                    tokens: 2,
                    ..limits
                },
                "tokens",
            ),
            (
                Limits {
                    decoded_bytes: decoded_bytes - 1,
                    ..limits
                },
                "decoded bytes",
            ),
        ] {
            assert_eq!(
                css::inspect(input, limits, |_, _| Ok::<_, Infallible>(())),
                Err(Error::Limit(boundary))
            );
        }
    }
    assert_eq!(
        css::inspect(
            Input::Decoded("@charset \"UTF-8\";"),
            Limits::default(),
            |_, _| Ok::<_, Infallible>(())
        ),
        Err(Error::Unsupported)
    );
}

#[test]
fn each_budget_has_an_exact_boundary_and_counts_non_text_tokens() {
    let exact = Limits {
        input_bytes: 9,
        tokens: 4,
        depth: 1,
        decoded_bytes: 2,
    };
    // Function, comment, identifier and close are all charged.
    let source = b"f(/*x*/a)";
    assert_eq!(source.len(), 9);
    text(source, exact).expect("exact ceilings");
    for (limits, expected) in [
        (
            Limits {
                input_bytes: 8,
                ..exact
            },
            "input bytes",
        ),
        (Limits { tokens: 3, ..exact }, "tokens"),
        (Limits { depth: 0, ..exact }, "nesting depth"),
        (
            Limits {
                decoded_bytes: 1,
                ..exact
            },
            "decoded bytes",
        ),
    ] {
        assert_eq!(text(source, limits), Err(Error::Limit(expected)));
    }
    text(
        b"",
        Limits {
            input_bytes: 0,
            tokens: 0,
            depth: 0,
            decoded_bytes: 0,
        },
    )
    .expect("empty stream");
    assert_eq!(
        text(
            b" ",
            Limits {
                tokens: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("tokens"))
    );
    assert_eq!(
        text(
            br#""\e9""#,
            Limits {
                decoded_bytes: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("decoded bytes"))
    );
    text(
        br#""\e9""#,
        Limits {
            decoded_bytes: 2,
            ..Limits::default()
        },
    )
    .expect("decoded UTF-8 byte ceiling");
}

#[test]
fn nesting_is_bounded_even_when_caller_supplies_unlimited_depth() {
    let nested = format!("{}x{}", "(".repeat(64), ")".repeat(64));
    text(nested.as_bytes(), Limits::default()).expect("exact default depth");
    assert_eq!(
        text(
            nested.as_bytes(),
            Limits {
                depth: 63,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("nesting depth"))
    );
    let excessive = format!("{}x{}", "(".repeat(257), ")".repeat(257));
    assert_eq!(
        text(
            excessive.as_bytes(),
            Limits {
                depth: usize::MAX,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("nesting depth"))
    );
}

#[test]
fn utf8_and_encoding_declarations_have_explicit_outcomes() {
    assert_eq!(text(&[0xff], Limits::default()), Err(Error::Utf8));
    for source in [
        "@charset \"windows-1252\";",
        "@CHARSET \"windows-1252\";",
        "@CHARSET \"UTF-8\";",
        "@ch\\61 rset \"UTF-8\";",
        " @charset \"UTF-8\";",
        "/*before*/@charset \"UTF-8\";",
        "a{@charset \"UTF-8\";}",
        "@charset \"UTF-8\";@charset \"UTF-8\";",
        "@charset \"UTF-8\"",
        "@charset \"UTF-8",
        "@charset \"UTF-8\" ;",
        "@charset  \"UTF-8\";",
        "@charset\t\"UTF-8\";",
        "@charset 'UTF-8';",
        "@charset \"UTF\\2d 8\";",
        "@charset /*comment*/\"UTF-8\";",
        "\u{feff}@charset \"windows-1252\";",
        "\u{feff}\u{feff}@charset \"UTF-8\";",
    ] {
        assert_eq!(
            text(source.as_bytes(), Limits::default()),
            Err(Error::Unsupported)
        );
    }
    text(".नमस्ते { x:'é'; }".as_bytes(), Limits::default()).expect("native Unicode tokens");
}

#[test]
fn exact_utf8_header_and_optional_bom_preserve_native_tokens_and_all_budgets() {
    for label in ["UTF-8", "utf-8", "uTf-8"] {
        for bom in ["", "\u{feff}"] {
            let source = format!("{bom}@charset \"{label}\";");
            let exact = Limits {
                input_bytes: source.len(),
                tokens: 4,
                depth: 0,
                decoded_bytes: 12,
            };
            assert_eq!(
                text(source.as_bytes(), exact).expect("exact UTF-8 header"),
                vec![
                    (TextKind::Identifier, "charset".into()),
                    (TextKind::String, label.into()),
                ]
            );
            for (limits, expected) in [
                (
                    Limits {
                        input_bytes: source.len() - 1,
                        ..exact
                    },
                    "input bytes",
                ),
                (Limits { tokens: 3, ..exact }, "tokens"),
                (
                    Limits {
                        decoded_bytes: 11,
                        ..exact
                    },
                    "decoded bytes",
                ),
            ] {
                assert_eq!(text(source.as_bytes(), limits), Err(Error::Limit(expected)));
            }
            let body = format!("{source}.é{{x:'नमस्ते'}}");
            let found =
                text(body.as_bytes(), Limits::default()).expect("header followed by UTF-8 CSS");
            assert!(found.contains(&(TextKind::String, "नमस्ते".into())));
        }
    }
    assert_eq!(
        text(
            b"\xef\xbb\xbf",
            Limits {
                input_bytes: 3,
                tokens: 0,
                depth: 0,
                decoded_bytes: 0,
            }
        )
        .expect("BOM has bytes but no CSS token"),
        vec![]
    );
    assert_eq!(
        text(b"@charset \"UTF-8\";\xff", Limits::default()),
        Err(Error::Utf8)
    );
}
