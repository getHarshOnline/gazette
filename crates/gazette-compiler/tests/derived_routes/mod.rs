//! Routes follow validated editorial placement without changing content identity.
mod authored;
mod browsing;
use gazette_compiler::workspace;
use gazette_core::publication::{
    Key,
    content::{Declaration, Error, Graph, Limits},
    routing::{RoutePath, RouteSlug},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const WORKSPACE: &str = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\nhome='content:home'\npublic_base_url='https://one.test/mount/'\nrouting={articles='/blog/',collections='/collections/',projects='/',pages='/'}\n";

fn key(value: &str) -> Key {
    value.parse().unwrap()
}
fn page(id: &str, slug: &str) -> Value {
    json!({"id":id,"site":"site:one","title":"Unrelated display title","lifecycle":"draft","slug":slug,"structure":{"kind":"page","family":{"kind":"article"}}})
}
fn collection(id: &str, slug: &str, parent: Option<&str>) -> Value {
    let mut value = page(id, slug);
    value["structure"] = json!({"kind":"collection","parent":parent});
    value
}
fn declaration(nodes: Vec<Value>, memberships: Value) -> Declaration {
    serde_json::from_value(
        json!({"nodes":nodes,"memberships":memberships,"related":[],"projects":[],"browsing":[]}),
    )
    .unwrap()
}
fn fixture() -> Declaration {
    let mut home = page("content:home", "unused-home-label");
    home.as_object_mut().unwrap().remove("slug");
    home["structure"]["family"]["kind"] = json!("index");
    let mut journey = page("journey:opaque", "walking");
    journey["structure"] = json!({"kind":"journey","collection":"collection:inner","members":[{"content":"content:opaque","primary":true}]});
    let mut docs = collection("collection:docs", "gadget", None);
    docs["structure"]["purpose"] = json!("documentation");
    let mut project = page("content:project", "project-label");
    project["structure"]["family"]["kind"] = json!("project");
    declaration(
        vec![
            home,
            collection("collection:root", "systems", None),
            collection("collection:inner", "practice", Some("collection:root")),
            journey,
            page("content:opaque", "first-step"),
            page("content:standalone", "standalone"),
            docs,
            page("content:help", "getting-started"),
            project,
        ],
        json!([{"content":"content:help","collection":"collection:docs"}]),
    )
}
fn has_code(result: Result<workspace::ResolvedContent<'_>, Error>, code: &str) {
    assert!(
        matches!(result, Err(Error::Invalid(ref rows)) if rows.iter().any(|row| row.code==code)),
        "expected {code}: {result:?}"
    );
}

#[test]
fn routes_use_home_root_rules_and_validated_journey_placement() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let content = config
        .resolve_content(fixture(), Default::default())
        .unwrap();
    for (id, path) in [
        ("content:home", "/"),
        ("content:standalone", "/blog/standalone/"),
        ("collection:root", "/collections/systems/"),
        ("collection:inner", "/collections/systems/practice/"),
        ("journey:opaque", "/collections/systems/practice/walking/"),
        (
            "content:opaque",
            "/collections/systems/practice/walking/first-step/",
        ),
        ("collection:docs", "/gadget/"),
        ("content:help", "/gadget/getting-started/"),
        ("content:project", "/project-label/"),
    ] {
        let node = content.graph().node(&key(id)).unwrap();
        assert_eq!(node.route.as_ref().unwrap().path.as_str(), path);
        assert_eq!(
            content.route(&key(id)).unwrap().url,
            format!("https://one.test/mount{path}")
        );
    }
    let mut moved = fixture();
    moved
        .nodes
        .iter_mut()
        .find(|n| n.id == key("collection:root"))
        .unwrap()
        .slug = Some("new-root".parse().unwrap());
    moved
        .nodes
        .iter_mut()
        .find(|n| n.id == key("content:opaque"))
        .unwrap()
        .title = "A changed title".into();
    let moved = config.resolve_content(moved, Default::default()).unwrap();
    assert_eq!(
        moved.route(&key("content:opaque")).unwrap().url,
        "https://one.test/mount/collections/new-root/practice/walking/first-step/"
    );
    assert_eq!(
        moved.graph().node(&key("content:opaque")).unwrap().title,
        "A changed title"
    );
}

#[test]
fn explicit_exceptions_and_canonical_targets_share_completed_routes() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let mut original = page("content:original", "original");
    original["structure"]["family"]["kind"] = json!("index");
    let mut copy = page("content:copy", "unused");
    copy["route"] = json!({"path":"/historical.html","aliases":["/released-old/"],"canonical":"content:original"});
    let content = config
        .resolve_content(
            declaration(vec![original, copy], json!([])),
            Default::default(),
        )
        .unwrap();
    let route = content.route(&key("content:copy")).unwrap();
    assert_eq!(route.url, "https://one.test/mount/historical.html");
    assert_eq!(route.canonical_url, "https://one.test/mount/original/");
    assert_eq!(
        content
            .graph()
            .node(&key("content:copy"))
            .unwrap()
            .route
            .as_ref()
            .unwrap()
            .aliases
            .len(),
        1
    );
}

#[test]
fn opt_in_does_not_invent_slugs_and_all_routes_still_collide_together() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let mut missing = page("content:missing", "x");
    missing.as_object_mut().unwrap().remove("slug");
    has_code(
        config.resolve_content(declaration(vec![missing], json!([])), Default::default()),
        "content_route_slug",
    );
    let no_rules = workspace::compile(
        &WORKSPACE.replace(
            "routing={articles='/blog/',collections='/collections/',projects='/',pages='/'}",
            "",
        ),
        Default::default(),
    )
    .unwrap();
    assert!(
        no_rules
            .resolve_content(
                declaration(vec![page("content:one", "one")], json!([])),
                Default::default()
            )
            .unwrap()
            .route(&key("content:one"))
            .is_none()
    );
    let mut explicit = page("content:explicit", "unused");
    explicit["route"] = json!({"path":"/blog/same/index.html"});
    has_code(
        config.resolve_content(
            declaration(vec![page("content:derived", "same"), explicit], json!([])),
            Default::default(),
        ),
        "content_route_collision",
    );
    let no_articles = workspace::compile(
        &WORKSPACE.replace("articles='/blog/',", ""),
        Default::default(),
    )
    .unwrap();
    has_code(
        no_articles.resolve_content(
            declaration(vec![page("content:one", "one")], json!([])),
            Default::default(),
        ),
        "content_route_rule",
    );
    assert!(
        workspace::compile(
            &WORKSPACE.replace("articles='/blog/'", "articles='/blog.html'"),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn invalid_containment_file_parents_and_missing_canonicals_are_rejected() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let mut root = collection("collection:root", "root", None);
    root["route"] = json!({"path":"/file.html"});
    has_code(
        config.resolve_content(
            declaration(
                vec![root, page("content:child", "child")],
                json!([{"content":"content:child","collection":"collection:root"}]),
            ),
            Default::default(),
        ),
        "content_route_parent_directory",
    );
    has_code(
        config.resolve_content(
            declaration(
                vec![
                    collection("collection:a", "a", Some("collection:b")),
                    collection("collection:b", "b", Some("collection:a")),
                ],
                json!([]),
            ),
            Default::default(),
        ),
        "collection_cycle",
    );
    let mut copy = page("content:copy", "copy");
    copy["route"] = json!({"path":"/copy/","canonical":"content:missing"});
    has_code(
        config.resolve_content(declaration(vec![copy], json!([])), Default::default()),
        "content_canonical_reference",
    );
}

#[test]
fn slugs_share_route_normalization_and_reject_path_syntax() {
    let base: RoutePath = "/docs/".parse().unwrap();
    for slug in ["café", "caf%c3%a9"] {
        let slug: RouteSlug = slug.parse().unwrap();
        let path = base.join_slug(&slug).unwrap();
        assert_eq!(path.as_str(), "/docs/caf%C3%A9/");
        assert_eq!(path.output().as_str(), "docs/café/index.html");
    }
    for slug in [
        "", ".", "..", "a/b", "a%2fb", "a\\b", "%252e", "%ff", "%xx", "a?b", "a#b", "a\nb",
    ] {
        assert!(slug.parse::<RouteSlug>().is_err(), "{slug}");
    }
}

#[test]
fn core_completion_cannot_replace_routes_or_escape_its_text_budget() {
    let input = || declaration(vec![page("content:one", "one")], json!([]));
    let result = Graph::resolve_with_routes(input(), Default::default(), |_| {
        Ok(BTreeMap::from([(
            key("content:unknown"),
            "/x/".parse().unwrap(),
        )]))
    });
    assert!(
        matches!(result,Err(Error::Invalid(ref rows)) if rows[0].code=="content_derived_route_reference")
    );
    let mut explicit = input();
    explicit.nodes[0].route = Some(serde_json::from_value(json!({"path":"/kept/"})).unwrap());
    let result = Graph::resolve_with_routes(explicit, Default::default(), |_| {
        Ok(BTreeMap::from([(
            key("content:one"),
            "/changed/".parse().unwrap(),
        )]))
    });
    assert!(
        matches!(result,Err(Error::Invalid(ref rows)) if rows[0].code=="content_derived_route_replacement")
    );
    let result = Graph::resolve_with_routes(
        input(),
        Limits {
            text_bytes: 128,
            ..Default::default()
        },
        |_| {
            Ok(BTreeMap::from([(
                key("content:one"),
                format!("/{}/", "x".repeat(100)).parse().unwrap(),
            )]))
        },
    );
    assert!(matches!(result, Err(Error::Limit("text bytes"))));
}

#[test]
fn deep_parent_routes_are_complete_and_expansion_is_bounded() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let nodes = (0..256)
        .map(|i| {
            collection(
                &format!("collection:{i}"),
                "x",
                if i == 0 {
                    None
                } else {
                    Some(format!("collection:{}", i - 1))
                }
                .as_deref(),
            )
        })
        .collect();
    let input = declaration(nodes, json!([]));
    let resolved = config
        .resolve_content(input.clone(), Default::default())
        .unwrap();
    let route = resolved.route(&key("collection:255")).unwrap();
    assert_eq!(
        route.url,
        format!("https://one.test/mount/collections/{}", "x/".repeat(256))
    );
    assert!(matches!(
        config.resolve_content(
            input,
            Limits {
                text_bytes: 32768,
                ..Default::default()
            }
        ),
        Err(Error::Limit("derived route text bytes"))
    ));
}
