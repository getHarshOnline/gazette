//! Browsing associations share native graph limits, never editorial residence.

use std::collections::BTreeMap;

use gazette_core::publication::{
    ContentLifecycle, Key,
    content::{
        BrowsingRelation, ContentKind, ContentNode, Declaration, Disposition, Error, Graph, Limits,
        PageFamily,
    },
};

fn key(value: &str) -> Key {
    value.parse().unwrap()
}
fn node(id: &str, family: PageFamily) -> ContentNode {
    ContentNode {
        id: key(id),
        site: key("site:one"),
        title: id.into(),
        description: None,
        keywords: vec![],
        schema_type: None,
        subject: None,
        license: None,
        citation: None,
        published: None,
        modified: None,
        lifecycle: ContentLifecycle::Draft,
        language: None,
        taxonomies: vec![],
        attributions: vec![],
        slug: None,
        route: Some(
            serde_json::from_value(serde_json::json!({"path":format!("/{}/",id.replace(':',"-"))}))
                .unwrap(),
        ),
        structure: ContentKind::Page { family },
    }
}
fn input() -> Declaration {
    Declaration {
        nodes: vec![
            node("content:page", PageFamily::Project),
            node("content:index-a", PageFamily::Index),
            node("content:index-b", PageFamily::Index),
        ],
        memberships: vec![],
        related: vec![],
        projects: vec![],
        browsing: vec![
            BrowsingRelation {
                source: key("content:page"),
                index: key("content:index-b"),
            },
            BrowsingRelation {
                source: key("content:page"),
                index: key("content:index-a"),
            },
        ],
    }
}
#[test]
fn multiple_direct_paths_are_sets_without_a_primary_or_editorial_parent() {
    let first = Graph::resolve(input(), Limits::default()).unwrap();
    assert_eq!(
        first
            .browsing_indexes(&key("content:page"))
            .map(Key::as_str)
            .collect::<Vec<_>>(),
        ["content:index-a", "content:index-b"]
    );
    assert!(first.editorial_parent(&key("content:page")).is_none());
    assert!(
        first
            .project_relations(&key("content:page"))
            .unwrap()
            .is_empty()
    );
    assert!(
        first
            .browsing_indexes(&key("content:index-a"))
            .next()
            .is_none()
    );
    let mut reversed = input();
    reversed.nodes.reverse();
    reversed.browsing.reverse();
    assert_eq!(first, Graph::resolve(reversed, Limits::default()).unwrap());
    let raw = serde_json::to_value(first).unwrap();
    assert!(
        raw["nodes"]["content:page"].get("browsing").is_none(),
        "raw nodes remain shallow; relationship has its own owner"
    );
    assert_eq!(raw["browsing"].as_array().unwrap().len(), 2);
}
#[test]
fn a_root_collection_can_retain_its_directory_without_an_editorial_parent() {
    let mut declaration = input();
    declaration.nodes[0].structure = ContentKind::Collection {
        parent: None,
        purpose: None,
        navigation: None,
        member_order: vec![],
    };
    let graph = Graph::resolve(declaration, Limits::default()).unwrap();
    assert_eq!(graph.browsing_indexes(&key("content:page")).count(), 2);
    assert!(graph.editorial_parent(&key("content:page")).is_none());
}

#[test]
fn invalid_endpoints_self_site_duplicates_and_unrouted_indexes_are_rejected() {
    for (case, expected) in [
        (0, "content_browsing_reference"),
        (1, "content_browsing_index"),
        (2, "content_browsing_self"),
        (3, "content_browsing_site"),
        (4, "content_browsing_route"),
        (5, "content_duplicate_browsing"),
        (6, "content_browsing_route"),
        (7, "content_browsing_reference"),
        (8, "content_browsing_source"),
    ] {
        let mut declaration = input();
        match case {
            0 => declaration.browsing[0].index = key("content:missing"),
            1 => {
                declaration.nodes[2].structure = ContentKind::Page {
                    family: PageFamily::Article,
                }
            }
            2 => declaration.browsing[0].source = key("content:index-b"),
            3 => declaration.nodes[2].site = key("site:two"),
            4 => declaration.nodes[2].route = None,
            5 => declaration.browsing.push(declaration.browsing[0].clone()),
            6 => {
                declaration.nodes[2].route.as_mut().unwrap().canonical =
                    Some(key("content:index-a"))
            }
            7 => declaration.browsing[0].source = key("content:missing-source"),
            8 => {
                declaration.nodes[0].structure = ContentKind::Collection {
                    parent: Some(key("content:index-a")),
                    purpose: None,
                    navigation: None,
                    member_order: vec![],
                }
            }
            _ => unreachable!(),
        }
        let Err(Error::Invalid(errors)) = Graph::resolve(declaration, Limits::default()) else {
            panic!("invalid browsing must refuse complete graph")
        };
        assert!(
            errors.iter().any(|e| e.code == expected),
            "{expected}: {errors:?}"
        );
    }
}
#[test]
fn browsing_input_edges_and_both_keys_hold_exact_existing_resource_bounds() {
    let declaration = input();
    let bytes = declaration
        .nodes
        .iter()
        .map(|n| {
            n.id.as_str().len()
                + n.site.as_str().len()
                + n.title.len()
                + n.route.as_ref().unwrap().path.as_str().len()
        })
        .sum::<usize>()
        + declaration
            .browsing
            .iter()
            .map(|e| e.source.as_str().len() + e.index.as_str().len())
            .sum::<usize>();
    Graph::resolve(
        declaration.clone(),
        Limits {
            edges: 2,
            text_bytes: bytes,
            ..Limits::default()
        },
    )
    .unwrap();
    for limits in [
        Limits {
            edges: 1,
            ..Limits::default()
        },
        Limits {
            text_bytes: bytes - 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            Graph::resolve(declaration.clone(), limits),
            Err(Error::Limit(_))
        ));
    }
}
#[test]
fn selection_keeps_source_ownership_and_refuses_dangling_selected_paths() {
    let graph = Graph::resolve(input(), Limits::default()).unwrap();
    let mut decisions = BTreeMap::from([
        (key("content:page"), Disposition::Exclude),
        (key("content:index-a"), Disposition::Include),
        (key("content:index-b"), Disposition::Exclude),
    ]);
    assert!(
        Graph::resolve(input(), Limits::default())
            .unwrap()
            .into_selected(&decisions, Limits::default())
            .unwrap()
            .browsing()
            .is_empty()
    );
    decisions.insert(key("content:page"), Disposition::Include);
    assert!(graph.into_selected(&decisions, Limits::default()).is_err());
}
