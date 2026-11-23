//! Language grammar and explicit classification through the bounded core resolver.

use gazette_core::publication::{
    ContentLifecycle, Key, LanguageTag,
    content::{
        CollectionMembership, ContentKind, ContentNode, Declaration, Error, Graph, Limits,
        PageFamily, TaxonomyMembership,
    },
};

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn membership(taxonomy: &str, term: &str) -> TaxonomyMembership {
    TaxonomyMembership {
        taxonomy: key(taxonomy),
        term: key(term),
    }
}

fn page() -> ContentNode {
    ContentNode {
        id: key("content:guide"),
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
        lifecycle: ContentLifecycle::Draft,
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

#[test]
fn language_tags_normalize_case_without_replacing_owner_language_choices() {
    for (input, expected) in [
        ("EN-us", "en-US"),
        ("sR-lATN-rs", "sr-Latn-RS"),
        ("es-419", "es-419"),
        ("de-CH-1901", "de-CH-1901"),
        ("en-US-u-CA-gregory", "en-US-u-ca-gregory"),
        ("X-PRIVATE", "x-private"),
        ("i-klingon", "i-klingon"),
        ("en-GB-oed", "en-GB-oed"),
        ("zh-min-nan", "zh-min-nan"),
        ("iw", "iw"),
    ] {
        let language: LanguageTag = input.parse().expect("well-formed language tag");
        assert_eq!(language.as_str(), expected, "input {input}");
        assert_eq!(language.to_string(), expected);
        let encoded = serde_json::to_value(&language).expect("language serialization");
        assert_eq!(encoded, expected);
        let decoded: LanguageTag = serde_json::from_value(encoded).expect("language round trip");
        assert_eq!(decoded.as_str(), expected);
        let normalized: LanguageTag = expected.parse().expect("normalization is idempotent");
        assert_eq!(normalized.as_str(), language.as_str());
    }
}

#[test]
fn invalid_tags_fail_parsing_and_deserialization_and_length_has_an_exact_ceiling() {
    for invalid in [
        "", "en_US", "en--US", "-en", "en-", "en US", "en\n", "é", "x",
    ] {
        assert!(
            invalid.parse::<LanguageTag>().is_err(),
            "accepted {invalid:?}"
        );
        assert!(
            serde_json::from_value::<LanguageTag>(serde_json::json!(invalid)).is_err(),
            "deserialized {invalid:?}"
        );
    }
    let boundary = format!("x{}-a", "-abcdefgh".repeat(28));
    assert_eq!(boundary.len(), 255);
    assert_eq!(
        boundary
            .parse::<LanguageTag>()
            .expect("255-byte private-use tag")
            .as_str(),
        boundary
    );
    let oversized = format!("{boundary}a");
    assert_eq!(oversized.len(), 256);
    assert!(oversized.parse::<LanguageTag>().is_err());
    assert!(serde_json::from_value::<LanguageTag>(serde_json::json!(oversized)).is_err());
    assert!(serde_json::from_value::<LanguageTag>(serde_json::json!(42)).is_err());
}

#[test]
fn omitted_classification_stays_undeclared_and_invalid_shapes_fail() {
    let mut value = serde_json::to_value(page()).expect("page serialization");
    let object = value.as_object_mut().expect("page object");
    object.remove("language");
    object.remove("taxonomies");
    let node: ContentNode = serde_json::from_value(value).expect("omitted defaults");
    assert!(node.language.is_none());
    assert!(node.taxonomies.is_empty());
    assert_eq!(node.lifecycle, ContentLifecycle::Draft);
    for invalid in [
        serde_json::json!({"taxonomy":"tags","term":"term:rust"}),
        serde_json::json!({"taxonomy":"taxonomy:tags","term":"term:"}),
        serde_json::json!({"taxonomy":"taxonomy:tags","term":"term:rust","inherited":true}),
    ] {
        assert!(serde_json::from_value::<TaxonomyMembership>(invalid).is_err());
    }
}

#[test]
fn taxonomy_identity_is_exact_and_set_order_does_not_depend_on_declaration_order() {
    let mut node = page();
    node.language = Some("FR-ca".parse().expect("language"));
    node.taxonomies = vec![
        membership("taxonomy:tags", "term:école"),
        membership("taxonomy:tags", "term:ecole"),
        membership("taxonomy:tags", "term:École"),
        membership("taxonomy:category", "term:ecole"),
    ];
    let mut reversed = node.clone();
    reversed.taxonomies.reverse();
    let first = Graph::resolve(declaration(vec![node]), Limits::default()).expect("exact keys");
    let second =
        Graph::resolve(declaration(vec![reversed]), Limits::default()).expect("reordered keys");
    let first = first.node(&key("content:guide")).expect("guide");
    let second = second.node(&key("content:guide")).expect("guide");
    assert_eq!(first, second);
    assert_eq!(
        first.taxonomies,
        vec![
            membership("taxonomy:category", "term:ecole"),
            membership("taxonomy:tags", "term:ecole"),
            membership("taxonomy:tags", "term:École"),
            membership("taxonomy:tags", "term:école"),
        ]
    );
    assert_eq!(
        first.language.as_ref().expect("declared language").as_str(),
        "fr-CA"
    );
    assert_eq!(first.lifecycle, ContentLifecycle::Draft);
    assert!(first.attributions.is_empty());
    assert!(first.route.is_none());
}

#[test]
fn exact_duplicate_taxonomy_pair_is_rejected_after_input_budget_accounting() {
    let mut node = page();
    let pair = membership("taxonomy:tags", "term:rust");
    node.taxonomies = vec![pair.clone(), pair];
    let input = declaration(vec![node]);
    assert!(matches!(
        Graph::resolve(
            input.clone(),
            Limits {
                edges: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("edges"))
    ));
    let Error::Invalid(diagnostics) = Graph::resolve(input, Limits::default())
        .expect_err("duplicates must not silently collapse")
    else {
        panic!("expected duplicate classification diagnostic");
    };
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "content_duplicate_taxonomy");
    assert_eq!(diagnostics[0].source, key("content:guide"));
}

#[test]
fn language_and_every_taxonomy_key_consume_exact_shared_text_and_edge_budgets() {
    let mut node = page();
    node.language = Some("zh-Hant-TW".parse().expect("language"));
    node.taxonomies = vec![membership("taxonomy:tags", "term:école")];
    let text_bytes = node.id.as_str().len()
        + node.site.as_str().len()
        + node.title.len()
        + "zh-Hant-TW".len()
        + "taxonomy:tags".len()
        + "term:école".len();
    let exact = Limits {
        nodes: 1,
        edges: 1,
        text_bytes,
    };
    let input = declaration(vec![node]);
    Graph::resolve(input.clone(), exact).expect("exact shared budgets");
    assert!(matches!(
        Graph::resolve(input.clone(), Limits { edges: 0, ..exact }),
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
fn container_classification_does_not_infer_leaf_language_subject_or_publication_state() {
    let mut container = page();
    container.id = key("collection:guide");
    container.structure = ContentKind::Collection {
        parent: None,
        purpose: None,
        navigation: None,
        member_order: vec![],
    };
    container.language = Some("fr".parse().expect("container language"));
    container.taxonomies = vec![membership("taxonomy:tags", "term:rust")];
    container.lifecycle = ContentLifecycle::Published;
    let mut input = declaration(vec![container, page()]);
    input.memberships.push(CollectionMembership {
        content: key("content:guide"),
        collection: key("collection:guide"),
    });
    let graph = Graph::resolve(input, Limits::default()).expect("explicit structural membership");
    let leaf = graph.node(&key("content:guide")).expect("leaf");
    assert!(leaf.language.is_none());
    assert!(leaf.taxonomies.is_empty());
    assert!(leaf.attributions.is_empty());
    assert!(
        graph
            .project_relations(&leaf.id)
            .expect("page project index")
            .is_empty()
    );
    assert_eq!(leaf.lifecycle, ContentLifecycle::Draft);
    assert_eq!(leaf.site, key("site:home"));
}
