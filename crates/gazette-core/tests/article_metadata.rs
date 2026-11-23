//! Article discovery facts remain distinct from classification and editorial residence.

use gazette_core::publication::content::{ContentNode, Declaration, Error, Graph, Limits};
use serde_json::{Value, json};

#[path = "article_metadata/subjects.rs"]
mod subjects;

fn node(fields: Value) -> ContentNode {
    let mut value = json!({
        "id":"article:one", "site":"site:one", "title":"One", "lifecycle":"draft",
        "structure":{"kind":"page","family":{"kind":"article"}}
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    serde_json::from_value(value).expect("typed metadata")
}

fn graph(node: ContentNode, limits: Limits) -> Result<Graph, Error> {
    Graph::resolve(
        Declaration {
            browsing: vec![],
            nodes: vec![node],
            memberships: vec![],
            related: vec![],
            projects: vec![],
        },
        limits,
    )
}

#[test]
fn contribution_types_and_keyword_order_are_retained_without_taxonomy_or_membership() {
    for kind in ["Article", "BlogPosting", "TechArticle"] {
        let graph = graph(
            node(json!({"keywords":["typed contracts", "Rust"], "schema_type":kind})),
            Limits::default(),
        )
        .unwrap();
        let value = serde_json::to_value(graph.nodes().next().unwrap()).unwrap();
        assert_eq!(value["schema_type"], kind);
        assert_eq!(value["keywords"], json!(["typed contracts", "Rust"]));
        assert!(value.get("taxonomies").is_none());
        assert!(graph.memberships().is_empty());
    }
    let absent = serde_json::to_value(node(json!({}))).unwrap();
    assert!(absent.get("keywords").is_none());
    assert!(absent.get("schema_type").is_none());
}

#[test]
fn empty_control_padded_duplicate_and_wrong_kind_metadata_fail_closed() {
    for (fields, code) in [
        (json!({"keywords":[""]}), "content_keyword"),
        (json!({"keywords":[" padded"]}), "content_keyword"),
        (json!({"keywords":["line\nbreak"]}), "content_keyword"),
        (
            json!({"keywords":["same", "same"]}),
            "content_duplicate_keyword",
        ),
        (
            json!({"schema_type":"TechArticle", "structure":{"kind":"collection"}}),
            "content_article_metadata_kind",
        ),
        (
            json!({"keywords":["topic"], "structure":{"kind":"page", "family":{"kind":"documentation"}}}),
            "content_article_metadata_kind",
        ),
    ] {
        let Error::Invalid(errors) = graph(node(fields), Limits::default()).expect_err(code) else {
            panic!("expected metadata diagnostic")
        };
        assert!(errors.iter().any(|error| error.code == code), "{errors:?}");
    }
    let mut value = serde_json::to_value(node(json!({}))).unwrap();
    for unsupported in [json!("NewsArticle"), json!(true), json!(["Article"])] {
        value["schema_type"] = unsupported;
        assert!(serde_json::from_value::<ContentNode>(value.clone()).is_err());
    }
}

#[test]
fn keyword_bytes_participate_in_the_whole_graph_budget() {
    let limits = Limits {
        text_bytes: 30,
        ..Limits::default()
    };
    graph(node(json!({})), limits).expect("base metadata fits");
    assert!(matches!(
        graph(node(json!({"keywords":["x".repeat(31)]})), limits),
        Err(Error::Limit("text bytes"))
    ));
}
