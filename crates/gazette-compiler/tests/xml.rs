//! Actual XML lexical and semantic inspection, without filesystem or provider effects.

use gazette_compiler::xml::{self, EmbeddedElement, Error, Limits, TextKind};

type Observation = (TextKind, String);

fn inspect(bytes: &[u8], limits: Limits) -> Result<Vec<Observation>, Error<&'static str>> {
    let mut values = Vec::new();
    xml::inspect(bytes, limits, |kind, value| {
        values.push((kind, value.to_owned()));
        Ok(())
    })?;
    Ok(values)
}

fn observed(kind: TextKind, value: &str) -> Observation {
    (kind, value.to_owned())
}

#[test]
fn namespace_resolved_embedded_elements_emit_even_without_body_text() {
    let source = concat!(
        "<s:svg xmlns:s='http://www.w3.org/2000/svg'>",
        "<s:script/><s:style>svg{fill:red}</s:style>",
        "<s:foreignObject><h:div xmlns:h='http://www.w3.org/1999/xhtml'/></s:foreignObject>",
        "</s:svg>",
    );
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("native expanded names"),
        vec![
            observed(TextKind::Namespace, "http://www.w3.org/2000/svg"),
            observed(TextKind::EmbeddedElement(EmbeddedElement::SvgScript), ""),
            observed(TextKind::EmbeddedElement(EmbeddedElement::SvgStyle), ""),
            observed(TextKind::Text, "svg{fill:red}"),
            observed(
                TextKind::EmbeddedElement(EmbeddedElement::SvgForeignObject),
                ""
            ),
            observed(TextKind::Namespace, "http://www.w3.org/1999/xhtml"),
            observed(TextKind::EmbeddedElement(EmbeddedElement::Xhtml), ""),
        ]
    );
    let values = inspect(b"<svg xmlns='http://www.w3.org/2000/svg'><script>before<![CDATA[&amp;]]>after</script></svg>", Limits::default())
        .expect("markers do not reinterpret raw XML text");
    assert_eq!(
        values.last(),
        Some(&observed(TextKind::Text, "before&amp;after"))
    );
}

#[test]
fn namespace_and_case_namesakes_do_not_gain_embedded_context() {
    let values = inspect(
        b"<r xmlns:s='urn:other'><script/><s:script/><style/><foreignObject/><div xmlns='urn:other'/><svg xmlns='http://www.w3.org/2000/svg'><Script/><foreignobject/></svg></r>",
        Limits::default(),
    ).expect("namespaces and XML local names remain exact");
    assert!(
        values
            .iter()
            .all(|(kind, _)| !matches!(kind, TextKind::EmbeddedElement(_)))
    );
}

#[test]
fn svg_style_and_on_prefix_attributes_keep_native_values_and_namespace_identity() {
    let values = inspect(
        br#"<svg xmlns='http://www.w3.org/2000/svg' xmlns:p='urn:other' style='fill:&#x72;ed' onload='run(&quot;x&quot;)' oncustom='' p:style='ordinary' p:onload='other'><r xmlns='' style='ordinary' onclick='text'/></svg>"#,
        Limits::default(),
    ).expect("native attribute context without embedded language parsing");
    assert_eq!(
        &values[2..],
        &[
            observed(TextKind::StyleAttribute, "fill:red"),
            observed(TextKind::EventAttribute, "run(\"x\")"),
            observed(TextKind::EventAttribute, ""),
            observed(TextKind::Attribute, "ordinary"),
            observed(TextKind::Attribute, "other"),
            observed(TextKind::Namespace, ""),
            observed(TextKind::Attribute, "ordinary"),
            observed(TextKind::Attribute, "text"),
        ]
    );
}

#[test]
fn embedded_markers_allow_immediate_consumer_refusal_without_payload_or_extra_parsing() {
    for source in [
        "<script xmlns='http://www.w3.org/2000/svg'/>",
        "<style xmlns='http://www.w3.org/2000/svg'/>",
        "<foreignObject xmlns='http://www.w3.org/2000/svg'/>",
        "<div xmlns='http://www.w3.org/1999/xhtml'/>",
    ] {
        let mut markers = 0;
        let result = xml::inspect(source.as_bytes(), Limits::default(), |kind, value| {
            if matches!(kind, TextKind::EmbeddedElement(_)) {
                assert!(value.is_empty());
                markers += 1;
                Err("unsupported embedded context")
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            result,
            Err(Error::Consumer("unsupported embedded context"))
        ));
        assert_eq!(markers, 1);
    }
}

#[test]
fn native_references_decode_once_and_adjacent_cdata_joins_text() {
    let source = concat!(
        "<?xml version='1.0' encoding='UTF-8'?>",
        "<r a='&lt;&gt;&amp;&apos;&quot;&#65;&#x1F642;'>",
        "sec<![CDATA[ret]]>&#45;canary &amp;lt;",
        "<![CDATA[ &amp; &#65;]]></r>",
    );
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("native XML decoding"),
        vec![
            observed(TextKind::Attribute, "<>&'\"A🙂"),
            observed(TextKind::Text, "secret-canary &lt; &amp; &#65;"),
        ],
    );
}

#[test]
fn native_newline_and_attribute_normalization_preserve_reference_whitespace() {
    let source = "<r a='A\tB\r\nC&#x9;D&#xA;E&#xD;F'>a\r\nb\rc<![CDATA[\r\nd]]></r>";
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("XML normalization"),
        vec![
            observed(TextKind::Attribute, "A B C\tD\nE\rF"),
            observed(TextKind::Text, "a\nb\nc\nd"),
        ],
    );
}

#[test]
fn explicit_unused_namespace_values_are_decoded_without_synthetic_findings() {
    let source = concat!(
        "<r xmlns:unused='urn:secret&#45;canary' xml:lang='en'>",
        "<child/><child xmlns:unused='urn:other'/></r>",
    );
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("explicit namespace evidence"),
        vec![
            observed(TextKind::Namespace, "urn:secret-canary"),
            observed(TextKind::Attribute, "en"),
            observed(TextKind::Namespace, "urn:other"),
        ],
    );
    assert!(
        inspect(b"<r><child/></r>", Limits::default())
            .expect("implicit namespace bindings are not source findings")
            .is_empty()
    );
    assert_eq!(
        inspect(
            b"<r xmlns:xml='http://www.w3.org/XML/1998/namespace'/>",
            Limits::default(),
        )
        .expect("an explicitly spelled predefined binding is evidence"),
        vec![observed(
            TextKind::Namespace,
            "http://www.w3.org/XML/1998/namespace",
        )],
    );
}

#[test]
fn comments_and_processing_instructions_stay_literal_and_separate_text() {
    let source = "<r>a<!-- &amp; &#65; -->b<?probe href='&amp;'?>c<child/>d</r>";
    assert_eq!(
        inspect(source.as_bytes(), Limits::default()).expect("literal non-executing channels"),
        vec![
            observed(TextKind::Text, "a"),
            observed(TextKind::Comment, " &amp; &#65; "),
            observed(TextKind::Text, "b"),
            observed(TextKind::ProcessingInstruction, "probe"),
            observed(TextKind::ProcessingInstruction, "href='&amp;'"),
            observed(TextKind::Text, "c"),
            observed(TextKind::Text, "d"),
        ],
    );
}

#[test]
fn duplicate_attributes_including_namespace_declarations_fail_before_callbacks() {
    for source in [
        "<r a='innocent' a='secret-canary'/>",
        "<r xmlns='urn:innocent' xmlns='urn:secret-canary'/>",
        "<r xmlns:p='urn:innocent' xmlns:p='urn:secret-canary'/>",
    ] {
        let mut callbacks = 0;
        let error = xml::inspect(source.as_bytes(), Limits::default(), |_, _| {
            callbacks += 1;
            Ok::<(), &'static str>(())
        })
        .expect_err("lexical duplicates invalidate the complete candidate");
        assert!(matches!(error, Error::Malformed), "{source}: {error:?}");
        assert_eq!(callbacks, 0, "no dropped declaration is accepted: {source}");
        assert!(!error.to_string().contains("secret-canary"));
    }
    assert_eq!(
        inspect(b"<r a='one' A='two'/>", Limits::default()).expect("XML names are case-sensitive"),
        vec![
            observed(TextKind::Attribute, "one"),
            observed(TextKind::Attribute, "two"),
        ],
    );
}

#[test]
fn expanded_attribute_identity_uses_normalized_namespace_values() {
    for source in [
        "<r xmlns:p='urn:same' xmlns:q='urn:same' p:a='one' q:a='secret-canary'/>",
        "<r xmlns:p='urn:same' xmlns:q='urn:sa&#109;e' p:a='one' q:a='secret-canary'/>",
    ] {
        let error = inspect(source.as_bytes(), Limits::default()).expect_err(source);
        assert!(matches!(error, Error::Malformed), "{error:?}");
        assert!(!error.to_string().contains("secret-canary"));
    }
}

#[test]
fn unbound_and_reserved_namespace_bindings_fail_without_fallback() {
    for source in [
        "<p:r/>",
        "<r p:a='secret'/>",
        "<r xmlns:xmlns='urn:secret'/>",
        "<r xmlns:xml='urn:wrong'/>",
        "<r xmlns:p='http://www.w3.org/XML/1998/namespace'/>",
        "<r xmlns:p='http://www.w3.org/2000/xmlns/'/>",
        "<r xmlns:p='http://www.w3.org/2000/&#120;mlns/'/>",
        "<r xmlns='http://www.w3.org/XML/1998/namespace'/>",
        "<r xmlns='http://www.w3.org/2000/xmlns/'/>",
    ] {
        assert!(
            matches!(
                inspect(source.as_bytes(), Limits::default()),
                Err(Error::Malformed)
            ),
            "{source}",
        );
    }
}

#[test]
fn all_actual_doctypes_fail_before_resolution_or_expansion() {
    for source in [
        "<!DOCTYPE r><r/>",
        "<!DOCTYPE r SYSTEM 'file:///secret-canary'><r/>",
        "<!DOCTYPE r PUBLIC 'secret' 'https://example.invalid/type'><r/>",
        "<!DOCTYPE r [<!ENTITY secret 'secret-canary'>]><r>&secret;</r>",
        concat!(
            "<!DOCTYPE r [<!ENTITY a 'secret-canary'>",
            "<!ENTITY b '&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;'>",
            "<!ENTITY c '&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;'>]><r>&c;</r>",
        ),
    ] {
        let mut callbacks = 0;
        let error = xml::inspect(source.as_bytes(), Limits::default(), |_, _| {
            callbacks += 1;
            Ok::<(), &'static str>(())
        })
        .expect_err("DOCTYPE is unsupported before semantic parsing");
        assert!(matches!(error, Error::Unsupported), "{source}: {error:?}");
        assert_eq!(callbacks, 0);
    }
}

#[test]
fn malformed_documents_and_incomplete_eof_never_return_partial_success() {
    for source in [
        "",
        "text<r/>",
        "<r/>trailing",
        "<r/><other/>",
        "<r>",
        "<r><child></r>",
        "<r a='unfinished",
        "<r><!-- unfinished",
        "<r><![CDATA[unfinished",
        "<r><?unfinished",
        "<r>&unfinished",
        "<r a=unquoted/>",
        "<r><!-- invalid--comment --></r>",
        "<r>&unknown;</r>",
    ] {
        let error = inspect(source.as_bytes(), Limits::default()).expect_err(source);
        assert!(matches!(error, Error::Malformed), "{source}: {error:?}");
    }
    assert!(matches!(
        inspect(b"<r>\xff</r>", Limits::default()),
        Err(Error::Utf8),
    ));
}

#[test]
fn illegal_xml10_characters_and_numeric_scalars_are_not_replaced() {
    for value in [
        "&#0;",
        "&#1;",
        "&#xB;",
        "&#xD800;",
        "&#x110000;",
        "&#xFFFF;",
        "&#xFFFE;",
        "&#+65;",
        "&#x+41;",
        "\0",
        "\u{1}",
        "\u{b}",
        "\u{fffe}",
        "\u{ffff}",
    ] {
        for source in [format!("<r>{value}</r>"), format!("<r a='{value}'/>")] {
            let error = inspect(source.as_bytes(), Limits::default()).expect_err(&source);
            assert!(matches!(error, Error::Malformed), "{source:?}: {error:?}");
        }
    }
}

#[test]
fn only_xml10_utf8_declarations_are_supported() {
    for source in [
        "<?xml version='1.1'?><r/>",
        "<?xml version='1.9'?><r/>",
        "<?xml version='1.0' encoding='ISO-8859-1'?><r>é</r>",
        "<?xml version='1.0' encoding='UTF-16'?><r/>",
    ] {
        assert!(
            matches!(
                inspect(source.as_bytes(), Limits::default()),
                Err(Error::Unsupported)
            ),
            "{source}",
        );
    }
    assert_eq!(
        inspect(
            b"<?xml version='1.0' encoding='utf-8' standalone='yes'?><r>ok</r>",
            Limits::default(),
        )
        .expect("supported declaration"),
        vec![observed(TextKind::Text, "ok")],
    );
}

#[test]
fn original_input_bytes_include_the_bom_and_interior_feff_survives() {
    let source = "\u{feff}<r a='x\u{feff}y'>z\u{feff}q</r>";
    let limits = Limits {
        input_bytes: source.len(),
        ..Limits::default()
    };
    assert_eq!(
        inspect(source.as_bytes(), limits).expect("one leading BOM only"),
        vec![
            observed(TextKind::Attribute, "x\u{feff}y"),
            observed(TextKind::Text, "z\u{feff}q"),
        ],
    );
    assert!(matches!(
        inspect(
            source.as_bytes(),
            Limits {
                input_bytes: source.len() - 1,
                ..limits
            }
        ),
        Err(Error::Limit("input bytes")),
    ));
    assert!(matches!(
        inspect("\u{feff}\u{feff}<r/>".as_bytes(), Limits::default()),
        Err(Error::Malformed),
    ));
}

#[test]
fn depth_and_event_limits_include_empty_elements_and_document_boundaries() {
    assert!(
        inspect(
            b"<r><child/></r>",
            Limits {
                depth: 2,
                ..Limits::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        inspect(
            b"<r><child/></r>",
            Limits {
                depth: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("depth")),
    ));
    assert!(matches!(
        inspect(
            b"<r/>",
            Limits {
                depth: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("depth")),
    ));
    assert!(
        inspect(
            b"<r/>",
            Limits {
                events: 4,
                ..Limits::default()
            }
        )
        .is_ok()
    );
    for events in [0, 1, 2, 3] {
        assert!(matches!(
            inspect(
                b"<r/>",
                Limits {
                    events,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("events")),
        ));
    }
}

#[test]
fn attribute_limits_count_namespace_declarations_and_names_and_raw_values() {
    let source = b"<r xmlns:p='urn:p' xmlns:q='urn:q' a='ok'/>";
    assert!(
        inspect(
            source,
            Limits {
                attributes: 3,
                ..Limits::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        inspect(
            source,
            Limits {
                attributes: 2,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("attributes")),
    ));
    assert!(
        inspect(
            b"<rr aa='ok'/>",
            Limits {
                name_bytes: 2,
                ..Limits::default()
            }
        )
        .is_ok()
    );
    for source in [b"<rrr/>".as_slice(), b"<r aaa='ok'/>".as_slice()] {
        assert!(matches!(
            inspect(
                source,
                Limits {
                    name_bytes: 2,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("name bytes")),
        ));
    }
    assert!(
        inspect(
            b"<r a='ok'/>",
            Limits {
                value_bytes: 2,
                ..Limits::default()
            }
        )
        .is_ok()
    );
    for source in [
        b"<r a='long'/>".as_slice(),
        b"<r xmlns:p='urn:long'/>".as_slice(),
    ] {
        assert!(matches!(
            inspect(
                source,
                Limits {
                    value_bytes: 2,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("value bytes")),
        ));
    }
}

#[test]
fn native_text_comment_and_pi_value_limits_do_not_silently_truncate() {
    for source in [
        "<r>long-value</r>",
        "<r><!--long-value--></r>",
        "<r><?p long-value?></r>",
    ] {
        let error = inspect(
            source.as_bytes(),
            Limits {
                value_bytes: 2,
                ..Limits::default()
            },
        )
        .expect_err("native parser ceiling must invalidate the candidate");
        assert!(matches!(error, Error::Malformed), "{source}: {error:?}");
    }
}

#[test]
fn cdata_and_reference_runs_cannot_bypass_the_complete_value_ceiling() {
    for source in [
        "<r><![CDATA[long-value]]></r>",
        "<r><![CDATA[ab]]><![CDATA[cd]]></r>",
        "<r>&#65;&#65;&#65;</r>",
    ] {
        let mut callbacks = 0;
        let error = xml::inspect(
            source.as_bytes(),
            Limits {
                value_bytes: 2,
                ..Limits::default()
            },
            |_, _| {
                callbacks += 1;
                Ok::<(), &'static str>(())
            },
        )
        .expect_err("a complete value is bounded before its consumer");
        assert!(
            matches!(error, Error::Limit("value bytes")),
            "{source}: {error:?}"
        );
        assert_eq!(callbacks, 0, "no truncated value is delivered");
    }
    assert_eq!(
        inspect(
            b"<r><![CDATA[a]]><![CDATA[b]]></r>",
            Limits {
                value_bytes: 2,
                ..Limits::default()
            },
        )
        .expect("exact joined value budget"),
        vec![observed(TextKind::Text, "ab")],
    );
}

#[test]
fn decoded_budget_counts_utf8_and_all_delivered_channels_without_truncation() {
    let source = "<r xmlns:p='urn:x' a='é'>🙂<!--q--><?p d?></r>";
    let expected = vec![
        observed(TextKind::Namespace, "urn:x"),
        observed(TextKind::Attribute, "é"),
        observed(TextKind::Text, "🙂"),
        observed(TextKind::Comment, "q"),
        observed(TextKind::ProcessingInstruction, "p"),
        observed(TextKind::ProcessingInstruction, "d"),
    ];
    let bytes = 5 + 2 + 4 + 1 + 1 + 1;
    assert_eq!(
        inspect(
            source.as_bytes(),
            Limits {
                decoded_bytes: bytes,
                ..Limits::default()
            }
        )
        .expect("exact aggregate UTF-8 budget"),
        expected,
    );
    assert!(matches!(
        inspect(
            source.as_bytes(),
            Limits {
                decoded_bytes: bytes - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("decoded bytes")),
    ));
}

#[test]
fn consumer_failure_stops_callbacks_and_preserves_the_input() {
    let source = b"<r a='first'>later<child a='never'/></r>".to_vec();
    let original = source.clone();
    let mut values = Vec::new();
    let error = xml::inspect(&source, Limits::default(), |kind, value| {
        values.push(observed(kind, value));
        Err("stop here")
    })
    .expect_err("consumer failure propagates without another callback");
    assert!(matches!(error, Error::Consumer("stop here")));
    assert_eq!(values, vec![observed(TextKind::Attribute, "first")]);
    assert_eq!(source, original);
}
