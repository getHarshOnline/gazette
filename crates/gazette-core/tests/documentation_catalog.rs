//! Catalog closure proves exact output artifacts rather than directory-shaped guesses.

use gazette_core::{
    documentation::{
        CatalogEvidence, CatalogLimits,
        projection::{AnchorProjection, LinkProjection, PageKind},
    },
    source::{CapturedSource, Content, InventoryBuilder, Sha256Digest},
};
use std::collections::{BTreeMap, BTreeSet};

fn output(names: &[&str]) -> CapturedSource {
    let mut inventory = InventoryBuilder::new(Default::default());
    let mut files = BTreeMap::new();
    for name in names {
        let path: gazette_core::source::EntryPath = name.parse().unwrap();
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: 1,
                    sha256: Sha256Digest::of(b"x"),
                },
            )
            .unwrap();
        files.insert(path, b"x".to_vec());
    }
    CapturedSource::from_parts(inventory.finish(), files).unwrap()
}
fn physical(companion: &str) -> LinkProjection {
    let mut links = LinkProjection::default();
    links
        .register_provider(
            "page",
            PageKind::Physical,
            Some(BTreeSet::from(["anchor".into()])),
        )
        .unwrap();
    links
        .describe(
            "page",
            "module",
            Some("Title".into()),
            Some("Body".into()),
            CatalogEvidence::Physical {
                provider: "page.mdx".parse().unwrap(),
                native_markdown: Some(companion.parse().unwrap()),
            },
            CatalogLimits::default(),
        )
        .unwrap();
    links
}
#[test]
fn companions_are_one_page_and_every_unlinked_artifact_is_required() {
    let tree = output(&["page.mdx", "page.md"]);
    let catalog = physical("page.md")
        .validate_catalog(&tree, Default::default())
        .unwrap();
    assert_eq!(catalog.output_identity(), tree.inventory().identity());
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].route.as_str(), "page");
    assert!(
        physical("page.md")
            .validate_catalog(&output(&["page.mdx"]), Default::default())
            .is_err()
    );
    assert!(
        physical("other.md")
            .validate_catalog(&output(&["page.mdx", "other.md"]), Default::default())
            .is_err()
    );
    assert!(
        physical("page.md")
            .validate_catalog(
                &tree,
                CatalogLimits {
                    anchors: 0,
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert!(
        physical("page.md")
            .validate_catalog(
                &tree,
                CatalogLimits {
                    pages: 0,
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert!(
        physical("page.md")
            .validate_catalog(
                &tree,
                CatalogLimits {
                    bytes: 1,
                    ..Default::default()
                }
            )
            .is_err()
    );
}
#[test]
fn virtual_routes_require_exact_artifacts_and_incomplete_roots_stay_separate() {
    let make = || {
        let mut links = LinkProjection::default();
        links
            .register_provider("virtual", PageKind::QualifiedVirtual, None)
            .unwrap();
        links
            .register_provider("pending", PageKind::Incomplete, None)
            .unwrap();
        links
            .describe(
                "virtual",
                "module",
                None,
                None,
                CatalogEvidence::QualifiedVirtual {
                    artifacts: BTreeSet::from(["provider.json".parse().unwrap()]),
                },
                Default::default(),
            )
            .unwrap();
        links
    };
    assert!(
        make()
            .validate_catalog(&output(&[]), Default::default())
            .is_err()
    );
    let catalog = make()
        .validate_catalog(&output(&["provider.json"]), Default::default())
        .unwrap();
    assert!(!catalog.complete());
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].route.as_str(), "virtual");
    assert!(catalog.entries()[0].anchors.is_none());
    assert!(catalog.incomplete_roots().contains("pending"));
}
#[test]
fn mapped_routes_and_scoped_anchors_preserve_the_existing_projection() {
    let mut links = LinkProjection::default()
        .with_page_routes(BTreeMap::from([("source".into(), "target".into())]))
        .unwrap()
        .with_anchor_projection(
            AnchorProjection::new("owned-", &["content"]).unwrap(),
            BTreeSet::from(["source".into()]),
        );
    links
        .register_provider(
            "source",
            PageKind::Physical,
            Some(BTreeSet::from(["content".into(), "ordinary".into()])),
        )
        .unwrap();
    let evidence = CatalogEvidence::Physical {
        provider: "target.md".parse().unwrap(),
        native_markdown: None,
    };
    links
        .describe(
            "source",
            "module",
            None,
            None,
            evidence.clone(),
            Default::default(),
        )
        .unwrap();
    assert!(
        links
            .describe("source", "module", None, None, evidence, Default::default())
            .is_err()
    );
    let catalog = links
        .validate_catalog(&output(&["target.md"]), Default::default())
        .unwrap();
    assert_eq!(catalog.entries()[0].route.as_str(), "target");
    assert_eq!(
        catalog.entries()[0].anchors.as_ref().unwrap(),
        &BTreeSet::from(["owned-content".into(), "ordinary".into()])
    );
}
#[test]
fn complete_registration_without_description_is_not_a_complete_catalog() {
    let mut links = LinkProjection::default();
    links
        .register_provider("page", PageKind::Physical, Some(BTreeSet::new()))
        .unwrap();
    assert!(
        links
            .validate_catalog(&output(&["page.md"]), Default::default())
            .is_err()
    );
}
