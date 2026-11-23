//! Required decoder/literal checks share retained diagnostic evidence, not certification.

use super::*;

fn absent_rules() -> BTreeMap<Key, String> {
    rules(&[("rule:private-check", "ABSENT-PRIVATE-NEEDLE-483920")])
}

fn assert_unsupported(error: build::DisclosureProbeError, expected_path: &str) {
    assert_eq!(error.code(), "disclosure_unsupported");
    let display = error.to_string();
    for private in [
        expected_path,
        "application/x-private",
        "OPAQUE-PRIVATE-PAYLOAD",
    ] {
        assert!(
            !display.contains(private),
            "unsupported diagnostic disclosed private input: {display}"
        );
    }
    let build::DisclosureProbeError::Unsupported { path, kind } = error else {
        panic!("required unknown representation must preserve typed unsupported evidence");
    };
    assert_eq!(path.as_str(), expected_path);
    assert!(!kind.is_empty(), "static unsupported category is explicit");
}

#[test]
fn required_check_uses_complete_producer_declared_rendering_with_required_profile_evidence() {
    for route in ["/declared", "/declared.json"] {
        let page = super::super::PAGE.replace("path='/guide/'", &format!("path='{route}'"));
        let template = b"<p>Supported retained HTML</p>";
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("theme/templates/page.html", template),
        ]);
        let inputs = acquire(&fixture.config());
        let rendered = render(&inputs).expect("native HTML and machine publication outputs");
        let tree = rendered.site().output();
        let path: source::EntryPath = route
            .trim_start_matches('/')
            .parse()
            .expect("emitted route");
        assert_eq!(
            rendered.site().media_types()[&path],
            gazette_sdk::render::MediaType::Html
        );
        assert_eq!(tree.file(&path), Some(template.as_slice()));
        let rules = absent_rules();
        let diagnostic = rendered
            .site()
            .probe_disclosure(&rules, Default::default())
            .expect("diagnostic traversal");
        let required = rendered
            .site()
            .check_disclosure(&rules, Default::default())
            .expect("complete supported profile, no literal findings");
        assert!(std::ptr::eq(required.output(), tree));
        let required_value = serde_json::to_value(&required).expect("required evidence");
        let diagnostic_value =
            serde_json::to_value(&diagnostic).expect("same traversal, diagnostic state");
        assert_eq!(
            required_value["status"],
            "required-supported-profile-evidence"
        );
        assert_eq!(diagnostic_value["status"], "partial-evidence");
        let mut actual = required_value.clone();
        let mut diagnostic = diagnostic_value;
        actual
            .as_object_mut()
            .expect("required report")
            .remove("status");
        diagnostic
            .as_object_mut()
            .expect("diagnostic report")
            .remove("status");
        assert_eq!(actual, diagnostic);
        assert_eq!(actual["findings"], json!([]));
        for unsupported_claim in ["certified", "approved", "admitted", "release"] {
            assert!(actual.get(unsupported_claim).is_none());
        }
        fs::write(
            fixture.directory.path().join("theme/templates/page.html"),
            b"changed native template",
        )
        .expect("disposable edit after rendering");
        let repeated = build::check_disclosure(
            tree,
            rendered.site().media_types(),
            &rules,
            Default::default(),
        )
        .expect("same retained bytes through the SDK function");
        assert_eq!(
            serde_json::to_value(repeated).expect("frozen repeated evidence"),
            required_value
        );
        assert_eq!(tree.file(&path), Some(template.as_slice()));
    }
}

#[test]
fn required_check_rejects_unknown_files_even_when_diagnostic_has_no_findings() {
    for (path, bytes) in [
        ("private.unknown", b"OPAQUE-PRIVATE-PAYLOAD".as_slice()),
        ("private-extensionless", b"otherwise plain text".as_slice()),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let rules = absent_rules();
        let diagnostic = fixture
            .probe_disclosure(&rules, Default::default())
            .expect("raw diagnostic remains available");
        assert!(diagnostic.findings().is_empty());
        assert!(matches!(
            diagnostic.files()[0].decoding,
            build::DisclosureDecoding::None
        ));
        assert_unsupported(
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            )
            .expect_err("zero literal matches do not qualify an unknown format"),
            path,
        );
        assert_eq!(
            fs::read(fixture.directory.path().join(path)).expect("unchanged native output"),
            bytes
        );
        assert_eq!(
            fixture.output().file(&path.parse().expect("retained path")),
            Some(bytes)
        );
    }
}

#[test]
fn required_check_rejects_unknown_data_at_nested_html_and_json_boundaries() {
    for (path, bytes) in [
        (
            "private.html",
            br#"<i data-value="data:application/x-private,OPAQUE-PRIVATE-PAYLOAD"></i>"#.as_slice(),
        ),
        (
            "private.json",
            br#"{"value":"data:application/x-private,OPAQUE-PRIVATE-PAYLOAD"}"#.as_slice(),
        ),
        (
            "private.json",
            br#"{"value":"data:text/plain,data:application/x-private,OPAQUE-PRIVATE-PAYLOAD"}"#
                .as_slice(),
        ),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let rules = absent_rules();
        let diagnostic = fixture
            .probe_disclosure(&rules, Default::default())
            .expect("diagnostic records unqualified embedded bytes");
        assert!(diagnostic.findings().is_empty());
        assert_unsupported(
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            )
            .expect_err("supported outer syntax cannot qualify its unknown embedded payload"),
            path,
        );
        assert_eq!(
            fixture
                .output()
                .file(&path.parse().expect("containing path")),
            Some(bytes)
        );
    }
}

#[test]
fn native_diagnostic_and_required_check_refuse_malformed_raster_files_and_data_payloads() {
    for (path, bytes) in [
        ("private.png", b"\x89PNG\r\n\x1a\n\x00\xff".as_slice()),
        (
            "private.html",
            br#"<i data-value="data:image/png;base64,AAEC/w=="></i>"#.as_slice(),
        ),
        (
            "private.json",
            br#"{"value":"data:image/png;base64,AAEC/w=="}"#.as_slice(),
        ),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let identity = fixture.output().inventory().identity();
        let rules = absent_rules();
        for result in [
            fixture.probe_disclosure(&rules, Default::default()),
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            ),
        ] {
            let error = result.expect_err(
                "a selected native decoder failure cannot become partial or required success",
            );
            assert_eq!(error.code(), "disclosure_probe_raster");
            assert!(!error.to_string().contains(path));
            let build::DisclosureProbeError::Raster {
                path: actual,
                reason,
            } = error
            else {
                panic!("malformed selected PNG must retain its typed decoder refusal");
            };
            assert_eq!(actual.as_str(), path);
            assert_eq!(reason, "container is malformed");
        }
        assert_eq!(fixture.output().inventory().identity(), identity);
        assert_eq!(
            fs::read(fixture.directory.path().join(path))
                .expect("malformed bytes were not repaired"),
            bytes
        );
        assert_eq!(
            fixture.output().file(&path.parse().expect("retained path")),
            Some(bytes)
        );
    }
}

#[test]
fn required_check_accepts_utf8_percent_text_but_rejects_binary_percent_and_unparsed_scripts() {
    let supported = RetainedOutput::new(&[("text.txt", "caf%C3%A9%20text".as_bytes())]);
    let rules = absent_rules();
    let report = build::check_disclosure(
        supported.output(),
        &supported.media_types,
        &rules,
        Default::default(),
    )
    .expect("percent text does not need a MIME declaration to remain supported UTF-8");
    let report = serde_json::to_value(report).expect("supported percent evidence");
    assert_eq!(report["status"], "required-supported-profile-evidence");
    assert!(
        report["embedded"]
            .as_array()
            .expect("native encoded coverage")
            .iter()
            .any(|value| value["representation"] == "percent-decoded")
    );
    for (path, bytes) in [
        ("private.txt", b"prefix%FF".as_slice()),
        (
            "private.html",
            b"<script type='application/x-private'></script>".as_slice(),
        ),
        (
            "private.html",
            b"<script type='application/x-private'>OPAQUE-PRIVATE-PAYLOAD</script>".as_slice(),
        ),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let diagnostic = fixture
            .probe_disclosure(&rules, Default::default())
            .expect("diagnostic remains partial on undecoded channel");
        assert!(diagnostic.findings().is_empty());
        assert_unsupported(
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            )
            .expect_err(
                "required check cannot omit this representation, even an empty data script",
            ),
            path,
        );
    }
}

#[test]
fn required_findings_preserve_diagnostic_coordinates_and_never_rewrite_output() {
    let path = "private.html";
    let bytes = b"<p title='PRIVATE&#45;CANARY'>PRIVATE%2DCANARY</p>";
    let fixture = RetainedOutput::new(&[(path, bytes)]);
    let rules = rules(&[("rule:private-check", "PRIVATE-CANARY")]);
    let diagnostic = fixture
        .probe_disclosure(&rules, Default::default())
        .expect("complete literal findings");
    let expected =
        serde_json::to_value(diagnostic.findings()).expect("actual diagnostic coordinates");
    assert!(has_finding(
        &serde_json::to_value(&diagnostic).expect("diagnostic report"),
        path,
        "rule:private-check",
        "html-attribute"
    ));
    assert!(has_finding(
        &serde_json::to_value(&diagnostic).expect("diagnostic report"),
        path,
        "rule:private-check",
        "percent-decoded"
    ));
    let error = build::check_disclosure(
        fixture.output(),
        &fixture.media_types,
        &rules,
        Default::default(),
    )
    .expect_err("required literal findings are a refusal, never a passing report");
    assert_eq!(error.code(), "disclosure_findings");
    let display = error.to_string();
    for private in [path, "rule:private-check", "PRIVATE-CANARY"] {
        assert!(
            !display.contains(private),
            "private finding appeared in Display"
        );
    }
    let build::DisclosureProbeError::Findings { findings } = error else {
        panic!("supported complete scan must preserve typed findings");
    };
    assert_eq!(
        serde_json::to_value(findings).expect("required private findings"),
        expected
    );
    assert_eq!(
        fixture
            .output()
            .file(&path.parse().expect("exact retained path")),
        Some(bytes.as_slice())
    );
    assert_eq!(
        fs::read(fixture.directory.path().join(path)).expect("native bytes preserved"),
        bytes
    );
}

#[test]
fn required_check_never_returns_early_evidence_when_late_syntax_or_budgets_fail() {
    let malformed =
        RetainedOutput::new(&[("a.txt", b"earlier supported output"), ("z.json", b"{")]);
    let rules = absent_rules();
    let error = build::check_disclosure(
        malformed.output(),
        &malformed.media_types,
        &rules,
        Default::default(),
    )
    .expect_err("late JSON failure cannot return an earlier partial success");
    assert!(
        matches!(error, build::DisclosureProbeError::Json { ref path, .. } if path.as_str() == "z.json")
    );
    let supported = RetainedOutput::new(&[("a.txt", b"supported%20text")]);
    for limits in [
        build::DisclosureProbeLimits {
            entries: 0,
            ..Default::default()
        },
        build::DisclosureProbeLimits {
            work_bytes: 0,
            ..Default::default()
        },
        build::DisclosureProbeLimits {
            encoded_values: 0,
            ..Default::default()
        },
    ] {
        let error =
            build::check_disclosure(supported.output(), &supported.media_types, &rules, limits)
                .expect_err("no successful truncated required scan");
        assert!(matches!(error, build::DisclosureProbeError::Limit(_)));
    }
    assert_eq!(
        fs::read(supported.directory.path().join("a.txt")).expect("limits never rewrite output"),
        b"supported%20text"
    );
}

#[test]
fn required_html_contexts_refuse_event_attributes_and_nonempty_external_script_bodies() {
    let path = "private.html";
    let rules = absent_rules();
    for bytes in [
        b"<button onclick=''>Empty event handler</button>".as_slice(),
        b"<button onclick='&#97;lert(1)'>Escaped handler</button>".as_slice(),
        b"<script src='https://example.invalid/selected.js'>const hidden = 1;</script>".as_slice(),
        b"<script type='module' src='selected.js'>import './other.js';</script>".as_slice(),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let diagnostic = fixture
            .probe_disclosure(&rules, Default::default())
            .expect("diagnostic evidence remains available without executing handlers or scripts");
        assert!(diagnostic.findings().is_empty());
        assert_unsupported(
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            )
            .expect_err("unparsed executable context cannot satisfy the required profile"),
            path,
        );
        assert_eq!(
            fixture.output().file(&path.parse().expect("retained path")),
            Some(bytes)
        );
    }
    for bytes in [
        b"<script src='https://example.invalid/selected.js'></script>".as_slice(),
        b"<script type='module' src='selected.js'></script>".as_slice(),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let report = build::check_disclosure(
            fixture.output(),
            &fixture.media_types,
            &rules,
            Default::default(),
        )
        .expect("bodyless external script has no inline payload to omit");
        let report = serde_json::to_value(report).expect("bounded lexical evidence");
        assert_eq!(report["status"], "required-supported-profile-evidence");
        assert_eq!(report["findings"], json!([]));
        assert_eq!(
            report["files"]
                .as_array()
                .expect("only retained file coverage")
                .len(),
            1
        );
        assert_eq!(report["files"][0]["path"], path);
        assert!(
            report.get("certified").is_none(),
            "referenced scripts were not acquired or qualified"
        );
    }
}

#[test]
fn required_xml_contexts_follow_native_namespaces_instead_of_same_word_elements() {
    let path = "private.svg";
    let rules = absent_rules();
    for bytes in [
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><style/></svg>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#.as_slice(),
        br#"<s:svg xmlns:s="http://www.w3.org/2000/svg"><s:script/></s:svg>"#.as_slice(),
        br#"<html xmlns="http://www.w3.org/1999/xhtml"/>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg" onload=""/>"#.as_slice(),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let diagnostic = fixture
            .probe_disclosure(&rules, Default::default())
            .expect("namespace-aware diagnostic traversal");
        assert!(diagnostic.findings().is_empty());
        assert_unsupported(
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            )
            .expect_err("even empty known embedded or event context remains unqualified"),
            path,
        );
    }
    for bytes in [
        br#"<script xmlns="urn:example">ordinary XML text</script>"#.as_slice(),
        br#"<root xmlns="urn:example"><style/><foreignObject/><script onclick="ordinary attribute"/></root>"#.as_slice(),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script xmlns="urn:example"/></svg>"#.as_slice(),
    ] {
        let fixture = RetainedOutput::new(&[(path, bytes)]);
        let diagnostic = fixture
            .probe_disclosure(&rules, Default::default())
            .expect("ordinary XML diagnostic");
        let required = build::check_disclosure(
            fixture.output(),
            &fixture.media_types,
            &rules,
            Default::default(),
        )
            .expect("unrelated namespace namesakes are not SVG or XHTML contexts");
        let mut required = serde_json::to_value(required).expect("required XML evidence");
        let mut diagnostic =
            serde_json::to_value(diagnostic).expect("same ordinary XML evidence");
        assert_eq!(
            required["status"],
            "required-supported-profile-evidence"
        );
        assert_eq!(diagnostic["status"], "partial-evidence");
        required
            .as_object_mut()
            .expect("required XML report")
            .remove("status");
        diagnostic
            .as_object_mut()
            .expect("diagnostic XML report")
            .remove("status");
        assert_eq!(required, diagnostic);
    }
}

#[test]
fn required_svg_style_attributes_use_css_and_processing_instructions_remain_unqualified() {
    let path = "private.svg";
    let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg"><g style='font-family: "CSS\2d CANARY"'/></svg>"#;
    let fixture = RetainedOutput::new(&[(path, bytes)]);
    let clean = build::check_disclosure(
        fixture.output(),
        &fixture.media_types,
        &absent_rules(),
        Default::default(),
    )
    .expect("supported SVG style attribute uses the CSS decoder");
    assert_eq!(
        serde_json::to_value(clean).expect("required supported evidence")["status"],
        "required-supported-profile-evidence"
    );
    let selected = rules(&[("rule:style", "CSS-CANARY")]);
    assert!(
        !std::str::from_utf8(bytes)
            .expect("XML fixture")
            .contains("CSS-CANARY")
    );
    let diagnostic = fixture
        .probe_disclosure(&selected, Default::default())
        .expect("actual escaped CSS finding");
    let expected = serde_json::to_value(diagnostic.findings()).expect("CSS findings");
    assert!(has_finding(
        &serde_json::to_value(&diagnostic).expect("CSS evidence"),
        path,
        "rule:style",
        "css-string"
    ));
    let error = build::check_disclosure(
        fixture.output(),
        &fixture.media_types,
        &selected,
        Default::default(),
    )
    .expect_err("required check must reject the decoded CSS canary");
    let build::DisclosureProbeError::Findings { findings } = error else {
        panic!("supported CSS findings remain distinct from unsupported XML contexts");
    };
    assert_eq!(
        serde_json::to_value(findings).expect("required CSS findings"),
        expected
    );
    assert_eq!(
        fs::read(fixture.directory.path().join(path)).expect("XML was never rewritten"),
        bytes
    );
    for bytes in [
        b"<?private?><root/>".as_slice(),
        b"<?xml-stylesheet href='https://example.invalid/private.css'?><root/>".as_slice(),
    ] {
        let fixture = RetainedOutput::new(&[("private.xml", bytes)]);
        let rules = absent_rules();
        assert!(
            fixture
                .probe_disclosure(&rules, Default::default())
                .expect("diagnostic preserves processing-instruction text")
                .findings()
                .is_empty()
        );
        assert_unsupported(
            build::check_disclosure(
                fixture.output(),
                &fixture.media_types,
                &rules,
                Default::default(),
            )
            .expect_err("processing instructions have no required semantic decoder"),
            "private.xml",
        );
    }
}
