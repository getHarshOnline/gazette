//! Partial diagnostics against actual rendered bytes, never public certification.

use super::{Fixture, OTHER, PAGE, THEME_MANIFEST, acquire, acquisition_limits, configuration};
use gazette_sdk::{build, source, workspace::Key};
use serde_json::json;
use std::{collections::BTreeMap, fs};

const BINARY: &[u8] = b"\x89PNG\r\n\x1a\n\xffBINARY-CANARY\0";

fn inputs(graph_template: &[u8]) -> (Fixture, Fixture, Fixture, build::AcquiredInputs) {
    let page = PAGE.replace(
        "title='One'",
        "title='Model \"private\" token'\nroute={path='/page/'}",
    );
    let other_page = OTHER.replace("title='Other'", "title='Other'\nroute={path='/other/'}");
    let one = Fixture::new(&[("page.md", page.as_bytes()), ("PATH-CANARY.png", BINARY)]);
    let other = Fixture::new(&[("page.md", other_page.as_bytes())]);
    let manifest = THEME_MANIFEST.replace(
        "render={page='page.html'}",
        "render={page='page.html',graph='probe.txt'}",
    );
    let theme = Fixture::new(&[
        ("theme.toml", manifest.as_bytes()),
        (
            "templates/page.html",
            b"<p>TEMPLATE-CANARY</p>{{ body_html | safe }}",
        ),
        ("templates/probe.txt", graph_template),
    ]);
    let mut config = configuration(&one, &other)
        .replace("source='source:one'\nmanifest='theme.toml'", "source='source:theme'\nmanifest='theme.toml'")
        .replace("site='site:one'\nauthored=[{source='source:one',target='one'}]",
            "site='site:one'\npublic_base_url='https://one.example/'\nauthored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]")
        .replace("site='site:other'\nauthored=", "site='site:other'\npublic_base_url='https://other.example/'\nauthored=");
    config.push_str(&theme.declaration("source:theme"));
    let acquired =
        acquire(&config, acquisition_limits()).expect("real isolated publication capture");
    (one, other, theme, acquired)
}

fn render<'a>(
    inputs: &'a build::AcquiredInputs,
    graph_output: &str,
) -> build::PublicationRender<'a> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(
                &"model.output-data"
                    .parse()
                    .expect("known model without JSON suffix"),
            ),
            graph: Some(build::GraphRenderOptions {
                output: &graph_output.parse().expect("explicit graph output"),
                limits: Default::default(),
            }),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("actual retained rendered site")
}

fn rules(values: &[(&str, &str)]) -> BTreeMap<Key, String> {
    values
        .iter()
        .map(|(id, literal)| (id.parse().expect("rule key"), (*literal).to_owned()))
        .collect()
}

fn has_finding(
    probe: &build::DisclosureProbe<'_>,
    path: &str,
    rule: &str,
    representation: &str,
) -> bool {
    serde_json::to_value(probe.findings())
        .expect("findings")
        .as_array()
        .expect("finding rows")
        .contains(&json!({"path":path,"rule":rule,"representation":representation}))
}

fn assert_same_evidence_under_different_resource_limits(
    mut actual: serde_json::Value,
    mut expected: serde_json::Value,
) {
    let actual_digest = actual
        .as_object_mut()
        .unwrap()
        .remove("resource_limits_sha256")
        .unwrap();
    let expected_digest = expected
        .as_object_mut()
        .unwrap()
        .remove("resource_limits_sha256")
        .unwrap();
    assert_ne!(actual_digest, expected_digest);
    assert_eq!(actual, expected);
}

fn retained_output(path: &str, bytes: &[u8]) -> (tempfile::TempDir, source::CapturedSource) {
    let directory = tempfile::tempdir().expect("disposable retained output");
    fs::write(directory.path().join(path), bytes).expect("exact native output bytes");
    let inventory = source::inspect(directory.path(), None, Default::default())
        .expect("native output inventory");
    let captured =
        source::capture_pinned(directory.path(), inventory.identity(), Default::default())
            .expect("complete independently pinned output");
    assert_eq!(
        captured.file(&path.parse().expect("output path")),
        Some(bytes)
    );
    (directory, captured)
}

#[test]
fn probe_reads_actual_theme_model_paths_and_binary_bytes_without_rewriting_output() {
    let (one, _other, theme, acquired) = inputs(b"{}");
    let rendered = render(&acquired, "graph.json");
    let site = rendered.site();
    let rules = rules(&[
        ("rule:template", "TEMPLATE-CANARY"),
        ("rule:model", "Model \"private\" token"),
        ("rule:binary", "BINARY-CANARY"),
        ("rule:path", "PATH-CANARY"),
    ]);
    let probe = site
        .probe_disclosure(&rules, Default::default())
        .expect("partial output probe");
    assert!(std::ptr::eq(probe.output(), site.output()));
    let report = serde_json::to_value(&probe).expect("private diagnostic report");
    assert_eq!(report["status"], "partial-evidence");
    assert_eq!(
        report["output_identity"],
        json!(site.output().inventory().identity())
    );
    assert_eq!(
        report["rules_sha256"],
        json!(source::Sha256Digest::of(
            &serde_json::to_vec(&rules).expect("exact sorted rules")
        ))
    );
    for forbidden in ["passed", "clean", "approved", "certified"] {
        assert!(report.get(forbidden).is_none());
    }
    assert!(has_finding(
        &probe,
        "page/index.html",
        "rule:template",
        "raw"
    ));
    assert!(has_finding(
        &probe,
        "model.output-data",
        "rule:model",
        "json-string"
    ));
    assert!(
        !has_finding(&probe, "model.output-data", "rule:model", "raw"),
        "escaped model spelling requires native known-output JSON decoding"
    );
    assert!(has_finding(
        &probe,
        "media/PATH-CANARY.png",
        "rule:binary",
        "raw"
    ));
    assert!(has_finding(
        &probe,
        "media/PATH-CANARY.png",
        "rule:path",
        "path"
    ));
    let files = serde_json::to_value(probe.files()).expect("per-file coverage");
    let files = files.as_array().expect("files");
    assert_eq!(
        files.len(),
        site.output()
            .inventory()
            .entries()
            .iter()
            .filter(|entry| matches!(entry.content, source::Content::File { .. }))
            .count()
    );
    for file in files {
        let path = file["path"].as_str().expect("exact path");
        let bytes = site
            .output()
            .file(&path.parse().expect("path"))
            .expect("retained file");
        assert_eq!(file["sha256"], json!(source::Sha256Digest::of(bytes)));
        assert_eq!(
            file["decoding"],
            if path == "model.output-data" {
                json!("json-strings")
            } else if path.ends_with(".html") || path == "graph.json" {
                json!({"html-tokens":{
                    "profile":gazette_sdk::html::PROFILE,
                "css_profile":gazette_sdk::css::PROFILE,
                "javascript_profile":gazette_sdk::javascript::PROFILE
                }})
            } else {
                json!("none")
            }
        );
    }
    let serialized = serde_json::to_string(&probe).expect("private findings");
    assert!(!serialized.contains("TEMPLATE-CANARY"));
    assert!(!serialized.contains("BINARY-CANARY"));
    assert!(
        serialized.contains("PATH-CANARY.png"),
        "a finding path may itself contain private material; this report is not fully redacted"
    );
    let before = site.output().inventory().identity();
    let payloads: BTreeMap<_, _> = site
        .output()
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            site.output()
                .file(&entry.path)
                .map(|bytes| (entry.path.clone(), bytes.to_vec()))
        })
        .collect();
    fs::write(
        one.directory.path().join("selected/page.md"),
        b"changed after retained rendering",
    )
    .expect("isolated input edit");
    fs::write(
        theme.directory.path().join("selected/templates/page.html"),
        b"changed template",
    )
    .expect("isolated template edit");
    let reversed: BTreeMap<_, _> = rules
        .iter()
        .rev()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    assert_eq!(
        serde_json::to_value(
            site.probe_disclosure(&reversed, Default::default())
                .expect("same exact retained output")
        )
        .expect("repeat report"),
        report
    );
    assert_eq!(site.output().inventory().identity(), before);
    for (path, bytes) in payloads {
        assert_eq!(
            site.output()
                .file(&path)
                .expect("unmodified retained bytes"),
            bytes
        );
    }
}

#[test]
fn json_decoder_sees_encoded_duplicate_keys_values_and_root_strings() {
    let rules = rules(&[("rule:json", "JSON-CANARY"), ("rule:key", "KEY-CANARY")]);
    for (template, expected_rule) in [
        (
            br#"{"repeat":"JSON\u002dCANARY","repeat":"safe"}"#.as_slice(),
            "rule:json",
        ),
        (
            br#"{"KEY\u002dCANARY":"first","KEY\u002dCANARY":"second"}"#.as_slice(),
            "rule:key",
        ),
        (br#""JSON\u002dCANARY""#.as_slice(), "rule:json"),
        (
            br#"[null,true,12,{"nested":["JSON\u002dCANARY"]}]"#.as_slice(),
            "rule:json",
        ),
    ] {
        let (_directory, captured) = retained_output("graph.json", template);
        let graph_path = "graph.json".parse().expect("graph output");
        assert_eq!(
            captured
                .file(&graph_path)
                .expect("actual captured output bytes"),
            template
        );
        let probe =
            build::probe_disclosure(&captured, &BTreeMap::new(), &rules, Default::default())
                .expect("decoded strings observed without collapsing duplicate members");
        assert!(has_finding(
            &probe,
            "graph.json",
            expected_rule,
            "json-string"
        ));
        assert!(!has_finding(&probe, "graph.json", expected_rule, "raw"));
    }
}

#[test]
fn malformed_candidate_json_and_native_depth_exhaustion_return_no_report() {
    let deep = format!("{}0{}", "[".repeat(140), "]".repeat(140));
    let rules = rules(&[("rule:canary", "JSON-CANARY")]);
    for bytes in [
        br#"{"first":"JSON\u002dCANARY","late":"#.as_slice(),
        br#"{"valid":true} false"#.as_slice(),
        br#""\uD800""#.as_slice(),
        deep.as_bytes(),
    ] {
        let (_directory, captured) = retained_output("graph.json", bytes);
        let before = captured.inventory().identity();
        let error =
            build::probe_disclosure(&captured, &BTreeMap::new(), &rules, Default::default())
                .expect_err("malformed structured candidate cannot produce partial success");
        assert_eq!(error.code(), "disclosure_probe_json");
        assert!(!error.to_string().contains("JSON-CANARY"));
        assert_eq!(captured.inventory().identity(), before);
        assert_eq!(
            captured
                .file(&"graph.json".parse().expect("path"))
                .expect("original malformed bytes"),
            bytes
        );
    }
}

#[test]
fn unsupported_encoding_remains_explicitly_partial_even_when_no_literal_is_found() {
    let (directory, captured) =
        retained_output("opaque.payload", b"{unknown-format:\"JSON-CANARY\"");
    let rules = rules(&[("rule:absent", "NOT-PRESENT-IN-OUTPUT")]);
    let probe = build::probe_disclosure(&captured, &BTreeMap::new(), &rules, Default::default())
        .expect("raw-only evidence is permitted, never certification");
    assert!(probe.findings().is_empty());
    let report = serde_json::to_value(&probe).expect("partial report");
    assert_eq!(report["status"], "partial-evidence");
    assert!(
        report["files"]
            .as_array()
            .expect("coverage")
            .iter()
            .any(|file| file["path"] == "opaque.payload" && file["decoding"] == "none")
    );
    assert_eq!(
        fs::read(directory.path().join("opaque.payload")).expect("unchanged unknown output"),
        b"{unknown-format:\"JSON-CANARY\""
    );
    let (_binary_directory, binary) = retained_output("PATH-CANARY.payload", BINARY);
    let binary_report = serde_json::to_value(
        build::probe_disclosure(&binary, &BTreeMap::new(), &rules, Default::default())
            .expect("binary has raw-only coverage, not container qualification"),
    )
    .expect("binary coverage report");
    assert_eq!(binary_report["files"][0]["decoding"], "none");
    assert_eq!(
        binary.file(&"PATH-CANARY.payload".parse().expect("binary path")),
        Some(BINARY)
    );
    let (_png_directory, png) = retained_output("PATH-CANARY.png", BINARY);
    let before = png.inventory().identity();
    let error = build::probe_disclosure(&png, &BTreeMap::new(), &rules, Default::default())
        .expect_err("selected native PNG inspection refuses malformed containers before a report");
    assert_eq!(error.code(), "disclosure_probe_raster");
    assert!(matches!(
        error,
        build::DisclosureProbeError::Raster { path, reason }
            if path.as_str() == "PATH-CANARY.png" && reason == "container is malformed"
    ));
    assert_eq!(png.inventory().identity(), before);
    assert_eq!(
        png.file(&"PATH-CANARY.png".parse().expect("PNG path")),
        Some(BINARY)
    );
}

#[test]
fn probe_rule_entry_work_and_finding_budgets_fail_without_partial_report() {
    let (_one, _other, _theme, acquired) = inputs(b"{}");
    let rendered = render(&acquired, "graph.json");
    let site = rendered.site();
    for empty in [BTreeMap::new(), rules(&[("rule:empty", "")])] {
        assert_eq!(
            site.probe_disclosure(&empty, Default::default())
                .expect_err("no empty rule bypass")
                .code(),
            "disclosure_probe_rules"
        );
    }
    let rules = rules(&[
        ("rule:template", "TEMPLATE-CANARY"),
        ("rule:model", "Model \"private\" token"),
    ]);
    let baseline = site
        .probe_disclosure(&rules, Default::default())
        .expect("baseline evidence");
    let report = serde_json::to_value(&baseline).expect("baseline work");
    let exact = build::DisclosureProbeLimits {
        rules: rules.len(),
        entries: site.output().inventory().entries().len(),
        rules_bytes: serde_json::to_vec(&rules).expect("encoded rules").len(),
        work_bytes: report["work_bytes"].as_u64().expect("charged work"),
        findings: baseline.findings().len(),
        html: Default::default(),
        xml: Default::default(),
        css: Default::default(),
        javascript: Default::default(),
        ..Default::default()
    };
    assert_same_evidence_under_different_resource_limits(
        serde_json::to_value(
            site.probe_disclosure(&rules, exact)
                .expect("all exact ceilings accepted"),
        )
        .expect("exact report"),
        report,
    );
    for smaller in [
        build::DisclosureProbeLimits {
            rules: exact.rules - 1,
            ..exact
        },
        build::DisclosureProbeLimits {
            entries: exact.entries - 1,
            ..exact
        },
        build::DisclosureProbeLimits {
            rules_bytes: exact.rules_bytes - 1,
            ..exact
        },
        build::DisclosureProbeLimits {
            work_bytes: exact.work_bytes - 1,
            ..exact
        },
        build::DisclosureProbeLimits {
            findings: exact.findings - 1,
            ..exact
        },
    ] {
        assert_eq!(
            site.probe_disclosure(&rules, smaller)
                .expect_err("no truncated result")
                .code(),
            "disclosure_probe_limit"
        );
    }
}

#[test]
fn html_entities_attributes_and_chunked_text_share_retained_output_probe() {
    // Put the character-reference opener at a native 256-byte feed boundary.
    // Detection must survive tokenizer fragments without joining separate elements.
    let template = format!(
        "<p>{}CHUNK&#45;CANARY</p><p>JOIN&#x45;&#x44;-CANARY</p><a data-note=\"ATTR&#45;CANARY\">link</a><title>TITLE&#45;CANARY</title><textarea>AREA&#45;CANARY</textarea>",
        "a".repeat(247)
    );
    let rules = rules(&[
        ("rule:chunk", "CHUNK-CANARY"),
        ("rule:adjacent", "JOINED-CANARY"),
        ("rule:attribute", "ATTR-CANARY"),
        ("rule:title", "TITLE-CANARY"),
        ("rule:textarea", "AREA-CANARY"),
    ]);
    for output in ["graph.html", "graph.HTM"] {
        let (_one, _other, _theme, acquired) = inputs(template.as_bytes());
        let rendered = render(&acquired, output);
        let tree = rendered.site().output();
        let identity = tree.inventory().identity();
        let probe = rendered
            .site()
            .probe_disclosure(&rules, Default::default())
            .expect("entity-aware HTML token evidence");
        for (rule, representation) in [
            ("rule:chunk", "html-text"),
            ("rule:adjacent", "html-text"),
            ("rule:attribute", "html-attribute"),
            ("rule:title", "html-text"),
            ("rule:textarea", "html-text"),
        ] {
            assert!(has_finding(&probe, output, rule, representation));
            assert!(!has_finding(&probe, output, rule, "raw"));
        }
        let report = serde_json::to_value(&probe).expect("partial HTML report");
        assert_eq!(report["status"], "partial-evidence");
        assert!(
            report["files"]
                .as_array()
                .expect("coverage")
                .iter()
                .any(|file| file["path"] == output
                    && file["decoding"]
                        == json!({"html-tokens":{
                            "profile":gazette_sdk::html::PROFILE,
                            "css_profile":gazette_sdk::css::PROFILE,
                            "javascript_profile":gazette_sdk::javascript::PROFILE
                        }}))
        );
        assert_eq!(tree.inventory().identity(), identity);
        assert_eq!(
            tree.file(&output.parse().expect("output path"))
                .expect("exact rendered HTML"),
            template.as_bytes()
        );
    }
}

#[test]
fn inline_json_uses_json_decoder_while_scripts_and_styles_keep_raw_context() {
    let template = br#"<script type=" Application/LD+JSON ">{"duplicate":"JSON\u002dCANARY","duplicate":"safe","KEY\u002dCANARY":true}</script><script>const a = "SCRIPT&#45;CANARY"; const b = "SCRIPT-RAW";</script><style>.x::before {content:"STYLE&#45;CANARY STYLE-RAW"}</style>"#;
    let (_one, _other, _theme, acquired) = inputs(template);
    let rendered = render(&acquired, "graph.html");
    let rules = rules(&[
        ("rule:json", "JSON-CANARY"),
        ("rule:key", "KEY-CANARY"),
        ("rule:script", "SCRIPT-RAW"),
        ("rule:style", "STYLE-RAW"),
        ("rule:script-literal", "SCRIPT&#45;CANARY"),
        ("rule:style-literal", "STYLE&#45;CANARY"),
        ("rule:not-script-decoded", "SCRIPT-CANARY"),
        ("rule:not-style-decoded", "STYLE-CANARY"),
    ]);
    let probe = rendered
        .site()
        .probe_disclosure(&rules, Default::default())
        .expect("independent HTML/script/style/JSON channels");
    for rule in ["rule:json", "rule:key"] {
        assert!(has_finding(&probe, "graph.html", rule, "json-string"));
        assert!(!has_finding(&probe, "graph.html", rule, "raw"));
    }
    for (rule, representation) in [
        ("rule:script", "html-script"),
        ("rule:script-literal", "html-script"),
        ("rule:style", "html-style"),
        ("rule:style-literal", "html-style"),
    ] {
        assert!(has_finding(&probe, "graph.html", rule, representation));
        assert!(!has_finding(&probe, "graph.html", rule, "html-text"));
    }
    assert!(!probe.findings().iter().any(|finding| matches!(
        finding.rule.as_str(),
        "rule:not-script-decoded" | "rule:not-style-decoded"
    )));
    assert_eq!(
        rendered
            .site()
            .output()
            .file(&"graph.html".parse().expect("output"))
            .expect("unchanged output"),
        template
    );
}

#[test]
fn malformed_inline_json_and_unsupported_html_never_return_partial_findings() {
    for (template, code) in [
        (
            "<p>EARLY-CANARY</p><script type='application/json'></script>",
            "disclosure_probe_json",
        ),
        (
            "<script type='application/ld+json'>   </script>",
            "disclosure_probe_json",
        ),
        (
            "<script type='application/json'>{\"first\":\"EARLY-CANARY\",\"late\":</script>",
            "disclosure_probe_json",
        ),
        (
            "<script type='application/json'>{} true</script>",
            "disclosure_probe_json",
        ),
        (
            "<p>EARLY-CANARY</p><a href='one' href='two'>duplicate</a>",
            "disclosure_probe_html",
        ),
        (
            "<p>EARLY-CANARY</p><svg><text>foreign</text></svg>",
            "disclosure_probe_html",
        ),
        ("<script>EARLY-CANARY", "disclosure_probe_html"),
    ] {
        let (_one, _other, _theme, acquired) = inputs(template.as_bytes());
        let rendered = render(&acquired, "graph.html");
        let identity = rendered.site().output().inventory().identity();
        let rules = rules(&[("rule:early", "EARLY-CANARY")]);
        let error = rendered
            .site()
            .probe_disclosure(&rules, Default::default())
            .expect_err("later decode failure discards all speculative findings");
        assert_eq!(error.code(), code, "{template}");
        assert!(!error.to_string().contains("EARLY-CANARY"));
        assert_eq!(rendered.site().output().inventory().identity(), identity);
        assert_eq!(
            rendered
                .site()
                .output()
                .file(&"graph.html".parse().expect("path"))
                .expect("unmodified rejected output"),
            template.as_bytes()
        );
    }
}

#[test]
fn html_tokenizer_budgets_apply_to_real_retained_html() {
    let template = format!("<p data-value='{}'>BOUND&#45;CANARY</p>", "a".repeat(1024));
    let (_one, _other, _theme, acquired) = inputs(template.as_bytes());
    let rendered = render(&acquired, "graph.html");
    let rules = rules(&[("rule:bounded", "BOUND-CANARY")]);
    let baseline = rendered
        .site()
        .probe_disclosure(&rules, Default::default())
        .expect("default bounded HTML profile");
    assert!(has_finding(
        &baseline,
        "graph.html",
        "rule:bounded",
        "html-text"
    ));
    let mut exact = build::DisclosureProbeLimits::default();
    exact.html.input_bytes = template.len();
    assert_same_evidence_under_different_resource_limits(
        serde_json::to_value(
            rendered
                .site()
                .probe_disclosure(&rules, exact)
                .expect("exact HTML input bytes"),
        )
        .expect("exact report"),
        serde_json::to_value(&baseline).expect("baseline report"),
    );
    for html in [
        gazette_sdk::html::Limits {
            input_bytes: template.len() - 1,
            ..Default::default()
        },
        gazette_sdk::html::Limits {
            unemitted_bytes: 256,
            ..Default::default()
        },
        gazette_sdk::html::Limits {
            tokens: 1,
            ..Default::default()
        },
        gazette_sdk::html::Limits {
            decoded_bytes: 1,
            ..Default::default()
        },
    ] {
        let error = rendered
            .site()
            .probe_disclosure(
                &rules,
                build::DisclosureProbeLimits {
                    html,
                    ..Default::default()
                },
            )
            .expect_err("no partial HTML evidence on a native bound");
        assert_eq!(error.code(), "disclosure_probe_limit");
    }
}

#[test]
fn invalid_utf8_template_cannot_manufacture_a_rendered_html_candidate() {
    // Real rendering rejects these bytes before a RenderedSite exists. Direct
    // tokenizer UTF-8 rejection belongs to the compiler's native HTML tests.
    let (_one, _other, _theme, acquired) = inputs(b"<p>\xff</p>");
    let error = build::render_publication(
        &acquired,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.output-data".parse().expect("model")),
            graph: Some(build::GraphRenderOptions {
                output: &"graph.html".parse().expect("HTML output"),
                limits: Default::default(),
            }),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect_err("native template UTF-8 validation precedes disclosure probing");
    assert!(matches!(error, build::PublicationRenderError::Template(_)));
}

#[test]
fn retained_xml_suffixes_share_decoded_channels_without_claiming_embedded_schema_safety() {
    let template = br#"<?xml version="1.0" encoding="UTF-8"?><root xmlns:unused="urn:NS&#45;CANARY" note="ATTR&#45;CANARY"><text>TEXT&#45;CANARY</text><joined>ADJ<![CDATA[ACENT]]>&#45;CANARY</joined><!--COMMENT&#45;CANARY--><?PI-CANARY PI&#45;DATA?></root>"#;
    let rules = rules(&[
        ("rule:text", "TEXT-CANARY"),
        ("rule:adjacent", "ADJACENT-CANARY"),
        ("rule:attribute", "ATTR-CANARY"),
        ("rule:namespace", "urn:NS-CANARY"),
        ("rule:comment", "COMMENT&#45;CANARY"),
        ("rule:pi-target", "PI-CANARY"),
        ("rule:pi-data", "PI&#45;DATA"),
        ("rule:not-comment-decoded", "COMMENT-CANARY"),
        ("rule:not-pi-decoded", "PI-DATA"),
    ]);
    // All four are XML candidates; a suffix is not proof of feed or SVG schema
    // conformance, embedded HTML/JavaScript safety, or public disclosure approval.
    for output in ["graph.XML", "graph.RsS", "graph.atom", "graph.SVG"] {
        let (directory, captured) = retained_output(output, template);
        let tree = &captured;
        let before = tree.inventory().identity();
        let probe = build::probe_disclosure(tree, &BTreeMap::new(), &rules, Default::default())
            .expect("complete partial XML decoding");
        assert!(std::ptr::eq(probe.output(), tree));
        for (rule, representation) in [
            ("rule:text", "xml-text"),
            ("rule:adjacent", "xml-text"),
            ("rule:attribute", "xml-attribute"),
            ("rule:namespace", "xml-namespace"),
            ("rule:comment", "xml-comment"),
            ("rule:pi-target", "xml-processing-instruction"),
            ("rule:pi-data", "xml-processing-instruction"),
        ] {
            assert!(
                has_finding(&probe, output, rule, representation),
                "{output}: {rule}"
            );
        }
        for rule in [
            "rule:text",
            "rule:adjacent",
            "rule:attribute",
            "rule:namespace",
        ] {
            assert!(
                !has_finding(&probe, output, rule, "raw"),
                "encoded/split bytes need native XML decoding"
            );
        }
        assert!(!probe.findings().iter().any(|finding| matches!(
            finding.rule.as_str(),
            "rule:not-comment-decoded" | "rule:not-pi-decoded"
        )));
        let report = serde_json::to_value(&probe).expect("XML report");
        assert_eq!(report["status"], "partial-evidence");
        assert_eq!(report["scanner"], "gazette-disclosure-probe/16");
        assert_eq!(report["output_identity"], json!(before));
        assert!(
            report["files"]
                .as_array()
                .expect("file coverage")
                .iter()
                .any(|file| file["path"] == output
                    && file["sha256"] == json!(source::Sha256Digest::of(template))
                    && file["decoding"]
                        == json!({"xml-events":{"profile":gazette_sdk::xml::PROFILE,"css_profile":gazette_sdk::css::PROFILE}}))
        );
        fs::write(
            directory.path().join(output),
            b"changed native XML after capture",
        )
        .expect("isolated output edit");
        assert_eq!(
            serde_json::to_value(
                build::probe_disclosure(tree, &BTreeMap::new(), &rules, Default::default())
                    .expect("same retained XML bytes")
            )
            .expect("deterministic report"),
            report
        );
        assert_eq!(tree.inventory().identity(), before);
        assert_eq!(
            tree.file(&output.parse().expect("XML output path"))
                .expect("unchanged original XML"),
            template
        );
    }
}

#[test]
fn late_xml_syntax_namespace_and_dtd_failures_discard_all_probe_findings() {
    let rules = rules(&[("rule:early", "EARLY-CANARY")]);
    for template in [
        "<root>EARLY-CANARY<late></root>",
        "<root>EARLY-CANARY<late xmlns:p='urn:one' xmlns:p='urn:two'/></root>",
        "<root>EARLY-CANARY<late xmlns:p='urn:same' xmlns:q='urn:same' p:value='one' q:value='two'/></root>",
        "<!DOCTYPE root [<!ENTITY hidden 'EARLY-CANARY'>]><root>&hidden;</root>",
        "<!DOCTYPE root SYSTEM 'urn:unread-external-fixture'><root>EARLY-CANARY</root>",
    ] {
        let (_directory, captured) = retained_output("graph.XML", template.as_bytes());
        let tree = &captured;
        let before = tree.inventory().identity();
        let error = build::probe_disclosure(tree, &BTreeMap::new(), &rules, Default::default())
            .expect_err(
                "malformed or unsupported XML must not yield an incomplete successful report",
            );
        assert_eq!(error.code(), "disclosure_probe_xml");
        assert!(!error.to_string().contains("EARLY-CANARY"));
        assert!(!error.to_string().contains("urn:unread-external-fixture"));
        assert_eq!(tree.inventory().identity(), before);
        assert_eq!(
            tree.file(&"graph.XML".parse().expect("path"))
                .expect("original rejected XML"),
            template.as_bytes()
        );
    }
}

#[test]
fn xml_input_depth_and_decoded_limits_bound_real_output_without_truncation() {
    let template = b"<root><child>BOUND&#45;CANARY</child></root>";
    let (_directory, captured) = retained_output("graph.xml", template);
    let rules = rules(&[("rule:bound", "BOUND-CANARY")]);
    let baseline = build::probe_disclosure(&captured, &BTreeMap::new(), &rules, Default::default())
        .expect("default XML profile");
    assert!(has_finding(
        &baseline,
        "graph.xml",
        "rule:bound",
        "xml-text"
    ));
    let exact = build::DisclosureProbeLimits {
        xml: gazette_sdk::xml::Limits {
            input_bytes: template.len(),
            depth: 2,
            decoded_bytes: "BOUND-CANARY".len(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_same_evidence_under_different_resource_limits(
        serde_json::to_value(
            build::probe_disclosure(&captured, &BTreeMap::new(), &rules, exact)
                .expect("exact original bytes, depth, and decoded text budgets"),
        )
        .expect("exact report"),
        serde_json::to_value(&baseline).expect("baseline report"),
    );
    for xml in [
        gazette_sdk::xml::Limits {
            input_bytes: template.len() - 1,
            ..exact.xml
        },
        gazette_sdk::xml::Limits {
            depth: 1,
            ..exact.xml
        },
        gazette_sdk::xml::Limits {
            decoded_bytes: "BOUND-CANARY".len() - 1,
            ..exact.xml
        },
    ] {
        let error = build::probe_disclosure(
            &captured,
            &BTreeMap::new(),
            &rules,
            build::DisclosureProbeLimits { xml, ..exact },
        )
        .expect_err("no partial output evidence at XML boundaries");
        assert_eq!(error.code(), "disclosure_probe_limit");
    }
    assert_eq!(
        captured
            .file(&"graph.xml".parse().expect("path"))
            .expect("unchanged XML bytes"),
        template
    );
}
