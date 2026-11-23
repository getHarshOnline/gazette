//! Certification binds one cross-platform filename profile without rewriting retained names.

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

fn policy() -> DisclosurePolicy {
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
                    format!("rule:absent-{index}").parse::<Key>().unwrap(),
                    DisclosureRule {
                        class,
                        pattern: DisclosurePattern::Literal {
                            value: format!("ABSENT-CANARY-{index}"),
                        },
                    },
                )
            })
            .collect(),
    }
}

fn captured(paths: &[&str]) -> CapturedSource {
    let mut inventory = InventoryBuilder::new(Default::default());
    let mut files = BTreeMap::new();
    for (index, value) in paths.iter().enumerate() {
        let path: EntryPath = value.parse().unwrap();
        let bytes = format!("supported text {index}").into_bytes();
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(&bytes),
                },
            )
            .unwrap();
        files.insert(path, bytes);
    }
    CapturedSource::from_parts(inventory.finish(), files).unwrap()
}

#[test]
fn canonically_equivalent_output_names_refuse_certification_without_rewriting() {
    let output = captured(&["caf\u{e9}.txt", "cafe\u{301}.txt"]);
    let identity = output.inventory().identity();
    let error = check_disclosure_policy(
        &output,
        &BTreeMap::new(),
        &policy(),
        &raster::unavailable_metadata,
        Default::default(),
    )
    .expect_err("equivalent names cannot share one certified destination namespace");
    assert!(matches!(error, DisclosureProbeError::Inventory));
    assert_eq!(output.inventory().identity(), identity);
    assert!(output.file(&"caf\u{e9}.txt".parse().unwrap()).is_some());
    assert!(output.file(&"cafe\u{301}.txt".parse().unwrap()).is_some());
}
