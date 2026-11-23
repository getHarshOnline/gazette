//! Explicit native Markdown decoding over retained output, not plain-text coverage.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_adapter_markdown::Comrak;
use gazette_application::build::{
    DisclosureClass, DisclosureDecoding, DisclosureInspectors, DisclosurePattern, DisclosurePolicy,
    DisclosureProbeError, DisclosureProbeLimits, DisclosureRepresentation, DisclosureRule,
    check_disclosure, check_disclosure_policy_with_inspectors, check_disclosure_with_inspectors,
    probe_disclosure, probe_disclosure_with_inspectors,
};
use gazette_core::{
    publication::Key,
    render::MediaType,
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest},
};

fn captured(path: &str, bytes: &[u8]) -> CapturedSource {
    let path: EntryPath = path.parse().expect("output path");
    let mut inventory = InventoryBuilder::new(Default::default());
    if let Some((directories, _)) = path.as_str().rsplit_once('/') {
        let mut parent = String::new();
        for component in directories.split('/') {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(component);
            inventory
                .insert(parent.parse().unwrap(), Content::Directory)
                .unwrap();
        }
    }
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )
        .expect("output inventory");
    CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, bytes.to_vec())]))
        .expect("retained output")
}

fn inspectors() -> DisclosureInspectors<'static> {
    DisclosureInspectors {
        markdown: Some(&Comrak),
        ..Default::default()
    }
}

fn rules() -> BTreeMap<Key, String> {
    BTreeMap::from([("rule:private".parse().expect("rule"), "SECRET".into())])
}

#[test]
fn generated_mdx_decodes_escaped_and_joined_literals_with_exact_coverage() {
    let source = concat!(
        "---\ntitle: \"\\u0053ECRET\"\n---\n\n",
        "<pre><code><span children={\"\\u0053E\"} /><span children={\"CRET\"} />",
        "<span children={\" %53E\"} /><span children={\"CRET\"} /></code></pre>\n",
    );
    assert!(!source.contains("SECRET"));
    let output = captured("reference.mdx", source.as_bytes());
    let identity = output.inventory().identity();
    let report = probe_disclosure(&output, &BTreeMap::new(), &rules(), Default::default()).unwrap();
    for representation in [
        DisclosureRepresentation::JsonString,
        DisclosureRepresentation::HtmlText,
        DisclosureRepresentation::PercentDecoded,
    ] {
        assert!(
            report
                .findings()
                .iter()
                .any(|row| row.representation == representation),
            "{representation:?}"
        );
    }
    assert!(matches!(
        report.files()[0].decoding,
        DisclosureDecoding::GeneratedMdxLiteralSyntax {
            profile: gazette_compiler::generated_mdx::PROFILE,
            ..
        }
    ));
    assert_eq!(output.inventory().identity(), identity);
    assert_eq!(
        output.file(&"reference.mdx".parse().unwrap()),
        Some(source.as_bytes())
    );
    assert!(matches!(
        check_disclosure(&output, &BTreeMap::new(), &rules(), Default::default()),
        Err(DisclosureProbeError::Findings { .. })
    ));
}

#[test]
fn generated_mdx_requires_complete_grammar_math_and_shared_budgets() {
    let math = b"---\ntitle: \"Public\"\n---\n\n<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mrow><mi>x</mi></mrow><annotation encoding=\"application/x-tex\"> x&#10; </annotation></semantics></math>\n";
    check_disclosure(
        &captured("formula.mdx", math),
        &BTreeMap::new(),
        &rules(),
        Default::default(),
    )
    .expect("existing finite MathML and its exact annotation remain supported");
    for body in [
        "import X from './x'",
        "<pre>{value}</pre>",
        "<pre><span children={\"x\"} /></p>",
        "<math xmlns=\"wrong\"><mi>x</mi></math>",
    ] {
        let source = format!("---\ntitle: \"Public\"\n---\n\n{body}\n");
        assert!(
            check_disclosure(
                &captured("index.mdx", source.as_bytes()),
                &BTreeMap::new(),
                &rules(),
                Default::default()
            )
            .is_err(),
            "{body}"
        );
    }
    let output = captured(
        "index.mdx",
        b"---\ntitle: \"Public\"\n---\n\n<pre><span children={\"text\"} /></pre>\n",
    );
    for boundary in 0..8 {
        let mut limits = DisclosureProbeLimits::default();
        match boundary {
            0 => limits.markdown.source_bytes = 1,
            1 => limits.markdown.nodes = 1,
            2 => limits.markdown.depth = 1,
            3 => limits.markdown.output_bytes = 1,
            4 => limits.html.decoded_bytes = 1,
            5 => limits.encoded_values = 0,
            6 => limits.encoded_bytes = 1,
            _ => limits.work_bytes = 0,
        }
        assert!(
            matches!(
                check_disclosure(&output, &BTreeMap::new(), &rules(), limits),
                Err(DisclosureProbeError::Limit(_))
            ),
            "boundary {boundary}"
        );
    }
}

#[test]
fn generated_literal_carriage_returns_remain_exact_disclosure_text() {
    for body in [
        "<pre><span children={\"SE\\r\"} /><span children={\"CRET\"} /></pre>",
        "<pre><span children={\"SE\\r\"} />CRET</pre>",
        "<pre>SE<span children={\"\\rCRET\"} /></pre>",
        "<p>SE<code><span children={\"\\r\"} /></code>CRET</p>",
    ] {
        let source = format!("---\ntitle: \"Public\"\n---\n\n{body}\n");
        let rules = BTreeMap::from([("rule:exact".parse().unwrap(), "SE\rCRET".into())]);
        let output = captured("index.mdx", source.as_bytes());
        let report =
            probe_disclosure(&output, &BTreeMap::new(), &rules, Default::default()).unwrap();
        assert!(
            report
                .findings()
                .iter()
                .any(|row| row.representation == DisclosureRepresentation::HtmlText),
            "{body}"
        );
    }
}

#[test]
#[ignore = "requires an explicit retained generated MDX tree"]
fn retained_generated_mdx_tree_has_complete_disclosure_coverage() {
    fn visit(root: &std::path::Path, directory: &std::path::Path, count: &mut usize) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, count);
            } else if path.extension().is_some_and(|extension| extension == "mdx") {
                let bytes = std::fs::read(&path).unwrap();
                let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
                let output = captured(relative, &bytes);
                let absent = BTreeMap::from([(
                    "rule:qualification".parse().unwrap(),
                    "GAZETTE_ABSENT_QUALIFICATION_LITERAL".into(),
                )]);
                let checked =
                    check_disclosure(&output, &BTreeMap::new(), &absent, Default::default())
                        .unwrap_or_else(|error| panic!("{relative}: {error}"));
                assert!(matches!(
                    checked.files()[0].decoding,
                    DisclosureDecoding::GeneratedMdxLiteralSyntax { .. }
                ));
                *count += 1;
            }
        }
    }
    let root = std::path::PathBuf::from(
        std::env::var_os("GAZETTE_TEST_GENERATED_MDX_TREE").expect("retained generated tree"),
    );
    assert!(root.is_absolute() && root.is_dir());
    let mut count = 0;
    visit(&root, &root, &mut count);
    assert!(count > 0);
    eprintln!("Complete generated MDX disclosure coverage: {count} pages");
}

#[test]
fn inert_svg_in_html_and_split_markdown_facts_retains_decoded_disclosure_channels() {
    for path in ["index.html", "index.md"] {
        let source = concat!(
            "Before <svg viewBox='0 0 24 24' aria-label='&#83;ECRET'>",
            "<title>&#83;ECRET</title><path d='M19 12H6m5-6-6 6 6 6'/>",
            "</svg> after [link](https://public.test/%53ECRET).\n",
        );
        assert!(!source.contains("SECRET"));
        let output = captured(path, source.as_bytes());
        let identity = output.inventory().identity();
        let report = probe_disclosure_with_inspectors(
            &output,
            &BTreeMap::new(),
            &rules(),
            inspectors(),
            Default::default(),
        )
        .expect("complete finite SVG inspection");
        for representation in [
            DisclosureRepresentation::HtmlAttribute,
            DisclosureRepresentation::HtmlText,
            DisclosureRepresentation::PercentDecoded,
        ] {
            assert!(
                report
                    .findings()
                    .iter()
                    .any(|finding| { finding.representation == representation }),
                "{path}: {representation:?}: {:?}",
                report.findings()
            );
        }
        assert_eq!(output.inventory().identity(), identity);
        assert_eq!(output.file(&path.parse().unwrap()), Some(source.as_bytes()));
    }
}

#[test]
fn unsafe_or_unfinished_svg_never_receives_partial_output_coverage() {
    for path in ["index.html", "index.md"] {
        for source in [
            "Before <svg><path d='M1 2'/> after\n",
            "Before <svg><title><script>private</script></title></svg>\n",
            "Before <svg><foreignObject><p>private</p></foreignObject></svg>\n",
            "Before <svg><path href='private'/></svg>\n",
            "Before <svg onload='private'/>",
        ] {
            let output = captured(path, source.as_bytes());
            assert!(
                probe_disclosure_with_inspectors(
                    &output,
                    &BTreeMap::new(),
                    &rules(),
                    inspectors(),
                    Default::default(),
                )
                .is_err(),
                "{path}: {source}"
            );
        }
    }
}

#[test]
fn encoded_native_destinations_titles_and_textual_constructs_cannot_hide_forbidden_tokens() {
    for (source, representation) in [
        (
            "[public](https://public.test/%53ECRET)",
            DisclosureRepresentation::PercentDecoded,
        ),
        (
            "![public](https://public.test/&#83;ECRET.png)",
            DisclosureRepresentation::MarkdownDestination,
        ),
        (
            "[public](https://public.test/ \"&#83;ECRET\")",
            DisclosureRepresentation::MarkdownTitle,
        ),
        (
            "![&#83;ECRET](https://public.test/image.png)",
            DisclosureRepresentation::MarkdownText,
        ),
        ("# &#83;ECRET\r\n", DisclosureRepresentation::MarkdownText),
        ("SE**CRET**", DisclosureRepresentation::MarkdownText),
        ("%53E**CRET**", DisclosureRepresentation::PercentDecoded),
        ("`%53ECRET`", DisclosureRepresentation::PercentDecoded),
        ("$%53ECRET$", DisclosureRepresentation::PercentDecoded),
        (
            "# Public {#%53ECRET}\n",
            DisclosureRepresentation::PercentDecoded,
        ),
        (
            "[^%53ECRET]: public unused definition\n",
            DisclosureRepresentation::PercentDecoded,
        ),
        (
            "[[https://public.test/%53ECRET|public]]",
            DisclosureRepresentation::PercentDecoded,
        ),
    ] {
        assert!(
            !source.contains("SECRET"),
            "fixture must require native decoding"
        );
        let output = captured("alternate.html", source.as_bytes());
        let media = BTreeMap::from([("alternate.html".parse().unwrap(), MediaType::Markdown)]);
        let identity = output.inventory().identity();
        let report = probe_disclosure_with_inspectors(
            &output,
            &media,
            &rules(),
            inspectors(),
            Default::default(),
        )
        .expect("complete native diagnostic traversal");
        assert!(
            report
                .findings()
                .iter()
                .any(|finding| finding.representation == representation),
            "{source}: {:?}",
            report.findings()
        );
        assert!(matches!(
            check_disclosure_with_inspectors(
                &output,
                &media,
                &rules(),
                inspectors(),
                Default::default()
            ),
            Err(DisclosureProbeError::Findings { .. })
        ));
        assert_eq!(output.inventory().identity(), identity);
        assert_eq!(
            output.file(&"alternate.html".parse().unwrap()),
            Some(source.as_bytes())
        );
    }
}

#[test]
fn native_html_payloads_use_existing_html_css_javascript_and_json_decoders() {
    for (source, representation) in [
        (
            "<div data-private='%53ECRET'></div>\n",
            DisclosureRepresentation::PercentDecoded,
        ),
        (
            "<script>const value='\\u0053ECRET';</script>\n",
            DisclosureRepresentation::JavascriptString,
        ),
        (
            "<style>.public::before{content:'\\53 ECRET'}</style>\n",
            DisclosureRepresentation::CssString,
        ),
        (
            "<script type='application/ld+json'>{\"value\":\"\\u0053ECRET\"}</script>\n",
            DisclosureRepresentation::JsonString,
        ),
    ] {
        let output = captured("index.md", source.as_bytes());
        let report = probe_disclosure_with_inspectors(
            &output,
            &BTreeMap::new(),
            &rules(),
            inspectors(),
            Default::default(),
        )
        .expect("native Markdown and embedded syntax complete");
        assert!(
            report
                .findings()
                .iter()
                .any(|finding| finding.representation == representation),
            "{source}: {:?}",
            report.findings()
        );
    }
}

#[test]
fn inline_mathml_keeps_native_state_across_tags_and_mixed_markdown_text() {
    let source = concat!(
        "Before <math xmlns='http://www.w3.org/1998/Math/MathML'>",
        "<semantics><mrow><mi>&#83;ECRET</mi></mrow>",
        "<annotation encoding='application/x-tex'>x</annotation></semantics></math>",
        " and [a link](https://example.test/%53ECRET).\n",
    );
    let output = captured("index.md", source.as_bytes());
    let report = probe_disclosure_with_inspectors(
        &output,
        &BTreeMap::new(),
        &rules(),
        inspectors(),
        Default::default(),
    )
    .expect("complete bounded inline MathML and Markdown traversal");
    assert!(
        report
            .findings()
            .iter()
            .any(|row| row.representation == DisclosureRepresentation::HtmlText)
    );
    assert!(
        report
            .findings()
            .iter()
            .any(|row| row.representation == DisclosureRepresentation::PercentDecoded)
    );
    assert_eq!(
        output.file(&"index.md".parse().unwrap()),
        Some(source.as_bytes())
    );
}

#[test]
fn unfinished_or_invalid_embedded_mathml_and_html_never_receive_complete_evidence() {
    for source in [
        "Before <math xmlns='http://www.w3.org/1998/Math/MathML'><mi>x</mi>\n",
        "Before <math xmlns='wrong'><mi>x</mi></math>\n",
        "Before <math xmlns='http://www.w3.org/1998/Math/MathML'><mfrac><mi>x</mi></mfrac></math>\n",
        "<div title='unfinished\n\nFollowing text\n",
        "<script>const value = 'unfinished\n",
    ] {
        assert!(
            probe_disclosure_with_inspectors(
                &captured("index.md", source.as_bytes()),
                &BTreeMap::new(),
                &rules(),
                inspectors(),
                Default::default(),
            )
            .is_err(),
            "{source}"
        );
    }
}

#[test]
fn html_looking_code_stays_text_and_nonemitted_html_stays_inspected() {
    for source in [
        "`<math xmlns='wrong'>` and <span>public</span>\n",
        "```html\n<math xmlns='wrong'>\n```\n\n<span>public</span>\n",
    ] {
        probe_disclosure_with_inspectors(
            &captured("index.md", source.as_bytes()),
            &BTreeMap::new(),
            &rules(),
            inspectors(),
            Default::default(),
        )
        .expect("code is textual evidence, not an active MathML opening");
    }
    let source = "Public.\n\n[^unused]: Before <span data-note='&#83;ECRET'>public</span>\n";
    let output = captured("index.md", source.as_bytes());
    let report = probe_disclosure_with_inspectors(
        &output,
        &BTreeMap::new(),
        &rules(),
        inspectors(),
        Default::default(),
    )
    .expect("unused definition facts remain in native inspection");
    assert!(
        report
            .findings()
            .iter()
            .any(|row| row.representation == DisclosureRepresentation::HtmlAttribute)
    );
}

#[test]
fn markdown_html_facts_share_aggregate_input_token_and_decoded_limits() {
    let source = "Before <span title='123456'>one</span> <span title='123456'>two</span> after.\n";
    let output = captured("index.md", source.as_bytes());
    for html in [
        gazette_compiler::html::Limits {
            input_bytes: 30,
            ..Default::default()
        },
        gazette_compiler::html::Limits {
            tokens: 10,
            ..Default::default()
        },
        gazette_compiler::html::Limits {
            decoded_bytes: 10,
            ..Default::default()
        },
    ] {
        let limits = DisclosureProbeLimits {
            html,
            ..Default::default()
        };
        assert!(matches!(
            probe_disclosure_with_inspectors(
                &output,
                &BTreeMap::new(),
                &rules(),
                inspectors(),
                limits,
            ),
            Err(DisclosureProbeError::Limit(_))
        ));
    }
}

#[test]
fn nested_markdown_data_payloads_reuse_the_selected_parser_and_shared_budgets() {
    let encoded = STANDARD.encode("[public](https://public.test/%53ECRET)");
    let source = format!("[payload](data:text/markdown;base64,{encoded})\n");
    let output = captured("index.md", source.as_bytes());
    let report = probe_disclosure_with_inspectors(
        &output,
        &BTreeMap::new(),
        &rules(),
        inspectors(),
        Default::default(),
    )
    .expect("complete nested native decoding");
    assert!(
        report
            .findings()
            .iter()
            .any(|finding| finding.representation == DisclosureRepresentation::PercentDecoded)
    );
    let report = serde_json::to_value(&report).expect("typed coverage");
    assert!(
        report["embedded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|coverage| {
                coverage["media_type"] == "text/markdown"
                    && coverage["decoding"]["markdown-syntax"]["parser_profile"]
                        == gazette_adapter_markdown::PROFILE
            })
    );
    let limits = DisclosureProbeLimits {
        encoded_depth: 1,
        ..Default::default()
    };
    assert!(matches!(
        probe_disclosure_with_inspectors(&output, &BTreeMap::new(), &rules(), inspectors(), limits),
        Err(DisclosureProbeError::Limit(_))
    ));
}

#[test]
fn markdown_has_exact_native_coverage_and_missing_parser_never_falls_back() {
    for name in ["index.md", "alternate.MARKDOWN"] {
        let output = captured(name, b"# Public\n\nA **retained** argument.\n");
        for check in [false, true] {
            let result = if check {
                check_disclosure(&output, &BTreeMap::new(), &rules(), Default::default())
            } else {
                probe_disclosure(&output, &BTreeMap::new(), &rules(), Default::default())
            };
            assert!(matches!(
                result,
                Err(DisclosureProbeError::Unsupported {
                    kind: "Markdown parser",
                    ..
                })
            ));
        }
        let report = check_disclosure_with_inspectors(
            &output,
            &BTreeMap::new(),
            &rules(),
            inspectors(),
            Default::default(),
        )
        .expect("selected bounded parser required");
        assert!(matches!(
            report.files()[0].decoding,
            DisclosureDecoding::MarkdownSyntax {
                parser_profile: gazette_adapter_markdown::PROFILE,
                ..
            }
        ));
        let schema = serde_json::to_value(schemars::schema_for!(DisclosureDecoding))
            .expect("coverage schema");
        assert!(schema.to_string().contains("markdown-syntax"));
        let report = serde_json::to_value(&report).unwrap();
        assert_eq!(report["scanner"], "gazette-disclosure-probe/16");
        assert_eq!(report["resource_profile"], "gazette-disclosure-resources/2");
        assert_eq!(
            report["files"][0]["decoding"]["markdown-syntax"]["profile"],
            "gazette-disclosure-markdown/2"
        );
    }
}

#[test]
fn invalid_utf8_unlowered_directives_and_unadmitted_html_refuse_complete_evidence() {
    for (bytes, expected) in [
        (b"\xff".as_slice(), "disclosure_probe_encoded"),
        (
            b":::note title='public'\nPublic\n:::\n",
            "disclosure_unsupported",
        ),
        (
            b"<span onclick='public()'>public</span>\n",
            "disclosure_unsupported",
        ),
    ] {
        let output = captured("index.md", bytes);
        let error = check_disclosure_with_inspectors(
            &output,
            &BTreeMap::new(),
            &rules(),
            inspectors(),
            Default::default(),
        )
        .expect_err("no partial supported-profile result");
        assert_eq!(error.code(), expected);
    }
}

#[test]
fn native_parser_joined_text_and_whole_output_decoded_work_are_independently_bounded() {
    let output = captured("index.md", b"> > > Public **bounded** text\n");
    for boundary in 0..8 {
        let mut limits = DisclosureProbeLimits::default();
        match boundary {
            0 => limits.markdown.source_bytes = 1,
            1 => limits.markdown.nodes = 1,
            2 => limits.markdown.depth = 1,
            3 => limits.markdown.fact_bytes = 1,
            4 => limits.markdown.output_bytes = 1,
            5 => limits.encoded_values = 0,
            6 => limits.encoded_bytes = 1,
            _ => limits.work_bytes = 0,
        }
        assert!(
            matches!(
                probe_disclosure_with_inspectors(
                    &output,
                    &BTreeMap::new(),
                    &rules(),
                    inspectors(),
                    limits
                ),
                Err(DisclosureProbeError::Limit(_))
            ),
            "boundary {boundary}"
        );
    }
}

#[test]
fn producer_media_type_selects_markdown_before_a_misleading_filename() {
    let output = captured("model.json", b"# Public\n");
    let media = BTreeMap::from([("model.json".parse().unwrap(), MediaType::Markdown)]);
    let report = check_disclosure_with_inspectors(
        &output,
        &media,
        &rules(),
        inspectors(),
        Default::default(),
    )
    .expect("producer selects native Markdown");
    assert!(matches!(
        report.files()[0].decoding,
        DisclosureDecoding::MarkdownSyntax { .. }
    ));
}

#[test]
fn complete_policy_uses_the_same_explicit_parser_and_records_markdown_resource_limits() {
    let policy = DisclosurePolicy {
        rules: [
            DisclosureClass::LocalPath,
            DisclosureClass::PrivateEndpoint,
            DisclosureClass::Secret,
            DisclosureClass::GeneratedArtifact,
            DisclosureClass::FixtureClaim,
            DisclosureClass::InternalIdentifier,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, class)| {
            (
                format!("rule:owner-{index}").parse().unwrap(),
                DisclosureRule {
                    class,
                    pattern: DisclosurePattern::Literal {
                        value: "SECRET".into(),
                    },
                },
            )
        })
        .collect(),
    };
    let clean = captured("index.md", b"# Public\n\nRetained text.\n");
    let report = check_disclosure_policy_with_inspectors(
        &clean,
        &BTreeMap::new(),
        &policy,
        inspectors(),
        Default::default(),
    )
    .expect("complete selected native policy traversal");
    assert!(report.findings().is_empty());
    assert!(matches!(
        report.files()[0].decoding,
        DisclosureDecoding::MarkdownSyntax { .. }
    ));
    let report = serde_json::to_value(&report).unwrap();
    let limits = DisclosureProbeLimits {
        markdown: gazette_core::markdown::Limits {
            nodes: 9999,
            ..Default::default()
        },
        ..Default::default()
    };
    let changed = check_disclosure_policy_with_inspectors(
        &clean,
        &BTreeMap::new(),
        &policy,
        inspectors(),
        limits,
    )
    .expect("different bounded selected profile");
    let changed = serde_json::to_value(&changed).unwrap();
    assert_ne!(
        report["resource_limits_sha256"],
        changed["resource_limits_sha256"]
    );
    let hidden = captured("index.md", b"[public](https://public.test/%53ECRET)\n");
    assert!(matches!(
        check_disclosure_policy_with_inspectors(
            &hidden,
            &BTreeMap::new(),
            &policy,
            inspectors(),
            Default::default()
        ),
        Err(DisclosureProbeError::Findings { .. })
    ));
}
