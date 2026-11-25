//! Declared entity meaning and public addresses remain separate from pages and state.
use super::*;
use serde_json::{Value, json};

fn public(resolved: &authored::ResolvedAuthored<'_, '_>, id: &str) -> Value {
    json!(
        resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .unwrap()
            .json_ld()
            .public()
    )
}

#[test]
fn an_index_family_home_without_a_listing_remains_a_web_page() {
    let source = configuration().replace(
        "public_base_url='https://one.test/'",
        "public_base_url='https://one.test/'\nhome='content:one'",
    );
    let config = workspace::compile(&source, Default::default()).unwrap();
    let text = landing().replace("family={kind='project'}", "family={kind='index'}");
    let corpus = parse(&text);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let value = public(&resolved, "content:one");
    let graph = value["@graph"].as_array().unwrap();
    let page = graph
        .iter()
        .find(|node| node["identifier"] == "content:one")
        .unwrap();
    assert_eq!(page["@type"], "WebPage");
    assert_eq!(page["about"][0]["subjectOf"][0]["@type"], "WebPage");
    assert!(graph.iter().any(|node| node["@type"] == "WebSite"));
}

#[test]
fn explicit_subject_kinds_do_not_change_lifecycle_or_infer_availability() {
    for (kind, expected) in [
        (None, "Thing"),
        (Some("software-application"), "SoftwareApplication"),
        (Some("software-source-code"), "SoftwareSourceCode"),
        (Some("creative-work"), "CreativeWork"),
        (Some("collection"), "Collection"),
        (Some("project"), "Project"),
        (Some("research-project"), "ResearchProject"),
    ] {
        let config = workspace::compile(&configuration(), Default::default()).unwrap();
        let text = landing().replace(
            "[project]",
            &kind.map_or_else(
                || "[project]".into(),
                |kind| format!("[project]\nkind='{kind}'"),
            ),
        );
        let text = if kind == Some("software-application") {
            text.replace(
                "[content]",
                "[project.software]\napplication_category='DeveloperApplication'\noperating_system='macOS, Linux'\n[content]",
            )
        } else {
            text
        };
        let corpus = parse(&text);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let value = public(&resolved, "content:one");
        let entity = &value["about"][0];
        assert_eq!(entity["@type"], expected);
        if kind == Some("software-application") {
            assert_eq!(entity["applicationCategory"], "DeveloperApplication");
            assert_eq!(entity["operatingSystem"], "macOS, Linux");
        } else {
            assert!(entity.get("applicationCategory").is_none());
            assert!(entity.get("operatingSystem").is_none());
        }
        assert_eq!(entity["@id"], "https://one.test/one/#entity");
        assert_ne!(entity["@id"], value["@id"]);
        assert_eq!(entity["subjectOf"][0]["@id"], value["@id"]);
        assert_eq!(
            resolved
                .content()
                .project(&key("project:one"))
                .unwrap()
                .lifecycle,
            ProjectLifecycle::Paused
        );
        assert_eq!(
            entity.get("codeRepository").is_some(),
            expected == "SoftwareSourceCode"
        );
        for absent in ["offers", "availability", "aggregateRating", "review"] {
            assert!(entity.get(absent).is_none());
        }
    }
}

#[test]
fn public_entity_identity_stays_public_when_serving_urls_are_local() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let text = landing().replace("[project]", "[project]\nkind='software-application'");
    let corpus = parse(&text);
    let resolved = authored::resolve(&config, &corpus, Default::default())
        .unwrap()
        .with_preview_urls(std::collections::BTreeMap::from([(
            key("site:one"),
            "http://127.0.0.1:8123/".parse().unwrap(),
        )]))
        .unwrap();
    let value = public(&resolved, "content:one");
    assert_eq!(value["url"], "http://127.0.0.1:8123/one/");
    assert_eq!(value["@id"], "https://one.test/one/");
    assert_eq!(value["about"][0]["@id"], "https://one.test/one/#entity");
}

#[test]
fn explicit_entity_iri_preserves_its_fragment_and_rejects_page_collisions() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    for (identity, valid) in [
        ("https://entities.test/catalog#actual", true),
        ("https://one.test/one/", false),
        ("https://ONE.test/one/#", false),
        ("http://127.0.0.1/entity", false),
        ("http://10.0.0.1/entity", false),
        ("https://user:secret@entities.test/entity", false),
        ("file:///private/entity", false),
    ] {
        let text = landing().replace("[project]", &format!("[project]\nidentity='{identity}'"));
        let corpus = parse(&text);
        let resolved = authored::resolve(&config, &corpus, Default::default());
        assert_eq!(resolved.is_ok(), valid, "{identity}");
        if let Ok(resolved) = resolved {
            assert_eq!(
                public(&resolved, "content:one")["about"][0]["@id"],
                identity
            );
        }
    }
}

#[test]
fn linked_article_is_the_actual_work_and_mentions_do_not_become_about() {
    let config = workspace::compile(&configuration(), Default::default()).unwrap();
    let text = landing().replace("[content]", "resources={learn='content:guide'}\n[content]");
    let help = "+++\n[content]\nid='content:help'\nsite='site:one'\ntitle='Help'\nlifecycle='draft'\nroute={path='/help/'}\nstructure={kind='collection',purpose='documentation'}\n+++\n# Help\n";
    for relation in ["about", "mentions"] {
        let guide = format!(
            "+++\nprojects=[{{project='project:one',kind='{relation}'}}]\ncollections=['content:help']\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Guide'\nlifecycle='draft'\nroute={{path='/help/guide/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Guide\n"
        );
        let corpus = ParsedCorpus::parse(
            [
                ("writing/one.md", text.as_str()),
                ("writing/help.md", help),
                ("writing/guide.md", guide.as_str()),
            ]
            .map(|(path, bytes)| Input {
                path: path.parse().unwrap(),
                bytes: bytes.as_bytes(),
            }),
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let value = public(&resolved, "content:one");
        let target = public(&resolved, "content:guide");
        let works = value["about"][0]["subjectOf"].as_array().unwrap();
        let linked = works
            .iter()
            .find(|work| work["identifier"] == "content:guide");
        assert_eq!(linked.is_some(), relation == "about");
        if let Some(linked) = linked {
            assert_eq!(linked["@id"], target["@id"]);
            assert_eq!(linked["@id"], "https://one.test/help/guide/#article");
            assert!(linked.get("@type").is_none());
            assert_eq!(target["@type"], "Article");
            assert_eq!(target["headline"], linked["name"]);
            assert_ne!(linked["@id"], target["mainEntityOfPage"]["@id"]);
        }
        assert!(
            resolved
                .project_resources_for(&key("content:one"))
                .unwrap()
                .unwrap()
                .actions
                .iter()
                .any(|action| action.url == "https://one.test/help/guide/")
        );
    }
}
