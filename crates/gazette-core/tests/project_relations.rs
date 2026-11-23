//! Real graph resolution of direct and inherited project links with bounded provenance.

use std::collections::BTreeSet;

use gazette_core::publication::{
    ContentLifecycle, Key,
    content::{
        CollectionMembership, ContentKind, ContentNode, Declaration, Diagnostic, EditorialRelation,
        Error, Graph, JourneyMember, Limits, PageFamily, ProjectDeclaration,
        ProjectDeclarationKind, ProjectEvidence, ProjectRelation,
    },
};

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn node(id: &str, structure: ContentKind) -> ContentNode {
    ContentNode {
        id: key(id),
        site: key("site:alpha"),
        title: "Same title".into(),
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

fn journey(id: &str, parent: Option<&str>, members: &[(&str, bool)]) -> ContentNode {
    node(
        id,
        ContentKind::Journey {
            collection: parent.map(key),
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

fn evidence(source: &str, kind: ProjectDeclarationKind) -> ProjectEvidence {
    ProjectEvidence {
        source: key(source),
        kind,
    }
}

fn resolve(declaration: Declaration) -> Graph {
    Graph::resolve(declaration, Limits::default()).expect("valid complete project graph")
}

fn diagnostics(declaration: Declaration) -> Vec<Diagnostic> {
    match Graph::resolve(declaration, Limits::default()).expect_err("invalid project declarations")
    {
        Error::Invalid(errors) => errors,
        other => panic!("structural fixture must not exhaust resources: {other:?}"),
    }
}

fn ancestral() -> Declaration {
    let mut declaration = input(vec![
        collection("collection:root", None),
        collection("collection:child", Some("collection:root")),
        collection("collection:inner", Some("collection:child")),
        journey(
            "journey:primary",
            Some("collection:inner"),
            &[("content:leaf", true)],
        ),
        page("content:leaf"),
    ]);
    declaration.projects = vec![
        project(
            "collection:root",
            "project:engine",
            ProjectDeclarationKind::Context,
        ),
        project(
            "collection:child",
            "project:engine",
            ProjectDeclarationKind::Context,
        ),
        project(
            "collection:inner",
            "project:engine",
            ProjectDeclarationKind::Context,
        ),
        project(
            "journey:primary",
            "project:engine",
            ProjectDeclarationKind::Context,
        ),
        project(
            "content:leaf",
            "project:engine",
            ProjectDeclarationKind::Mentions,
        ),
        project(
            "content:leaf",
            "project:engine",
            ProjectDeclarationKind::About,
        ),
    ];
    declaration
}

#[test]
fn about_wins_projection_while_every_distinct_declaration_origin_survives() {
    let declaration = ancestral();
    let expected_declarations: BTreeSet<_> = declaration.projects.iter().cloned().collect();
    let graph = resolve(declaration);
    assert_eq!(graph.declared_projects(), &expected_declarations);
    let links = graph
        .project_relations(&key("content:leaf"))
        .expect("known page");
    assert_eq!(
        links.len(),
        1,
        "one project, not one output per path or relation kind"
    );
    let link = &links[&key("project:engine")];
    assert_eq!(link.relation(), ProjectRelation::About);
    let expected = BTreeSet::from([
        evidence("collection:root", ProjectDeclarationKind::Context),
        evidence("collection:child", ProjectDeclarationKind::Context),
        evidence("collection:inner", ProjectDeclarationKind::Context),
        evidence("journey:primary", ProjectDeclarationKind::Context),
        evidence("content:leaf", ProjectDeclarationKind::Mentions),
        evidence("content:leaf", ProjectDeclarationKind::About),
    ]);
    assert_eq!(
        link.evidence(),
        &expected,
        "every declared ancestor and direct relation contributes one origin"
    );
    let projection = serde_json::to_value(link).expect("machine-facing project relation");
    assert_eq!(projection["relation"], "about");
    assert_eq!(
        projection["evidence"].as_array().expect("origin set").len(),
        6
    );
    assert_eq!(
        graph
            .primary_journey(&key("content:leaf"))
            .expect("sole containing journey")
            .id,
        key("journey:primary")
    );
    assert_eq!(
        graph
            .editorial_ancestry(&key("content:leaf"))
            .expect("complete declared ancestry")
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        [
            "content:leaf",
            "journey:primary",
            "collection:inner",
            "collection:child",
            "collection:root",
        ]
    );
}

#[test]
fn inherited_context_is_mentions_and_reaches_draft_and_archived_members() {
    let mut draft = page("content:draft");
    draft.lifecycle = ContentLifecycle::Draft;
    let mut archived = page("content:archived");
    archived.lifecycle = ContentLifecycle::Archived;
    let mut declaration = input(vec![
        collection("collection:root", None),
        collection("collection:child", Some("collection:root")),
        journey(
            "journey:context",
            Some("collection:child"),
            &[("content:archived", true), ("content:draft", true)],
        ),
        draft,
        archived,
    ]);
    declaration.projects.push(project(
        "collection:root",
        "project:alpha:tool",
        ProjectDeclarationKind::Context,
    ));
    let graph = resolve(declaration);
    for id in ["content:draft", "content:archived"] {
        let links = graph
            .project_relations(&key(id))
            .expect("nonpublished page is retained");
        let link = &links[&key("project:alpha:tool")];
        assert_eq!(link.relation(), ProjectRelation::Mentions);
        assert_eq!(
            link.evidence(),
            &BTreeSet::from([evidence("collection:root", ProjectDeclarationKind::Context)])
        );
        assert_eq!(
            graph
                .primary_journey(&key(id))
                .expect("member residence")
                .id,
            key("journey:context")
        );
    }
    assert_eq!(
        graph
            .node(&key("content:archived"))
            .expect("exact home")
            .site,
        key("site:alpha")
    );
    assert_eq!(
        graph
            .journey_view(&key("journey:context"))
            .expect("original order")
            .steps[0]
            .id,
        key("content:archived")
    );
}

#[test]
fn editorial_edges_titles_and_key_proximity_do_not_inherit_or_infer_identity() {
    let mut foreign = page("content:beta:guide");
    foreign.site = key("site:beta");
    let mut declaration = input(vec![
        collection("collection:root", None),
        page("content:alpha:guide"),
        foreign,
        page("content:plain"),
        journey("journey:empty", Some("collection:root"), &[]),
    ]);
    declaration.projects = vec![
        project(
            "collection:root",
            "project:unrelated",
            ProjectDeclarationKind::Context,
        ),
        project(
            "content:alpha:guide",
            "project:alpha:tool",
            ProjectDeclarationKind::About,
        ),
        project(
            "content:alpha:guide",
            "project:beta:tool",
            ProjectDeclarationKind::Mentions,
        ),
    ];
    declaration.related = vec![
        EditorialRelation {
            source: key("content:beta:guide"),
            target: key("collection:root"),
        },
        EditorialRelation {
            source: key("content:beta:guide"),
            target: key("content:alpha:guide"),
        },
        EditorialRelation {
            source: key("content:alpha:guide"),
            target: key("content:plain"),
        },
    ];
    let graph = resolve(declaration);
    let alpha = graph
        .project_relations(&key("content:alpha:guide"))
        .expect("explicit page");
    assert_eq!(alpha.len(), 2);
    assert_eq!(
        alpha[&key("project:alpha:tool")].relation(),
        ProjectRelation::About
    );
    assert_eq!(
        alpha[&key("project:beta:tool")].relation(),
        ProjectRelation::Mentions
    );
    assert!(
        !alpha.contains_key(&key("project:tool")),
        "no local-name fallback"
    );
    for id in ["content:beta:guide", "content:plain"] {
        assert!(
            graph
                .project_relations(&key(id))
                .expect("known empty page")
                .is_empty()
        );
        assert!(graph.editorial_parent(&key(id)).is_none());
    }
    assert_eq!(
        graph.related().len(),
        3,
        "cross-site references are retained"
    );
    assert_eq!(
        graph
            .node(&key("content:beta:guide"))
            .expect("foreign page")
            .site,
        key("site:beta")
    );
    for id in ["collection:root", "journey:empty", "content:missing"] {
        assert!(graph.project_relations(&key(id)).is_none());
    }
}

#[test]
fn context_flows_only_down_structural_edges_not_up_or_between_siblings() {
    let mut declaration = input(vec![
        collection("collection:root", None),
        collection("collection:left", Some("collection:root")),
        collection("collection:right", Some("collection:root")),
        page("content:root"),
        page("content:left"),
        page("content:right"),
    ]);
    for label in ["root", "left", "right"] {
        declaration.memberships.push(CollectionMembership {
            content: key(&format!("content:{label}")),
            collection: key(&format!("collection:{label}")),
        });
    }
    declaration.projects.push(project(
        "collection:left",
        "project:left",
        ProjectDeclarationKind::Context,
    ));
    let graph = resolve(declaration);
    assert_eq!(
        graph
            .project_relations(&key("content:left"))
            .expect("left leaf")
            .len(),
        1
    );
    for id in ["content:root", "content:right"] {
        assert!(
            graph
                .project_relations(&key(id))
                .expect("unaffected page")
                .is_empty()
        );
    }
}

#[test]
fn declarations_reject_missing_sources_wrong_kinds_and_exact_duplicates() {
    let mut declaration = input(vec![
        page("content:leaf"),
        collection("collection:root", None),
        collection("collection:nested", Some("collection:root")),
        journey("journey:empty", Some("collection:root"), &[]),
    ]);
    declaration.projects = vec![
        project(
            "content:missing",
            "project:x",
            ProjectDeclarationKind::About,
        ),
        project("content:leaf", "project:x", ProjectDeclarationKind::Context),
        project(
            "collection:nested",
            "project:x",
            ProjectDeclarationKind::Mentions,
        ),
        project(
            "journey:empty",
            "project:x",
            ProjectDeclarationKind::Mentions,
        ),
        project("content:leaf", "project:y", ProjectDeclarationKind::About),
        project("content:leaf", "project:y", ProjectDeclarationKind::About),
    ];
    let errors = diagnostics(declaration.clone());
    assert!(errors.windows(2).all(|pair| pair[0] < pair[1]));
    for (code, source) in [
        ("content_reference", "content:missing"),
        ("content_reference_kind", "content:leaf"),
        ("content_reference_kind", "collection:nested"),
        ("content_reference_kind", "journey:empty"),
        ("project_duplicate_declaration", "content:leaf"),
    ] {
        assert!(
            errors
                .iter()
                .any(|error| error.code == code && error.source == key(source)),
            "missing {code} at {source}: {errors:?}"
        );
    }
    assert_eq!(
        errors.len(),
        5,
        "independent declaration failures retained without invented project ownership"
    );
    declaration.nodes.reverse();
    declaration.projects.reverse();
    assert_eq!(errors, diagnostics(declaration));
}

#[test]
fn a_root_collection_subject_and_its_context_keep_independent_evidence() {
    let mut declaration = input(vec![
        collection("collection:product", None),
        page("content:task"),
    ]);
    declaration.memberships.push(CollectionMembership {
        content: key("content:task"),
        collection: key("collection:product"),
    });
    declaration.projects = vec![
        project(
            "collection:product",
            "project:one",
            ProjectDeclarationKind::About,
        ),
        project(
            "collection:product",
            "project:one",
            ProjectDeclarationKind::Context,
        ),
        project(
            "collection:product",
            "project:other",
            ProjectDeclarationKind::Mentions,
        ),
    ];
    let graph = resolve(declaration);
    let root = &graph.project_relations(&key("collection:product")).unwrap()[&key("project:one")];
    assert_eq!(root.relation(), ProjectRelation::About);
    assert_eq!(
        root.evidence(),
        &BTreeSet::from([evidence(
            "collection:product",
            ProjectDeclarationKind::About
        )])
    );
    let task = &graph.project_relations(&key("content:task")).unwrap()[&key("project:one")];
    assert_eq!(task.relation(), ProjectRelation::Mentions);
    assert_eq!(
        task.evidence(),
        &BTreeSet::from([evidence(
            "collection:product",
            ProjectDeclarationKind::Context
        )])
    );
    assert_eq!(
        graph.project_relations(&key("collection:product")).unwrap()[&key("project:other")]
            .relation(),
        ProjectRelation::Mentions
    );
    assert!(
        !graph
            .project_relations(&key("content:task"))
            .unwrap()
            .contains_key(&key("project:other")),
        "direct root mentions do not propagate"
    );
}

#[test]
fn duplicate_origins_are_not_confused_with_distinct_kinds_or_distinct_projects() {
    let mut declaration = input(vec![page("content:leaf")]);
    declaration.projects = vec![
        project("content:leaf", "project:a", ProjectDeclarationKind::About),
        project(
            "content:leaf",
            "project:a",
            ProjectDeclarationKind::Mentions,
        ),
        project(
            "content:leaf",
            "project:b",
            ProjectDeclarationKind::Mentions,
        ),
    ];
    let graph = resolve(declaration);
    assert_eq!(graph.declared_projects().len(), 3);
    let links = graph
        .project_relations(&key("content:leaf"))
        .expect("leaf links");
    assert_eq!(links.len(), 2);
    assert_eq!(links[&key("project:a")].evidence().len(), 2);
    assert_eq!(links[&key("project:a")].relation(), ProjectRelation::About);
    assert_eq!(
        links[&key("project:b")].evidence(),
        &BTreeSet::from([evidence("content:leaf", ProjectDeclarationKind::Mentions)])
    );
}

#[test]
fn reordering_declarations_preserves_graph_projection_and_provenance() {
    let input = ancestral();
    let expected = resolve(input.clone());
    let mut reordered = input;
    reordered.nodes.reverse();
    reordered.memberships.reverse();
    reordered.projects.reverse();
    reordered.related.reverse();
    let actual = resolve(reordered);
    assert_eq!(expected, actual);
    assert_eq!(
        serde_json::to_vec(&expected).expect("deterministic shared graph"),
        serde_json::to_vec(&actual).expect("reordered shared graph")
    );
    assert_eq!(
        expected.project_relations(&key("content:leaf")),
        actual.project_relations(&key("content:leaf"))
    );
}

fn fanout() -> Declaration {
    let mut declaration = input(vec![collection("collection:root", None)]);
    for index in 0..4 {
        let id = format!("content:leaf{index}");
        declaration.nodes.push(page(&id));
        declaration.memberships.push(CollectionMembership {
            content: key(&id),
            collection: key("collection:root"),
        });
        declaration.projects.push(project(
            "collection:root",
            &format!("project:tool{index}"),
            ProjectDeclarationKind::Context,
        ));
    }
    declaration
}

#[test]
fn small_declarations_cannot_expand_unbounded_derived_evidence() {
    let declaration = fanout();
    let graph = resolve(declaration.clone());
    let derived: usize = (0..4)
        .map(|index| {
            graph
                .project_relations(&key(&format!("content:leaf{index}")))
                .expect("leaf")
                .values()
                .map(|link| link.evidence().len())
                .sum::<usize>()
        })
        .sum();
    assert_eq!(derived, 16);
    assert_eq!(
        declaration.projects.len() + declaration.memberships.len(),
        8
    );
    assert!(
        matches!(
            Graph::resolve(
                declaration,
                Limits {
                    edges: 10,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("project traversal steps"))
        ),
        "a declaration within eight edges must not bypass the ten-evidence/work ceiling"
    );
}

#[test]
fn derived_logical_key_bytes_have_an_independent_resource_ceiling() {
    let declaration = fanout();
    // This fixture contains only pages, one root collection, explicit memberships
    // and project declarations: account for each supplied string exactly once.
    let declared_text: usize = declaration
        .nodes
        .iter()
        .map(|node| node.id.as_str().len() + node.site.as_str().len() + node.title.len())
        .sum::<usize>()
        + declaration
            .memberships
            .iter()
            .map(|edge| edge.content.as_str().len() + edge.collection.as_str().len())
            .sum::<usize>()
        + declaration
            .projects
            .iter()
            .map(|edge| edge.content.as_str().len() + edge.project.as_str().len())
            .sum::<usize>();
    let full = resolve(declaration.clone());
    let derived_text: usize = full
        .nodes()
        .filter_map(|node| {
            full.project_relations(&node.id).map(|links| {
                links
                    .iter()
                    .map(|(project, link)| {
                        link.evidence()
                            .iter()
                            .map(|origin| {
                                node.id.as_str().len()
                                    + project.as_str().len()
                                    + origin.source.as_str().len()
                            })
                            .sum::<usize>()
                    })
                    .sum::<usize>()
            })
        })
        .sum();
    assert!(
        derived_text > declared_text,
        "fan-out expands exact logical references"
    );
    let error = Graph::resolve(
        declaration.clone(),
        Limits {
            text_bytes: declared_text,
            ..Limits::default()
        },
    )
    .expect_err("input-sized text budget must not permit unbounded derived references");
    assert!(
        matches!(error, Error::Limit("project evidence text bytes")),
        "resource exhaustion, not semantic rejection: {error:?}"
    );
    assert!(matches!(
        Graph::resolve(
            declaration.clone(),
            Limits {
                text_bytes: derived_text - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("project evidence text bytes"))
    ));
    assert_eq!(
        Graph::resolve(
            declaration,
            Limits {
                text_bytes: derived_text,
                ..Limits::default()
            }
        )
        .expect("exact derived byte boundary"),
        full
    );
}

#[test]
fn retained_evidence_limit_is_independent_of_input_and_traversal_limits() {
    let mut declaration = fanout();
    for index in 0..4 {
        declaration.projects.push(project(
            &format!("content:leaf{index}"),
            "project:tool0",
            ProjectDeclarationKind::About,
        ));
    }
    // Twelve supplied edges, sixteen examined child edges, twenty distinct
    // origins. About changes projection but cannot erase the suppressed Context.
    assert_eq!(
        declaration.projects.len() + declaration.memberships.len(),
        12
    );
    assert!(matches!(
        Graph::resolve(
            declaration.clone(),
            Limits {
                edges: 19,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("project evidence"))
    ));
    let graph = Graph::resolve(
        declaration,
        Limits {
            edges: 20,
            ..Limits::default()
        },
    )
    .expect("exact retained-evidence boundary");
    for index in 0..4 {
        let links = graph
            .project_relations(&key(&format!("content:leaf{index}")))
            .expect("page links");
        assert_eq!(
            links[&key("project:tool0")].relation(),
            ProjectRelation::About
        );
        assert_eq!(links[&key("project:tool0")].evidence().len(), 2);
    }
}
