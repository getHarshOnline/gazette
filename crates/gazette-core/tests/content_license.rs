//! Exact declared licence references remain independent of structural membership.

use gazette_core::publication::{
    Key,
    content::{
        CollectionMembership, ContentLicense, ContentNode, Declaration, Error, Graph, Limits,
    },
};
use serde_json::{Value, json};

const LICENSE: &str = "https://licenses.example/Terms/%2fKeep?edition=2026&x=%7e#Part%2FOne";

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn node(id: &str, structure: Value) -> ContentNode {
    serde_json::from_value(json!({
        "id": id,
        "site": "site:home",
        "title": "Explicit title",
        "lifecycle": "draft",
        "structure": structure,
    }))
    .expect("unlicensed authored node")
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
fn explicit_uri_roundtrips_without_normalizing_escaped_query_or_fragment_bytes() {
    for uri in [
        LICENSE,
        "http://licenses.example:8080/terms?edition=1#section-2",
        "HTTPS://LICENSES.EXAMPLE/%7Eterms?source=a%2Fb#Part",
    ] {
        let parsed: ContentLicense = uri.parse().expect("explicit HTTP(S) URI");
        assert_eq!(parsed.as_str(), uri);
        assert_eq!(parsed.to_string(), uri);
        let encoded = serde_json::to_value(&parsed).expect("native licence serialization");
        assert_eq!(encoded, json!(uri));
        let decoded: ContentLicense = serde_json::from_value(encoded).expect("native URI decoding");
        assert_eq!(decoded, parsed);
        assert_eq!(String::from(decoded), uri);
    }
}

#[test]
fn unsafe_relative_credentials_and_malformed_uri_spellings_are_refused() {
    for uri in [
        "",
        "LICENSE",
        "/licenses/terms",
        "//licenses.example/terms",
        "https:licenses.example/terms",
        "https:///licenses.example/terms",
        "javascript:alert(1)",
        "data:text/plain,terms",
        "file:///license",
        "ftp://licenses.example/terms",
        "https://reader@licenses.example/terms",
        "https://reader:secret@licenses.example/terms",
        "https://@licenses.example/terms",
        " https://licenses.example/terms",
        "https://licenses.example/terms ",
        "https://licenses.example/a b",
        "https://licenses.example/a\nb",
        "https://licenses.example/a\tb",
        "https://licenses.example/é",
        "https://licenses.example/a\\b",
        "https://licenses.example/<terms>",
        "https://licenses.example/%",
        "https://licenses.example/%2",
        "https://licenses.example/%2g",
    ] {
        assert!(uri.parse::<ContentLicense>().is_err(), "accepted {uri:?}");
        assert!(
            serde_json::from_value::<ContentLicense>(json!(uri)).is_err(),
            "serde accepted {uri:?}"
        );
    }
    for value in [
        Value::Null,
        json!(7),
        json!([LICENSE]),
        json!({"uri": LICENSE}),
    ] {
        assert!(serde_json::from_value::<ContentLicense>(value).is_err());
    }
}

#[test]
fn absent_licence_does_not_inherit_from_parent_collection_or_membership() {
    let mut parent = node(
        "collection:parent",
        json!({"kind": "collection", "parent": null}),
    );
    parent.license = Some(LICENSE.parse().expect("explicit parent licence"));
    let child = node(
        "collection:child",
        json!({"kind": "collection", "parent": "collection:parent"}),
    );
    let page = node(
        "content:article",
        json!({"kind": "page", "family": {"kind": "article"}}),
    );
    assert!(child.license.is_none());
    assert!(page.license.is_none());
    let mut input = declaration(vec![parent, child, page]);
    input.memberships.push(CollectionMembership {
        content: key("content:article"),
        collection: key("collection:child"),
    });
    let graph = Graph::resolve(input, Limits::default()).expect("complete structural graph");
    assert_eq!(
        graph
            .node(&key("collection:parent"))
            .expect("retained parent")
            .license
            .as_ref()
            .expect("explicit parent licence")
            .as_str(),
        LICENSE
    );
    for id in ["collection:child", "content:article"] {
        let member = graph.node(&key(id)).expect("retained native member");
        assert!(member.license.is_none());
        assert!(
            serde_json::to_value(member)
                .expect("native member serialization")
                .get("license")
                .is_none()
        );
    }
    assert_eq!(graph.memberships().len(), 1);
}

#[test]
fn exact_uri_bytes_are_charged_to_the_existing_graph_text_budget() {
    let mut page = node(
        "content:article",
        json!({"kind": "page", "family": {"kind": "article"}}),
    );
    let base = page.id.as_str().len() + page.site.as_str().len() + page.title.len();
    Graph::resolve(
        declaration(vec![page.clone()]),
        Limits {
            text_bytes: base,
            ..Limits::default()
        },
    )
    .expect("no invented licence bytes when omitted");
    page.license = Some(LICENSE.parse().expect("exact URI"));
    let exact = base + LICENSE.len();
    let input = declaration(vec![page]);
    let accepted = Graph::resolve(
        input.clone(),
        Limits {
            text_bytes: exact,
            ..Limits::default()
        },
    )
    .expect("exact unchanged URI spelling fits the graph budget");
    assert_eq!(
        accepted
            .node(&key("content:article"))
            .expect("accepted article")
            .license
            .as_ref()
            .expect("explicit article licence")
            .as_str(),
        LICENSE
    );
    assert!(matches!(
        Graph::resolve(
            input,
            Limits {
                text_bytes: exact - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("text bytes"))
    ));
}
