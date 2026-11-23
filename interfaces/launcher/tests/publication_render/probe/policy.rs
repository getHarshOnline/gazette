//! Complete typed policy coverage uses the same retained decoders, never approval heuristics.

use super::{
    BTreeMap, Fixture, RetainedOutput, Value, acquire, build, fs, json, render, rules, source,
};
use build::{
    DisclosureClass as Class, DisclosurePattern as Pattern, DisclosurePolicy, DisclosureRule,
};

pub(super) fn policy() -> DisclosurePolicy {
    let entries = [
        (
            "rule:local",
            Class::LocalPath,
            Pattern::PathTree {
                root: "/private/owner".into(),
            },
        ),
        (
            "rule:host",
            Class::PrivateEndpoint,
            Pattern::Host {
                name: "hidden.example".into(),
            },
        ),
        (
            "rule:network",
            Class::PrivateEndpoint,
            Pattern::IpNetwork {
                address: "10.0.0.0".parse().expect("IPv4 network"),
                prefix: 8,
            },
        ),
        (
            "rule:secret",
            Class::Secret,
            Pattern::Literal {
                value: "PRIVATE_SECRET_619827".into(),
            },
        ),
        (
            "rule:artifact",
            Class::GeneratedArtifact,
            Pattern::Literal {
                value: "registry.sqlite-wal".into(),
            },
        ),
        (
            "rule:normalized",
            Class::GeneratedArtifact,
            Pattern::Literal {
                value: "private-name.txt".into(),
            },
        ),
        (
            "rule:fixture",
            Class::FixtureClaim,
            Pattern::Literal {
                value: "CLAIM_ONLY_CANARY".into(),
            },
        ),
        (
            "rule:internal",
            Class::InternalIdentifier,
            Pattern::Literal {
                value: "internal_record_619827".into(),
            },
        ),
    ];
    DisclosurePolicy {
        rules: entries
            .into_iter()
            .map(|(id, class, pattern)| {
                (
                    id.parse().expect("rule identity"),
                    DisclosureRule { class, pattern },
                )
            })
            .collect(),
    }
}

fn findings(fixture: &RetainedOutput, policy: &DisclosurePolicy) -> Vec<build::DisclosureFinding> {
    let error = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        policy,
        Default::default(),
    )
    .expect_err("typed matches refuse a successful report");
    assert_eq!(error.code(), "disclosure_findings");
    let display = error.to_string();
    for private in [
        "owner",
        "hidden.example",
        "PRIVATE_SECRET",
        "CLAIM_ONLY",
        "internal_record",
    ] {
        assert!(
            !display.contains(private),
            "error display discloses private values: {display}"
        );
    }
    let build::DisclosureProbeError::Findings { findings } = error else {
        panic!("supported syntax must produce typed findings, not another refusal");
    };
    findings
}

#[test]
fn complete_policy_finds_all_families_across_decoders_and_does_not_exempt_code_examples() {
    let files: &[(&str, &[u8])] = &[
        ("index.html", br#"<pre><code>/private/owner/document.md</code></pre><a href="https://hidden&#46;example:8443/">Endpoint example</a>"#),
        ("model.json", br#"{"secret":"PRIVATE\u005fSECRET_619827","endpoint":"http://10.2.3.4:8080/"}"#),
        ("style.css", br#".example { content: "CLAIM\5f ONLY_CANARY"; }"#),
        ("module.mjs", br#"export const example = "internal\u005frecord_619827";"#),
        ("registry.sqlite-wal.txt", b"Supported text, not an actual database container"),
        ("PRIVATE-NAME.txt", b"Filename normalization fixture"),
    ];
    let fixture = RetainedOutput::new(files);
    let identity = fixture.output().inventory().identity();
    let policy = policy();
    let observed = findings(&fixture, &policy);
    for (path, id, class, representation) in [
        (
            "index.html",
            "rule:local",
            Class::LocalPath,
            build::DisclosureRepresentation::HtmlText,
        ),
        (
            "index.html",
            "rule:host",
            Class::PrivateEndpoint,
            build::DisclosureRepresentation::HtmlAttribute,
        ),
        (
            "model.json",
            "rule:secret",
            Class::Secret,
            build::DisclosureRepresentation::JsonString,
        ),
        (
            "model.json",
            "rule:network",
            Class::PrivateEndpoint,
            build::DisclosureRepresentation::JsonString,
        ),
        (
            "style.css",
            "rule:fixture",
            Class::FixtureClaim,
            build::DisclosureRepresentation::CssString,
        ),
        (
            "module.mjs",
            "rule:internal",
            Class::InternalIdentifier,
            build::DisclosureRepresentation::JavascriptString,
        ),
        (
            "registry.sqlite-wal.txt",
            "rule:artifact",
            Class::GeneratedArtifact,
            build::DisclosureRepresentation::Path,
        ),
        (
            "PRIVATE-NAME.txt",
            "rule:normalized",
            Class::GeneratedArtifact,
            build::DisclosureRepresentation::NormalizedPath,
        ),
    ] {
        assert!(
            observed.iter().any(|finding| finding.path.as_str() == path
                && finding.rule.as_str() == id
                && finding.class == Some(class)
                && finding.representation == representation),
            "missing native {id} match at {path} in {representation:?}: {observed:?}"
        );
    }
    for (path, bytes) in files {
        assert_eq!(
            fs::read(fixture.directory.path().join(path)).expect("unchanged native output"),
            *bytes
        );
        assert_eq!(
            fixture.output().file(&path.parse().expect("path")),
            Some(*bytes)
        );
    }
    fs::write(
        fixture.directory.path().join("index.html"),
        b"<p>Later clean native bytes</p>",
    )
    .expect("explicit later edit");
    assert_eq!(findings(&fixture, &policy), observed);
    assert_eq!(fixture.output().inventory().identity(), identity);
}

#[test]
fn supported_boundary_nonmatches_and_real_publication_distinguish_required_from_diagnostic_evidence()
 {
    let fixture = RetainedOutput::new(&[("examples.html", br#"<pre><code>/private/ownerish/file https://sub.hidden.example/ https://hidden.example.invalid/ http://110.2.3.4/</code></pre>"#)]);
    let policy = policy();
    let report = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &policy,
        Default::default(),
    )
    .expect("lookalikes are not selected roots, exact hosts or IP network members");
    assert!(report.findings().is_empty());
    assert!(std::ptr::eq(report.output(), fixture.output()));

    let native = Fixture::new(&[]);
    let inputs = acquire(&native.config());
    let rendered = render(&inputs).expect("real source, compiler, Theme and machine output");
    let expected_identity = rendered.site().output().inventory().identity();
    let checked = build::check_disclosure_policy(
        rendered.site().output(),
        rendered.site().media_types(),
        &policy,
        Default::default(),
    )
    .expect("complete supported output with no selected matches");
    let actual = serde_json::to_value(&checked).expect("native policy scan evidence");
    assert_eq!(actual["status"], "required-supported-profile-evidence");
    assert_eq!(
        actual["filename_profile"],
        "gazette-canonical-caseless-filenames/1"
    );
    assert_eq!(actual["findings"], json!([]));
    assert_eq!(
        actual["output_identity"],
        serde_json::to_value(expected_identity).expect("exact tree identity")
    );
    let digest = source::Sha256Digest::of(
        &serde_json::to_vec(&policy).expect("exact typed policy encoding"),
    );
    assert_eq!(
        policy
            .validate(Default::default())
            .expect("complete policy"),
        digest
    );
    assert_eq!(actual["policy_sha256"], digest.to_string());
    assert!(
        actual["generic_rules_sha256"]
            .as_str()
            .expect("generic matcher profile digest")
            .parse::<source::Sha256Digest>()
            .is_ok()
    );
    assert_eq!(
        actual["findings_sha256"],
        source::Sha256Digest::of(b"[]").to_string()
    );
    for forbidden_claim in ["approved", "certified", "admitted", "release"] {
        assert!(actual.get(forbidden_claim).is_none());
    }
    let diagnostic = rendered
        .site()
        .probe_disclosure(
            &rules(&[("rule:absent", "ABSENT-DIAGNOSTIC-814927")]),
            Default::default(),
        )
        .expect("existing literal diagnostic remains available");
    let diagnostic = serde_json::to_value(diagnostic).expect("diagnostic evidence");
    assert_eq!(diagnostic["status"], "partial-evidence");
    assert!(diagnostic.get("policy_sha256").is_none());
    assert!(diagnostic.get("generic_rules_sha256").is_none());
    assert_eq!(
        rendered.site().output().inventory().identity(),
        expected_identity
    );
}

#[test]
fn complete_class_coverage_and_all_selected_bounds_fail_closed() {
    let fixture = RetainedOutput::new(&[("clean.txt", b"Clean supported output")]);
    for missing in [
        Class::LocalPath,
        Class::PrivateEndpoint,
        Class::Secret,
        Class::GeneratedArtifact,
        Class::FixtureClaim,
        Class::InternalIdentifier,
    ] {
        let mut incomplete = policy();
        incomplete.rules.retain(|_, rule| rule.class != missing);
        let error = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &incomplete,
            Default::default(),
        )
        .expect_err("one missing class is incomplete policy coverage");
        assert!(matches!(error, build::DisclosureProbeError::Policy(_)));
        assert_eq!(error.code(), "disclosure_policy");
    }
    for case in [
        "empty",
        "empty-literal",
        "invalid-network",
        "rules",
        "policy-bytes",
        "findings-evidence",
        "entries",
        "work",
        "depth",
    ] {
        let mut policy = policy();
        let mut limits = build::DisclosureProbeLimits::default();
        match case {
            "empty" => policy.rules.clear(),
            "empty-literal" => {
                policy
                    .rules
                    .get_mut(&"rule:secret".parse().expect("rule"))
                    .expect("secret rule")
                    .pattern = Pattern::Literal {
                    value: String::new(),
                }
            }
            "invalid-network" => {
                policy
                    .rules
                    .get_mut(&"rule:network".parse().expect("rule"))
                    .expect("network rule")
                    .pattern = Pattern::IpNetwork {
                    address: "10.0.0.0".parse().expect("IPv4 address"),
                    prefix: 33,
                }
            }
            "rules" => limits.rules = 1,
            "policy-bytes" => limits.rules_bytes = 1,
            "findings-evidence" => limits.evidence_bytes = 1,
            "entries" => limits.entries = 0,
            "work" => limits.work_bytes = 0,
            "depth" => limits.encoded_depth = 33,
            _ => unreachable!(),
        }
        let error =
            build::check_disclosure_policy(fixture.output(), &fixture.media_types, &policy, limits)
                .expect_err("no partial successful policy check");
        match case {
            "empty" | "empty-literal" | "invalid-network" => assert!(
                matches!(error, build::DisclosureProbeError::Policy(_)),
                "{case}: {error:?}"
            ),
            _ => assert!(
                matches!(error, build::DisclosureProbeError::Limit(_)),
                "{case}: {error:?}"
            ),
        }
        assert_eq!(
            fs::read(fixture.directory.path().join("clean.txt")).expect("unchanged native output"),
            b"Clean supported output"
        );
    }
}

#[test]
fn typed_policy_preserves_required_decoder_refusals_without_a_partial_report() {
    for (path, bytes, expected_code) in [
        (
            "z-late.mjs",
            b"export const broken = ;".as_slice(),
            "disclosure_probe_javascript",
        ),
        (
            "z-unknown.bin",
            b"plain-looking but unclassified".as_slice(),
            "disclosure_unsupported",
        ),
        (
            "z-embedded.html",
            br#"<img src="data:application/octet-stream;base64,AAE=">"#.as_slice(),
            "disclosure_unsupported",
        ),
    ] {
        let fixture =
            RetainedOutput::new(&[("a-clean.txt", b"Earlier supported file"), (path, bytes)]);
        let identity = fixture.output().inventory().identity();
        let error = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &policy(),
            Default::default(),
        )
        .expect_err("complete policy declarations cannot admit unsupported or malformed bytes");
        assert_eq!(error.code(), expected_code, "{path}: {error:?}");
        assert_eq!(fixture.output().inventory().identity(), identity);
        assert_eq!(
            fs::read(fixture.directory.path().join(path)).expect("no output rewrite"),
            bytes
        );
    }
}

#[test]
fn policy_identity_binds_classes_matchers_coordinates_and_not_map_insertion_order() {
    let fixture = RetainedOutput::new(&[("clean.txt", b"Clean supported output")]);
    let original = policy();
    let digest = original
        .validate(Default::default())
        .expect("complete policy");
    let reverse = DisclosurePolicy {
        rules: original
            .rules
            .clone()
            .into_iter()
            .rev()
            .collect::<BTreeMap<_, _>>(),
    };
    assert_eq!(
        reverse
            .validate(Default::default())
            .expect("same native ordered map"),
        digest
    );
    let original_report = build::check_disclosure_policy(
        fixture.output(),
        &fixture.media_types,
        &original,
        Default::default(),
    )
    .expect("complete original profile");
    let original_report: Value = serde_json::to_value(original_report).expect("native report");
    for change in ["class", "matcher", "coordinate"] {
        let mut changed = policy();
        match change {
            "class" => {
                changed
                    .rules
                    .get_mut(&"rule:secret".parse().expect("rule"))
                    .expect("rule")
                    .class = Class::InternalIdentifier;
                changed
                    .rules
                    .get_mut(&"rule:internal".parse().expect("rule"))
                    .expect("rule")
                    .class = Class::Secret;
            }
            "matcher" => {
                changed
                    .rules
                    .get_mut(&"rule:local".parse().expect("rule"))
                    .expect("rule")
                    .pattern = Pattern::Literal {
                    value: "/private/owner".into(),
                }
            }
            "coordinate" => {
                changed
                    .rules
                    .get_mut(&"rule:host".parse().expect("rule"))
                    .expect("rule")
                    .pattern = Pattern::Host {
                    name: "other-hidden.example".into(),
                }
            }
            _ => unreachable!(),
        }
        let changed_digest = changed
            .validate(Default::default())
            .expect("still complete policy");
        assert_ne!(changed_digest, digest, "{change}");
        let report = build::check_disclosure_policy(
            fixture.output(),
            &fixture.media_types,
            &changed,
            Default::default(),
        )
        .expect("same clean retained output under changed policy");
        let report = serde_json::to_value(report).expect("changed policy evidence");
        assert_eq!(report["policy_sha256"], changed_digest.to_string());
        assert_eq!(
            report["generic_rules_sha256"],
            original_report["generic_rules_sha256"]
        );
        assert_eq!(
            report["output_identity"],
            original_report["output_identity"]
        );
    }
}
