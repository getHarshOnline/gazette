//! Public vocabulary preserves publication meaning independently of native inspection.

#[path = "public/material.rs"]
mod material;

use super::{authored, doc, key, parse, with_resolved, workspace};
use gazette_core::render::LimitedWriter;
use serde_json::{Value, json};

fn standards_only(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                assert!(
                    name.starts_with('@')
                        || [
                            "identifier",
                            "name",
                            "headline",
                            "keywords",
                            "creativeWorkStatus",
                            "inLanguage",
                            "license",
                            "url",
                            "description",
                            "abstract",
                            "text",
                            "datePublished",
                            "dateModified",
                            "image",
                            "contentUrl",
                            "caption",
                            "publisher",
                            "author",
                            "copyrightHolder",
                            "sameAs",
                            "about",
                            "mentions",
                            "isPartOf",
                            "hasPart",
                            "mainEntity",
                            "mainEntityOfPage",
                            "provider",
                            "breadcrumb",
                            "numberOfItems",
                            "itemListElement",
                            "position",
                            "item",
                            "itemListOrder",
                            "relatedLink",
                            "subjectOf",
                            "codeRepository",
                            "inDefinedTermSet",
                            "acceptedAnswer",
                        ]
                        .contains(&name.as_str()),
                    "unexpected public property: {name}"
                );
                if name == "@type" {
                    assert!(
                        !value
                            .as_str()
                            .is_some_and(|kind| kind.starts_with("gazette:"))
                    );
                }
                standards_only(value);
            }
        }
        Value::Array(values) => values.iter().for_each(standards_only),
        _ => {}
    }
}

#[test]
fn subjects_roles_and_scoped_terms_survive_without_recasting_internal_evidence() {
    with_resolved(|resolved| {
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let native = json!(page.json_ld());
        let value = json!(page.json_ld().public());
        standards_only(&value);
        assert_eq!(
            value["@context"],
            json!({"@version":1.1,"@vocab":"https://schema.org/","url":{"@id":"https://schema.org/url","@type":"@id"}})
        );
        for field in [
            "identifier",
            "name",
            "headline",
            "creativeWorkStatus",
            "text",
            "url",
            "author",
        ] {
            assert_eq!(value[field], native[field], "preserve {field}");
        }
        // A rights holder already described as an author is named by identity alone.
        let holders = value["copyrightHolder"].as_array().unwrap();
        let native_holders = native["copyrightHolder"].as_array().unwrap();
        assert_eq!(holders.len(), native_holders.len());
        for (holder, native_holder) in holders.iter().zip(native_holders) {
            assert_eq!(holder["@id"], native_holder["@id"]);
            let described = value["author"]
                .as_array()
                .unwrap()
                .iter()
                .any(|author| author["@id"] == holder["@id"]);
            if described {
                assert_eq!(holder, &json!({"@id": holder["@id"]}));
            } else {
                assert_eq!(holder, native_holder);
            }
        }
        for field in ["@id", "identifier"] {
            assert_eq!(
                value["publisher"][field], native["publisher"][field],
                "preserve publisher {field}"
            );
        }
        assert_eq!(value["publisher"]["@type"], "Organization");
        assert_eq!(value["about"][0]["identifier"], "project:p");
        assert_eq!(value["@id"], "https://one.test/mount/article/#article");
        assert_eq!(
            value["mainEntityOfPage"]["@id"],
            "https://one.test/mount/article/"
        );
        assert_eq!(value["mentions"][0]["identifier"], "project:q");
        for relation in ["about", "mentions"] {
            assert_eq!(value[relation][0]["@type"], "Thing");
            assert_eq!(value[relation][0]["@id"], native[relation][0]["@id"]);
        }
        let terms = value["keywords"].as_array().unwrap();
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0]["@type"], "DefinedTerm");
        assert_eq!(terms[0]["name"], "First meaning");
        assert_eq!(terms[1]["name"], "Second meaning");
        assert_eq!(terms[0]["identifier"], terms[1]["identifier"]);
        assert_ne!(terms[0]["@id"], terms[1]["@id"]);
        assert_eq!(terms[0]["inDefinedTermSet"]["identifier"], "taxonomy:a");
        assert_eq!(terms[1]["inDefinedTermSet"]["identifier"], "taxonomy:b");
        assert_eq!(
            native["gazette:projectRelations"][0]["gazette:evidence"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            native["gazette:provenance"]["gazette:project"]["identifier"],
            "project:source"
        );
        let encoded = value.to_string();
        for absent in [
            "gazette:",
            "source:private",
            "project:source",
            "Never emit this",
            "isBasedOn",
            "citation",
            "Offer",
            "availability",
        ] {
            assert!(
                !encoded.contains(absent),
                "unexpected inference or private fact: {absent}"
            );
        }
        assert_eq!(
            json!(page.json_ld()),
            native,
            "public serialization does not mutate native metadata"
        );
    });
}

#[test]
fn project_resources_keep_reader_links_without_claiming_a_software_type_or_offer() {
    let source = super::configuration_source().replace("id='project:p'", "id='project:p'\nname='Named project'\nrepository='https://code.test/project'\nlicense='MIT'\nresources={code_documentation='https://docs.test/project/'}");
    let compilation = workspace::compile(&source, Default::default()).unwrap();
    let files = super::files();
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let value = json!(page.json_ld().public());
    standards_only(&value);
    let project = &value["about"][0];
    assert_eq!(project["@type"], "Thing");
    assert_eq!(project["name"], "Named project");
    assert_eq!(
        project["subjectOf"],
        json!([{"@id":"https://docs.test/project/"}])
    );
    for field in [
        "license",
        "codeRepository",
        "offers",
        "availability",
        "isBasedOn",
        "citation",
    ] {
        assert!(
            project.get(field).is_none(),
            "unsupported project claim: {field}"
        );
    }
    assert_eq!(
        page.data.projects[0].project.repository.as_deref(),
        Some("https://code.test/project")
    );
    assert_eq!(
        page.data.projects[0]
            .project
            .license
            .as_ref()
            .unwrap()
            .as_str(),
        "MIT"
    );
}

#[test]
fn breadcrumb_page_identity_and_journey_positions_stay_exact() {
    with_resolved(|resolved| {
        for id in ["article:a", "journey:z", "collection:root", "docs:local"] {
            let page = resolved
                .project_page(
                    &key(id),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                )
                .unwrap();
            let value = json!(page.json_ld().public());
            standards_only(&value);
            let presentation = if id == "article:a" {
                assert_eq!(value["mainEntityOfPage"]["@type"], "WebPage");
                assert_ne!(value["@id"], value["mainEntityOfPage"]["@id"]);
                assert_eq!(value["isPartOf"][0]["identifier"], "journey:z");
                &value["mainEntityOfPage"]
            } else {
                &value
            };
            // A trail holding only the page itself names no place and is omitted.
            let originals: Vec<_> = page
                .data
                .breadcrumbs
                .iter()
                .filter(|trail| trail.items.len() > 1)
                .collect();
            let trails = presentation
                .get("breadcrumb")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            assert_eq!(trails.len(), originals.len());
            for (trail, original) in trails.iter().zip(originals) {
                assert_eq!(trail["numberOfItems"], original.items.len());
                for (index, (item, original)) in trail["itemListElement"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(&original.items)
                    .enumerate()
                {
                    assert_eq!(item["position"], index + 1);
                    assert_eq!(item["name"], original.node.title.as_str());
                    // The item names its target; the target's facts stay on its own page.
                    let target = item["item"].as_object().unwrap();
                    assert_eq!(target.len(), 1);
                    if let Some(route) = original.route {
                        assert_eq!(target["@id"], route.public_url.as_str());
                    }
                }
            }
            if id == "journey:z" {
                let list = &value["mainEntity"];
                assert_eq!(list["@type"], "ItemList");
                assert_eq!(list["numberOfItems"], 2);
                assert_eq!(
                    list["itemListElement"][0]["item"]["identifier"],
                    "docs:local"
                );
                assert_eq!(
                    list["itemListElement"][1]["item"]["identifier"],
                    "article:a"
                );
                assert_eq!(list["itemListElement"][1]["position"], 2);
            }
        }
    });
}

#[test]
fn explicit_principal_and_service_subjects_keep_separate_page_and_entity_facts() {
    let compilation = super::configuration();
    for (fields, kind, subject) in [
        (
            "schema_type='AboutPage'\nsubject={kind='principal',principal='principal:writer'}\n",
            "AboutPage",
            "principal:writer",
        ),
        (
            "subject={kind='service',id='service:one',name='Actual service',provider='principal:rights',description='Service description'}\n",
            "WebPage",
            "service:one",
        ),
    ] {
        let source = doc("content:subject", "site:a", "published", "", "kind='page'\nfamily={kind='utility'}", "Actual body.\n")
            .replace("[content]", &format!("[content]\n{fields}published='2026-11-01'\nmodified='2026-11-02T12:30:00Z'\ndescription='Page description'\n"));
        let files = vec![("subject.md", source)];
        let corpus = parse(&files, Default::default());
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
        let page = resolved
            .project_page(
                &key("content:subject"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let value = json!(page.json_ld().public());
        standards_only(&value);
        assert_eq!(value["@type"], kind);
        assert_eq!(value["mainEntity"]["identifier"], subject);
        assert_ne!(value["mainEntity"]["@id"], value["@id"]);
        assert_eq!(value["datePublished"]["@value"], "2026-11-01");
        assert_eq!(
            value["datePublished"]["@type"],
            "http://www.w3.org/2001/XMLSchema#date"
        );
        assert_eq!(
            value["dateModified"]["@type"],
            "http://www.w3.org/2001/XMLSchema#dateTime"
        );
        if kind == "WebPage" {
            assert_eq!(value["mainEntity"]["@type"], "Service");
            assert_eq!(value["mainEntity"]["name"], "Actual service");
            assert_eq!(
                value["mainEntity"]["provider"]["identifier"],
                "principal:rights"
            );
            assert_eq!(value["mainEntity"]["description"], "Service description");
            assert!(value["mainEntity"].get("datePublished").is_none());
        }
    }
}

#[test]
fn public_serialization_uses_the_callers_byte_limit_without_mutating_native_output() {
    with_resolved(|resolved| {
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let expected = serde_json::to_vec(&page.json_ld().public()).unwrap();
        let mut exact = LimitedWriter::new(Vec::new(), expected.len());
        serde_json::to_writer(&mut exact, &page.json_ld().public()).unwrap();
        assert_eq!(exact.into_inner(), expected);
        let mut below = LimitedWriter::new(Vec::new(), expected.len() - 1);
        assert!(serde_json::to_writer(&mut below, &page.json_ld().public()).is_err());
        assert!(below.exceeded());
        assert_eq!(
            serde_json::to_vec(&page.json_ld().public()).unwrap(),
            expected
        );
    });
}

#[test]
fn discovery_filters_generated_members_but_preserves_their_original_positions() {
    let source = super::configuration_source().replace(
        "public_base_url='https://one.test/mount/'",
        "public_base_url='https://one.test/mount/'\nauthored=[{source='source:private',target='public'}]",
    );
    let compilation = workspace::compile(&source, Default::default()).unwrap();
    let files = vec![
        ("public/collection.md", doc("collection:visible", "site:a", "published", "", "kind='collection'", "Collection\n")),
        ("public/journey.md", doc("journey:visible", "site:a", "published", "", "kind='journey'\ncollection='collection:visible'\nmembers=[{content='content:hidden',primary=true},{content='content:visible',primary=true}]", "Journey\n").replace("[content]", "[content]\nroute={path='/journey/'}")),
        ("native/hidden.md", doc("content:hidden", "site:a", "published", "", "kind='page'\nfamily={kind='article'}", "Hidden\n").replace("[content]", "[content]\nroute={path='/hidden/'}")),
        ("public/visible.md", doc("content:visible", "site:a", "published", "", "kind='page'\nfamily={kind='article'}", "Visible\n").replace("[content]", "[content]\nroute={path='/visible/'}")),
    ];
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let page = resolved
        .project_page(
            &key("journey:visible"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let direct = json!(page.discovery_json_ld(&discovery).unwrap().public());
    let aggregate = json!(page.listing_json_ld(&discovery).unwrap().public());
    standards_only(&direct);
    standards_only(&aggregate);
    assert_eq!(direct["mainEntity"]["numberOfItems"], 2);
    assert_eq!(aggregate["mainEntity"]["numberOfItems"], 1);
    assert_eq!(aggregate["mainEntity"]["itemListElement"][0]["position"], 2);
    assert_eq!(
        aggregate["mainEntity"]["itemListElement"][0]["item"]["identifier"],
        "content:visible"
    );
    assert!(!aggregate.to_string().contains("content:hidden"));
    let hidden = resolved
        .project_page(
            &key("content:hidden"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(hidden.listing_json_ld(&discovery).is_none());
    assert!(hidden.discovery_json_ld(&discovery).is_some());
}

#[test]
fn generated_listing_views_reuse_exact_chronology_and_items_without_route_metadata() {
    let source = super::configuration_source().replace(
        "public_base_url='https://one.test/mount/'",
        "public_base_url='https://one.test/mount/'\nauthored=[{source='source:private',target='public'}]\nlistings=[{content='content:index',select={kind='standalone-articles'},order={by='published',direction='descending',undated='last-title-then-id'},pagination={page_size=1,next_pages='page/{number}/'}}]",
    );
    let compilation = workspace::compile(&source, Default::default()).unwrap();
    let files = vec![
        (
            "public/index.md",
            doc(
                "content:index",
                "site:a",
                "published",
                "",
                "kind='page'\nfamily={kind='index'}",
                "",
            )
            .replace("[content]", "[content]\nroute={path='/articles/'}"),
        ),
        (
            "public/older.md",
            doc(
                "content:older",
                "site:a",
                "published",
                "",
                "kind='page'\nfamily={kind='article'}",
                "Older\n",
            )
            .replace(
                "[content]",
                "[content]\npublished='2026-11-01'\nroute={path='/older/'}",
            ),
        ),
        (
            "public/newer.md",
            doc(
                "content:newer",
                "site:a",
                "published",
                "",
                "kind='page'\nfamily={kind='article'}",
                "Newer\n",
            )
            .replace(
                "[content]",
                "[content]\npublished='2026-11-02'\nroute={path='/newer/'}",
            ),
        ),
    ];
    let corpus = parse(&files, Default::default());
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&key("site:a"), &discovery)
        .unwrap();
    assert_eq!(pages.len(), 2);
    for (page, id) in pages.iter().zip(["content:newer", "content:older"]) {
        let value = json!(page.public_json_ld());
        standards_only(&value);
        assert_eq!(value["mainEntity"], page.public_item_list());
        assert_eq!(value["@id"], page.url);
        assert_eq!(value["url"], page.url);
        assert_eq!(
            value["mainEntity"]["itemListOrder"],
            "https://schema.org/ItemListOrderDescending"
        );
        assert_eq!(
            value["mainEntity"]["itemListElement"][0]["item"]["identifier"],
            id
        );
        assert_eq!(value["mainEntity"]["itemListElement"][0]["position"], 1);
        assert!(page.json_ld().get("gazette:route").is_some());
    }
}
