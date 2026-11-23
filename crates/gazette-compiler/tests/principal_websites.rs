//! One explicit principal website across page, listing and linked-data consumers.

use std::collections::BTreeMap;

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    toml::PathSegment,
    workspace::{self, DeclarationKind, FieldQuery, ParsedWorkspace},
};
use gazette_core::{publication::Key, raster};
use serde_json::json;

fn key(value: &str) -> Key {
    value.parse().unwrap()
}

fn configuration(fields: &str) -> String {
    format!(
        "[[principals]]\nid='person:writer'\nkind='person'\nname='Writer & colleague'\n{fields}\n\
         [[principals]]\nid='organization:publisher'\nkind='organization'\nname='Separate publisher'\n\
         [[principals]]\nid='person:guest'\nkind='person'\nname='Guest without website'\n\
         [[principals]]\nid='person:unused'\nkind='person'\nname='Unrelated roster canary'\n\
         [[sites]]\nid='site:home'\npublisher='person:writer'\ndeployable=false\n\
         [[sites]]\nid='site:journal'\npublisher='organization:publisher'\ndeployable=false\n\
         [[sources]]\nid='source:writing'\nlocation={{kind='filesystem',root='/not-read'}}\n\
         [[brands]]\nid='brand:one'\nname='Fixture'\n\
         [[brands.publications]]\nsite='site:home'\npublic_base_url='https://writer.test/home/'\n\
         [[brands.publications]]\nsite='site:journal'\npublic_base_url='https://journal.test/press/'\n\
         authored=[{{source='source:writing',target='writing'}}]\n\
         listings=[{{content='content:index',select={{kind='standalone-articles'}},\
         order={{first=[],remaining='title-then-id'}},pagination={{page_size=1,next_pages='page/{{number}}/'}}}}]\n"
    )
}

#[test]
fn website_has_exact_origin_and_typed_site_reference_without_retyping_a_url() {
    let source = configuration(
        "website={kind='site',site='site:home'}\nsame_as=['https://writer.test/home/']",
    )
    .replace('\n', "\r\n");
    let parsed = ParsedWorkspace::parse(&source, Default::default()).unwrap();
    let origin = parsed
        .origin(&FieldQuery {
            kind: DeclarationKind::Principal,
            identity: key("person:writer"),
            field: vec![
                PathSegment::Key("website".into()),
                PathSegment::Key("site".into()),
            ],
        })
        .unwrap();
    assert_eq!(origin.span.slice(&source), Some("'site:home'"));
    let reference = parsed
        .navigation()
        .reference_at(origin.span.start + 1)
        .unwrap();
    assert_eq!(reference.symbol.unwrap().name(), "site:home");
    assert_eq!(
        reference.kind,
        workspace::WorkspaceSymbolKind::Declaration(DeclarationKind::Site)
    );
    let schema = gazette_compiler::contracts::workspace();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(validator.is_valid(&json!({"principals":[{
        "id":"person:writer", "kind":"person",
        "website":{"kind":"site","site":"site:home"},
        "same_as":["https://writer.test/home/"]
    }]})));
}

#[test]
fn malformed_or_unknown_destinations_fail_instead_of_inventing_identity_links() {
    for fields in [
        "website={kind='site',site='site:absent'}",
        "website={kind='external',url='javascript:alert(1)'}",
        "website={kind='external',url='https://user:secret@example.test/'}",
        "website={kind='external',url='https://example.test/with space'}",
        "same_as=['mailto:writer@example.test']",
        "same_as=['https://example.test/','https://example.test/']",
    ] {
        assert!(
            matches!(
                workspace::compile(&configuration(fields), Default::default()),
                Err(workspace::Error::Invalid(_))
            ),
            "accepted {fields}"
        );
    }
    for fields in [
        "website='https://writer.test/'",
        "website={kind='site',site='site:home',url='https://conflict.test/'}",
        "website={kind='external'}",
        "same_as=true",
        "sameAs=['https://writer.test/']",
        "contact_email='writer@example.test'",
    ] {
        assert!(
            matches!(
                workspace::compile(&configuration(fields), Default::default()),
                Err(workspace::Error::Toml(_))
            ),
            "accepted {fields}"
        );
    }
    let missing_base = configuration("website={kind='site',site='site:home'}")
        .replace("public_base_url='https://writer.test/home/'", "");
    assert!(workspace::compile(&missing_base, Default::default()).is_err());
}

fn files() -> Vec<(&'static str, String)> {
    let page = |id, title, metadata, family, body| {
        format!(
            "+++\n[content]\nid='{id}'\nsite='site:journal'\ntitle='{title}'\nlifecycle='published'\nroute={{path='/{id}/'}}\n{metadata}\nstructure={{kind='page',family={{kind='{family}'}}}}\n+++\n{body}"
        )
    };
    vec![
        (
            "writing/index.md",
            page("content:index", "Index", "", "index", ""),
        ),
        (
            "writing/article.md",
            page(
                "content:article",
                "A named article",
                "published='2026-10-01'\nmodified='2026-10-26'\nattributions=[{principal='person:writer',role='author'},{principal='person:guest',role='author'},{principal='organization:publisher',role='rights-holder'}]",
                "article",
                "# Argument\nRetained words.",
            ),
        ),
        (
            "writing/unsigned.md",
            page(
                "content:unsigned",
                "Unsigned article",
                "",
                "article",
                "# Argument\nNo author declared.",
            ),
        ),
    ]
}

#[test]
fn page_and_paginated_listings_share_attributions_while_json_ld_keeps_public_identity() {
    let compiled = workspace::compile(
        &configuration(
            "website={kind='site',site='site:home'}\nsame_as=['https://writer.test/home/']",
        ),
        Default::default(),
    )
    .unwrap();
    let before = serde_json::to_value(&compiled).unwrap();
    let files = files();
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    for preview in [false, true] {
        let mut resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
        if preview {
            resolved = resolved
                .with_preview_urls(BTreeMap::from([
                    (
                        key("site:home"),
                        "http://127.0.0.1:4310/home/".parse().unwrap(),
                    ),
                    (
                        key("site:journal"),
                        "http://127.0.0.1:4311/press/".parse().unwrap(),
                    ),
                ]))
                .unwrap();
        }
        let page = resolved
            .project_page(
                &key("content:article"),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let value = serde_json::to_value(&page).unwrap();
        assert_eq!(
            value["attributions"][0]["website"],
            json!({
                "url":if preview {"http://127.0.0.1:4310/home/"} else {"https://writer.test/home/"},
                "canonical_url":"https://writer.test/home/",
                "destination":{"kind":"cross-site","content":null,"site":"site:home","relationships":[],"presentation":null}
            })
        );
        assert!(value["attributions"][1].get("website").is_none());
        assert!(std::ptr::eq(
            page.data.attributions[0].principal,
            compiled.workspace.principal(&key("person:writer")).unwrap()
        ));
        let json_ld = serde_json::to_value(page.json_ld()).unwrap();
        assert_eq!(json_ld["author"][0]["@type"], "Person");
        assert_eq!(json_ld["author"][0]["url"], "https://writer.test/home/");
        assert_eq!(
            json_ld["author"][0]["sameAs"],
            json!([{"@id":"https://writer.test/home/"}])
        );
        assert!(json_ld["author"][1].get("url").is_none());
        assert!(!json_ld.to_string().contains("Unrelated roster canary"));
        let discovery = resolved.discovery().unwrap();
        assert_eq!(
            serde_json::to_value(page.listing_json_ld(&discovery).unwrap()).unwrap()["author"],
            json_ld["author"]
        );
        let listings = resolved
            .project_listing_pages(&key("site:journal"), &discovery)
            .unwrap();
        assert_eq!(listings.len(), 2);
        let item = serde_json::to_value(&listings[0].items[0]).unwrap();
        assert_eq!(item["attributions"], value["attributions"]);
        assert_eq!(item["published"], "2026-10-01");
        assert_eq!(item["modified"], "2026-10-26");
        let unsigned = serde_json::to_value(&listings[1].items[0]).unwrap();
        for absent in ["attributions", "published", "modified"] {
            assert!(unsigned.get(absent).is_none(), "invented {absent}");
        }
        assert!(listings[1].previous.is_some());
    }
    assert_eq!(serde_json::to_value(&compiled).unwrap(), before);
}

#[test]
fn an_external_website_and_identity_reference_remain_distinct_explicit_values() {
    let compiled = workspace::compile(&configuration(
        "website={kind='external',url='https://outside.test/writer'}\nsame_as=['https://identity.test/profile']",
    ), Default::default()).unwrap();
    let files = files();
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:article"),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let website = serde_json::to_value(&page).unwrap();
    assert_eq!(
        website["attributions"][0]["website"]["destination"]["kind"],
        "external"
    );
    assert!(website["attributions"][0]["website"]["destination"]["site"].is_null());
    let author = &serde_json::to_value(page.json_ld()).unwrap()["author"][0];
    assert_eq!(author["url"], "https://outside.test/writer");
    assert_eq!(
        author["sameAs"],
        json!([{"@id":"https://identity.test/profile"}])
    );
}

#[test]
fn public_publisher_uses_its_declared_profile_without_inventing_authorship() {
    for person in [false, true] {
        let config = configuration("website={kind='site',site='site:home'}");
        let config = if person {
            config.replace(
                "publisher='organization:publisher'",
                "publisher='person:writer'",
            )
        } else {
            config
        };
        let compiled = workspace::compile(&config, Default::default()).unwrap();
        let files = files();
        let corpus = ParsedCorpus::parse(
            files.iter().map(|(path, text)| Input {
                path: path.parse().unwrap(),
                bytes: text.as_bytes(),
            }),
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let resolved = authored::resolve(&compiled, &corpus, Default::default())
            .unwrap()
            .with_preview_urls(BTreeMap::from([
                (
                    key("site:home"),
                    "http://127.0.0.1:4310/home/".parse().unwrap(),
                ),
                (
                    key("site:journal"),
                    "http://127.0.0.1:4311/press/".parse().unwrap(),
                ),
            ]))
            .unwrap();
        let page = resolved
            .project_page(
                &key("content:unsigned"),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let public = serde_json::to_value(page.json_ld().public()).unwrap();
        let publisher = &public["publisher"];
        assert_eq!(
            publisher["@type"],
            if person { "Person" } else { "Organization" }
        );
        assert_eq!(
            publisher["name"],
            if person {
                "Writer & colleague"
            } else {
                "Separate publisher"
            }
        );
        if person {
            assert_eq!(publisher["url"], "https://writer.test/home/");
            assert_eq!(
                page.data.site.publisher_website.as_ref().unwrap().url,
                "http://127.0.0.1:4310/home/"
            );
        } else {
            assert!(
                publisher.get("url").is_none(),
                "a publication address is not an organisation website"
            );
        }
        assert!(public.get("author").is_none());
        assert!(public.get("copyrightHolder").is_none());
        assert!(!publisher.to_string().contains("127.0.0.1"));
        assert!(!public.to_string().contains("Unrelated roster canary"));
    }
}

#[test]
fn a_principal_in_two_roles_is_one_public_entity_described_once() {
    let config = configuration("website={kind='site',site='site:home'}").replace(
        "publisher='organization:publisher'",
        "publisher='person:writer'",
    );
    let compiled = workspace::compile(&config, Default::default()).unwrap();
    let files = files();
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:article"),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let public = serde_json::to_value(page.json_ld().public()).unwrap();
    let identity = "https://writer.test/home/#person";
    assert_eq!(public["publisher"]["@id"], identity);
    assert_eq!(public["publisher"]["name"], "Writer & colleague");
    // The author is the publisher already described above, so it is named, not repeated.
    assert_eq!(public["author"][0], json!({"@id": identity}));
    // A principal without a website keeps a document-local blank node with its facts.
    let guest = &public["author"][1];
    assert!(guest["@id"].as_str().unwrap().starts_with("_:principal_"));
    assert_eq!(guest["name"], "Guest without website");
    assert_eq!(public["copyrightHolder"][0]["@type"], "Organization");
}
