//! One authored contribution feeds templates, search, graph and linked data.

use super::{compilation, fixtures, key, parse};
use gazette_compiler::{authored, toml::PathSegment};
use serde_json::json;

#[test]
fn native_article_facts_reach_every_shared_projection_with_original_coordinates() {
    let workspace = compilation();
    for kind in ["Article", "BlogPosting", "TechArticle"] {
        let mut files = fixtures();
        files[4].1 = files[4].1.replace(
            "[content]\n",
            &format!(
                "[content]\nkeywords=['Rust', 'typed publication graphs']\nschema_type='{kind}'\n"
            ),
        );
        let corpus = parse(&files, Default::default());
        let resolved =
            authored::resolve(&workspace, &corpus, Default::default()).expect("article metadata");
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page");
        let graph = resolved
            .project_site_graph(&key("site:a"), Default::default())
            .expect("graph");
        let expected = json!(["Rust", "typed publication graphs"]);
        for value in [
            json!(page)["content"].clone(),
            json!(page.search())["content"].clone(),
            json!(graph)["nodes"]["article:a"].clone(),
        ] {
            assert_eq!(value["schema_type"], kind);
            assert_eq!(value["keywords"], expected);
            assert!(
                value.get("taxonomies").is_none(),
                "phrases are not taxonomy membership"
            );
        }
        let linked = json!(page.json_ld());
        assert_eq!(linked["@type"], kind);
        assert_eq!(linked["headline"], linked["name"]);
        assert_eq!(linked["keywords"], expected);
        let origin = resolved
            .origin(
                &key("article:a"),
                &[
                    PathSegment::Key("content".into()),
                    PathSegment::Key("schema_type".into()),
                ],
            )
            .expect("exact authored origin");
        assert_eq!(
            origin.span.unwrap().slice(&files[4].1),
            Some(format!("'{kind}'").as_str())
        );
        assert!(
            json!(
                resolved
                    .project_page(
                        &key("article:empty"),
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak
                    )
                    .unwrap()
            )["content"]
                .get("schema_type")
                .is_none(),
            "no metadata spreads across references"
        );
    }
}

#[test]
fn discovery_metadata_keeps_the_same_article_facts() {
    let workspace = gazette_compiler::workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='repository',root='/not-opened/content',commit='0000000000000000000000000000000000000000',subdirectory='content'}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:a'\nauthored=[{source='source:one',target='authored'}]\n",
        Default::default(),
    ).expect("explicit source binding");
    let files = [(
        "authored/article.md",
        super::doc(
            "article:a",
            "site:a",
            "published",
            "",
            "kind='page'\nfamily={kind='article'}",
            "# Article\n",
        )
        .replace(
            "lifecycle='published'",
            "lifecycle='published'\nkeywords=['Rust']\nschema_type='TechArticle'",
        ),
    )];
    let corpus = parse(&files, Default::default());
    let resolved =
        authored::resolve(&workspace, &corpus, Default::default()).expect("article metadata");
    let discovery = resolved.discovery().expect("complete discovery");
    let node = resolved.content().graph().node(&key("article:a")).unwrap();
    let value = json!(discovery.node(node).expect("published node"));
    assert_eq!(value["schema_type"], "TechArticle");
    assert_eq!(value["keywords"], json!(["Rust"]));
}
