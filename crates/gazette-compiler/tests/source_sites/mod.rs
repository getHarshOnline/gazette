//! A page's site follows its selected source, never a path name or About edge.
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::{publication::Key, source::EntryPath};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
[[sources]]\nid='source:content'\nlocation={kind='filesystem',root='/not-opened'}\n\
[[brands]]\nid='brand:one'\nname='Fixture'\n\
[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/'\nauthored=[{source='source:content',target='misleading-two'}]\n";

fn key(value: &str) -> Key {
    value.parse().unwrap()
}
fn parse<'a>(path: &str, text: &'a str) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        [Input {
            path: path.parse::<EntryPath>().unwrap(),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}
fn markdown(site: &str) -> String {
    format!(
        "+++\n[content]\nid='content:stable'\n{site}\ntitle='A retained title'\ndescription='A retained description'\nlifecycle='draft'\nkeywords=['retained facts']\nroute={{path='/unchanged/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# A retained title\n"
    )
}

#[test]
fn omitted_toml_and_yaml_sites_use_the_exact_binding_without_fabricated_origins() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let yaml = "format: gazette-landing/v1\ncontent:\n  id: content:stable\n  title: A retained title\n  description: A retained description\n  lifecycle: draft\n  route: {path: /unchanged/}\n  structure: {kind: page, family: {kind: index}}\nsections:\n- body: A retained body.\n";
    for (path, text) in [
        ("misleading-two/article.md", markdown("")),
        ("misleading-two/index.landing.yaml", yaml.into()),
    ] {
        let corpus = parse(path, &text);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        resolved.validate_source_bindings().unwrap();
        let node = resolved
            .content()
            .graph()
            .node(&key("content:stable"))
            .unwrap();
        assert_eq!(node.site, key("site:one"));
        assert_eq!(node.title, "A retained title");
        assert_eq!(node.description.as_deref(), Some("A retained description"));
        assert_eq!(
            resolved.content().route(&node.id).unwrap().url.as_str(),
            "https://one.test/unchanged/"
        );
        assert!(
            resolved
                .origin(
                    &node.id,
                    &[
                        gazette_compiler::toml::PathSegment::Key("content".into()),
                        gazette_compiler::toml::PathSegment::Key("site".into())
                    ]
                )
                .is_none()
        );
        assert_eq!(serde_json::to_value(node).unwrap()["site"], "site:one");
    }
}

#[test]
fn changing_the_selected_source_home_changes_the_derived_site_from_retained_bytes() {
    let text = markdown("");
    let corpus = parse("misleading-two/article.md", &text);
    for site in ["site:one", "site:two"] {
        let config = WORKSPACE.replace(
            "[[brands.publications]]\nsite='site:one'",
            &format!("[[brands.publications]]\nsite='{site}'"),
        );
        let config = workspace::compile(&config, Default::default()).unwrap();
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let facts = authored::editorial_authoring(&config, &corpus, 100, 1 << 20).unwrap();
        assert_eq!(facts.declarations()[0].site, key(site));
        assert_eq!(
            resolved
                .content()
                .graph()
                .node(&key("content:stable"))
                .unwrap()
                .site,
            key(site)
        );
    }
}

#[test]
fn authored_schema_allows_omission_but_resolved_nodes_require_a_site() {
    let node = serde_json::json!({
        "id":"content:stable", "title":"A title", "lifecycle":"draft",
        "structure":{"kind":"page","family":{"kind":"article"}}
    });
    let authored_schema = authored::declaration_schema();
    let validator = jsonschema::validator_for(authored_schema.as_value()).unwrap();
    assert!(validator.is_valid(&serde_json::json!({"content":node})));
    assert!(
        serde_json::from_value::<gazette_core::publication::content::ContentNode>(node).is_err()
    );
}

#[test]
fn missing_bindings_refuse_omitted_sites_and_explicit_homes_are_never_overwritten() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let text = markdown("");
    let corpus = parse("unbound/article.md", &text);
    let error = authored::resolve(&config, &corpus, Default::default()).unwrap_err();
    assert!(
        matches!(error,authored::Error::Files(ref failures) if matches!(failures[0].error,authored::FileError::SiteBindingRequired))
    );
    let text = markdown("site='site:two'");
    let corpus = parse("misleading-two/article.md", &text);
    // A base for the conflicting explicit home allows structural inspection;
    // source binding must still refuse publication rather than relocate it.
    let config=workspace::compile(&format!("{WORKSPACE}\n[[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/'\n"),Default::default()).unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    assert_eq!(
        resolved
            .content()
            .graph()
            .node(&key("content:stable"))
            .unwrap()
            .site,
        key("site:two")
    );
    assert!(matches!(
        resolved.validate_source_bindings(),
        Err(authored::SourceBindingError::Home { .. })
    ));
}
