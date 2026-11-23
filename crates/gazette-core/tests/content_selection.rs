//! Declaration-owner selection through the real core resolver, not publication policy.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::publication::{
    ContentLifecycle, Key,
    content::{
        CollectionMembership, ContentKind, ContentNode, Declaration, Diagnostic, Disposition,
        EditorialRelation, Error, Graph, JourneyMember, Limits, PageFamily, ProjectDeclaration,
        ProjectDeclarationKind, ProjectEvidence, ProjectRelation,
    },
};

fn key(value: &str) -> Key {
    value.parse().expect("namespaced fixture identity")
}

fn node(id: &str, structure: ContentKind) -> ContentNode {
    ContentNode {
        id: key(id),
        site: key("site:home"),
        title: "A retained display title".into(),
        description: None,
        keywords: vec![],
        schema_type: None,
        subject: None,
        license: None,
        citation: None,
        published: None,
        modified: None,
        lifecycle: ContentLifecycle::Published,
        language: None,
        taxonomies: vec![],
        attributions: vec![],
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

fn input(nodes: Vec<ContentNode>) -> Declaration {
    Declaration {
        browsing: vec![],
        nodes,
        memberships: vec![],
        related: vec![],
        projects: vec![],
    }
}

fn project(content: &str, project: &str, kind: ProjectDeclarationKind) -> ProjectDeclaration {
    ProjectDeclaration {
        content: key(content),
        project: key(project),
        kind,
    }
}

fn resolve(input: Declaration) -> Graph {
    Graph::resolve(input, Limits::default()).expect("complete valid original graph")
}

fn decisions(graph: &Graph, included: &[&str]) -> BTreeMap<Key, Disposition> {
    graph
        .nodes()
        .map(|node| {
            (
                node.id.clone(),
                if included.contains(&node.id.as_str()) {
                    Disposition::Include
                } else {
                    Disposition::Exclude
                },
            )
        })
        .collect()
}

fn structural(error: Error) -> Vec<Diagnostic> {
    let Error::Invalid(diagnostics) = error else {
        panic!("small valid graph selection must fail a structural constraint, not its budget");
    };
    assert!(!diagnostics.is_empty());
    assert!(diagnostics.windows(2).all(|pair| pair[0] < pair[1]));
    diagnostics
}

fn mixed_collection() -> Declaration {
    let mut draft = page("content:excluded");
    draft.lifecycle = ContentLifecycle::Draft;
    let mut result = input(vec![
        collection("collection:root", None),
        page("content:included"),
        draft,
    ]);
    result.memberships = ["content:included", "content:excluded"]
        .into_iter()
        .map(|content| CollectionMembership {
            content: key(content),
            collection: key("collection:root"),
        })
        .collect();
    result.related.push(EditorialRelation {
        source: key("content:excluded"),
        target: key("content:included"),
    });
    result.projects = vec![
        project(
            "collection:root",
            "project:context",
            ProjectDeclarationKind::Context,
        ),
        project(
            "content:included",
            "project:direct",
            ProjectDeclarationKind::About,
        ),
        project(
            "content:excluded",
            "project:excluded",
            ProjectDeclarationKind::About,
        ),
    ];
    result
}

#[test]
fn mixed_collection_excludes_leaf_owned_edges_without_removing_the_selected_collection() {
    let graph = resolve(mixed_collection());
    assert_eq!(graph.memberships().len(), 2);
    assert_eq!(graph.related().len(), 1);
    let decisions = decisions(&graph, &["collection:root", "content:included"]);
    let selected = graph
        .into_selected(&decisions, Limits::default())
        .expect("closed selected declarations");
    assert_eq!(
        selected
            .nodes()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["collection:root", "content:included"]
    );
    assert_eq!(
        selected.memberships(),
        &BTreeSet::from([CollectionMembership {
            content: key("content:included"),
            collection: key("collection:root"),
        }])
    );
    assert!(
        selected.related().is_empty(),
        "editorial ownership is the excluded source, not retained target"
    );
    assert!(selected.node(&key("content:excluded")).is_none());
    assert!(
        selected
            .project_relations(&key("content:excluded"))
            .is_none()
    );
    assert_eq!(selected.declared_projects().len(), 2);
    let links = selected
        .project_relations(&key("content:included"))
        .expect("selected leaf relationships");
    assert_eq!(links.len(), 2);
    assert_eq!(
        links[&key("project:context")].relation(),
        ProjectRelation::Mentions
    );
    assert_eq!(
        links[&key("project:direct")].relation(),
        ProjectRelation::About
    );
    assert!(
        !serde_json::to_string(&selected)
            .expect("shared graph output")
            .contains("excluded")
    );
}

#[test]
fn selected_owners_cannot_silently_drop_required_parent_member_or_editorial_targets() {
    let mut leaf_parent = input(vec![
        page("content:leaf"),
        collection("collection:parent", None),
    ]);
    leaf_parent.memberships.push(CollectionMembership {
        content: key("content:leaf"),
        collection: key("collection:parent"),
    });
    let child_parent = input(vec![
        collection("collection:child", Some("collection:parent")),
        collection("collection:parent", None),
    ]);
    let traversal = input(vec![
        journey(
            "journey:ordered",
            Some("collection:parent"),
            &[("content:leaf", true)],
        ),
        collection("collection:parent", None),
        page("content:leaf"),
    ]);
    let journey_parent = input(vec![
        journey("journey:ordered", Some("collection:parent"), &[]),
        collection("collection:parent", None),
    ]);
    let mut editorial = input(vec![page("content:source"), page("content:target")]);
    editorial.related.push(EditorialRelation {
        source: key("content:source"),
        target: key("content:target"),
    });
    for (input, included, source, target) in [
        (
            leaf_parent,
            vec!["content:leaf"],
            "content:leaf",
            "collection:parent",
        ),
        (
            child_parent,
            vec!["collection:child"],
            "collection:child",
            "collection:parent",
        ),
        (
            traversal,
            vec!["journey:ordered", "collection:parent"],
            "journey:ordered",
            "content:leaf",
        ),
        (
            journey_parent,
            vec!["journey:ordered"],
            "journey:ordered",
            "collection:parent",
        ),
        (
            editorial,
            vec!["content:source"],
            "content:source",
            "content:target",
        ),
    ] {
        let graph = resolve(input);
        let decisions = decisions(&graph, &included);
        let diagnostics = structural(
            graph
                .into_selected(&decisions, Limits::default())
                .expect_err("selected declaration has excluded endpoint"),
        );
        assert_eq!(
            diagnostics,
            vec![Diagnostic {
                code: "content_reference",
                source: key(source),
                target: Some(key(target)),
            }]
        );
    }
}

#[test]
fn excluded_journey_removes_inherited_evidence_and_primary_without_changing_direct_about() {
    let mut input = input(vec![
        page("content:leaf"),
        collection("collection:root", None),
        journey(
            "journey:excluded",
            Some("collection:root"),
            &[("content:leaf", true)],
        ),
        journey("journey:retained", Some("collection:root"), &[]),
    ]);
    input.projects = vec![
        project(
            "content:leaf",
            "project:engine",
            ProjectDeclarationKind::About,
        ),
        project(
            "journey:excluded",
            "project:engine",
            ProjectDeclarationKind::Context,
        ),
        project(
            "journey:excluded",
            "project:only-excluded",
            ProjectDeclarationKind::Context,
        ),
        project(
            "content:leaf",
            "project:engine",
            ProjectDeclarationKind::Mentions,
        ),
    ];
    let graph = resolve(input);
    let original = graph
        .project_relations(&key("content:leaf"))
        .expect("original links");
    assert_eq!(original.len(), 2);
    assert_eq!(original[&key("project:engine")].evidence().len(), 3);
    assert_eq!(
        graph
            .primary_journey(&key("content:leaf"))
            .expect("original primary")
            .id,
        key("journey:excluded")
    );
    let decisions = decisions(
        &graph,
        &["content:leaf", "journey:retained", "collection:root"],
    );
    let selected = graph
        .into_selected(&decisions, Limits::default())
        .expect("excluded journey owns its context");
    let links = selected
        .project_relations(&key("content:leaf"))
        .expect("selected links");
    assert_eq!(
        links.len(),
        1,
        "context-only project does not survive its only declaration"
    );
    let link = &links[&key("project:engine")];
    assert_eq!(link.relation(), ProjectRelation::About);
    assert_eq!(
        link.evidence(),
        &BTreeSet::from([
            ProjectEvidence {
                source: key("content:leaf"),
                kind: ProjectDeclarationKind::About
            },
            ProjectEvidence {
                source: key("content:leaf"),
                kind: ProjectDeclarationKind::Mentions
            },
        ])
    );
    assert!(
        selected.primary_journey(&key("content:leaf")).is_none(),
        "an editorially unrelated journey does not acquire the retained page"
    );
    assert!(selected.editorial_parent(&key("content:leaf")).is_none());
    assert_eq!(
        selected
            .editorial_ancestry(&key("content:leaf"))
            .expect("retained standalone page")
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["content:leaf"]
    );
    assert_eq!(
        selected
            .journey_members(&key("journey:retained"))
            .expect("retained sequence"),
        &[]
    );
    assert!(
        selected
            .declared_projects()
            .iter()
            .all(|project| project.content != key("journey:excluded"))
    );
}

#[test]
fn selection_requires_exact_complete_identity_coverage_not_merely_the_same_record_count() {
    let input = input(vec![page("content:a"), page("content:b")]);
    let graph = resolve(input.clone());
    let missing = structural(
        graph
            .into_selected(&BTreeMap::new(), Limits::default())
            .expect_err("no implicit default disposition"),
    );
    assert_eq!(
        missing,
        vec![
            Diagnostic {
                code: "content_selection_missing",
                source: key("content:a"),
                target: None
            },
            Diagnostic {
                code: "content_selection_missing",
                source: key("content:b"),
                target: None
            },
        ]
    );
    let wrong = BTreeMap::from([
        (key("content:a"), Disposition::Exclude),
        (key("content:unknown"), Disposition::Exclude),
    ]);
    let graph = resolve(input.clone());
    assert_eq!(
        wrong.len(),
        graph.nodes().len(),
        "same size is not exact coverage"
    );
    let errors = structural(
        graph
            .into_selected(&wrong, Limits::default())
            .expect_err("missing and unknown dispositions"),
    );
    assert_eq!(
        errors,
        vec![
            Diagnostic {
                code: "content_selection_missing",
                source: key("content:b"),
                target: None
            },
            Diagnostic {
                code: "content_selection_unknown",
                source: key("content:unknown"),
                target: None
            },
        ]
    );
    let graph = resolve(input);
    let excluded = decisions(&graph, &[]);
    let selected = graph
        .into_selected(&excluded, Limits::default())
        .expect("explicit complete exclusion is a valid generic empty graph");
    assert_eq!(selected.nodes().len(), 0);
    assert!(selected.memberships().is_empty());
    assert!(selected.related().is_empty());
    assert!(selected.declared_projects().is_empty());
}

#[test]
fn core_selection_does_not_invent_lifecycle_or_site_policy_and_preserves_traversal_order() {
    let mut draft = page("content:draft");
    draft.lifecycle = ContentLifecycle::Draft;
    let mut archived = page("content:archived");
    archived.lifecycle = ContentLifecycle::Archived;
    let mut foreign = page("content:foreign");
    foreign.site = key("site:foreign");
    let mut declaration = input(vec![
        draft,
        archived,
        foreign,
        page("content:published"),
        collection("collection:root", None),
        journey(
            "journey:ordered",
            Some("collection:root"),
            &[
                ("content:published", true),
                ("content:draft", true),
                ("content:archived", true),
            ],
        ),
    ]);
    declaration.related.push(EditorialRelation {
        source: key("content:archived"),
        target: key("content:foreign"),
    });
    let graph = resolve(declaration);
    let before = serde_json::to_vec(&graph).expect("original resolved metadata");
    let included = graph
        .nodes()
        .map(|node| (node.id.clone(), Disposition::Include))
        .collect();
    let selected = graph
        .into_selected(&included, Limits::default())
        .expect("generic include means include even for drafts");
    assert_eq!(
        serde_json::to_vec(&selected).expect("selected metadata"),
        before
    );
    assert_eq!(
        selected
            .node(&key("content:draft"))
            .expect("draft retained")
            .lifecycle,
        ContentLifecycle::Draft
    );
    assert_eq!(
        selected
            .node(&key("content:foreign"))
            .expect("foreign standalone page retained")
            .site,
        key("site:foreign")
    );
    assert_eq!(
        selected
            .node(&key("content:archived"))
            .expect("archived member retained")
            .lifecycle,
        ContentLifecycle::Archived
    );
    assert_eq!(selected.related().len(), 1);
    assert!(selected.editorial_parent(&key("content:foreign")).is_none());
    assert_eq!(
        selected
            .journey_view(&key("journey:ordered"))
            .expect("complete ordered view")
            .steps
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["content:published", "content:draft", "content:archived"]
    );
}

#[test]
fn selected_resolution_is_order_independent_and_enforces_fresh_resource_limits() {
    let input = mixed_collection();
    let mut reordered = input.clone();
    reordered.nodes.reverse();
    reordered.memberships.reverse();
    reordered.related.reverse();
    reordered.projects.reverse();
    let first = resolve(input.clone());
    let decisions = decisions(&first, &["content:included", "collection:root"]);
    let selected = first
        .into_selected(&decisions, Limits::default())
        .expect("first order");
    let second = resolve(reordered)
        .into_selected(&decisions, Limits::default())
        .expect("opposite declaration order");
    assert_eq!(selected, second);
    for limits in [
        Limits {
            nodes: 0,
            ..Limits::default()
        },
        Limits {
            edges: 0,
            ..Limits::default()
        },
        Limits {
            text_bytes: 0,
            ..Limits::default()
        },
    ] {
        let error = resolve(input.clone())
            .into_selected(&decisions, limits)
            .expect_err("selection must obey supplied limits, not original resolution allowance");
        assert!(
            matches!(error, Error::Limit(_)),
            "actual limit failure: {error:?}"
        );
    }
    let graph = resolve(input);
    let mut oversized = decisions;
    oversized.insert(key("content:unknown-a"), Disposition::Exclude);
    oversized.insert(key("content:unknown-b"), Disposition::Exclude);
    let error = graph
        .into_selected(
            &oversized,
            Limits {
                nodes: 3,
                ..Limits::default()
            },
        )
        .expect_err(
            "decision input cannot evade the node allowance by excluding every extra identity",
        );
    assert!(
        matches!(error, Error::Limit(_)),
        "decision cap precedes diagnostic expansion: {error:?}"
    );
}
