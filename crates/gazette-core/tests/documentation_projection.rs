//! Source identity stays independent from an explicitly selected target anchor policy.

use gazette_core::documentation::projection::{AnchorProjection, LinkProjection, PageKind};
use std::collections::BTreeSet;

#[test]
fn reserved_anchor_escaping_is_injective_and_keeps_ordinary_source_ids() {
    let projection = AnchorProjection::new("gazette-anchor-", &["content", "page-title"]).unwrap();
    let source = [
        "content",
        "page-title",
        "ordinary",
        "gazette-anchor-content",
        "gazette-anchor-gazette-anchor-content",
    ];
    let target: BTreeSet<_> = source.iter().map(|id| projection.id(id)).collect();
    assert_eq!(target.len(), source.len());
    assert_eq!(projection.id("content"), "gazette-anchor-content");
    assert_eq!(
        projection.id("gazette-anchor-content"),
        "gazette-anchor-gazette-anchor-content"
    );
    assert_eq!(projection.id("ordinary"), "ordinary");
    assert!(!target.contains("content"));
    assert!(!target.contains("page-title"));
    assert!(AnchorProjection::new("", &["content"]).is_err());
    assert!(AnchorProjection::new("unsafe\"", &["content"]).is_err());
    assert!(AnchorProjection::new("gazette-anchor-", &["gazette-anchor-shell"]).is_err());
}

#[test]
fn forward_target_projection_validates_original_fragments_and_exact_physical_routes() {
    let projection = AnchorProjection::new("gazette-anchor-", &["content", "page-title"]).unwrap();
    let mut links = LinkProjection::default()
        .with_anchor_projection(projection.clone(), BTreeSet::from(["manual".into()]));
    assert_eq!(
        links.project("/manual.md#%63ontent").unwrap(),
        "/manual#gazette-anchor-%63ontent"
    );
    assert_eq!(
        links.project("/manual#gazette-anchor-content").unwrap(),
        "/manual#gazette-anchor-gazette-anchor-content"
    );
    assert_eq!(
        links.project("/virtual#content").unwrap(),
        "/virtual#content"
    );
    links
        .register_provider(
            "manual",
            PageKind::Physical,
            Some(BTreeSet::from([
                "content".into(),
                "gazette-anchor-content".into(),
            ])),
        )
        .unwrap();
    links
        .register_provider(
            "virtual",
            PageKind::QualifiedVirtual,
            Some(BTreeSet::from(["content".into()])),
        )
        .unwrap();
    links
        .validate(|route| route == "manual", |_| false)
        .unwrap();

    let mut invalid = LinkProjection::default()
        .with_anchor_projection(projection, BTreeSet::from(["manual".into()]));
    invalid.project("/manual#gazette-anchor-content").unwrap();
    invalid
        .register_provider(
            "manual",
            PageKind::Physical,
            Some(BTreeSet::from(["content".into()])),
        )
        .unwrap();
    assert!(
        invalid.validate(|_| true, |_| false).is_err(),
        "a projected target alias never becomes a source definition"
    );
}

#[test]
fn physical_route_projection_preserves_logical_closure_and_exact_anchors() {
    use std::collections::BTreeMap;
    let routes = BTreeMap::from([
        ("library/index".into(), "library/page-index".into()),
        (
            "library/index/index".into(),
            "library/index/page-index".into(),
        ),
    ]);
    let mut links = LinkProjection::default().with_page_routes(routes).unwrap();
    links
        .register_provider(
            "library/index",
            PageKind::Physical,
            Some(BTreeSet::from(["package".into()])),
        )
        .unwrap();
    links
        .register_provider(
            "library/index/index",
            PageKind::Physical,
            Some(BTreeSet::from(["module".into()])),
        )
        .unwrap();
    assert_eq!(
        links.project("/library/index#package").unwrap(),
        "/library/page-index#package"
    );
    assert_eq!(
        links.project("/library/index/index.md#module").unwrap(),
        "/library/index/page-index#module"
    );
    let mut checked = BTreeSet::new();
    links
        .validate(
            |route| {
                checked.insert(route.to_owned());
                true
            },
            |_| false,
        )
        .unwrap();
    assert_eq!(
        checked,
        BTreeSet::from([
            "library/page-index".into(),
            "library/index/page-index".into()
        ])
    );
    assert!(
        LinkProjection::default()
            .with_page_routes(BTreeMap::from([
                ("one".into(), "same".into()),
                ("two".into(), "same".into())
            ]))
            .is_err()
    );
    assert!(
        LinkProjection::default()
            .with_page_routes(BTreeMap::from([("one".into(), "../outside".into())]))
            .is_err()
    );
}
