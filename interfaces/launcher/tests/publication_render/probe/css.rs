//! CSS decoding reaches pinned retained output and the real private CLI probe.

use super::super::bytes;
use super::{
    RetainedOutput, Value, assert_same_evidence_under_different_resource_limits, build, failure,
    fs, has_finding, rules,
};

const OUTPUT: &str = "zzzz.CsS";

#[test]
fn escaped_external_and_inline_css_has_sdk_cli_parity_without_native_rewrites() {
    let stylesheet = br#".EXT\2d ID{content:"EXT\2d STRING";background:url(https://example.invalid/EXT\2d URL)}"#;
    let html = br#"<style>.INLINE\2d ID{content:"INLINE\2d STRING";background:url(https://example.invalid/INLINE\2d URL)}p{content:"RAW-STYLE"}</style><p style="--ATTR\2d ID:&#34;ATTR\2d STRING&#34;;background:url(https://example.invalid/ATTR\2d URL);content:'RAW-ATTRIBUTE'">CSS fixture</p>"#;
    let fixture = RetainedOutput::new(&[("guide/index.html", html), (OUTPUT, stylesheet)]);

    let sources = fixture.output().inventory().identity();

    assert_eq!(bytes(fixture.output(), OUTPUT), stylesheet);
    assert_eq!(bytes(fixture.output(), "guide/index.html"), html);
    let selected = [
        ("rule:external-id", "EXT-ID"),
        ("rule:external-string", "EXT-STRING"),
        ("rule:external-url", "EXT-URL"),
        ("rule:inline-id", "INLINE-ID"),
        ("rule:inline-string", "INLINE-STRING"),
        ("rule:inline-url", "INLINE-URL"),
        ("rule:attribute-id", "ATTR-ID"),
        ("rule:attribute-string", "ATTR-STRING"),
        ("rule:attribute-url", "ATTR-URL"),
        ("rule:raw-style", "RAW-STYLE"),
        ("rule:raw-attribute", "RAW-ATTRIBUTE"),
    ];
    for (_, literal) in &selected[..9] {
        for entry in fixture.output().inventory().entries() {
            if let Some(payload) = fixture.output().file(&entry.path) {
                assert!(
                    !payload
                        .windows(literal.len())
                        .any(|bytes| bytes == literal.as_bytes()),
                    "escaped canary must be absent from every raw output"
                );
            }
        }
    }
    let rules = rules(&selected);
    let probe = fixture
        .probe_disclosure(&rules, Default::default())
        .expect("SDK CSS probe");
    let expected = serde_json::to_value(&probe).expect("SDK report");
    let output = fixture.cli(&selected);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("CLI report");
    assert_eq!(report, expected);
    assert_eq!(report["status"], "partial-evidence");
    assert_eq!(report["scanner"], "gazette-disclosure-probe/16");
    for (path, rule, representation) in [
        (OUTPUT, "rule:external-id", "css-identifier"),
        (OUTPUT, "rule:external-string", "css-string"),
        (OUTPUT, "rule:external-url", "css-url"),
        ("guide/index.html", "rule:inline-id", "css-identifier"),
        ("guide/index.html", "rule:inline-string", "css-string"),
        ("guide/index.html", "rule:inline-url", "css-url"),
        ("guide/index.html", "rule:attribute-id", "css-identifier"),
        ("guide/index.html", "rule:attribute-string", "css-string"),
        ("guide/index.html", "rule:attribute-url", "css-url"),
        ("guide/index.html", "rule:raw-style", "html-style"),
        ("guide/index.html", "rule:raw-attribute", "html-attribute"),
    ] {
        assert!(
            has_finding(&report, path, rule, representation),
            "missing {path}/{rule}/{representation}"
        );
    }
    let coverage = |path| {
        report["files"]
            .as_array()
            .expect("coverage")
            .iter()
            .find(|file| file["path"] == path)
            .expect("covered file")
    };
    assert_eq!(
        coverage(OUTPUT)["decoding"]["css-tokens"]["profile"],
        gazette_sdk::css::PROFILE
    );
    assert_eq!(
        coverage("guide/index.html")["decoding"]["html-tokens"]["profile"],
        gazette_sdk::html::PROFILE
    );
    assert_eq!(
        coverage("guide/index.html")["decoding"]["html-tokens"]["css_profile"],
        gazette_sdk::css::PROFILE
    );
    assert_eq!(
        gazette_sdk::source::inspect(fixture.directory.path(), None, Default::default())
            .expect("fresh native evidence")
            .identity(),
        sources
    );
    assert_eq!(
        fs::read(fixture.directory.path().join("guide/index.html")).expect("HTML source"),
        html
    );
    assert_eq!(
        fs::read(fixture.directory.path().join(OUTPUT)).expect("CSS source"),
        stylesheet
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no stage")
            .count(),
        2
    );

    // Later native edits do not participate in scanning an already captured output.
    fs::write(
        fixture.directory.path().join("guide/index.html"),
        "CHANGED HTML",
    )
    .expect("native edit");
    fs::write(fixture.directory.path().join(OUTPUT), "CHANGED CSS").expect("native edit");
    let repeated = fixture
        .probe_disclosure(&rules, Default::default())
        .expect("retained probe");
    assert_eq!(
        serde_json::to_value(repeated).expect("repeated evidence"),
        expected
    );
    assert_eq!(bytes(fixture.output(), OUTPUT), stylesheet);
    assert_eq!(bytes(fixture.output(), "guide/index.html"), html);
    assert_eq!(
        fs::read(fixture.directory.path().join(OUTPUT)).expect("no rewrite"),
        b"CHANGED CSS"
    );
}

#[test]
fn leading_utf8_charset_preserves_external_css_detection_through_sdk_and_cli() {
    for prefix in ["@charset \"UTF-8\";", "\u{feff}@charset \"uTf-8\";"] {
        let stylesheet = format!(r#"{prefix}p{{content:"CHARSET\2d CANARY"}}"#);
        assert!(!stylesheet.contains("CHARSET-CANARY"));
        let fixture = RetainedOutput::new(&[
            ("guide/index.html", b"<p>plain</p>"),
            (OUTPUT, stylesheet.as_bytes()),
        ]);

        let selected = [("rule:charset", "CHARSET-CANARY")];
        let expected = serde_json::to_value(
            fixture
                .probe_disclosure(&rules(&selected), Default::default())
                .expect("supported UTF-8 declaration"),
        )
        .expect("SDK report");
        assert!(has_finding(&expected, OUTPUT, "rule:charset", "css-string"));
        assert!(!has_finding(&expected, OUTPUT, "rule:charset", "raw"));
        let output = fixture.cli(&selected);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).expect("CLI report"),
            expected
        );
        assert_eq!(expected["status"], "partial-evidence");
        assert_eq!(bytes(fixture.output(), OUTPUT), stylesheet.as_bytes());
        assert_eq!(
            fs::read(fixture.directory.path().join(OUTPUT)).expect("unchanged stylesheet"),
            stylesheet.as_bytes()
        );
        assert_eq!(
            fs::read_dir(fixture.directory.path())
                .expect("no stage")
                .count(),
            2
        );
    }
}

#[test]
fn decoded_html_styles_preserve_leading_unicode_while_stylesheet_bom_is_a_signature() {
    let html =
        "<style>\u{feff}\\73 ecret{}</style><p style=\"&#xfeff;\\61 ttribute:value\">text</p>";
    let stylesheet = b"\xef\xbb\xbf\\65 xternal{}";
    let fixture =
        RetainedOutput::new(&[("guide/index.html", html.as_bytes()), (OUTPUT, stylesheet)]);

    let selected = [
        ("rule:style", "\u{feff}secret"),
        ("rule:attribute", "\u{feff}attribute"),
        ("rule:external", "external"),
    ];
    let expected = serde_json::to_value(
        fixture
            .probe_disclosure(&rules(&selected), Default::default())
            .expect("input phases preserve CSS characters"),
    )
    .expect("SDK evidence");
    for (path, rule) in [
        ("guide/index.html", "rule:style"),
        ("guide/index.html", "rule:attribute"),
        (OUTPUT, "rule:external"),
    ] {
        assert!(has_finding(&expected, path, rule, "css-identifier"));
        assert!(!has_finding(&expected, path, rule, "raw"));
    }
    let output = fixture.cli(&selected);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI evidence"),
        expected
    );
    assert_eq!(bytes(fixture.output(), "guide/index.html"), html.as_bytes());
    assert_eq!(bytes(fixture.output(), OUTPUT), stylesheet);
}

#[test]
fn malformed_late_external_or_inline_css_never_emits_a_partial_report_or_stage() {
    let cases: &[(&[u8], &[u8], &str)] = &[
        (
            b"<p>PRIVATE-LITERAL</p>",
            b"p{content:\"PRIVATE-LITERAL\"}q{content:\"broken\n}",
            "disclosure_probe_css",
        ),
        (
            b"<style>p{content:\"broken\n}</style>",
            b"p{}",
            "disclosure_probe_css",
        ),
        (
            b"<p style=\"content:&#34;broken\n\">text</p>",
            b"p{}",
            "disclosure_probe_css",
        ),
        (
            b"<style type=\"text/other\">p{color:red}</style>",
            b"p{}",
            "disclosure_probe_html",
        ),
        (
            b"<p>plain</p>",
            b"@charset \"iso-8859-1\";p{}",
            "disclosure_probe_css",
        ),
        (
            b"<p>plain</p>",
            b"p{}@charset \"UTF-8\";",
            "disclosure_probe_css",
        ),
    ];
    for &(html, stylesheet, code) in cases {
        let fixture = RetainedOutput::new(&[("guide/index.html", html), (OUTPUT, stylesheet)]);

        let selected = [("rule:private-rule", "PRIVATE-LITERAL")];
        let error = fixture
            .probe_disclosure(&rules(&selected), Default::default())
            .expect_err("invalid CSS cannot yield probe");
        assert_eq!(error.code(), code);
        if html == b"<p>PRIVATE-LITERAL</p>" {
            assert!(
                matches!(error, build::DisclosureProbeError::Css { ref path, .. } if path.as_str() == OUTPUT)
            );
        }
        failure(&fixture.cli(&selected), code);
        assert_eq!(
            fs::read(fixture.directory.path().join(OUTPUT)).expect("unchanged CSS"),
            stylesheet
        );
        assert_eq!(
            fs::read(fixture.directory.path().join("guide/index.html")).expect("unchanged HTML"),
            html
        );
        assert_eq!(bytes(fixture.output(), OUTPUT), stylesheet);
        assert_eq!(
            fs::read_dir(fixture.directory.path())
                .expect("no stage")
                .count(),
            2
        );
    }
}

#[test]
fn css_and_total_comparison_limits_bound_the_complete_connected_probe() {
    let stylesheet = br#"a{b:"x"}"#;
    let fixture =
        RetainedOutput::new(&[("guide/index.html", b"<p>plain</p>"), (OUTPUT, stylesheet)]);

    let rules = rules(&[("rule:one", "x")]);
    // a, {, b, :, "x", }: six lexical tokens, one block, three decoded bytes.
    let mut limits = build::DisclosureProbeLimits {
        css: gazette_sdk::css::Limits {
            input_bytes: stylesheet.len(),
            tokens: 6,
            depth: 1,
            decoded_bytes: 3,
        },
        ..Default::default()
    };
    let probe = fixture
        .probe_disclosure(&rules, limits)
        .expect("exact CSS input ceiling");
    let expected = serde_json::to_value(&probe).expect("complete evidence");
    assert!(has_finding(&expected, OUTPUT, "rule:one", "css-string"));
    let mut below = limits;
    below.css.input_bytes -= 1;
    let mut no_tokens = limits;
    no_tokens.css.tokens -= 1;
    let mut no_depth = limits;
    no_depth.css.depth = 0;
    let mut no_decoding = limits;
    no_decoding.css.decoded_bytes -= 1;
    for (insufficient, resource) in [
        (below, "CSS input bytes"),
        (no_tokens, "CSS tokens"),
        (no_depth, "CSS nesting depth"),
        (no_decoding, "CSS decoded bytes"),
    ] {
        let error = fixture
            .probe_disclosure(&rules, insufficient)
            .expect_err("CSS ceiling must fail whole probe");
        assert!(
            matches!(&error, build::DisclosureProbeError::Limit(actual) if *actual == resource),
            "wrong resource: {error}"
        );
        assert_eq!(error.code(), "disclosure_probe_limit");
    }
    let work = expected["work_bytes"]
        .as_u64()
        .expect("complete charged work");
    limits.work_bytes = work;
    assert_same_evidence_under_different_resource_limits(
        serde_json::to_value(
            fixture
                .probe_disclosure(&rules, limits)
                .expect("exact aggregate ceiling"),
        )
        .expect("exact report"),
        expected,
    );
    limits.work_bytes = work.checked_sub(1).expect("nonzero aggregate work");
    assert_eq!(
        fixture
            .probe_disclosure(&rules, limits)
            .expect_err("one byte below aggregate charge")
            .code(),
        "disclosure_probe_limit"
    );
    assert_eq!(bytes(fixture.output(), OUTPUT), stylesheet);
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no stage")
            .count(),
        2
    );
}
