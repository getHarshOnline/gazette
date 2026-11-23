//! Real HTML token decoding through the bounded, fallible public inspection seam.

use gazette_compiler::html::{self, Error, Limits, TextKind};

type Observation = (TextKind, String);

fn inspect(bytes: &[u8], limits: Limits) -> Result<Vec<Observation>, Error<&'static str>> {
    let mut values = Vec::new();
    html::inspect(bytes, limits, |kind, value| {
        values.push((kind, value.to_owned()));
        Ok(())
    })?;
    Ok(values)
}

fn observed(kind: TextKind, value: &str) -> Observation {
    (kind, value.to_owned())
}

#[test]
fn inert_navigation_and_folder_svg_preserve_decoded_attributes_and_accessible_text() {
    let input = concat!(
        "<button><svg viewBox='0 0 24 24' aria-hidden='true'>",
        "<path d='M19 12H6m5-6-6 6 6 6'/></svg></button>",
        "<svg class='folder-card__edge' preserveAspectRatio='none' aria-label='&#83;ECRET'>",
        "<title>A&amp;B</title><desc>&lt;b&gt;plain&lt;/b&gt;</desc>",
        "<path vector-effect='non-scaling-stroke'></path></svg><p>after</p>",
    );
    assert_eq!(
        inspect(input.as_bytes(), Limits::default()).expect("finite inert SVG"),
        vec![
            observed(TextKind::Attribute, "0 0 24 24"),
            observed(TextKind::Attribute, "true"),
            observed(TextKind::Attribute, "M19 12H6m5-6-6 6 6 6"),
            observed(TextKind::Attribute, "folder-card__edge"),
            observed(TextKind::Attribute, "none"),
            observed(TextKind::Attribute, "SECRET"),
            observed(TextKind::Text, "A&B"),
            observed(TextKind::Text, "<b>plain</b>"),
            observed(TextKind::Attribute, "non-scaling-stroke"),
            observed(TextKind::Text, "after"),
        ],
    );
    for source in [
        "<svg/>",
        "<svg xmlns='http://www.w3.org/2000/svg'><path d='M1e-3,2.5L-1+2Z'/></svg>",
        "<svg preserveAspectRatio='xMidYMid meet'> \n<path/>\t</svg>",
    ] {
        inspect(source.as_bytes(), Limits::default()).expect("bounded SVG forms");
    }
}

#[test]
fn svg_active_attributes_foreign_markup_and_unfinished_structure_fail_closed() {
    for source in [
        "<svg onload='x()'/>",
        "<svg style='color:red'/>",
        "<svg fill='url(private)'/>",
        "<svg xmlns='other'/>",
        "<svg xmlns:xlink='http://www.w3.org/1999/xlink'/>",
        "<svg><path href='private'/></svg>",
        "<svg><path xlink:href='private'/></svg>",
        "<svg><path style='color:red'/></svg>",
        "<svg><path onload='x()'/></svg>",
        "<svg><path d='path(\"M1 2\")'/></svg>",
        "<svg><path d='M1\\32 3'/></svg>",
        "<svg><path vector-effect='non-\\73caling-stroke'/></svg>",
        "<svg viewBox='var(private)'/>",
        "<svg preserveAspectRatio='none other'/>",
        "<svg><script>private</script></svg>",
        "<svg><style>private</style></svg>",
        "<svg><foreignObject><p>private</p></foreignObject></svg>",
        "<svg><image href='private'/></svg>",
        "<svg><use href='private'/></svg>",
        "<svg><a href='private'/></svg>",
        "<svg><animate attributeName='d'/></svg>",
        "<svg><set attributeName='d'/></svg>",
        "<svg><g><path/></g></svg>",
        "<svg><svg/></svg>",
        "<svg><path><path/></path></svg>",
        "<svg><path>private</path></svg>",
        "<svg>private</svg>",
        "<svg><title><b>private</b></title></svg>",
        "<svg><desc><script>private</script></desc></svg>",
        "<svg><math xmlns='http://www.w3.org/1998/Math/MathML'/></svg>",
        "<math xmlns='http://www.w3.org/1998/Math/MathML'><svg/></math>",
        "<svg><!--private--></svg>",
        "<svg><![CDATA[private]]></svg>",
        "<svg><!DOCTYPE html></svg>",
        "<svg viewBox='0 0 1 1' VIEWBOX='0 0 2 2'/>",
        "<svg><path d='M1 2' D='M3 4'/></svg>",
        "<svg><path></svg>",
        "<svg></path></svg>",
        "<svg></svg extra>",
        "<svg>",
        "<svg><path/>",
        "<svg/><path/></svg>",
        "</svg>",
    ] {
        assert!(
            inspect(source.as_bytes(), Limits::default()).is_err(),
            "unsupported/malformed SVG must fail: {source}",
        );
    }
}

#[test]
fn svg_uses_shared_html_limits_and_stops_on_consumer_failure() {
    let source = b"<svg aria-label='private'><title>text</title><path/></svg>";
    for limits in [
        Limits {
            input_bytes: 8,
            ..Limits::default()
        },
        Limits {
            decoded_bytes: 6,
            ..Limits::default()
        },
        Limits {
            tokens: 2,
            ..Limits::default()
        },
    ] {
        assert!(matches!(inspect(source, limits), Err(Error::Limit(_))));
    }
    let mut calls = 0;
    let result = html::inspect(source, Limits::default(), |_, _| {
        calls += 1;
        Err("stop")
    });
    assert!(matches!(result, Err(Error::Consumer("stop"))));
    assert_eq!(calls, 1);
}

#[test]
fn style_attributes_are_entity_decoded_once_in_their_own_channel() {
    assert_eq!(
        inspect(
            br#"<p STYLE="content:&#39;\73 ecret&amp;value&#39;" title="ordinary">body</p>"#,
            Limits::default(),
        )
        .expect("native style attribute context"),
        vec![
            observed(TextKind::StyleAttribute, r"content:'\73 ecret&value'"),
            observed(TextKind::Attribute, "ordinary"),
            observed(TextKind::Text, "body"),
        ]
    );
}

#[test]
fn style_type_must_select_css_before_delivering_its_body() {
    for kind in ["", "text/css", " TEXT/CSS "] {
        let input = format!("<style type='{kind}'>p{{color:red}}</style>");
        let values = inspect(input.as_bytes(), Limits::default()).expect("explicit CSS type");
        assert_eq!(
            values.last(),
            Some(&observed(TextKind::Style, "p{color:red}"))
        );
    }
    let mut bodies = 0;
    let result = html::inspect(
        b"<style type='text/other'>private-body</style>",
        Limits::default(),
        |kind, _| {
            if kind == TextKind::Style {
                bodies += 1;
            }
            Ok::<_, &'static str>(())
        },
    );
    assert!(matches!(result, Err(Error::Unsupported { .. })));
    assert_eq!(bodies, 0, "an unknown style language cannot be called CSS");
}

#[test]
fn native_entities_decode_once_in_data_attributes_and_rcdata() {
    let source = concat!(
        "<!DOCTYPE html><p data-note='A&amp;B &#x1F642; &quot;'>",
        "&NotEqualTilde; &#65; &lt;b&gt;literal&lt;/b&gt; &amp;lt;</p>",
        "<title>T&amp;T</title><textarea>&lt;i&gt; &copy;</textarea>",
    );
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("native decoding"),
        vec![
            observed(TextKind::Attribute, "A&B 🙂 \""),
            observed(TextKind::Text, "≂\u{338} A <b>literal</b> &lt;"),
            observed(TextKind::Text, "T&T"),
            observed(TextKind::Text, "<i> ©"),
        ],
    );
}

#[test]
fn entities_and_utf8_remain_whole_across_fixed_feed_boundaries() {
    for padding in 242..260 {
        let prefix = "a".repeat(padding);
        for encoded in ["&NotEqualTilde;🙂", "🙂&#x1F642;", "&amp;&#65;"] {
            let decoded = match encoded {
                "&NotEqualTilde;🙂" => "≂\u{338}🙂",
                "🙂&#x1F642;" => "🙂🙂",
                _ => "&A",
            };
            let value = format!("{prefix}{decoded}tail");
            let source = format!("<p>{prefix}{encoded}tail</p>");
            assert_eq!(
                inspect(source.as_bytes(), Limits::default()).expect("split text input"),
                vec![observed(TextKind::Text, &value)],
                "text padding {padding}, encoded {encoded}",
            );
            let source = format!("<p data-v='{prefix}{encoded}tail'></p>");
            assert_eq!(
                inspect(source.as_bytes(), Limits::default()).expect("split attribute input"),
                vec![observed(TextKind::Attribute, &value)],
                "attribute padding {padding}, encoded {encoded}",
            );
        }
    }
}

#[test]
fn raw_script_style_and_json_script_have_distinct_undecoded_channels() {
    let source = concat!(
        "<script>const value = '&amp;';</script>",
        "<style>p::before{content:'&amp;';}</style>",
        "<script type=' Application/LD+JSON '>{\"value\":\"&amp;\\u0041\"}</script>",
        "<script type='application/json'>{\"value\":1}</script>",
        "<script type='text/plain'>&copy;</script><p>after</p>",
    );
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("separate raw channels"),
        vec![
            observed(TextKind::Script, "const value = '&amp;';"),
            observed(TextKind::Style, "p::before{content:'&amp;';}"),
            observed(TextKind::Attribute, " Application/LD+JSON "),
            observed(TextKind::JsonScript, "{\"value\":\"&amp;\\u0041\"}"),
            observed(TextKind::Attribute, "application/json"),
            observed(TextKind::JsonScript, "{\"value\":1}"),
            observed(TextKind::Attribute, "text/plain"),
            observed(TextKind::DataScript, "&copy;"),
            observed(TextKind::Text, "after"),
        ],
    );
}

#[test]
fn empty_json_script_is_not_silently_omitted_from_consumer_validation() {
    let values = inspect(
        b"<script type='application/ld+json'></script><p>after</p>",
        Limits::default(),
    )
    .expect("empty raw JSON is delivered, not interpreted");
    assert_eq!(
        values,
        vec![
            observed(TextKind::Attribute, "application/ld+json"),
            observed(TextKind::JsonScript, ""),
            observed(TextKind::Text, "after"),
        ],
    );
}

#[test]
fn script_classification_uses_decoded_attributes_and_exact_ascii_rules() {
    for (attributes, expected) in [
        ("", TextKind::Script),
        ("type", TextKind::Script),
        ("type='' language='other'", TextKind::Script),
        ("type=' ' language='javascript'", TextKind::DataScript),
        ("type='\t\n\r\u{c} MoDuLe \t'", TextKind::ModuleScript),
        ("TYPE='m&#111;dule'", TextKind::ModuleScript),
        ("type='\u{a0}module\u{a0}'", TextKind::DataScript),
        ("type='\u{feff}module'", TextKind::DataScript),
        (
            "type='text/javascript; charset=utf-8'",
            TextKind::DataScript,
        ),
        ("type='text/plain'", TextKind::DataScript),
        ("type='application/json'", TextKind::JsonScript),
        (
            "type=' APPLICATION/LD+JSON ' language='javascript'",
            TextKind::JsonScript,
        ),
        ("type=' ImportMap '", TextKind::JsonScript),
        ("type='SPECULATIONRULES'", TextKind::JsonScript),
        (
            "type='application/json; charset=utf-8'",
            TextKind::DataScript,
        ),
        ("type='\u{a0}application/json'", TextKind::DataScript),
        ("language=''", TextKind::Script),
        ("language='JavaScript1.2'", TextKind::Script),
        ("language='ECMAScript'", TextKind::Script),
        ("language='j&#97;vascript'", TextKind::Script),
        ("language=' javascript'", TextKind::DataScript),
        ("language='javascript '", TextKind::DataScript),
        ("language='text/javascript'", TextKind::DataScript),
        ("language='module'", TextKind::DataScript),
        ("language='vbscript'", TextKind::DataScript),
        ("type='module' language='vbscript'", TextKind::ModuleScript),
    ] {
        let source = format!("<script {attributes}>same-body</script>");
        let values = inspect(source.as_bytes(), Limits::default()).expect("native script context");
        assert_eq!(
            values.last(),
            Some(&observed(expected, "same-body")),
            "{attributes}"
        );
        assert!(
            values[..values.len() - 1]
                .iter()
                .all(|(kind, _)| *kind == TextKind::Attribute)
        );
    }
}

#[test]
fn every_javascript_mime_essence_matches_without_parameter_parsing() {
    for kind in [
        "application/ecmascript",
        "application/javascript",
        "application/x-ecmascript",
        "application/x-javascript",
        "text/ecmascript",
        "text/javascript",
        "text/javascript1.0",
        "text/javascript1.1",
        "text/javascript1.2",
        "text/javascript1.3",
        "text/javascript1.4",
        "text/javascript1.5",
        "text/jscript",
        "text/livescript",
        "text/x-ecmascript",
        "text/x-javascript",
    ] {
        let source = format!(
            "<script type=' {} '>body</script>",
            kind.to_ascii_uppercase()
        );
        assert_eq!(
            inspect(source.as_bytes(), Limits::default())
                .expect("recognized MIME essence")
                .last(),
            Some(&observed(TextKind::Script, "body")),
            "{kind}",
        );
    }
}

#[test]
fn src_presence_keeps_the_body_raw_without_implying_external_coverage() {
    for (attributes, expected) in [
        ("src", TextKind::ExternalScript),
        ("src=''", TextKind::ExternalScript),
        ("src='external.js'", TextKind::ExternalScript),
        ("type='module' src='external.js'", TextKind::ExternalScript),
        (
            "src='&#101;xternal.js' type='application/json'",
            TextKind::DataScript,
        ),
        ("src='external.json' type='importmap'", TextKind::DataScript),
        (
            "src='external.json' type='speculationrules'",
            TextKind::DataScript,
        ),
        ("src='external.txt' type='text/plain'", TextKind::DataScript),
    ] {
        let source = format!("<script {attributes}>not JS or JSON &amp; \\u0073</script>");
        assert_eq!(
            inspect(source.as_bytes(), Limits::default())
                .expect("external body stays raw")
                .last(),
            Some(&observed(expected, "not JS or JSON &amp; \\u0073")),
            "{attributes}",
        );
    }
    let values = inspect(
        b"<script src type='application/json'></script>",
        Limits::default(),
    )
    .expect("no inline JSON claim for an external source");
    assert_eq!(values.last(), Some(&observed(TextKind::DataScript, "")));
}

#[test]
fn native_on_prefix_attributes_preserve_values_and_do_not_claim_handler_parsing() {
    assert_eq!(
        inspect(br#"<p ONCLICK="&#x61;lert(&quot;x&quot;)" oncustom='' data-onclick='ordinary'>after</p>"#, Limits::default())
            .expect("native normalized attribute names"),
        vec![
            observed(TextKind::EventAttribute, "alert(\"x\")"),
            observed(TextKind::EventAttribute, ""),
            observed(TextKind::Attribute, "ordinary"),
            observed(TextKind::Text, "after"),
        ]
    );
    assert!(matches!(
        html::inspect(
            b"<p onclick='secret'>after</p>",
            Limits::default(),
            |kind, _| {
                if kind == TextKind::EventAttribute {
                    Err("unqualified handler")
                } else {
                    Ok(())
                }
            }
        ),
        Err(Error::Consumer("unqualified handler"))
    ));
}

#[test]
fn empty_script_classes_and_references_are_visible_to_the_consumer() {
    for (attributes, expected) in [
        ("type='text/unknown'", TextKind::DataScript),
        ("language='vbscript'", TextKind::DataScript),
        ("src='external' type='text/unknown'", TextKind::DataScript),
        ("src='external' language='vbscript'", TextKind::DataScript),
        (
            "src='external' type='application/json'",
            TextKind::DataScript,
        ),
        ("src='external'", TextKind::ExternalScript),
        ("src='external' type='module'", TextKind::ExternalScript),
        (
            "src='external' language='javascript'",
            TextKind::ExternalScript,
        ),
    ] {
        let source = format!("<script {attributes}></script>");
        let values = inspect(source.as_bytes(), Limits::default()).expect("native empty context");
        assert_eq!(values.last(), Some(&observed(expected, "")), "{attributes}");
        let mut bodies = 0;
        let result = html::inspect(source.as_bytes(), Limits::default(), |kind, value| {
            if kind == expected {
                assert!(value.is_empty());
                bodies += 1;
                Err("context refused")
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::Consumer("context refused"))));
        assert_eq!(bodies, 1);
    }
}

#[test]
fn json_grammar_channels_include_empty_maps_and_rules_but_do_not_parse_schemas() {
    for kind in [
        "application/json",
        "application/ld+json",
        "importmap",
        "speculationrules",
    ] {
        for body in ["", "not valid JSON", "[]", r#"{"arbitrary":true}"#] {
            let source = format!("<script type='{kind}'>{body}</script>");
            assert_eq!(
                inspect(source.as_bytes(), Limits::default()).expect("body classification only"),
                vec![
                    observed(TextKind::Attribute, kind),
                    observed(TextKind::JsonScript, body)
                ],
            );
        }
    }
}

#[test]
fn script_rawtext_end_tags_follow_html_not_javascript_or_data_syntax() {
    assert_eq!(
        inspect(
            b"<script type='module'>const value = \"</ScRiPt><p>after</p>",
            Limits::default()
        )
        .expect("HTML closes inside a JavaScript string"),
        vec![
            observed(TextKind::Attribute, "module"),
            observed(TextKind::ModuleScript, "const value = \""),
            observed(TextKind::Text, "after")
        ],
    );
    assert_eq!(
        inspect(b"<script type='text/plain'>before<!--<script>inside</script>still--></script><p>after</p>", Limits::default())
            .expect("native double-escaped script state also applies to data blocks"),
        vec![observed(TextKind::Attribute, "text/plain"), observed(TextKind::DataScript, "before<!--<script>inside</script>still-->"), observed(TextKind::Text, "after")],
    );
    for kind in ["module", "text/plain", "application/json"] {
        let source = format!("<script type='{kind}'>unterminated");
        assert!(matches!(
            inspect(source.as_bytes(), Limits::default()),
            Err(Error::Malformed { .. })
        ));
    }
}

#[test]
fn unicode_whitespace_does_not_select_css() {
    for kind in ["\u{a0}text/css", "text/css\u{feff}", "\u{a0}"] {
        let source = format!("<style type='{kind}'>p{{}}</style>");
        assert!(matches!(
            inspect(source.as_bytes(), Limits::default()),
            Err(Error::Unsupported { .. })
        ));
    }
}

#[test]
fn duplicate_attributes_fail_before_a_dropped_value_can_be_called_clean() {
    for source in [
        "<p title='innocent' title='secret-canary'>after</p>",
        "<p TITLE='innocent' title='&#115;ecret-canary'>after</p>",
        "<script type='application/json' TYPE='text/javascript'>{}</script>",
    ] {
        let mut callbacks = 0;
        let error = html::inspect(source.as_bytes(), Limits::default(), |_, _| {
            callbacks += 1;
            Ok::<(), &'static str>(())
        })
        .expect_err("duplicate attributes invalidate the whole candidate");
        assert!(matches!(error, Error::Malformed { .. }), "{error:?}");
        assert_eq!(callbacks, 0, "no deduplicated tag is accepted: {source}");
        assert!(!error.to_string().contains("secret-canary"));
    }
}

#[test]
fn malformed_native_tokens_and_invalid_utf8_fail_without_lossy_repair() {
    for source in [
        "<p>&unknown;</p>",
        "<p>&#0;</p>",
        "<p>&#xD800;</p>",
        "<p>&#x110000;</p>",
        "<p>&copy</p>",
        "<p a='&bogus;'>value</p>",
        "<p>\0</p>",
        "<p>value</p extra='invalid'>",
    ] {
        let error = inspect(source.as_bytes(), Limits::default()).expect_err(source);
        assert!(
            matches!(error, Error::Malformed { .. }),
            "{source}: {error:?}"
        );
    }
    assert!(matches!(
        inspect(b"<p>\xff</p>", Limits::default()),
        Err(Error::Utf8)
    ));
}

#[test]
fn unsupported_contexts_and_doctypes_never_fall_back_to_ordinary_text() {
    for tag in [
        "svg",
        "noscript",
        "plaintext",
        "xmp",
        "iframe",
        "noembed",
        "noframes",
    ] {
        let source = format!("<{tag}>secret</{tag}>");
        assert!(
            matches!(
                inspect(source.as_bytes(), Limits::default()),
                Err(Error::Unsupported { .. })
            ),
            "{source}",
        );
    }
    for source in [
        "<!DOCTYPE svg><p>value</p>",
        "<!DOCTYPE html SYSTEM 'https://example.test/type'><p>value</p>",
        "<script/>",
        "<style/>",
        "<title/>",
        "<textarea/>",
    ] {
        assert!(
            matches!(
                inspect(source.as_bytes(), Limits::default()),
                Err(Error::Unsupported { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn bounded_mathml_uses_the_same_decoded_channels_without_adding_an_active_context() {
    let source = concat!(
        "<p>before</p>",
        "<math xmlns='http://www.w3.org/1998/Math/MathML' display='block'>",
        "<semantics><mrow><mfrac><mi mathvariant='bold'>a</mi>",
        "<mrow><mn>2</mn><mo stretchy='false'>&lt;</mo><mi>b</mi></mrow>",
        "</mfrac></mrow><annotation encoding='application/x-tex'>",
        "\\frac{a}{2&lt;b}</annotation></semantics></math>",
        "<p>after</p>",
    );
    let values = inspect(source.as_bytes(), Limits::default()).expect("bounded MathML");
    assert_eq!(
        values,
        vec![
            observed(TextKind::Text, "before"),
            observed(TextKind::Attribute, "http://www.w3.org/1998/Math/MathML"),
            observed(TextKind::Attribute, "block"),
            observed(TextKind::Attribute, "bold"),
            observed(TextKind::Text, "a"),
            observed(TextKind::Text, "2"),
            observed(TextKind::Attribute, "false"),
            observed(TextKind::Text, "<"),
            observed(TextKind::Text, "b"),
            observed(TextKind::Attribute, "application/x-tex"),
            observed(TextKind::Text, r"\frac{a}{2<b}"),
            observed(TextKind::Text, "after"),
        ]
    );
}

#[test]
fn mathml_namespace_structure_and_inert_vocabulary_fail_closed() {
    let valid = concat!(
        "<math xmlns='http://www.w3.org/1998/Math/MathML'>",
        "<semantics><mrow><mi>x</mi></mrow>",
        "<annotation encoding='application/x-tex'>x</annotation>",
        "</semantics></math>",
    );
    assert!(inspect(valid.as_bytes(), Limits::default()).is_ok());
    for invalid in [
        valid.replace(" xmlns='http://www.w3.org/1998/Math/MathML'", ""),
        valid.replace("1998/Math/MathML", "1999/Math/MathML"),
        valid.replace("<mi>x</mi>", "<script>x</script>"),
        valid.replace("<mi>x</mi>", "<svg><text>x</text></svg>"),
        valid.replace("<mi>x</mi>", "<a href='https://example.test/'>x</a>"),
        valid.replace("<mi>x</mi>", "<mi onclick='run()'>x</mi>"),
        valid.replace("<mi>x</mi>", "<mfrac><mi>x</mi></mfrac>"),
        valid.replace("<mi>x</mi>", "<mi><!-- hidden -->x</mi>"),
        valid.replace("application/x-tex", "text/html"),
        valid.replace("x</annotation>", "</annotation>"),
        format!("{valid}<math xmlns='http://www.w3.org/1998/Math/MathML'>"),
    ] {
        assert!(
            matches!(
                inspect(invalid.as_bytes(), Limits::default()),
                Err(Error::Unsupported { .. } | Error::Malformed { .. })
            ),
            "{invalid}",
        );
    }
}

#[test]
fn incomplete_eof_fails_instead_of_accepting_a_partial_token_or_raw_run() {
    for source in [
        "<p a='unfinished",
        "<!-- unfinished",
        "<!DOCTYPE",
        "<",
        "<script>unfinished",
        "<style>unfinished",
        "<title>unfinished",
        "<textarea>unfinished",
        "<script type='application/json'>",
    ] {
        let error = inspect(source.as_bytes(), Limits::default()).expect_err(source);
        assert!(
            matches!(error, Error::Malformed { .. } | Error::Incomplete),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn input_decoded_and_token_limits_apply_inclusively_without_truncation() {
    let bytes = "<p a='é'>🙂</p>".as_bytes();
    let limits = Limits {
        input_bytes: bytes.len(),
        decoded_bytes: 6,
        ..Limits::default()
    };
    assert_eq!(
        inspect(bytes, limits).expect("exact byte budgets"),
        vec![
            observed(TextKind::Attribute, "é"),
            observed(TextKind::Text, "🙂")
        ],
    );
    assert!(matches!(
        inspect(
            bytes,
            Limits {
                input_bytes: bytes.len() - 1,
                ..limits
            }
        ),
        Err(Error::Limit("input bytes"))
    ));
    assert!(matches!(
        inspect(
            bytes,
            Limits {
                decoded_bytes: 5,
                ..limits
            }
        ),
        Err(Error::Limit("decoded bytes"))
    ));
    assert!(
        inspect(
            b"",
            Limits {
                tokens: 1,
                ..Limits::default()
            }
        )
        .expect("one EOF token")
        .is_empty()
    );
    assert!(matches!(
        inspect(
            b"",
            Limits {
                tokens: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("tokens"))
    ));
}

#[test]
fn unterminated_large_tag_stops_at_unemitted_input_budget_before_any_consumer() {
    let source = format!("<p title='{}'>after</p>", "a".repeat(600));
    let mut callbacks = 0;
    let error = html::inspect(
        source.as_bytes(),
        Limits {
            unemitted_bytes: 256,
            ..Limits::default()
        },
        |_, _| {
            callbacks += 1;
            Ok::<(), &'static str>(())
        },
    )
    .expect_err("complete tag exceeds buffered-input allowance");
    assert!(matches!(error, Error::Limit("unemitted input bytes")));
    assert_eq!(callbacks, 0);
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("same source within default budget"),
        vec![
            observed(TextKind::Attribute, &"a".repeat(600)),
            observed(TextKind::Text, "after")
        ],
    );
}

#[test]
fn callback_failure_preserves_the_error_and_stops_later_callbacks_without_panic() {
    let bytes = format!("<p title='first'>second</p>{}", "<p>later</p>".repeat(100)).into_bytes();
    let original = bytes.clone();
    let mut calls = Vec::new();
    let result = html::inspect(&bytes, Limits::default(), |kind, value| {
        calls.push(observed(kind, value));
        Err("consumer-stopped")
    });
    assert!(matches!(result, Err(Error::Consumer("consumer-stopped"))));
    assert_eq!(calls, vec![observed(TextKind::Attribute, "first")]);
    assert_eq!(bytes, original, "inspection never rewrites source bytes");
}

#[test]
fn text_boundaries_and_source_line_endings_are_not_reinterpreted_as_dom_layout() {
    assert_eq!(
        inspect(
            b"<p>a\r\nb<!-- comment -->c<b>d</b>e</p>",
            Limits::default()
        )
        .expect("native token runs"),
        vec![
            observed(TextKind::Text, "a\nb"),
            observed(TextKind::Text, "c"),
            observed(TextKind::Text, "d"),
            observed(TextKind::Text, "e"),
        ],
    );
}

#[test]
fn interior_bom_at_feed_boundaries_survives_text_attributes_and_json_script() {
    for boundary in [256, 512] {
        let opening = "<p>";
        let padding = "a".repeat(boundary - opening.len());
        let value = format!("{padding}\u{feff}tail");
        let source = format!("{opening}{value}</p>");
        assert_eq!(
            &source.as_bytes()[boundary..boundary + 3],
            "\u{feff}".as_bytes()
        );
        assert_eq!(
            inspect(source.as_bytes(), Limits::default()).expect("interior text BOM"),
            vec![observed(TextKind::Text, &value)],
            "text boundary {boundary}",
        );

        let opening = "<p data-v='";
        let padding = "a".repeat(boundary - opening.len());
        let value = format!("{padding}\u{feff}tail");
        let source = format!("{opening}{value}'></p>");
        assert_eq!(
            &source.as_bytes()[boundary..boundary + 3],
            "\u{feff}".as_bytes()
        );
        assert_eq!(
            inspect(source.as_bytes(), Limits::default()).expect("interior attribute BOM"),
            vec![observed(TextKind::Attribute, &value)],
            "attribute boundary {boundary}",
        );

        let opening = "<script type='application/json'>{\"v\":\"";
        let padding = "a".repeat(boundary - opening.len());
        let source = format!("{opening}{padding}\u{feff}tail\"}}</script>");
        let value = format!("{{\"v\":\"{padding}\u{feff}tail\"}}");
        assert_eq!(
            &source.as_bytes()[boundary..boundary + 3],
            "\u{feff}".as_bytes()
        );
        assert_eq!(
            inspect(source.as_bytes(), Limits::default()).expect("interior JSON-script BOM"),
            vec![
                observed(TextKind::Attribute, "application/json"),
                observed(TextKind::JsonScript, &value)
            ],
            "JSON-script boundary {boundary}",
        );
    }
}

#[test]
fn only_one_stream_leading_bom_is_removed() {
    assert_eq!(
        inspect("\u{feff}plain".as_bytes(), Limits::default()).expect("single leading BOM"),
        vec![observed(TextKind::Text, "plain")],
    );
    assert_eq!(
        inspect("\u{feff}\u{feff}plain".as_bytes(), Limits::default())
            .expect("second BOM is content"),
        vec![observed(TextKind::Text, "\u{feff}plain")],
    );
    assert!(
        inspect("\u{feff}".as_bytes(), Limits::default())
            .expect("BOM-only stream")
            .is_empty()
    );
}

#[test]
fn original_input_limit_counts_bom_bytes_before_removing_one() {
    let source = "\u{feff}plain".as_bytes();
    assert_eq!(
        inspect(
            source,
            Limits {
                input_bytes: source.len(),
                ..Limits::default()
            }
        )
        .expect("exact original bytes"),
        vec![observed(TextKind::Text, "plain")],
    );
    let mut callbacks = 0;
    let result = html::inspect(
        source,
        Limits {
            input_bytes: source.len() - 1,
            ..Limits::default()
        },
        |_, _| {
            callbacks += 1;
            Ok::<(), &'static str>(())
        },
    );
    assert!(matches!(result, Err(Error::Limit("input bytes"))));
    assert_eq!(callbacks, 0);
}
