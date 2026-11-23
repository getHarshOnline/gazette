//! Private stdin transport reaches the same retained-output probe, never staging.

#[path = "probe/check.rs"]
mod check;
#[path = "probe/css.rs"]
mod css;
#[path = "probe/encoded.rs"]
mod encoded;
#[path = "probe/javascript.rs"]
mod javascript;
#[path = "probe/policy.rs"]
mod policy;
#[path = "probe/raster.rs"]
pub(super) mod raster;

use std::collections::BTreeMap;

use gazette_sdk::{build, source, workspace::Key};
use serde_json::{Value, json};

use super::{Command, Fixture, Output, RENDER_ARGUMENTS, Stdio, acquire, cli, fs, render, tempdir};

fn request(workspace: &str, rules: &[(&str, &str)]) -> String {
    json!({
        "format": "gazette-disclosure-probe-input/1",
        "workspace": workspace,
        "rules": rules.iter().map(|(id, literal)| json!({"id":id,"literal":literal})).collect::<Vec<_>>()
    })
    .to_string()
}

fn probe_arguments() -> Vec<&'static str> {
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.push("--probe-disclosure");
    arguments
}

fn failure(output: &Output, code: &str) {
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "failure must not emit a partial report"
    );
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains(code), "{diagnostic}");
    for private in ["PRIVATE-WORKSPACE", "PRIVATE-LITERAL", "private-rule"] {
        assert!(
            !diagnostic.contains(private),
            "private input echoed: {diagnostic}"
        );
    }
}

fn rules(selected: &[(&str, &str)]) -> BTreeMap<Key, String> {
    selected
        .iter()
        .map(|(id, text)| (id.parse().expect("rule identity"), (*text).to_owned()))
        .collect()
}

fn has_finding(report: &Value, path: &str, rule: &str, representation: &str) -> bool {
    report["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|finding| {
            finding["path"] == path
                && finding["rule"] == rule
                && finding["representation"] == representation
        })
}

fn assert_same_evidence_under_different_resource_limits(mut actual: Value, mut expected: Value) {
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

/// A real disposable output tree captured through the same independently pinned
/// native operation as probe-output. No template is mislabeled as another format.
struct RetainedOutput {
    directory: tempfile::TempDir,
    output: source::CapturedSource,
    media_types: BTreeMap<source::EntryPath, gazette_sdk::render::MediaType>,
}

impl RetainedOutput {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let directory = tempdir().expect("disposable retained-output fixture");
        for (path, bytes) in files {
            let path = directory.path().join(path);
            fs::create_dir_all(path.parent().expect("output parent")).expect("output directories");
            fs::write(path, bytes).expect("exact fixture output bytes");
        }
        let inventory = source::inspect(directory.path(), None, Default::default())
            .expect("native output observation");
        let output =
            source::capture_pinned(directory.path(), inventory.identity(), Default::default())
                .expect("same public retained-output capture path as probe-output");
        for (path, bytes) in files {
            assert_eq!(
                output.file(&path.parse().expect("output path")),
                Some(*bytes)
            );
        }
        Self {
            directory,
            output,
            media_types: BTreeMap::new(),
        }
    }

    fn output(&self) -> &source::CapturedSource {
        &self.output
    }

    fn probe_disclosure<'a>(
        &'a self,
        rules: &BTreeMap<Key, String>,
        limits: build::DisclosureProbeLimits,
    ) -> Result<build::DisclosureProbe<'a>, build::DisclosureProbeError> {
        build::probe_disclosure(&self.output, &self.media_types, rules, limits)
    }

    fn cli(&self, selected: &[(&str, &str)]) -> Output {
        let request = json!({
            "format": "gazette-disclosure-output-input/1",
            "rules": selected.iter().map(|(id,literal)| json!({"id":id,"literal":literal})).collect::<Vec<_>>()
        }).to_string();
        self.cli_request(&request)
    }

    fn cli_request(&self, request: &str) -> Output {
        let identity = serde_json::to_string(&self.output.inventory().identity())
            .expect("exact output identity JSON");
        cli(
            request,
            &[
                "probe-output",
                "--tree",
                self.directory.path().to_str().expect("native output root"),
                "--tree-identity",
                &identity,
            ],
        )
    }
}

#[test]
fn output_probe_refuses_changed_native_tree_and_strict_private_request_without_repair() {
    let fixture = RetainedOutput::new(&[("result.txt", b"Retained output")]);
    let selected = [("rule:one", "Retained")];
    let rules = rules(&selected);
    let expected = serde_json::to_value(
        fixture
            .probe_disclosure(&rules, Default::default())
            .expect("retained probe"),
    )
    .expect("retained evidence");
    fs::write(
        fixture.directory.path().join("result.txt"),
        b"Changed native output",
    )
    .expect("explicit fixture mutation");
    failure(&fixture.cli(&selected), "source_tree_mismatch");
    for request in [
        r#"{"format":"gazette-disclosure-output-input/1","rules":[],"workspace":"PRIVATE-WORKSPACE"}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private-rule","literal":"PRIVATE-LITERAL","extra":true}]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[],"rules":[]}"#,
        r#"["gazette-disclosure-output-input/1",[]]"#,
    ] {
        failure(&fixture.cli_request(request), "disclosure_output_input");
    }
    assert_eq!(
        serde_json::to_value(
            fixture
                .probe_disclosure(&rules, Default::default())
                .expect("same retained capture")
        )
        .expect("same retained evidence"),
        expected
    );
    assert_eq!(
        fs::read(fixture.directory.path().join("result.txt")).expect("native bytes preserved"),
        b"Changed native output"
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no repair/stage")
            .count(),
        1
    );
}

#[test]
fn pinned_xml_output_keeps_attribute_namespace_and_text_decoding_without_rewriting() {
    let xml =
        br#"<root xmlns:unused="urn:NS&#45;CANARY" note="XML&#45;CANARY">XML&#45;CANARY</root>"#;
    let fixture = RetainedOutput::new(&[("graph.XML", xml)]);
    let selected = [("rule:xml", "XML-CANARY"), ("rule:namespace", "NS-CANARY")];
    let rules = rules(&selected);
    let probe = fixture
        .probe_disclosure(&rules, Default::default())
        .expect("native XML probe");
    let expected = serde_json::to_value(&probe).expect("SDK report");
    let output = fixture.cli(&selected);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI XML report"),
        expected
    );
    assert_eq!(
        fixture.cli(&selected).stdout,
        output.stdout,
        "deterministic read-only repeat"
    );
    for (rule, representation) in [
        ("rule:xml", "xml-text"),
        ("rule:xml", "xml-attribute"),
        ("rule:namespace", "xml-namespace"),
    ] {
        assert!(has_finding(&expected, "graph.XML", rule, representation));
        assert!(!has_finding(&expected, "graph.XML", rule, "raw"));
    }
    assert_eq!(expected["status"], "partial-evidence");
    assert!(std::ptr::eq(probe.output(), fixture.output()));
    assert_eq!(
        fs::read(fixture.directory.path().join("graph.XML")).expect("unchanged XML"),
        xml
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("no stage")
            .count(),
        1
    );
}

#[test]
fn private_cli_probe_matches_sdk_bytes_for_encoded_html_and_json_without_writes() {
    let template = br#"<p title="ATTR&#45;CANARY">HTML&#45;CANARY</p><script type="application/ld+json">{"name":"JSON\u002dCANARY","name":"JSON\u002dCANARY"}</script>"#;
    let fixture = Fixture::new(&[("theme/templates/page.html", template)]);
    let repository = git2::Repository::open(fixture.directory.path()).expect("fixture Git");
    let native_frontier = || {
        (
            repository.head().expect("fixture HEAD").target(),
            repository
                .statuses(None)
                .expect("native status")
                .iter()
                .map(|entry| (entry.path_bytes().to_vec(), entry.status().bits()))
                .collect::<Vec<_>>(),
        )
    };
    let frontier_before = native_frontier();
    let config = fixture.config();
    let inputs = acquire(&config);
    let sources_before = serde_json::to_value(inputs.inspect()).expect("captured source evidence");
    let rendered = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            model_output: Some(&"model.json".parse().expect("model")),
            graph: None,
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect("real retained render");
    let selected = [
        ("rule:html", "HTML-CANARY"),
        ("rule:attribute", "ATTR-CANARY"),
        ("rule:json", "JSON-CANARY"),
    ];
    let rules: BTreeMap<Key, String> = selected
        .iter()
        .map(|(id, literal)| (id.parse().expect("rule"), (*literal).to_owned()))
        .collect();
    let probe = rendered
        .site()
        .probe_disclosure(&rules, Default::default())
        .expect("SDK probe");
    let mut expected = serde_json::to_vec_pretty(&probe).expect("SDK report");
    expected.push(b'\n');
    let arguments = probe_arguments();
    let input = request(&config, &selected);
    let output = cli(&input, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, expected);
    assert_eq!(cli(&input, &arguments).stdout, expected, "repeat identity");
    let report: Value = serde_json::from_slice(&output.stdout).expect("partial report");
    assert_eq!(report["status"], "partial-evidence");
    for (rule, representation) in [
        ("rule:html", "html-text"),
        ("rule:attribute", "html-attribute"),
        ("rule:json", "json-string"),
    ] {
        assert!(
            report["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .any(|finding| finding["rule"] == rule
                    && finding["representation"] == representation)
        );
    }
    assert!(report.get("directory").is_none());
    assert!(report.get("approved").is_none());
    assert_eq!(
        fs::read(fixture.directory.path().join("theme/templates/page.html"))
            .expect("unchanged template"),
        template
    );
    assert_eq!(
        native_frontier(),
        frontier_before,
        "preserve the actual initial frontier, including the fixture's unwritten index"
    );
    assert_eq!(
        serde_json::to_value(acquire(&config).inspect()).expect("fresh complete source evidence"),
        sources_before,
        "fresh native capture retains exact theme, authored and foreign source tree identities"
    );
    assert_eq!(
        fs::read_dir(fixture.directory.path())
            .expect("native root")
            .count(),
        4,
        "only .git, theme, one, other; no retained stage"
    );
    assert_eq!(
        probe.output().inventory().identity(),
        rendered.site().output().inventory().identity()
    );
}

#[test]
fn empty_findings_still_return_partial_evidence_and_match_the_sdk() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("native render");
    let rules = BTreeMap::from([(
        "rule:absent".parse().expect("rule"),
        "ABSENT-PRIVATE-NEEDLE-938491".to_owned(),
    )]);
    let probe = rendered
        .site()
        .probe_disclosure(&rules, Default::default())
        .expect("SDK probe");
    let output = cli(
        &request(&config, &[("rule:absent", "ABSENT-PRIVATE-NEEDLE-938491")]),
        &probe_arguments(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report");
    assert_eq!(report, serde_json::to_value(probe).expect("SDK shape"));
    assert_eq!(report["findings"], json!([]));
    assert_eq!(report["status"], "partial-evidence");
    for absent in ["approved", "certified", "pass", "directory"] {
        assert!(report.get(absent).is_none());
    }
}

#[test]
fn producer_declared_html_is_decoded_for_extensionless_and_misleading_routes() {
    let template = b"<p>DECLARED&#45;HTML</p>";
    let selected = [("rule:declared", "DECLARED-HTML")];
    for route in ["/declared", "/declared.json"] {
        let page = super::PAGE.replace("path='/guide/'", &format!("path='{route}'"));
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("theme/templates/page.html", template),
        ]);
        let config = fixture.config();
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("real producer-declared HTML publication");
        let path = route.trim_start_matches('/');
        let output_path = path.parse().expect("declared output path");
        assert_eq!(
            rendered.site().media_types().get(&output_path),
            Some(&gazette_sdk::render::MediaType::Html),
            "native renderer declares the format independently of its filename"
        );
        assert_eq!(
            rendered.site().output().file(&output_path),
            Some(template.as_slice())
        );
        let before = rendered.site().output().inventory().identity();
        let rules = rules(&selected);
        let probe = rendered
            .site()
            .probe_disclosure(&rules, Default::default())
            .expect("producer HTML declaration takes precedence over suffix");
        let expected = serde_json::to_value(&probe).expect("SDK report");
        let output = cli(&request(&config, &selected), &probe_arguments());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("native CLI report");
        assert_eq!(report, expected);
        assert_eq!(report["status"], "partial-evidence");
        assert!(has_finding(&report, path, "rule:declared", "html-text"));
        assert!(!has_finding(&report, path, "rule:declared", "raw"));
        let coverage = report["files"]
            .as_array()
            .expect("coverage")
            .iter()
            .find(|file| file["path"] == path)
            .expect("exact generated page coverage");
        assert_eq!(
            coverage["decoding"]["html-tokens"]["profile"],
            gazette_sdk::html::PROFILE
        );
        assert!(std::ptr::eq(probe.output(), rendered.site().output()));
        assert_eq!(probe.output().inventory().identity(), before);
        assert_eq!(
            fs::read(fixture.directory.path().join("one/page.md")).expect("native source"),
            page.as_bytes()
        );
    }
}

#[test]
fn strict_private_input_rejects_before_malformed_workspace_without_echoing_payloads() {
    let valid = request(
        "PRIVATE-WORKSPACE = [",
        &[("rule:private-rule", "PRIVATE-LITERAL")],
    );
    let mut cases = vec![
        (
            "not JSON PRIVATE-LITERAL".to_owned(),
            "disclosure_probe_input",
        ),
        (
            json!([
                "gazette-disclosure-probe-input/1",
                "PRIVATE-WORKSPACE = [",
                [{"id":"rule:private-rule","literal":"PRIVATE-LITERAL"}]
            ])
            .to_string(),
            "disclosure_probe_input",
        ),
        (
            json!({
                "format":"gazette-disclosure-probe-input/1",
                "workspace":"PRIVATE-WORKSPACE = [",
                "rules":[["rule:private-rule","PRIVATE-LITERAL"]]
            })
            .to_string(),
            "disclosure_probe_input",
        ),
        (
            valid.replace("probe-input/1", "probe-input/2"),
            "disclosure_probe_input",
        ),
        (
            valid.replace(
                "\"format\":\"gazette-disclosure-probe-input/1\"",
                "\"format\":{\"gazette-disclosure-probe-input/1\":null}",
            ),
            "disclosure_probe_input",
        ),
        (
            valid.replacen('{', "{\"unknown\":\"PRIVATE-LITERAL\",", 1),
            "disclosure_probe_input",
        ),
        (
            valid.replacen('{', "{\"workspace\":\"PRIVATE-WORKSPACE\",", 1),
            "disclosure_probe_input",
        ),
        (
            valid.replacen(
                '{',
                "{\"form\\u0061t\":\"gazette-disclosure-probe-input/1\",",
                1,
            ),
            "disclosure_probe_input",
        ),
        (
            valid.replace("\"literal\":", "\"id\":\"rule:private-rule\",\"literal\":"),
            "disclosure_probe_input",
        ),
        (
            valid.replace(
                "\"literal\":",
                "\"literal\":\"PRIVATE-LITERAL\",\"literal\":",
            ),
            "disclosure_probe_input",
        ),
        (
            valid.replace("\"literal\":", "\"extra\":\"PRIVATE-LITERAL\",\"literal\":"),
            "disclosure_probe_input",
        ),
        (format!("{valid} {{}}"), "disclosure_probe_input"),
        (
            request(
                "PRIVATE-WORKSPACE = [",
                &[
                    ("rule:private-rule", "PRIVATE-LITERAL"),
                    ("rule:private-rule", "other"),
                ],
            ),
            "disclosure_probe_input",
        ),
        (
            request("PRIVATE-WORKSPACE = [", &[]),
            "disclosure_probe_rules",
        ),
        (
            request("PRIVATE-WORKSPACE = [", &[("rule:private-rule", "")]),
            "disclosure_probe_rules",
        ),
    ];
    let rules = (0..=build::DisclosureProbeLimits::default().rules)
        .map(|index| json!({"id":format!("rule:private-rule-{index}"),"literal":"PRIVATE-LITERAL"}))
        .collect::<Vec<_>>();
    cases.push((json!({"format":"gazette-disclosure-probe-input/1","workspace":"PRIVATE-WORKSPACE = [","rules":rules}).to_string(), "disclosure_probe_limit"));
    for (input, code) in cases {
        failure(&cli(&input, &probe_arguments()), code);
    }
}

#[test]
fn private_json_and_raw_workspace_modes_are_explicit_and_input_bytes_are_exactly_bounded() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let input = request(&config, &[("rule:one", "Retained")]);
    failure(&cli(&config, &probe_arguments()), "disclosure_probe_input");
    let wrong_mode = cli(&input, &RENDER_ARGUMENTS);
    assert!(!wrong_mode.status.success());
    assert!(wrong_mode.stdout.is_empty());
    assert!(String::from_utf8_lossy(&wrong_mode.stderr).contains("toml"));
    let ordinary = cli(&config, &RENDER_ARGUMENTS);
    assert!(ordinary.status.success());
    let report: Value = serde_json::from_slice(&ordinary.stdout).expect("ordinary render report");
    assert_eq!(report["status"], "local_rendered");
    assert!(report.get("findings").is_none());
    let exact = input.len().to_string();
    let below = (input.len() - 1).to_string();
    for (bound, accepted) in [(&exact, true), (&below, false)] {
        let mut arguments = probe_arguments();
        arguments.extend(["--max-source-bytes", bound]);
        let output = cli(&input, &arguments);
        if accepted {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            failure(&output, "disclosure_probe_input_limit");
        }
    }
}

#[test]
fn probe_cannot_stage_and_help_describes_private_partial_operation() {
    let parent = tempdir().expect("unrelated output parent");
    let keep = parent.path().join("keep.txt");
    fs::write(&keep, b"unrelated retained bytes").expect("sentinel");
    let mut arguments = probe_arguments();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("parent path"),
    ]);
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(&arguments)
        .stdin(Stdio::null())
        .output()
        .expect("Clap before input");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("cannot be used with"), "{diagnostic}");
    assert!(diagnostic.contains("--probe-disclosure"));
    assert!(diagnostic.contains("--output-parent"));
    assert_eq!(
        fs::read(&keep).expect("sentinel survives"),
        b"unrelated retained bytes"
    );
    assert_eq!(fs::read_dir(parent.path()).expect("no staging").count(), 1);
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["render-site", "--help"])
        .stdin(Stdio::null())
        .output()
        .expect("help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help text");
    for phrase in [
        "--probe-disclosure",
        "private stdin JSON",
        "partial evidence",
        "never certification or staging",
        "probe JSON request",
    ] {
        assert!(help.contains(phrase), "missing {phrase}: {help}");
    }
}
