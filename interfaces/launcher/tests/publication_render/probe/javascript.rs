//! Native JavaScript decoding over actual pinned retained output, never execution.

use super::super::bytes;
use super::{
    RetainedOutput, Value, assert_same_evidence_under_different_resource_limits, build, failure,
    fs, has_finding, rules,
};

const OUTPUT: &str = "zzzz.Js";

fn coverage<'a>(report: &'a Value, path: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("coverage")
        .iter()
        .find(|file| file["path"] == path)
        .expect("covered retained file")
}

#[test]
fn decoded_external_and_inline_javascript_matches_cli_without_execution_or_rewrites() {
    let script = br#"const EXT\u005fID=1; "EXT\u002dSTRING"; `EXT\u002dTEMPLATE${EXT\u005fID}`; "DEL\u007fCANARY"; throw new Error("must not execute");"#;
    let html = br#"<script>const INLINE\u005fID=1; "INLINE\u002dSTRING"; `INLINE\u002dTEMPLATE${INLINE\u005fID}`;</script><script type="module">export const MODULE\u005fID="MODULE\u002dSTRING"; `MODULE\u002dTEMPLATE${MODULE\u005fID}`;</script><script type="application/ld+json">{"name":"JSON\u002dCANARY"}</script>"#;
    let fixture = RetainedOutput::new(&[("guide/index.html", html), (OUTPUT, script)]);

    let sources = fixture.output().inventory().identity();

    let identity = fixture.output().inventory().identity();
    let selected = [
        ("rule:external-id", "EXT_ID"),
        ("rule:external-string", "EXT-STRING"),
        ("rule:external-template", "EXT-TEMPLATE"),
        ("rule:control", "DEL\u{7f}CANARY"),
        ("rule:inline-id", "INLINE_ID"),
        ("rule:inline-string", "INLINE-STRING"),
        ("rule:inline-template", "INLINE-TEMPLATE"),
        ("rule:module-id", "MODULE_ID"),
        ("rule:module-string", "MODULE-STRING"),
        ("rule:module-template", "MODULE-TEMPLATE"),
        ("rule:json", "JSON-CANARY"),
    ];
    for (_, literal) in selected {
        for entry in fixture.output().inventory().entries() {
            if let Some(payload) = fixture.output().file(&entry.path) {
                assert!(
                    !payload
                        .windows(literal.len())
                        .any(|part| part == literal.as_bytes()),
                    "escaped needle must be absent from raw outputs"
                );
            }
        }
    }
    let rules = rules(&selected);
    let probe = fixture
        .probe_disclosure(&rules, Default::default())
        .expect("native syntax only; throwing program must not execute");
    let expected = serde_json::to_value(&probe).expect("SDK report");
    let output = fixture.cli(&selected);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("CLI partial evidence");
    assert_eq!(report, expected);
    assert_eq!(report["status"], "partial-evidence");
    assert_eq!(report["scanner"], "gazette-disclosure-probe/16");
    for (path, rule, representation) in [
        (OUTPUT, "rule:external-id", "javascript-identifier"),
        (OUTPUT, "rule:external-string", "javascript-string"),
        (OUTPUT, "rule:external-template", "javascript-template"),
        (OUTPUT, "rule:control", "javascript-string"),
        (
            "guide/index.html",
            "rule:inline-id",
            "javascript-identifier",
        ),
        (
            "guide/index.html",
            "rule:inline-string",
            "javascript-string",
        ),
        (
            "guide/index.html",
            "rule:inline-template",
            "javascript-template",
        ),
        (
            "guide/index.html",
            "rule:module-id",
            "javascript-identifier",
        ),
        (
            "guide/index.html",
            "rule:module-string",
            "javascript-string",
        ),
        (
            "guide/index.html",
            "rule:module-template",
            "javascript-template",
        ),
        ("guide/index.html", "rule:json", "json-string"),
    ] {
        assert!(
            has_finding(&report, path, rule, representation),
            "missing {path}/{rule}/{representation}"
        );
    }
    assert!(!has_finding(
        &report,
        "guide/index.html",
        "rule:json",
        "javascript-string"
    ));
    assert_eq!(
        coverage(&report, OUTPUT)["decoding"]["javascript-syntax"]["profile"],
        gazette_sdk::javascript::PROFILE
    );
    assert_eq!(
        coverage(&report, OUTPUT)["decoding"]["javascript-syntax"]["goal"],
        "unambiguous"
    );
    assert_eq!(
        coverage(&report, "guide/index.html")["decoding"]["html-tokens"]["javascript_profile"],
        gazette_sdk::javascript::PROFILE
    );
    assert_eq!(probe.output().inventory().identity(), identity);
    assert_eq!(bytes(fixture.output(), OUTPUT), script);
    assert_eq!(bytes(fixture.output(), "guide/index.html"), html);
    assert_eq!(
        gazette_sdk::source::inspect(fixture.directory.path(), None, Default::default())
            .expect("fresh native evidence")
            .identity(),
        sources
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no stage")
            .count(),
        2
    );

    fs::write(
        fixture.directory.path().join(OUTPUT),
        b"BROKEN NEW NATIVE SOURCE (",
    )
    .expect("later native edit");
    fs::write(
        fixture.directory.path().join("guide/index.html"),
        b"CHANGED HTML",
    )
    .expect("later native edit");
    let repeated = fixture
        .probe_disclosure(&rules, Default::default())
        .expect("retained output only");
    assert_eq!(
        serde_json::to_value(&repeated).expect("repeated report"),
        expected
    );
    assert_eq!(repeated.output().inventory().identity(), identity);
    assert_eq!(bytes(fixture.output(), OUTPUT), script);
    assert_eq!(
        fs::read(fixture.directory.path().join(OUTPUT)).expect("no repair"),
        b"BROKEN NEW NATIVE SOURCE ("
    );
}

#[test]
fn javascript_suffixes_select_their_native_goal_and_nonjavascript_script_bodies_stay_raw() {
    let html=br#"<script type="text/plain">not valid JavaScript ( "RAW\u002dDATA" RAW-DATA</script><script src="https://example.invalid/never-fetch.js">not valid JavaScript ( "EXTERNAL\u002dBODY" EXTERNAL-BODY</script>"#;
    for (output, script, goal) in [
        (
            "zzzz.Js",
            br#""SUFFIX\u002dCANARY";"#.as_slice(),
            "unambiguous",
        ),
        (
            "zzzz.MjS",
            br#"export const value="SUFFIX\u002dCANARY";"#.as_slice(),
            "module",
        ),
        (
            "zzzz.CjS",
            br#"module.exports="SUFFIX\u002dCANARY";"#.as_slice(),
            "script",
        ),
    ] {
        let fixture = RetainedOutput::new(&[("guide/index.html", html), (output, script)]);

        let selected = [
            ("rule:suffix", "SUFFIX-CANARY"),
            ("rule:raw", "RAW-DATA"),
            ("rule:external", "EXTERNAL-BODY"),
        ];
        let report = serde_json::to_value(
            fixture
                .probe_disclosure(&rules(&selected), Default::default())
                .expect("only supported script channels are parsed"),
        )
        .expect("SDK evidence");
        assert!(has_finding(
            &report,
            output,
            "rule:suffix",
            "javascript-string"
        ));
        assert_eq!(
            coverage(&report, output)["decoding"]["javascript-syntax"]["goal"],
            goal
        );
        for rule in ["rule:raw", "rule:external"] {
            assert!(has_finding(
                &report,
                "guide/index.html",
                rule,
                "html-script"
            ));
            for representation in [
                "javascript-string",
                "javascript-identifier",
                "javascript-template",
            ] {
                assert!(!has_finding(
                    &report,
                    "guide/index.html",
                    rule,
                    representation
                ));
            }
        }
        let cli_output = fixture.cli(&selected);
        assert!(
            cli_output.status.success(),
            "{}",
            String::from_utf8_lossy(&cli_output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&cli_output.stdout).expect("CLI report"),
            report
        );
        assert_eq!(bytes(fixture.output(), output), script);
        assert_eq!(
            fs::read_dir(fixture.directory.path())
                .expect("no stage")
                .count(),
            2
        );
    }
}

#[test]
fn late_malformed_external_or_inline_javascript_returns_no_partial_report_or_stage() {
    for (html, script, output) in [
        (
            br#"<p>PRIVATE-LITERAL</p>"#.as_slice(),
            br#""PRIVATE-LITERAL"; const broken = ;"#.as_slice(),
            OUTPUT,
        ),
        (
            br#"<script>const broken = ;</script>"#.as_slice(),
            b"0;".as_slice(),
            OUTPUT,
        ),
        (
            br#"<script type="module">export const broken = ;</script>"#.as_slice(),
            b"0;".as_slice(),
            OUTPUT,
        ),
        (
            br#"<p>plain</p>"#.as_slice(),
            b"export const value=1;".as_slice(),
            "zzzz.CjS",
        ),
    ] {
        let fixture = RetainedOutput::new(&[("guide/index.html", html), (output, script)]);

        let identity = fixture.output().inventory().identity();
        let selected = [("rule:private-rule", "PRIVATE-LITERAL")];
        let error = fixture
            .probe_disclosure(&rules(&selected), Default::default())
            .expect_err("whole probe fails after invalid native syntax");
        assert_eq!(error.code(), "disclosure_probe_javascript");
        failure(&fixture.cli(&selected), "disclosure_probe_javascript");
        assert_eq!(fixture.output().inventory().identity(), identity);
        assert_eq!(bytes(fixture.output(), output), script);
        assert_eq!(
            fs::read(fixture.directory.path().join("guide/index.html")).expect("unchanged source"),
            html
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
fn javascript_and_aggregate_limits_refuse_the_whole_retained_probe() {
    // The numeric statement prevents the quoted string becoming a directive.
    let script = br#"0;"x";"#;
    let fixture = RetainedOutput::new(&[("guide/index.html", b"<p>plain</p>"), (OUTPUT, script)]);

    let rules = rules(&[("rule:one", "x")]);
    let mut limits = build::DisclosureProbeLimits {
        javascript: gazette_sdk::javascript::Limits {
            input_bytes: script.len(),
            decoded_bytes: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let report = serde_json::to_value(
        fixture
            .probe_disclosure(&rules, limits)
            .expect("exact source bytes and decoded UTF-8 byte budget"),
    )
    .expect("complete SDK evidence");
    assert!(has_finding(
        &report,
        OUTPUT,
        "rule:one",
        "javascript-string"
    ));
    let mut input = limits;
    input.javascript.input_bytes -= 1;
    let mut nodes = limits;
    nodes.javascript.nodes = 0;
    let mut depth = limits;
    depth.javascript.depth = 0;
    let mut decoded = limits;
    decoded.javascript.decoded_bytes = 0;
    for (insufficient, resource) in [
        (input, "JavaScript input bytes"),
        (nodes, "JavaScript syntax nodes"),
        (depth, "JavaScript syntax depth"),
        (decoded, "JavaScript decoded bytes"),
    ] {
        let error = fixture
            .probe_disclosure(&rules, insufficient)
            .expect_err("each native JavaScript limit invalidates the entire report");
        assert!(
            matches!(&error, build::DisclosureProbeError::Limit(actual) if *actual == resource),
            "wrong boundary: {error}"
        );
        assert_eq!(error.code(), "disclosure_probe_limit");
    }
    let work = report["work_bytes"]
        .as_u64()
        .expect("total charged comparisons");
    limits.work_bytes = work;
    assert_same_evidence_under_different_resource_limits(
        serde_json::to_value(
            fixture
                .probe_disclosure(&rules, limits)
                .expect("exact shared raw and decoded comparison budget"),
        )
        .expect("report"),
        report,
    );
    limits.work_bytes = work.checked_sub(1).expect("nonzero shared work");
    assert_eq!(
        fixture
            .probe_disclosure(&rules, limits)
            .expect_err("one byte below complete comparison work")
            .code(),
        "disclosure_probe_limit"
    );
    assert_eq!(bytes(fixture.output(), OUTPUT), script);
    assert_eq!(
        fs::read(fixture.directory.path().join(OUTPUT)).expect("source unchanged"),
        script
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no stage")
            .count(),
        2
    );
}
