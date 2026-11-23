//! Whole-graph behavior through the public resolver, not renderer or SQL stand-ins.

use gazette_core::publication::{
    ContentLifecycle, Key,
    content::{
        CollectionMembership, CollectionPurpose, ContentKind, ContentNode, Declaration, Diagnostic,
        EditorialRelation, Error, Graph, JourneyMember, Limits, PageFamily,
    },
};

#[path = "content_graph/order.rs"]
mod order;

fn key(value: &str) -> Key {
    value.parse().expect("fixture namespaced identity")
}

fn node(id: &str, structure: ContentKind) -> ContentNode {
    ContentNode {
        id: key(id),
        site: key("site:home"),
        title: "Same display title".into(),
        description: None,
        keywords: vec![],
        schema_type: None,
        subject: None,
        license: None,
        citation: None,
        published: None,
        modified: None,
        lifecycle: ContentLifecycle::Published,
        attributions: vec![],
        language: None,
        taxonomies: vec![],
        slug: None,
        route: None,
        structure,
    }
}

fn page(id: &str) -> ContentNode {
    node(
        id,
        ContentKind::Page {
            family: PageFamily::Article,
        },
    )
}

fn collection(id: &str, parent: Option<&str>) -> ContentNode {
    node(
        id,
        ContentKind::Collection {
            parent: parent.map(key),
            purpose: None,
            navigation: None,
            member_order: vec![],
        },
    )
}

fn journey(id: &str, collection: Option<&str>, members: &[(&str, bool)]) -> ContentNode {
    node(
        id,
        ContentKind::Journey {
            collection: collection.map(key),
            members: members
                .iter()
                .map(|(content, primary)| JourneyMember {
                    content: key(content),
                    primary: *primary,
                })
                .collect(),
        },
    )
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

fn resolved(declaration: Declaration) -> Graph {
    Graph::resolve(declaration, Limits::default()).expect("valid complete fixture")
}

fn invalid(declaration: Declaration) -> Vec<Diagnostic> {
    let Error::Invalid(errors) = Graph::resolve(declaration, Limits::default())
        .expect_err("invalid structure must not return a graph")
    else {
        panic!("small structural fixture must fail validation, not resource limits");
    };
    assert!(
        !errors.is_empty(),
        "rejection must identify the invalid declaration"
    );
    assert!(
        errors.windows(2).all(|pair| pair[0] < pair[1]),
        "diagnostics must be sorted and unique"
    );
    errors
}

fn mixed() -> Declaration {
    let mut draft = page("content:draft");
    draft.lifecycle = ContentLifecycle::Draft;
    let mut archived = page("content:archived");
    archived.lifecycle = ContentLifecycle::Archived;
    let mut input = declaration(vec![
        page("content:article"),
        draft,
        archived,
        collection("collection:blog", None),
        collection("collection:related", None),
        journey(
            "journey:primary",
            Some("collection:blog"),
            &[
                ("content:archived", true),
                ("content:article", true),
                ("content:draft", true),
            ],
        ),
    ]);
    input.related.push(EditorialRelation {
        source: key("content:article"),
        target: key("collection:related"),
    });
    input
}

#[test]
fn documentation_purpose_follows_only_containment_and_preserves_leaf_families() {
    let mut root = collection("collection:manual", None);
    root.structure = ContentKind::Collection {
        parent: None,
        purpose: Some(CollectionPurpose::Documentation),
        navigation: None,
        member_order: vec![],
    };
    let mut input = declaration(vec![
        root,
        collection("collection:chapter", Some("collection:manual")),
        journey(
            "journey:task",
            Some("collection:chapter"),
            &[("content:example", true)],
        ),
        page("content:example"),
        collection("collection:essays", None),
        page("content:essay"),
        node(
            "content:legacy",
            ContentKind::Page {
                family: PageFamily::Documentation,
            },
        ),
        node(
            "content:standalone",
            ContentKind::Page {
                family: PageFamily::Documentation,
            },
        ),
    ]);
    input.memberships = vec![
        CollectionMembership {
            content: key("content:essay"),
            collection: key("collection:essays"),
        },
        CollectionMembership {
            content: key("content:legacy"),
            collection: key("collection:essays"),
        },
    ];
    input.related.push(EditorialRelation {
        source: key("content:essay"),
        target: key("collection:manual"),
    });
    let graph = resolved(input);
    for id in [
        "collection:manual",
        "collection:chapter",
        "journey:task",
        "content:example",
        "content:legacy",
        "content:standalone",
    ] {
        assert!(graph.is_documentation(&key(id)), "{id}");
    }
    for id in ["collection:essays", "content:essay", "content:absent"] {
        assert!(!graph.is_documentation(&key(id)), "{id}");
    }
    assert_eq!(
        graph.editorial_parent(&key("content:legacy")).unwrap().id,
        key("collection:essays")
    );
    assert!(matches!(
        graph.node(&key("content:example")).unwrap().structure,
        ContentKind::Page {
            family: PageFamily::Article
        }
    ));
}

#[test]
fn nested_collections_cannot_change_the_inherited_purpose_in_either_direction() {
    for (parent, child) in [
        (
            CollectionPurpose::Editorial,
            CollectionPurpose::Documentation,
        ),
        (
            CollectionPurpose::Documentation,
            CollectionPurpose::Editorial,
        ),
    ] {
        let make = |purpose| {
            node(
                "collection:root",
                ContentKind::Collection {
                    parent: None,
                    purpose: Some(purpose),
                    navigation: None,
                    member_order: vec![],
                },
            )
        };
        let nested = |purpose| {
            node(
                "collection:child",
                ContentKind::Collection {
                    parent: Some(key("collection:root")),
                    purpose: Some(purpose),
                    navigation: None,
                    member_order: vec![],
                },
            )
        };
        let errors = invalid(declaration(vec![make(parent), nested(child)]));
        assert!(
            errors
                .iter()
                .any(|error| error.code == "collection_purpose_conflict"
                    && error.source == key("collection:child"))
        );
        resolved(declaration(vec![make(parent), nested(parent)]));
    }
    let unknown =
        serde_json::from_str::<ContentKind>(r#"{"kind":"collection","purpose":"tutorial"}"#);
    assert!(unknown.is_err());
}

#[test]
fn sole_journey_residence_keeps_authored_order_and_all_content_states() {
    let graph = resolved(mixed());
    assert_eq!(graph.nodes().len(), 6);
    assert_eq!(graph.memberships().len(), 0);
    assert_eq!(graph.related().len(), 1);
    assert_eq!(
        graph
            .primary_journey(&key("content:article"))
            .expect("explicit primary")
            .id,
        key("journey:primary")
    );
    assert!(graph.primary_journey(&key("content:draft")).is_some());
    assert_eq!(
        graph
            .journey_members(&key("journey:primary"))
            .expect("journey")
            .iter()
            .map(|member| member.content.as_str())
            .collect::<Vec<_>>(),
        ["content:archived", "content:article", "content:draft"]
    );
    assert_eq!(
        graph
            .node(&key("content:draft"))
            .expect("retained draft")
            .lifecycle,
        ContentLifecycle::Draft
    );
    assert_eq!(
        graph
            .node(&key("content:archived"))
            .expect("retained archive")
            .lifecycle,
        ContentLifecycle::Archived
    );
    assert!(graph.journey_members(&key("content:article")).is_none());
    assert!(graph.journey_members(&key("journey:missing")).is_none());
    let serialized = serde_json::to_value(&graph).expect("public graph projection");
    assert_eq!(serialized["nodes"]["content:draft"]["lifecycle"], "draft");
    assert_eq!(
        serialized["nodes"]["content:archived"]["lifecycle"],
        "archived"
    );
    assert_eq!(
        serialized["nodes"]["journey:primary"]["structure"]["members"][0]["content"],
        "content:archived"
    );
    assert_eq!(
        serialized["memberships"]
            .as_array()
            .expect("memberships")
            .len(),
        0
    );
    assert_eq!(serialized["related"][0]["target"], "collection:related");
}

#[test]
fn nonprimary_members_are_not_alternate_homes_and_multiple_journeys_fail() {
    let base = declaration(vec![
        page("content:a"),
        collection("collection:home", None),
        journey(
            "journey:z",
            Some("collection:home"),
            &[("content:a", false)],
        ),
        journey(
            "journey:a",
            Some("collection:home"),
            &[("content:a", false)],
        ),
    ]);
    assert!(
        invalid(base.clone())
            .iter()
            .any(|error| error.code == "journey_member_not_primary")
    );
    let mut conflict = base;
    for item in &mut conflict.nodes {
        if let ContentKind::Journey { members, .. } = &mut item.structure {
            members[0].primary = true;
        }
    }
    let first = invalid(conflict.clone());
    assert!(
        first
            .iter()
            .any(|error| error.code == "content_multiple_residence")
    );
    conflict.nodes.reverse();
    assert_eq!(first, invalid(conflict));
}

#[test]
fn journeys_require_a_declared_container_even_when_empty() {
    let failures = invalid(declaration(vec![journey("journey:standalone", None, &[])]));
    assert_eq!(failures[0].code, "journey_collection_required");
    let graph = resolved(declaration(vec![
        collection("collection:home", None),
        journey("journey:contained", Some("collection:home"), &[]),
    ]));
    assert!(
        graph
            .journey_members(&key("journey:contained"))
            .expect("valid empty traversal")
            .is_empty()
    );
    assert!(graph.memberships().is_empty());
    assert_eq!(
        graph
            .editorial_parent(&key("journey:contained"))
            .expect("container")
            .id,
        key("collection:home")
    );
}

#[test]
fn cross_site_references_preserve_identity_but_containment_is_rejected() {
    let mut alpha = page("content:alpha:guide");
    alpha.site = key("site:alpha");
    let mut beta = page("content:beta:guide");
    beta.site = key("site:beta");
    let mut parent = collection("collection:beta", None);
    parent.site = key("site:beta");
    let mut traversal = journey(
        "journey:beta",
        Some("collection:beta"),
        &[("content:beta:guide", true)],
    );
    traversal.site = key("site:beta");
    let mut input = declaration(vec![alpha, beta, parent, traversal]);
    input.related.push(EditorialRelation {
        source: key("content:beta:guide"),
        target: key("content:alpha:guide"),
    });
    let graph = resolved(input.clone());
    assert_eq!(
        graph
            .node(&key("content:alpha:guide"))
            .expect("exact alpha")
            .site,
        key("site:alpha")
    );
    assert_eq!(
        graph
            .node(&key("content:beta:guide"))
            .expect("exact beta")
            .site,
        key("site:beta")
    );
    assert_eq!(
        graph
            .primary_journey(&key("content:beta:guide"))
            .expect("same-site residence")
            .site,
        key("site:beta")
    );
    assert!(graph.primary_journey(&key("content:alpha:guide")).is_none());
    assert!(
        graph.node(&key("content:guide")).is_none(),
        "display-title equality creates no lookup alias"
    );
    let mut crossing = input.clone();
    if let ContentKind::Journey { members, .. } = &mut crossing.nodes[3].structure {
        members[0].content = key("content:alpha:guide");
    }
    assert!(
        invalid(crossing)
            .iter()
            .any(|error| error.code == "content_containment_site")
    );
    input.related[0].target = key("content:missing:guide");
    assert!(
        invalid(input)
            .iter()
            .any(|error| error.target.as_ref() == Some(&key("content:missing:guide")))
    );
}

#[test]
fn missing_and_wrong_kind_endpoints_are_rejected_in_each_structural_role() {
    for (nodes, membership, related) in [
        (
            vec![collection("collection:a", Some("collection:missing"))],
            None,
            None,
        ),
        (
            vec![
                collection("collection:a", Some("content:a")),
                page("content:a"),
            ],
            None,
            None,
        ),
        (
            vec![journey("journey:a", Some("collection:missing"), &[])],
            None,
            None,
        ),
        (
            vec![
                journey("journey:a", Some("content:a"), &[]),
                page("content:a"),
            ],
            None,
            None,
        ),
        (
            vec![journey("journey:a", None, &[("content:missing", false)])],
            None,
            None,
        ),
        (
            vec![
                journey("journey:a", None, &[("collection:a", false)]),
                collection("collection:a", None),
            ],
            None,
            None,
        ),
        (
            vec![page("content:a")],
            Some(("content:a", "collection:missing")),
            None,
        ),
        (
            vec![collection("collection:a", None)],
            Some(("content:missing", "collection:a")),
            None,
        ),
        (
            vec![page("content:a"), page("content:b")],
            Some(("content:a", "content:b")),
            None,
        ),
        (
            vec![
                collection("collection:a", None),
                collection("collection:b", None),
            ],
            Some(("collection:a", "collection:b")),
            None,
        ),
        (
            vec![page("content:a")],
            None,
            Some(("content:a", "content:missing")),
        ),
        (
            vec![page("content:a")],
            None,
            Some(("content:missing", "content:a")),
        ),
    ] {
        let mut input = declaration(nodes);
        if let Some((content, collection)) = membership {
            input.memberships.push(CollectionMembership {
                content: key(content),
                collection: key(collection),
            });
        }
        if let Some((source, target)) = related {
            input.related.push(EditorialRelation {
                source: key(source),
                target: key(target),
            });
        }
        invalid(input);
    }
}

#[test]
fn collection_cycles_fail_but_an_acyclic_hierarchy_is_retained() {
    let valid = declaration(vec![
        collection("collection:a", None),
        collection("collection:b", Some("collection:a")),
        collection("collection:c", Some("collection:b")),
    ]);
    let graph = resolved(valid);
    assert_eq!(
        graph
            .node(&key("collection:c"))
            .expect("leaf collection")
            .structure,
        ContentKind::Collection {
            parent: Some(key("collection:b")),
            purpose: None,
            navigation: None,
            member_order: vec![],
        }
    );
    invalid(declaration(vec![collection(
        "collection:self",
        Some("collection:self"),
    )]));
    let mut cycle = declaration(vec![
        collection("collection:a", Some("collection:c")),
        collection("collection:b", Some("collection:a")),
        collection("collection:c", Some("collection:b")),
    ]);
    let errors = invalid(cycle.clone());
    cycle.nodes.reverse();
    assert_eq!(errors, invalid(cycle));
}

#[test]
fn duplicates_are_errors_not_last_writer_wins_or_implicit_deduplication() {
    invalid(declaration(vec![page("content:a"), page("content:a")]));
    let mut conflicting = declaration(vec![page("content:a"), collection("content:a", None)]);
    let errors = invalid(conflicting.clone());
    conflicting.nodes.reverse();
    assert_eq!(
        errors,
        invalid(conflicting),
        "duplicate identity cannot select a different body by declaration order"
    );
    let mut membership = mixed();
    membership.memberships.push(CollectionMembership {
        content: key("content:article"),
        collection: key("collection:blog"),
    });
    membership
        .memberships
        .push(membership.memberships[0].clone());
    invalid(membership);
    let mut editorial = mixed();
    editorial.related.push(editorial.related[0].clone());
    invalid(editorial);
    for primary in [false, true] {
        invalid(declaration(vec![
            page("content:a"),
            journey(
                "journey:a",
                None,
                &[("content:a", false), ("content:a", primary)],
            ),
        ]));
    }
}

#[test]
fn unrelated_declaration_reordering_preserves_graph_bytes_but_journey_order_is_semantic() {
    let mut first = mixed();
    first.related.push(EditorialRelation {
        source: key("content:draft"),
        target: key("content:article"),
    });
    let expected = resolved(first.clone());
    let mut reordered = first.clone();
    reordered.nodes.reverse();
    reordered.memberships.reverse();
    reordered.related.reverse();
    let actual = resolved(reordered);
    assert_eq!(expected, actual);
    assert_eq!(
        serde_json::to_vec(&expected).expect("expected projection"),
        serde_json::to_vec(&actual).expect("reordered projection")
    );
    for item in &mut first.nodes {
        if let ContentKind::Journey { members, .. } = &mut item.structure {
            members.reverse();
        }
    }
    let changed = resolved(first);
    assert_ne!(expected, changed);
    assert_ne!(
        serde_json::to_vec(&expected).expect("old order"),
        serde_json::to_vec(&changed).expect("new order")
    );
}

#[test]
fn independent_failures_remain_identical_when_declarations_are_reordered() {
    let mut input = declaration(vec![
        page("content:unique"),
        collection("collection:a", Some("collection:missing")),
        journey("journey:a", None, &[("content:missing", false)]),
    ]);
    input.related = vec![EditorialRelation {
        source: key("content:missing"),
        target: key("collection:a"),
    }];
    let errors = invalid(input.clone());
    assert!(
        errors.len() >= 3,
        "independent errors must not be discarded after the first failure"
    );
    input.nodes.reverse();
    assert_eq!(
        serde_json::to_vec(&errors).expect("first diagnostics"),
        serde_json::to_vec(&invalid(input)).expect("permuted diagnostics")
    );
}

#[test]
fn editorial_ancestry_has_one_parent_and_relatedness_never_changes_it() {
    let mut input = declaration(vec![
        collection("collection:root", None),
        collection("collection:child", Some("collection:root")),
        collection("collection:related", None),
        journey(
            "journey:ordered",
            Some("collection:child"),
            &[("content:step", true)],
        ),
        page("content:step"),
        page("content:direct"),
        page("content:standalone"),
    ]);
    input.memberships.push(CollectionMembership {
        content: key("content:direct"),
        collection: key("collection:child"),
    });
    input.related.push(EditorialRelation {
        source: key("content:step"),
        target: key("collection:related"),
    });
    let graph = resolved(input);
    for (id, expected) in [
        (
            "content:step",
            vec![
                "content:step",
                "journey:ordered",
                "collection:child",
                "collection:root",
            ],
        ),
        (
            "content:direct",
            vec!["content:direct", "collection:child", "collection:root"],
        ),
        ("content:standalone", vec!["content:standalone"]),
    ] {
        assert_eq!(
            graph
                .editorial_ancestry(&key(id))
                .expect("known content")
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert!(graph.editorial_parent(&key("content:standalone")).is_none());
    assert!(graph.editorial_ancestry(&key("content:missing")).is_none());
    assert!(graph.container_ancestry(&key("content:step")).is_none());
    assert_eq!(
        graph
            .editorial_parent(&key("content:step"))
            .expect("sole residence")
            .id,
        key("journey:ordered")
    );
}

#[test]
fn direct_collection_and_journey_homes_conflict_independent_of_declaration_order() {
    for homes in [vec!["collection:a", "collection:b"], vec!["collection:a"]] {
        let mut input = declaration(vec![
            page("content:article"),
            collection("collection:a", None),
            collection("collection:b", None),
        ]);
        if homes.len() == 1 {
            input.nodes.push(journey(
                "journey:home",
                Some("collection:a"),
                &[("content:article", true)],
            ));
        }
        input.memberships = homes
            .into_iter()
            .map(|home| CollectionMembership {
                content: key("content:article"),
                collection: key(home),
            })
            .collect();
        let expected = invalid(input.clone());
        assert_eq!(expected.len(), 2);
        assert!(
            expected
                .iter()
                .all(|error| error.code == "content_multiple_residence"
                    && error.source == key("content:article"))
        );
        input.nodes.reverse();
        input.memberships.reverse();
        assert_eq!(invalid(input), expected);
    }
}

#[test]
fn aggregate_node_edge_and_text_limits_are_enforced_before_returning_a_graph() {
    let mut input = declaration(vec![
        page("content:a"),
        page("content:b"),
        collection("collection:a", None),
        collection("collection:b", Some("collection:a")),
        journey("journey:a", Some("collection:a"), &[("content:a", true)]),
    ]);
    input.memberships.push(CollectionMembership {
        content: key("content:b"),
        collection: key("collection:b"),
    });
    input.related.push(EditorialRelation {
        source: key("content:a"),
        target: key("collection:a"),
    });
    Graph::resolve(
        input.clone(),
        Limits {
            nodes: 5,
            edges: 5,
            ..Limits::default()
        },
    )
    .expect("exact structural limits");
    let node_text: usize = input
        .nodes
        .iter()
        .map(|node| node.id.as_str().len() + node.site.as_str().len() + node.title.len())
        .sum();
    // This fixture has seven reference occurrences, including both endpoints
    // of collection/editorial edges. Repeated references still consume input.
    let reference_text: usize = [
        "collection:a",
        "collection:a",
        "content:a",
        "content:b",
        "collection:b",
        "content:a",
        "collection:a",
    ]
    .iter()
    .map(|value| value.len())
    .sum();
    let total_text = node_text + reference_text;
    Graph::resolve(
        input.clone(),
        Limits {
            text_bytes: total_text,
            ..Limits::default()
        },
    )
    .expect("all reference bytes counted at the exact boundary");
    assert!(matches!(
        Graph::resolve(
            input.clone(),
            Limits {
                text_bytes: total_text - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(_))
    ));
    for limits in [
        Limits {
            nodes: 4,
            ..Limits::default()
        },
        Limits {
            edges: 4,
            ..Limits::default()
        },
        Limits {
            text_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            Graph::resolve(input.clone(), limits),
            Err(Error::Limit(_))
        ));
    }
    let leaf = page("content:exact");
    let exact_text = leaf.id.as_str().len() + leaf.site.as_str().len() + leaf.title.len();
    let one = declaration(vec![leaf]);
    Graph::resolve(
        one.clone(),
        Limits {
            text_bytes: exact_text,
            ..Limits::default()
        },
    )
    .expect("exact text boundary");
    assert!(matches!(
        Graph::resolve(
            one,
            Limits {
                text_bytes: exact_text - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(_))
    ));
}

fn documentation_outline_fixture() -> Declaration {
    use gazette_core::publication::content::NavigationItem;
    let mut root = collection("collection:manual", None);
    root.structure = ContentKind::Collection {
        parent: None,
        purpose: Some(CollectionPurpose::Documentation),
        navigation: Some(vec![NavigationItem::Group {
            group: "Use the tool".into(),
            pages: vec![
                NavigationItem::Content {
                    content: key("page:b"),
                },
                NavigationItem::Content {
                    content: key("collection:chapter"),
                },
                NavigationItem::Content {
                    content: key("page:a"),
                },
            ],
        }]),
        member_order: vec![],
    };
    let mut input = declaration(vec![
        root,
        collection("collection:chapter", Some("collection:manual")),
        page("page:a"),
        page("page:b"),
        page("page:c"),
    ]);
    input.memberships = [
        ("page:a", "collection:manual"),
        ("page:b", "collection:manual"),
        ("page:c", "collection:chapter"),
    ]
    .map(|(content, collection)| CollectionMembership {
        content: key(content),
        collection: key(collection),
    })
    .to_vec();
    input
}

#[test]
fn documentation_outline_reorders_without_changing_residence_and_uses_nearest_owner() {
    use gazette_core::publication::content::NavigationItem;
    let mut input = documentation_outline_fixture();
    let graph = resolved(input.clone());
    assert_eq!(
        graph.editorial_parent(&key("page:c")).unwrap().id,
        key("collection:chapter")
    );
    assert_eq!(
        graph
            .documentation_navigation_owner(&key("page:c"))
            .unwrap()
            .id,
        key("collection:manual")
    );
    let mut unordered = input.clone();
    let ContentKind::Collection { navigation, .. } = &mut unordered.nodes[0].structure else {
        panic!()
    };
    *navigation = None;
    assert_eq!(
        resolved(unordered)
            .documentation_navigation_owner(&key("page:c"))
            .unwrap()
            .id,
        key("collection:manual")
    );
    let ContentKind::Collection { navigation, .. } = &mut input.nodes[1].structure else {
        panic!()
    };
    *navigation = Some(vec![NavigationItem::Content {
        content: key("page:c"),
    }]);
    let graph = resolved(input);
    assert_eq!(
        graph
            .documentation_navigation_owner(&key("page:c"))
            .unwrap()
            .id,
        key("collection:chapter")
    );
    assert_eq!(
        graph
            .documentation_navigation_owner(&key("page:a"))
            .unwrap()
            .id,
        key("collection:manual")
    );
}

#[test]
fn documentation_outline_rejects_missing_duplicate_foreign_and_unknown_destinations() {
    use gazette_core::publication::content::NavigationItem;
    for (target, expected) in [
        ("page:a", "collection_navigation_duplicate"),
        ("collection:manual", "collection_navigation_outside"),
        ("page:outside", "collection_navigation_outside"),
        ("page:missing", "collection_navigation_reference"),
    ] {
        let mut input = documentation_outline_fixture();
        input.nodes.push(page("page:outside"));
        let ContentKind::Collection {
            navigation: Some(items),
            ..
        } = &mut input.nodes[0].structure
        else {
            panic!()
        };
        items.push(NavigationItem::Content {
            content: key(target),
        });
        assert!(
            invalid(input).iter().any(|error| error.code == expected),
            "{target}: {expected}"
        );
    }
    let mut input = documentation_outline_fixture();
    let ContentKind::Collection {
        navigation: Some(items),
        ..
    } = &mut input.nodes[0].structure
    else {
        panic!()
    };
    items.clear();
    let errors = invalid(input);
    assert!(
        errors
            .iter()
            .any(|error| error.code == "collection_navigation_missing"
                && error.target == Some(key("page:c")))
    );
}

#[test]
fn documentation_outline_groups_are_bounded_and_do_not_accept_unknown_fields() {
    use gazette_core::publication::content::NavigationItem;
    assert!(
        serde_json::from_value::<NavigationItem>(
            serde_json::json!({"content":"page:a", "pages":[]})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<NavigationItem>(serde_json::json!(["page:a"])).is_err());
    assert!(
        serde_json::from_value::<NavigationItem>(
            serde_json::json!({"group":"Read", "pages":[], "unknown":true})
        )
        .is_err()
    );
    let mut item = NavigationItem::Content {
        content: key("page:a"),
    };
    for _ in 0..66 {
        item = NavigationItem::Group {
            group: "Nested".into(),
            pages: vec![item],
        };
    }
    let mut input = documentation_outline_fixture();
    let ContentKind::Collection { navigation, .. } = &mut input.nodes[0].structure else {
        panic!()
    };
    *navigation = Some(vec![item]);
    assert!(matches!(
        Graph::resolve(input, Default::default()),
        Err(Error::Limit("documentation navigation depth"))
    ));
}
