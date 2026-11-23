//! Explicit content licences remain source facts, not inherited permissions.

use super::{compilation, fixtures, key, parse};
use gazette_compiler::{authored, toml::PathSegment};
use gazette_core::source::Sha256Digest;
use serde_json::json;

const LICENSE: &str = "https://licenses.example/Content%2Fterms?edition=1&lang=en#grant";

#[test]
fn explicit_license_preserves_source_origin_and_shared_page_search_graph_and_json_ld() {
    let mut files = fixtures();
    files[4].1 = files[4]
        .1
        .replace("[content]\n", &format!("[content]\nlicense='{LICENSE}'\n"))
        .replace('\n', "\r\n");
    let corpus = parse(&files, Default::default());
    let workspace = compilation();
    let resolved =
        authored::resolve(&workspace, &corpus, Default::default()).expect("explicit licence");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("native page");
    let model = json!(page);
    assert_eq!(model["content"]["license"], LICENSE);
    assert_eq!(json!(page.search())["content"]["license"], LICENSE);
    assert_eq!(
        json!(
            resolved
                .project_site_graph(&key("site:a"), Default::default())
                .expect("graph")
        )["nodes"]["article:a"]["license"],
        LICENSE
    );
    assert_eq!(json!(page.json_ld())["license"], json!({"@id":LICENSE}));
    assert_eq!(
        model["content"]["lifecycle"], "draft",
        "licence is not publication approval"
    );
    let origin = resolved
        .origin(
            &key("article:a"),
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("license".into()),
            ],
        )
        .expect("native authored origin");
    assert_eq!(origin.path, "article.md".parse().expect("path"));
    assert_eq!(
        origin.source_sha256,
        Sha256Digest::of(files[4].1.as_bytes())
    );
    assert_eq!(
        origin.span.expect("exact quoted scalar").slice(&files[4].1),
        Some(format!("'{LICENSE}'").as_str())
    );
}

#[test]
fn collection_and_related_licenses_do_not_fill_an_absent_article_license() {
    let mut files = fixtures();
    for index in [0, 5] {
        files[index].1 = files[index]
            .1
            .replace("[content]\n", &format!("[content]\nlicense='{LICENSE}'\n"));
    }
    let corpus = parse(&files, Default::default());
    let workspace = compilation();
    let resolved =
        authored::resolve(&workspace, &corpus, Default::default()).expect("native relationships");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("unlicensed article");
    assert!(json!(page)["content"].get("license").is_none());
    assert!(json!(page.search())["content"].get("license").is_none());
    assert!(json!(page.json_ld()).get("license").is_none());
    assert!(
        json!(
            resolved
                .project_site_graph(&key("site:a"), Default::default())
                .expect("graph")
        )["nodes"]["article:a"]
            .get("license")
            .is_none()
    );
    assert_eq!(
        json!(
            resolved
                .project_page(
                    &key("collection:root"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect("explicit parent")
                .json_ld()
        )["license"],
        json!({"@id":LICENSE})
    );
}

#[test]
fn site_terms_add_usage_info_without_becoming_a_content_license() {
    let mut files = fixtures();
    files[4].1 = files[4]
        .1
        .replace("[content]\n", &format!("[content]\nlicense='{LICENSE}'\n"));
    let corpus = parse(&files, Default::default());
    let public = |workspace: &gazette_compiler::workspace::Compilation, id: &str| {
        let resolved = authored::resolve(workspace, &corpus, Default::default()).expect("corpus");
        let page = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page");
        (json!(page.json_ld().public()), json!(page))
    };
    // The shared fixture workspace, plus one publication binding for site terms.
    let base = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=true\n[[projects]]\nid='project:p'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n[[projects]]\nid='project:q'\nlifecycle='planned'\nvisibility='public'\navailability='gated'\n[[sources]]\nid='source:private'\nlocation={kind='repository',root='/private/operator-only',commit='0000000000000000000000000000000000000000',subdirectory='secret'}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/mount/'\n";
    for terms in [
        "machine={terms={path='/terms/'}}\n",
        "machine={terms={path='/terms/',tdm_reservation=true}}\n",
    ] {
        let source = format!("{base}{terms}");
        let workspace =
            gazette_compiler::workspace::compile(&source, Default::default()).expect("terms");
        let usage = json!({"@id":"https://a.test/mount/terms/"});
        let (article, model) = public(&workspace, "article:a");
        assert_eq!(article["usageInfo"], usage);
        assert_eq!(article["license"], json!({"@id":LICENSE}));
        assert!(model["site"].get("terms_url").is_none());
        let (collection, _) = public(&workspace, "collection:root");
        assert_eq!(collection["usageInfo"], usage);
        assert!(
            collection.get("license").is_none(),
            "site terms never fill an absent content licence"
        );
    }
    let workspace =
        gazette_compiler::workspace::compile(base, Default::default()).expect("no terms");
    let (article, _) = public(&workspace, "article:a");
    assert!(article.get("usageInfo").is_none());
    assert_eq!(article["license"], json!({"@id":LICENSE}));
}
