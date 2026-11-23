//! Encoded disclosure reaches the same retained SDK output and private CLI probe.

use super::*;

// Fixture construction only: unambiguously encode every input byte, independently
// of the production decoder whose behavior these complete paths exercise.
fn percent(text: &str) -> String {
    text.as_bytes()
        .iter()
        .map(|byte| format!("%{byte:02X}"))
        .collect()
}

fn data(media: &str, text: &str) -> String {
    format!("data:{media},{}", percent(text))
}

fn report(config: &str, selected: &[(&str, &str)]) -> Value {
    let inputs = acquire(config);
    let rendered = render(&inputs).expect("actual immutable publication output");
    let probe = rendered
        .site()
        .probe_disclosure(&rules(selected), Default::default())
        .expect("SDK encoded disclosure evidence");
    let output = cli(&request(config, selected), &probe_arguments());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).expect("complete CLI evidence");
    assert_eq!(
        actual,
        serde_json::to_value(probe).expect("SDK evidence projection")
    );
    assert_eq!(actual["status"], "partial-evidence");
    assert!(actual.get("certified").is_none());
    actual
}

#[test]
fn embedded_media_use_native_decoders_and_match_cli_without_source_rewrites() {
    let json = r#"{"JSON%2DKEY":"JSON\u002dCANARY","nested":"data:text/plain,NESTED%252DCANARY"}"#;
    let payloads = [
        ("text/plain", "PLAIN%2DCANARY"),
        ("application/json", json),
        (
            "text/html",
            "<p title=\"HTML&#45;CANARY\">HTML%2DCANARY</p>",
        ),
        ("text/css", r#"p{content:"CSS\2d CANARY"}"#),
        (
            "application/javascript",
            r#""JS\u002dCANARY"; `TEMPLATE%2DCANARY`;"#,
        ),
        (
            "application/xml",
            "<r a=\"XML&#45;CANARY\">XML%2DCANARY</r>",
        ),
    ];
    let mut template = String::from("<!doctype html><html><body>");
    for (media, text) in payloads {
        template.push_str(&format!(
            "<span data-payload=\"{}\"></span>",
            data(media, text)
        ));
    }
    template.push_str("<span data-payload=\"data:application/octet-stream;base64,AAECAw==\"></span></body></html>");
    let fixture = Fixture::new(&[("theme/templates/page.html", template.as_bytes())]);
    let repository =
        git2::Repository::open(fixture.directory.path()).expect("native fixture repository");
    let native = || {
        repository
            .statuses(None)
            .expect("native status")
            .iter()
            .map(|entry| (entry.path_bytes().to_vec(), entry.status().bits()))
            .collect::<Vec<_>>()
    };
    let before = native();
    let selected = [
        ("rule:plain", "PLAIN-CANARY"),
        ("rule:key", "JSON-KEY"),
        ("rule:json", "JSON-CANARY"),
        ("rule:nested", "NESTED-CANARY"),
        ("rule:html", "HTML-CANARY"),
        ("rule:css", "CSS-CANARY"),
        ("rule:js", "JS-CANARY"),
        ("rule:template", "TEMPLATE-CANARY"),
        ("rule:xml", "XML-CANARY"),
    ];
    for (_, literal) in selected {
        assert!(!template.contains(literal));
    }
    let actual = report(&fixture.config(), &selected);
    for (rule, representation) in [
        ("rule:plain", "percent-decoded"),
        ("rule:key", "percent-decoded"),
        ("rule:json", "json-string"),
        ("rule:nested", "percent-decoded"),
        ("rule:html", "html-attribute"),
        ("rule:css", "css-string"),
        ("rule:js", "javascript-string"),
        ("rule:template", "percent-decoded"),
        ("rule:xml", "xml-attribute"),
    ] {
        assert!(
            has_finding(&actual, "guide/index.html", rule, representation),
            "missing native {rule}/{representation}"
        );
    }
    let embedded = actual["embedded"]
        .as_array()
        .expect("explicit embedded coverage");
    for (media, text) in payloads {
        assert!(
            embedded
                .iter()
                .any(|entry| entry["path"] == "guide/index.html"
                    && entry["media_type"] == media
                    && entry["bytes"] == text.len()
                    && entry["sha256"] == json!(source::Sha256Digest::of(text.as_bytes()))),
            "exact payload coverage for {media}"
        );
    }
    assert!(
        embedded
            .iter()
            .any(|entry| entry["path"] == "guide/index.html"
                && entry["media_type"] == "application/octet-stream"
                && entry["sha256"] == json!(source::Sha256Digest::of(&[0, 1, 2, 3]))
                && entry["bytes"] == 4
                && entry["decoding"] == "none"),
        "binary bytes are observed but not claimed structurally decoded"
    );
    assert_eq!(native(), before);
    assert_eq!(
        fs::read(fixture.directory.path().join("theme/templates/page.html"))
            .expect("untouched template"),
        template.as_bytes()
    );
}

#[test]
fn late_malformed_data_discards_earlier_findings_without_emitting_a_partial_report() {
    for invalid in ["data:text/plain;base64,%%%", "data:text/plain"] {
        let template = format!("<p title=\"EARLY%2DCANARY\"></p><p data-last=\"{invalid}\"></p>");
        let fixture = Fixture::new(&[("theme/templates/page.html", template.as_bytes())]);
        let config = fixture.config();
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("renderer preserves encoded source literally");
        let selected = [("rule:early", "EARLY-CANARY")];
        let error = rendered
            .site()
            .probe_disclosure(&rules(&selected), Default::default())
            .expect_err("late invalid data invalidates the whole probe");
        let output = cli(&request(&config, &selected), &probe_arguments());
        failure(&output, error.code());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(invalid));
        assert_eq!(
            fs::read(fixture.directory.path().join("theme/templates/page.html"))
                .expect("source not repaired"),
            template.as_bytes()
        );
    }
}

#[test]
fn cli_encoded_limits_match_sdk_failures_at_each_declared_boundary() {
    let template = format!(
        "<p data-value=\"{}\"></p>",
        data("application/json", r#"{"value":"DEEP%252DCANARY"}"#)
    );
    let fixture = Fixture::new(&[("theme/templates/page.html", template.as_bytes())]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("real bounded probe candidate");
    let selected = [("rule:deep", "DEEP-CANARY")];
    let input = request(&config, &selected);
    for (flag, value) in [
        ("--max-probe-encoded-input-bytes", 1usize),
        ("--max-probe-decoded-bytes", 1),
        ("--max-probe-encoded-depth", 0),
        ("--max-probe-encoded-depth", 33),
        ("--max-probe-encoded-values", 0),
        ("--max-probe-encoded-bytes", 0),
        ("--max-probe-work-bytes", 1),
    ] {
        let mut limits = build::DisclosureProbeLimits::default();
        match flag {
            "--max-probe-encoded-input-bytes" => limits.encoded.input_bytes = value,
            "--max-probe-decoded-bytes" => limits.encoded.decoded_bytes = value,
            "--max-probe-encoded-depth" => limits.encoded_depth = value,
            "--max-probe-encoded-values" => limits.encoded_values = value,
            "--max-probe-encoded-bytes" => limits.encoded_bytes = value,
            "--max-probe-work-bytes" => limits.work_bytes = value as u64,
            _ => unreachable!("closed fixture flag table"),
        }
        let error = rendered
            .site()
            .probe_disclosure(&rules(&selected), limits)
            .expect_err("this exact independent ceiling must stop the probe");
        let value = value.to_string();
        let mut arguments = probe_arguments();
        arguments.extend([flag, &value]);
        failure(&cli(&input, &arguments), error.code());
    }
    assert!(has_finding(
        &report(&fixture.config(), &selected),
        "guide/index.html",
        "rule:deep",
        "percent-decoded"
    ));
}

#[test]
fn static_filenames_and_plain_payloads_retain_exact_bytes_across_decoding_layers() {
    let fixture = Fixture::new(&[
        ("one/text.txt", b"TEXT%252DCANARY"),
        (
            "one/payload.txt",
            b"data:text/plain;base64,UEFZTE9BRC1DQU5BUlk=",
        ),
    ]);
    let config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]",
    );
    let selected = [
        ("rule:text", "TEXT-CANARY"),
        ("rule:payload", "PAYLOAD-CANARY"),
    ];
    let actual = report(&config, &selected);
    for (path, rule, representation) in [
        ("media/text.txt", "rule:text", "percent-decoded"),
        ("media/payload.txt", "rule:payload", "data-payload"),
    ] {
        assert!(
            has_finding(&actual, path, rule, representation),
            "{path}: {rule}/{representation}"
        );
    }
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("unchanged pinned sources can be rendered again");
    for (path, bytes) in [
        ("text.txt", b"TEXT%252DCANARY".as_slice()),
        (
            "payload.txt",
            b"data:text/plain;base64,UEFZTE9BRC1DQU5BUlk=".as_slice(),
        ),
    ] {
        assert_eq!(
            fs::read(fixture.directory.path().join("one").join(path))
                .expect("unchanged authored asset"),
            bytes
        );
        assert_eq!(
            rendered
                .site()
                .output()
                .file(&format!("media/{path}").parse().expect("native output path")),
            Some(bytes)
        );
    }
    // Asset route admission deliberately rejects nested encoded filenames.
    // Imported output inspection must still expose them without rewriting names.
    let retained = RetainedOutput::new(&[("FILES%252DCANARY.txt", b"retained literal bytes")]);
    let selected = [("rule:filename", "FILES-CANARY")];
    let expected = serde_json::to_value(
        retained
            .probe_disclosure(&rules(&selected), Default::default())
            .expect("native retained filename probe"),
    )
    .expect("private filename evidence");
    assert!(has_finding(
        &expected,
        "FILES%252DCANARY.txt",
        "rule:filename",
        "percent-decoded"
    ));
    let output = retained.cli(&selected);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("exact pinned CLI probe"),
        expected
    );
    assert_eq!(
        source::inspect(retained.directory.path(), None, Default::default())
            .expect("native spelling preserved")
            .identity(),
        retained.output().inventory().identity()
    );
    assert_eq!(
        fs::read(retained.directory.path().join("FILES%252DCANARY.txt"))
            .expect("literal filename remains"),
        b"retained literal bytes"
    );
}

#[test]
fn output_probe_validates_private_rule_uniqueness_and_limits_before_unreadable_tree() {
    let fixture = RetainedOutput::new(&[("unchanged.txt", b"exact retained bytes")]);
    let identity = fixture.output().inventory().identity();
    let identity_json = serde_json::to_string(&identity).expect("independent complete identity");
    let missing = fixture.directory.path().join("absent-output");
    let arguments = [
        "probe-output",
        "--tree",
        missing.to_str().expect("native missing root"),
        "--tree-identity",
        &identity_json,
    ];
    let valid = r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private-rule","literal":"PRIVATE-LITERAL"}]}"#;
    let source_error = source::capture_pinned(&missing, identity, Default::default())
        .expect_err("valid transport would reach this actual native refusal");
    failure(&cli(valid, &arguments), source_error.code());
    for invalid in [
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private-rule","literal":"PRIVATE-LITERAL"},{"id":"rule:private-rule","literal":"different"}]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private-rule","literal":"PRIVATE-LITERAL","lit\u0065ral":"different"}]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[["rule:private-rule","PRIVATE-LITERAL"]]}"#,
    ] {
        failure(&cli(invalid, &arguments), "disclosure_output_input");
    }
    let mut bounded = arguments.to_vec();
    bounded.extend(["--max-source-bytes", "1"]);
    failure(&cli(valid, &bounded), "disclosure_output_input_limit");
    let mut invalid_depth = arguments.to_vec();
    invalid_depth.extend(["--max-probe-encoded-depth", "33"]);
    failure(
        &cli("not JSON: PRIVATE-LITERAL", &invalid_depth),
        "disclosure_probe_limit",
    );
    assert!(!missing.exists());
    assert_eq!(
        fs::read(fixture.directory.path().join("unchanged.txt")).expect("native fixture untouched"),
        b"exact retained bytes"
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no capture or repair artifacts")
            .count(),
        1
    );
}

#[test]
fn encoded_budgets_are_exact_and_shared_across_independent_retained_files() {
    // Each native text file decodes to "%41" (3 bytes), then "A" (1 byte).
    // Together these require exactly four values, eight bytes and depth two.
    let fixture = RetainedOutput::new(&[
        ("a.txt", b"data:text/plain,%2541"),
        ("b.txt", b"data:text/plain,%2541"),
    ]);
    let selected = rules(&[("rule:decoded", "A")]);
    let exact = build::DisclosureProbeLimits {
        encoded_values: 4,
        encoded_bytes: 8,
        encoded_depth: 2,
        ..Default::default()
    };
    let report = serde_json::to_value(
        fixture
            .probe_disclosure(&selected, exact)
            .expect("exact whole-output budgets admit both files"),
    )
    .expect("native application evidence");
    assert_eq!(report["encoded_bytes"], 8);
    let embedded = report["embedded"]
        .as_array()
        .expect("four bounded observations");
    assert_eq!(embedded.len(), 4);
    for path in ["a.txt", "b.txt"] {
        for (depth, bytes) in [(1, b"%41".as_slice()), (2, b"A".as_slice())] {
            assert!(embedded.iter().any(|value| value["path"] == path
                && value["depth"] == depth
                && value["bytes"] == bytes.len()
                && value["sha256"] == json!(source::Sha256Digest::of(bytes))));
        }
        assert!(has_finding(
            &report,
            path,
            "rule:decoded",
            "percent-decoded"
        ));
    }
    for less in [
        build::DisclosureProbeLimits {
            encoded_values: 3,
            ..exact
        },
        build::DisclosureProbeLimits {
            encoded_bytes: 7,
            ..exact
        },
        build::DisclosureProbeLimits {
            encoded_depth: 1,
            ..exact
        },
    ] {
        assert!(
            matches!(
                fixture.probe_disclosure(&selected, less),
                Err(build::DisclosureProbeError::Limit(_))
            ),
            "no partial or per-file reset may bypass one-less budget"
        );
    }
    let one = RetainedOutput::new(&[("a.txt", b"data:text/plain,%2541")]);
    let per_file = build::DisclosureProbeLimits {
        encoded_values: 2,
        encoded_bytes: 4,
        ..exact
    };
    one.probe_disclosure(&selected, per_file)
        .expect("one file fits its exact independent cost");
    assert!(
        matches!(
            fixture.probe_disclosure(&selected, per_file),
            Err(build::DisclosureProbeError::Limit(_))
        ),
        "the same budget is not renewed for the second file"
    );
    assert_eq!(
        fixture.output().inventory().identity(),
        source::inspect(fixture.directory.path(), None, Default::default())
            .expect("unchanged native tree")
            .identity()
    );
}

#[test]
fn known_text_requires_supported_charset_and_utf8_while_binary_remains_unqualified() {
    let selected = [("rule:absent", "PRIVATE-LITERAL")];
    for body in [
        b"data:text/plain;charset=iso-8859-1,%FF".as_slice(),
        b"data:text/plain;charset=utf-8,%FF".as_slice(),
        b"data:text/plain;charset=us-ascii,%C3%A9".as_slice(),
    ] {
        let fixture = RetainedOutput::new(&[("payload.txt", body)]);
        assert!(
            matches!(
                fixture.probe_disclosure(&rules(&selected), Default::default()),
                Err(build::DisclosureProbeError::Encoded { .. })
            ),
            "declared text cannot receive lossy conversion or a successful opaque fallback"
        );
        failure(&fixture.cli(&selected), "disclosure_probe_encoded");
        assert_eq!(
            fs::read(fixture.directory.path().join("payload.txt"))
                .expect("untouched invalid payload"),
            body
        );
    }
    let fixture = RetainedOutput::new(&[("payload.txt", b"data:application/octet-stream,%FF")]);
    let report = serde_json::to_value(
        fixture
            .probe_disclosure(&rules(&selected), Default::default())
            .expect("the same byte is preserved as opaque binary"),
    )
    .expect("binary coverage");
    let embedded = report["embedded"]
        .as_array()
        .expect("explicit binary observation");
    assert_eq!(embedded.len(), 1);
    assert_eq!(embedded[0]["bytes"], 1);
    assert_eq!(
        embedded[0]["sha256"],
        json!(source::Sha256Digest::of(&[255]))
    );
    assert_eq!(embedded[0]["media_type"], "application/octet-stream");
    assert_eq!(embedded[0]["decoding"], "none");
    assert_eq!(report["status"], "partial-evidence");
    let output = fixture.cli(&selected);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI binary evidence"),
        report
    );
}
