//! Immutable-output qualification accepts only Gazette's bounded `MathML` profile.

use std::collections::BTreeMap;

use gazette_application::build::{
    DisclosureClass, DisclosurePattern, DisclosurePolicy, DisclosureProbeError, DisclosureRule,
    check_disclosure_policy,
};
use gazette_core::{
    publication::Key,
    raster,
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest},
};

fn policy(needle: &str) -> DisclosurePolicy {
    let classes = [
        DisclosureClass::LocalPath,
        DisclosureClass::PrivateEndpoint,
        DisclosureClass::Secret,
        DisclosureClass::GeneratedArtifact,
        DisclosureClass::FixtureClaim,
        DisclosureClass::InternalIdentifier,
    ];
    DisclosurePolicy {
        rules: classes
            .into_iter()
            .enumerate()
            .map(|(index, class)| {
                (
                    format!("rule:math-{index}").parse::<Key>().unwrap(),
                    DisclosureRule {
                        class,
                        pattern: DisclosurePattern::Literal {
                            value: if index == 0 {
                                needle.to_owned()
                            } else {
                                format!("ABSENT-MATH-CANARY-{index}")
                            },
                        },
                    },
                )
            })
            .collect(),
    }
}

fn output(html: &str) -> CapturedSource {
    let path: EntryPath = "index.html".parse().unwrap();
    let bytes = html.as_bytes().to_vec();
    let mut inventory = InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(&bytes),
            },
        )
        .unwrap();
    CapturedSource::from_parts(inventory.finish(), BTreeMap::from([(path, bytes)])).unwrap()
}

fn math(annotation: &str) -> String {
    format!(
        concat!(
            "<!doctype html><html><body>",
            "<math xmlns='http://www.w3.org/1998/Math/MathML'>",
            "<semantics><msup><mi>x</mi><mn>2</mn></msup>",
            "<annotation encoding='application/x-tex'>{}</annotation>",
            "</semantics></math></body></html>"
        ),
        annotation,
    )
}

#[test]
fn bounded_mathml_reaches_required_disclosure_qualification() {
    let output = output(&math("x^2"));
    let report = check_disclosure_policy(
        &output,
        &BTreeMap::new(),
        &policy("ABSENT-MATH-CANARY"),
        &raster::unavailable_metadata,
        Default::default(),
    )
    .expect("safe retained MathML must qualify through the immutable output scanner");
    assert!(report.findings().is_empty());
    assert_eq!(
        report.output().inventory().identity(),
        output.inventory().identity()
    );
}

#[test]
fn mathml_text_remains_visible_to_owner_selected_disclosure_rules() {
    let output = output(&math("PRIVATE-MATH-CANARY"));
    let error = check_disclosure_policy(
        &output,
        &BTreeMap::new(),
        &policy("PRIVATE-MATH-CANARY"),
        &raster::unavailable_metadata,
        Default::default(),
    )
    .expect_err("a safe structure cannot hide prohibited decoded text");
    assert!(matches!(error, DisclosureProbeError::Findings { .. }));
}

#[test]
fn active_or_foreign_mathml_never_qualifies_as_safe_presentation() {
    for unsafe_html in [
        math("x^2").replace("<mi>x</mi>", "<mi onclick='run()'>x</mi>"),
        math("x^2").replace("<mi>x</mi>", "<script>x</script>"),
        math("x^2").replace("1998/Math/MathML", "1999/Math/MathML"),
    ] {
        let output = output(&unsafe_html);
        let error = check_disclosure_policy(
            &output,
            &BTreeMap::new(),
            &policy("ABSENT-MATH-CANARY"),
            &raster::unavailable_metadata,
            Default::default(),
        )
        .expect_err("unsupported MathML must fail closed");
        assert!(matches!(error, DisclosureProbeError::Html { .. }));
    }
}
