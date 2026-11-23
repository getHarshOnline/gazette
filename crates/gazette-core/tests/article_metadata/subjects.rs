use super::*;
use gazette_core::{
    publication::{
        content::{ArticleType, SchemaType},
        registry::Projection,
    },
    source::Sha256Digest,
};

fn utility(fields: Value) -> ContentNode {
    let mut fields = fields.as_object().unwrap().clone();
    fields.insert(
        "structure".into(),
        json!({"kind":"page","family":{"kind":"utility"}}),
    );
    node(Value::Object(fields))
}
fn service() -> Value {
    json!({"kind":"service","id":"service:one","name":"Explicit service","provider":"organization:provider","description":"Explicit description"})
}
fn many(nodes: Vec<ContentNode>, limits: Limits) -> Result<Graph, Error> {
    Graph::resolve(
        Declaration {
            nodes,
            memberships: vec![],
            related: vec![],
            browsing: vec![],
            projects: vec![],
        },
        limits,
    )
}
fn projection(graph: &Graph) -> Projection<'_> {
    Projection {
        principals: Default::default(),
        projects: Default::default(),
        taxonomies: Default::default(),
        extensions: Default::default(),
        brands: Default::default(),
        sites: Default::default(),
        placements: vec![],
        content: graph,
        routes: Default::default(),
        provenance: Default::default(),
    }
}

#[test]
fn absent_subject_preserves_prechange_graph_bytes_and_registry_hash() {
    // Pinned from the pre-subject field order and graph layout, before modifying
    // the model. This is not a second serialization of the changed model.
    let legacy = include_str!("legacy_registry.json").trim_end();
    let graph = graph(node(json!({})), Limits::default()).unwrap();
    let model = projection(&graph);
    assert_eq!(
        serde_json::to_string(&("gazette.registry/v1", &model)).unwrap(),
        legacy
    );
    assert_eq!(
        model.semantic_sha256(1 << 20).unwrap(),
        Sha256Digest::of(legacy.as_bytes())
    );
    assert_eq!(
        model.semantic_sha256(1 << 20).unwrap().to_string(),
        "48075212aa0c3af2224fb95582ea1738677621bc3af965159968157d61bb8e0a"
    );
    let old_path: ArticleType = SchemaType::TechArticle;
    assert_eq!(serde_json::to_value(old_path).unwrap(), "TechArticle");
}

#[test]
fn subject_kind_and_service_facts_are_explicit_and_bounded() {
    graph(
        utility(json!({"schema_type":"AboutPage"})),
        Limits::default(),
    )
    .unwrap();
    graph(utility(json!({"schema_type":"AboutPage","subject":{"kind":"principal","principal":"person:one"}})), Limits::default()).unwrap();
    graph(utility(json!({"subject":service()})), Limits::default()).unwrap();
    for field in ["id", "name", "provider"] {
        let mut incomplete = service();
        incomplete.as_object_mut().unwrap().remove(field);
        let mut value = serde_json::to_value(utility(json!({}))).unwrap();
        value["subject"] = incomplete;
        assert!(serde_json::from_value::<ContentNode>(value).is_err());
    }
    let mut undeclared = service();
    undeclared["offers"] = json!([]);
    let mut value = serde_json::to_value(utility(json!({}))).unwrap();
    value["subject"] = undeclared;
    assert!(serde_json::from_value::<ContentNode>(value).is_err());
    for fields in [
        json!({"subject":{"kind":"principal","principal":"person:one"}}),
        json!({"schema_type":"AboutPage","subject":service()}),
        json!({"schema_type":"Article","subject":service()}),
        json!({"keywords":["article-only"],"subject":service()}),
    ] {
        assert!(graph(utility(fields), Limits::default()).is_err());
    }
    assert!(graph(node(json!({"schema_type":"AboutPage","subject":{"kind":"principal","principal":"person:one"}})), Limits::default()).is_err());
    assert!(graph(node(json!({"schema_type":"AboutPage"})), Limits::default()).is_err());
    for (field, value) in [
        ("name", ""),
        ("name", "Bad\nname"),
        ("description", " "),
        ("description", "Bad\u{0}description"),
    ] {
        let mut value_ = service();
        value_[field] = json!(value);
        assert!(graph(utility(json!({"subject":value_})), Limits::default()).is_err());
    }
    assert!(matches!(
        graph(
            utility(json!({"subject":service()})),
            Limits {
                edges: 0,
                ..Default::default()
            }
        ),
        Err(Error::Limit("edges"))
    ));
    let mut huge = service();
    huge["description"] = json!("x".repeat(1024));
    assert!(matches!(
        graph(
            utility(json!({"subject":huge})),
            Limits {
                text_bytes: 512,
                ..Default::default()
            }
        ),
        Err(Error::Limit("text bytes"))
    ));
}

#[test]
fn repeated_service_identity_requires_equal_facts_and_changes_semantic_identity() {
    let first = utility(json!({"subject":service()}));
    let mut second = first.clone();
    second.id = "content:second".parse().unwrap();
    many(vec![first.clone(), second.clone()], Limits::default()).unwrap();
    let baseline = graph(first.clone(), Limits::default()).unwrap();
    let digest = projection(&baseline).semantic_sha256(1 << 20).unwrap();
    for (field, value) in [
        ("name", "Other name"),
        ("description", "Other description"),
        ("provider", "person:other"),
        ("id", "service:other"),
    ] {
        let mut changed = service();
        changed[field] = json!(value);
        let altered = utility(json!({"subject":changed}));
        let graph = graph(altered.clone(), Limits::default()).unwrap();
        assert_ne!(digest, projection(&graph).semantic_sha256(1 << 20).unwrap());
        let mut repeated = altered;
        repeated.id = second.id.clone();
        let result = many(vec![first.clone(), repeated], Limits::default());
        if field == "id" {
            result.unwrap();
        } else {
            let Error::Invalid(errors) = result.unwrap_err() else {
                panic!("expected conflict")
            };
            assert_eq!(
                errors
                    .iter()
                    .filter(|error| error.code == "content_service_conflict")
                    .count(),
                2
            );
        }
    }
}
