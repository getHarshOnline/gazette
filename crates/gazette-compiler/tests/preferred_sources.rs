//! Preferred-source invitations preserve canonical addressing and explicit page policy.
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use std::collections::BTreeMap;

fn config(policy: &str) -> String {
    format!(
        "[[principals]]\nid='person:a'\nkind='person'\n[[sites]]\nid='site:a'\npublisher='person:a'\ndeployable=false\n[[sources]]\nid='source:fixture'\nlocation={{kind='filesystem',root='/not-read'}}\n[[brands]]\nid='brand:a'\nname='Specimen'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://journal.example/'\nauthored=[{{source='source:fixture',target='docs'}}]\n{policy}"
    )
}
fn document(id: &str, family: &str) -> String {
    format!(
        "+++\n[content]\nid='content:{id}'\nsite='site:a'\ntitle='{id}'\nlifecycle='published'\nroute={{path='/{id}/'}}\nstructure={{kind='page',family={{kind='{family}'}}}}\n+++\n# {id}\n"
    )
}
const POLICY: &str = "[brands.publications.processing.preferred_sources]\nenabled=true\nprovider='google'\narticles=true\nlabel='Prefer this publication'\ndescription='Read more from this source.'\n[brands.publications.processing.preferred_sources.pages.'content:second']\nenabled=false\n";

#[test]
fn source_invitation_uses_public_host_during_local_preview_and_honors_page_policy() {
    let compilation = workspace::compile(&config(POLICY), Default::default()).unwrap();
    let first = document("first", "article");
    let second = document("second", "article");
    let utility = document("utility", "utility");
    let corpus = ParsedCorpus::parse(
        [
            ("docs/first.md", &first),
            ("docs/second.md", &second),
            ("docs/utility.md", &utility),
        ]
        .into_iter()
        .map(|(path, source)| Input {
            path: path.parse().unwrap(),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let preview = resolved
        .with_preview_urls(BTreeMap::from([(
            "site:a".parse().unwrap(),
            "http://127.0.0.1:4310/".parse().unwrap(),
        )]))
        .unwrap();
    let discovery = preview.discovery().unwrap();
    for (id, expected) in [("first", true), ("second", false), ("utility", false)] {
        let page = preview
            .project_page(
                &format!("content:{id}").parse().unwrap(),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let value = serde_json::to_value(&page).unwrap();
        let rendered = serde_json::to_value(page.discovery_view(&discovery).unwrap()).unwrap();
        assert_eq!(
            rendered.get("preferred_source"),
            value.get("preferred_source")
        );
        assert_eq!(value.get("preferred_source").is_some(), expected);
        if expected {
            assert_eq!(
                value["preferred_source"]["href"],
                "https://www.google.com/preferences/source?q=journal.example"
            );
            assert!(
                value["route"]["url"]
                    .as_str()
                    .unwrap()
                    .starts_with("http://127.0.0.1:4310/")
            );
        }
    }
}

#[test]
fn invalid_source_scope_and_unbounded_copy_fail_at_configuration_boundary() {
    for source in [
        config(POLICY).replace("https://journal.example/", "https://journal.example/path/"),
        config(POLICY).replace("label='Prefer this publication'", "label=''"),
    ] {
        assert!(workspace::compile(&source, Default::default()).is_err());
    }
    assert!(
        workspace::compile(
            &config(&POLICY.replace("enabled=true", "enabled=false")),
            Default::default()
        )
        .is_ok()
    );
}

#[test]
fn preferred_source_page_overrides_must_bind_to_real_content() {
    let compilation = workspace::compile(&config(POLICY), Default::default()).unwrap();
    let first = document("first", "article");
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "docs/first.md".parse().unwrap(),
            bytes: first.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(authored::resolve(&compilation, &corpus, Default::default()).is_err());
}
