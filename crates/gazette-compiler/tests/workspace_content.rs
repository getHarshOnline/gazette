//! Actual TOML compilation joined to the shared structural content resolver.

use gazette_compiler::{toml, workspace};
use gazette_core::{
    publication::{
        ContentLifecycle, Key,
        content::{
            ContentKind, ContentNode, Declaration, Diagnostic, EditorialRelation, Error, Graph,
            JourneyMember, Limits, PageFamily,
        },
    },
    source::Sha256Digest,
};

const OWNER: &str = "[[principals]]\nid='owner:publisher'\nkind='person'\n";
const ALPHA: &str = "[[sites]]\nid='site:alpha'\npublisher='owner:publisher'\ndeployable=false\n";
const BETA: &str = "[[sites]]\nid='site:beta'\npublisher='owner:publisher'\ndeployable=true\n";
const PROJECT: &str = "[[projects]]\nid='project:alpha'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n";

fn key(value: &str) -> Key {
    value.parse().expect("fixture namespaced key")
}

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, toml::Limits::default()).expect("real workspace compiler")
}

fn page(id: &str, site: &str, lifecycle: ContentLifecycle) -> ContentNode {
    ContentNode {
        id: key(id),
        site: key(site),
        title: "Guide".into(),
        description: None,
        keywords: vec![],
        schema_type: None,
        subject: None,
        license: None,
        citation: None,
        published: None,
        modified: None,
        lifecycle,
        attributions: vec![],
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

fn diagnostics<T>(result: Result<T, Error>) -> Vec<Diagnostic> {
    match result {
        Err(Error::Invalid(errors)) => errors,
        Err(error) => panic!("expected structural diagnostics, got {error}"),
        Ok(_) => panic!("invalid content must not produce a resolved join"),
    }
}

#[test]
fn single_site_join_borrows_exact_compilation_without_filtering_lifecycle_or_deployability() {
    let source = ["# exact source bytes\n", OWNER, ALPHA, PROJECT].concat();
    let compiled = compile(&source);
    let content = declaration(vec![
        page(
            "content:published",
            "site:alpha",
            ContentLifecycle::Published,
        ),
        page("content:draft", "site:alpha", ContentLifecycle::Draft),
        page("content:archived", "site:alpha", ContentLifecycle::Archived),
    ]);
    let joined = compiled
        .resolve_content(content, Limits::default())
        .expect("valid structural join");
    assert!(
        std::ptr::eq(joined.compilation(), &compiled),
        "do not manufacture a second compilation"
    );
    assert_eq!(
        joined.compilation().source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(joined.graph().nodes().len(), 3);
    for id in ["content:published", "content:draft", "content:archived"] {
        let site = joined
            .site_for(&key(id))
            .expect("each declared content home resolves");
        assert_eq!(site.id, key("site:alpha"));
        assert!(
            !site.deployable,
            "deployability is not a structural admission filter"
        );
        assert!(std::ptr::eq(
            site,
            compiled
                .workspace
                .site(&key("site:alpha"))
                .expect("native site")
        ));
    }
    let json = serde_json::to_value(joined.graph()).expect("shared graph projection");
    assert_eq!(json["nodes"]["content:draft"]["lifecycle"], "draft");
    assert_eq!(json["nodes"]["content:archived"]["lifecycle"], "archived");
    assert!(joined.site_for(&key("content:missing")).is_none());
    assert!(
        joined.site_for(&key("site:alpha")).is_none(),
        "site identity is not a content identity"
    );
    assert!(compiled.workspace.site(&key("owner:publisher")).is_none());
    assert!(compiled.workspace.site(&key("project:alpha")).is_none());
}

#[test]
fn multisite_related_links_and_site_local_journey_members_preserve_exact_homes() {
    let compiled = compile(&[OWNER, BETA, ALPHA].concat());
    let mut journey = page("journey:beta", "site:beta", ContentLifecycle::Published);
    journey.structure = ContentKind::Journey {
        collection: Some(key("collection:beta")),
        members: vec![JourneyMember {
            content: key("content:beta:guide"),
            primary: true,
        }],
    };
    let mut collection = page("collection:beta", "site:beta", ContentLifecycle::Published);
    collection.structure = ContentKind::Collection {
        parent: None,
        purpose: None,
        navigation: None,
        member_order: vec![],
    };
    let mut input = declaration(vec![
        page(
            "content:alpha:guide",
            "site:alpha",
            ContentLifecycle::Published,
        ),
        page(
            "content:beta:guide",
            "site:beta",
            ContentLifecycle::Published,
        ),
        journey,
        collection,
    ]);
    input.related.push(EditorialRelation {
        source: key("content:beta:guide"),
        target: key("content:alpha:guide"),
    });
    let joined = compiled
        .resolve_content(input, Limits::default())
        .expect("explicit references and editorial containment");
    assert_eq!(
        joined
            .site_for(&key("content:alpha:guide"))
            .expect("alpha home")
            .id,
        key("site:alpha")
    );
    assert_eq!(
        joined
            .site_for(&key("content:beta:guide"))
            .expect("beta home")
            .id,
        key("site:beta")
    );
    assert_eq!(
        joined
            .site_for(&key("journey:beta"))
            .expect("journey home")
            .id,
        key("site:beta")
    );
    assert_eq!(
        joined
            .graph()
            .primary_journey(&key("content:beta:guide"))
            .expect("named journey")
            .id,
        key("journey:beta")
    );
    assert!(
        joined
            .graph()
            .primary_journey(&key("content:alpha:guide"))
            .is_none()
    );
    assert_eq!(
        joined
            .graph()
            .related()
            .iter()
            .next()
            .expect("editorial edge")
            .target,
        key("content:alpha:guide")
    );
    assert!(
        joined.site_for(&key("content:guide")).is_none(),
        "same title does not create an inferred identity"
    );
}

#[test]
fn every_unknown_or_wrong_namespace_site_is_reported_including_draft_and_archived_nodes() {
    let compiled = compile(&[OWNER, ALPHA, PROJECT].concat());
    let mut input = declaration(vec![
        page("content:z", "site:missing", ContentLifecycle::Published),
        page("content:a", "owner:publisher", ContentLifecycle::Draft),
        page("content:m", "project:alpha", ContentLifecycle::Archived),
        page("content:valid", "site:alpha", ContentLifecycle::Published),
    ]);
    let expected: Vec<_> = [
        ("content:a", "owner:publisher"),
        ("content:m", "project:alpha"),
        ("content:z", "site:missing"),
    ]
    .into_iter()
    .map(|(source, target)| Diagnostic {
        code: "content_site_reference",
        source: key(source),
        target: Some(key(target)),
    })
    .collect();
    assert_eq!(
        diagnostics(compiled.resolve_content(input.clone(), Limits::default())),
        expected
    );
    input.nodes.reverse();
    assert_eq!(
        diagnostics(compiled.resolve_content(input, Limits::default())),
        expected
    );
}

#[test]
fn original_structural_failures_precede_site_join_without_being_rewritten() {
    let compiled = compile(&[OWNER, ALPHA].concat());
    let mut journey = page("journey:invalid", "site:missing", ContentLifecycle::Draft);
    journey.structure = ContentKind::Journey {
        collection: None,
        members: vec![JourneyMember {
            content: key("content:missing"),
            primary: true,
        }],
    };
    let input = declaration(vec![journey]);
    let original = diagnostics(Graph::resolve(input.clone(), Limits::default()));
    assert!(!original.is_empty());
    let joined = diagnostics(compiled.resolve_content(input, Limits::default()));
    assert_eq!(
        joined, original,
        "the compiler join preserves the core resolver's failure"
    );
    assert!(
        joined
            .iter()
            .all(|error| error.code != "content_site_reference")
    );
}

#[test]
fn core_limits_are_preserved_and_never_bypassed_by_site_filtering() {
    let compiled = compile(&[OWNER, ALPHA].concat());
    let input = declaration(vec![page(
        "content:a",
        "site:missing",
        ContentLifecycle::Archived,
    )]);
    for limits in [
        Limits {
            nodes: 0,
            ..Limits::default()
        },
        Limits {
            text_bytes: 1,
            ..Limits::default()
        },
    ] {
        let Err(Error::Limit(expected)) = Graph::resolve(input.clone(), limits) else {
            panic!("fixture must exceed a core resource bound");
        };
        match compiled.resolve_content(input.clone(), limits) {
            Err(Error::Limit(actual)) => assert_eq!(actual, expected),
            Err(error) => panic!("core limit was rewritten: {error}"),
            Ok(_) => panic!("resource violation returned a graph"),
        }
    }
}

#[test]
fn equivalent_workspace_order_has_equal_graphs_but_distinct_exact_source_bindings() {
    let first_source = [OWNER, ALPHA, BETA].concat();
    let second_source = [BETA, ALPHA, OWNER].concat();
    let first = compile(&first_source);
    let second = compile(&second_source);
    let input = declaration(vec![page(
        "content:one",
        "site:alpha",
        ContentLifecycle::Published,
    )]);
    let a = first
        .resolve_content(input.clone(), Limits::default())
        .expect("first join");
    let b = second
        .resolve_content(input, Limits::default())
        .expect("reordered join");
    assert_eq!(a.graph(), b.graph());
    assert_eq!(
        serde_json::to_vec(a.graph()).expect("first graph"),
        serde_json::to_vec(b.graph()).expect("second graph")
    );
    assert_eq!(
        a.site_for(&key("content:one")),
        b.site_for(&key("content:one"))
    );
    assert_ne!(a.compilation().source_sha256, b.compilation().source_sha256);
    assert_eq!(
        a.compilation().source_sha256,
        Sha256Digest::of(first_source.as_bytes())
    );
    assert_eq!(
        b.compilation().source_sha256,
        Sha256Digest::of(second_source.as_bytes())
    );
    assert!(std::ptr::eq(a.compilation(), &first));
    assert!(std::ptr::eq(b.compilation(), &second));
}
