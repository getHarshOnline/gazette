//! Native TOML/YAML subject declarations and their shared public projections.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use serde_json::{Value, json};

const CONFIG: &str = "[[principals]]\nid='organization:publisher'\nkind='organization'\nname='Page publisher'\n[[principals]]\nid='person:subject'\nkind='person'\nname='Explicit person'\nwebsite={kind='site',site='site:one'}\nsame_as=['https://identity.test/person']\n[[principals]]\nid='organization:provider'\nkind='organization'\nname='Explicit provider'\n[[sites]]\nid='site:one'\npublisher='organization:publisher'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/fixture/not-opened'}\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{source='source:one',target='one'}]\n";

fn toml(fields: &str) -> String {
    format!(
        "+++\n[content]\nid='content:page'\nsite='site:one'\ntitle='Page title'\ndescription='Page description'\nlifecycle='published'\nroute={{path='/page/'}}\nstructure={{kind='page',family={{kind='utility'}}}}\n{fields}\n+++\nThe actual page body.\n"
    )
}
fn corpus<'a>(path: &str, source: &'a str) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        [Input {
            path: path.parse().unwrap(),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}
fn project(path: &str, source: &str) -> (Value, Value, Value) {
    project_with_config(CONFIG, path, source)
}
fn project_with_config(config: &str, path: &str, source: &str) -> (Value, Value, Value) {
    let config = workspace::compile(config, Default::default()).unwrap();
    let corpus = corpus(path, source);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let id = "content:page".parse().unwrap();
    let page = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let discovery = resolved.discovery().unwrap();
    let node = resolved.content().graph().node(&id).unwrap();
    (
        json!(page.json_ld()),
        json!(page),
        json!(discovery.node(node).unwrap()),
    )
}

#[test]
fn programme_about_page_without_principal_retains_project_and_brand_relationships() {
    let config = format!(
        "{CONFIG}\n[[projects]]\nid='project:programme'\nname='Programme'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[projects]]\nid='project:related'\nlifecycle='planned'\nvisibility='public'\navailability='unavailable'\n"
    );
    let edges = "projects=[{project='project:programme',kind='about'},{project='project:related',kind='mentions'}]";
    let document = |fields| toml(fields).replacen("[content]", &format!("{edges}\n[content]"), 1);
    let baseline = project_with_config(&config, "one/page.md", &document(""));
    let source = document("schema_type='AboutPage'");
    let (ld, view, discovery) = project_with_config(&config, "one/page.md", &source);
    assert_eq!(ld["@type"], "AboutPage");
    assert!(ld.get("mainEntity").is_none());
    assert!(view.get("subject").is_none());
    assert!(discovery.get("subject").is_none());
    assert_eq!(ld["about"][0]["identifier"], "project:programme");
    assert_eq!(ld["mentions"][0]["identifier"], "project:related");
    for field in [
        "about",
        "mentions",
        "gazette:projectRelations",
        "gazette:projectDeclarations",
        "publisher",
    ] {
        assert_eq!(ld[field], baseline.0[field], "{field}");
    }
    assert_eq!(view["site"]["metadata"]["brand"]["id"], "brand:one");
    assert_eq!(view["site"], baseline.1["site"]);
    assert_eq!(view["projects"], baseline.1["projects"]);
    assert_eq!(discovery["structure"]["family"]["kind"], "utility");
}

#[test]
fn about_toml_and_yaml_reuse_principal_identity_without_attribution_or_publisher_fallback() {
    let toml =
        toml("schema_type='AboutPage'\nsubject={kind='principal',principal='person:subject'}");
    let yaml = "format: gazette-landing/v1\ncontent:\n  id: content:page\n  site: site:one\n  title: Page title\n  description: Page description\n  lifecycle: published\n  route: {path: /page/}\n  structure: {kind: page, family: {kind: utility}}\n  schema_type: AboutPage\n  subject: {kind: principal, principal: 'person:subject'}\nsections:\n- body: The actual page body.\n";
    for (path, source) in [
        ("one/page.md", toml.as_str()),
        ("one/page.landing.yaml", yaml),
    ] {
        let (ld, view, discovery) = project(path, source);
        assert_eq!(ld["@type"], "AboutPage");
        assert_eq!(ld["mainEntity"]["@type"], "Person");
        assert_eq!(ld["mainEntity"]["identifier"], "person:subject");
        assert_eq!(ld["mainEntity"]["name"], "Explicit person");
        assert_eq!(ld["mainEntity"]["url"], "https://example.test/");
        assert_ne!(ld["mainEntity"]["@id"], ld["publisher"]["@id"]);
        assert!(ld.get("author").is_none());
        assert!(ld.get("datePublished").is_none());
        assert_eq!(view["subject"]["principal"]["id"], "person:subject");
        assert_eq!(
            discovery["subject"],
            json!({"kind":"principal","principal":"person:subject"})
        );
        assert_eq!(discovery["structure"]["family"]["kind"], "utility");
    }
    let attributed = toml.replace(
        "schema_type=",
        "attributions=[{principal='person:subject',role='author'}]\nschema_type=",
    );
    let (ld, _, _) = project("one/page.md", &attributed);
    assert_eq!(ld["mainEntity"]["@id"], ld["author"][0]["@id"]);
}

#[test]
fn service_is_a_distinct_explicit_entity_without_copied_page_facts() {
    for description in ["", ",description='Service description'"] {
        let source = toml(&format!(
            "published='2026-01-02'\nsubject={{kind='service',id='service:explicit',name='Service name',provider='organization:provider'{description}}}"
        ));
        let (ld, _, discovery) = project("one/page.md", &source);
        assert_eq!(ld["@type"], "WebPage");
        assert_eq!(ld["name"], "Page title");
        assert_eq!(ld["description"], "Page description");
        let entity = &ld["mainEntity"];
        assert_eq!(entity["@type"], "Service");
        assert_eq!(entity["identifier"], "service:explicit");
        assert_eq!(entity["name"], "Service name");
        assert_eq!(entity["provider"]["identifier"], "organization:provider");
        assert_eq!(entity["provider"]["@type"], "Organization");
        assert_ne!(entity["@id"], ld["@id"]);
        assert_ne!(entity["provider"]["@id"], ld["publisher"]["@id"]);
        for field in [
            "datePublished",
            "dateModified",
            "creativeWorkStatus",
            "offers",
            "price",
            "availability",
            "url",
        ] {
            assert!(entity.get(field).is_none(), "{field}");
        }
        if description.is_empty() {
            assert!(entity.get("description").is_none());
        } else {
            assert_eq!(entity["description"], "Service description");
        }
        assert_eq!(discovery["subject"]["name"], "Service name");
    }
}

#[test]
fn unknown_subjects_and_providers_are_rejected_even_with_a_valid_publisher() {
    let config = workspace::compile(CONFIG, Default::default()).unwrap();
    for fields in [
        "schema_type='AboutPage'\nsubject={kind='principal',principal='person:missing'}",
        "subject={kind='service',id='service:explicit',name='Name',provider='organization:missing'}",
    ] {
        let source = toml(fields);
        let corpus = corpus("one/page.md", &source);
        let error =
            authored::resolve(&config, &corpus, Default::default()).expect_err("unknown principal");
        assert!(
            format!("{error:?}").contains("content_subject_principal_reference"),
            "{error:?}"
        );
    }
}

#[test]
fn old_utility_output_has_no_subject_or_invented_main_entity() {
    let (ld, view, discovery) = project("one/page.md", &toml(""));
    assert_eq!(ld["@type"], "WebPage");
    assert!(ld.get("mainEntity").is_none());
    assert!(view.get("subject").is_none());
    assert!(discovery.get("subject").is_none());
}
