//! Explicit content roles through the real bounded graph resolver.

use gazette_core::publication::{
    ContentLifecycle, Key,
    content::{
        Attribution, AttributionRole, ContentKind, ContentNode, Declaration, Diagnostic, Error,
        Graph, JourneyMember, Limits, PageFamily,
    },
};

fn key(value: &str) -> Key {
    value.parse().expect("exact namespaced fixture identity")
}

fn role(principal: &str, role: AttributionRole) -> Attribution {
    Attribution {
        principal: key(principal),
        role,
    }
}

fn page(id: &str, attributions: Vec<Attribution>) -> ContentNode {
    ContentNode {
        id: key(id),
        site: key("site:home"),
        title: "Guide".into(),
        description: None,
        keywords: vec![],
        schema_type: None,
        subject: None,
        license: None,
        citation: None,
        published: None,
        modified: None,
        lifecycle: ContentLifecycle::Published,
        attributions,
        language: None,
        taxonomies: vec![],
        slug: None,
        route: None,
        structure: ContentKind::Page {
            family: PageFamily::Documentation,
        },
    }
}

fn declaration(nodes: Vec<ContentNode>) -> Declaration {
    Declaration {
        browsing: vec![],
        nodes,
        memberships: vec![],
        related: vec![],
        projects: vec![],
    }
}

#[test]
fn explicit_roles_are_independent_and_keep_author_declaration_order() {
    let roles = vec![
        role("person:zeta", AttributionRole::Author),
        role("person:zeta", AttributionRole::RightsHolder),
        role("person:alpha", AttributionRole::Author),
    ];
    let rights_only = role("organization:publisher", AttributionRole::RightsHolder);
    let graph = Graph::resolve(
        declaration(vec![
            page("content:guide", roles.clone()),
            page("content:rights-only", vec![rights_only.clone()]),
            page("content:undeclared", vec![]),
        ]),
        Limits::default(),
    )
    .expect("one principal may explicitly hold distinct roles");
    let node = graph.node(&key("content:guide")).expect("guide");
    assert_eq!(node.attributions, roles);
    let authors: Vec<_> = node
        .attributions
        .iter()
        .filter(|entry| entry.role == AttributionRole::Author)
        .map(|entry| entry.principal.as_str())
        .collect();
    assert_eq!(authors, ["person:zeta", "person:alpha"]);
    assert_eq!(
        graph
            .node(&key("content:rights-only"))
            .expect("declared rights-only page")
            .attributions,
        [rights_only]
    );
    assert!(
        graph
            .node(&key("content:undeclared"))
            .expect("declared unattributed page")
            .attributions
            .is_empty()
    );
    let encoded = serde_json::to_value(node).expect("typed serialization");
    assert_eq!(encoded["attributions"][1]["role"], "rights-holder");
    assert_eq!(encoded["attributions"][0]["role"], "author");
    let decoded: ContentNode = serde_json::from_value(encoded).expect("typed round trip");
    assert_eq!(&decoded, node);
}

#[test]
fn omitted_roles_remain_undeclared_and_unknown_role_shapes_fail() {
    let mut encoded = serde_json::to_value(page("content:guide", vec![]))
        .expect("serialize unattributed fixture");
    encoded
        .as_object_mut()
        .expect("content node object")
        .remove("attributions");
    let decoded: ContentNode =
        serde_json::from_value(encoded).expect("omitted attributions default to empty");
    assert!(decoded.attributions.is_empty());
    for invalid in [
        serde_json::json!({"principal":"person:a","role":"publisher"}),
        serde_json::json!({"principal":"person:a","role":"rights_holder"}),
        serde_json::json!({"principal":"person:a","role":"author","consent":true}),
    ] {
        assert!(serde_json::from_value::<Attribution>(invalid).is_err());
    }
}

#[test]
fn duplicate_identical_role_fails_instead_of_deduplicating_the_claim() {
    for duplicate_role in [AttributionRole::Author, AttributionRole::RightsHolder] {
        let entry = role("person:alpha", duplicate_role);
        let result = Graph::resolve(
            declaration(vec![page("content:guide", vec![entry.clone(), entry])]),
            Limits::default(),
        );
        let Error::Invalid(diagnostics) = result.expect_err("duplicate role rejected") else {
            panic!("expected exact duplicate-attribution diagnostics");
        };
        assert_eq!(
            diagnostics,
            vec![Diagnostic {
                code: "content_duplicate_attribution",
                source: key("content:guide"),
                target: Some(key("person:alpha")),
            }]
        );
    }
}

#[test]
fn every_role_reference_consumes_shared_edge_and_text_budgets() {
    let leaf = page(
        "content:guide",
        vec![
            role("person:alpha", AttributionRole::Author),
            role("person:alpha", AttributionRole::RightsHolder),
        ],
    );
    let text_bytes = leaf.id.as_str().len()
        + leaf.site.as_str().len()
        + leaf.title.len()
        + 2 * "person:alpha".len();
    let input = declaration(vec![leaf]);
    let exact = Limits {
        nodes: 1,
        edges: 2,
        text_bytes,
    };
    Graph::resolve(input.clone(), exact).expect("exact reference budgets");
    assert!(matches!(
        Graph::resolve(input.clone(), Limits { edges: 1, ..exact }),
        Err(Error::Limit("edges"))
    ));
    assert!(matches!(
        Graph::resolve(
            input,
            Limits {
                text_bytes: text_bytes - 1,
                ..exact
            }
        ),
        Err(Error::Limit("text bytes"))
    ));
}

#[test]
fn collection_and_journey_roles_do_not_become_member_authorship() {
    let owner = role("person:container", AttributionRole::Author);
    let mut collection = page("collection:root", vec![owner.clone()]);
    collection.structure = ContentKind::Collection {
        parent: None,
        purpose: None,
        navigation: None,
        member_order: vec![],
    };
    let mut child = page("collection:child", vec![]);
    child.structure = ContentKind::Collection {
        parent: Some(key("collection:root")),
        purpose: None,
        navigation: None,
        member_order: vec![],
    };
    let mut journey = page("journey:guide", vec![owner.clone()]);
    journey.structure = ContentKind::Journey {
        collection: Some(key("collection:child")),
        members: vec![JourneyMember {
            content: key("content:guide"),
            primary: true,
        }],
    };
    let input = declaration(vec![
        collection,
        child,
        journey,
        page("content:guide", vec![]),
    ]);
    let graph = Graph::resolve(input, Limits::default()).expect("valid structural nesting");
    for id in ["collection:root", "journey:guide"] {
        assert_eq!(
            graph
                .node(&key(id))
                .expect("declared container")
                .attributions
                .as_slice(),
            std::slice::from_ref(&owner)
        );
    }
    for id in ["collection:child", "content:guide"] {
        assert!(
            graph
                .node(&key(id))
                .expect("declared descendant")
                .attributions
                .is_empty()
        );
    }
}
