//! Retained authored metadata reaches shared projections without chronology inference.

use super::{compilation, doc, fixtures, key, parse};
use gazette_compiler::{authored, toml::PathSegment};
use gazette_core::source::Sha256Digest;
use serde_json::json;

#[test]
fn quoted_editorial_facts_retain_original_origins_and_project_typed_json_ld_dates() {
    let mut files = fixtures();
    files[4].1 = files[4].1.replace("lifecycle='draft'", "lifecycle='draft'\ndescription='Literal **summary** — not rendered Markdown'\npublished='2024-02-29'\nmodified='2023-01-01T00:00:00.1+00:00'").replace('\n', "\r\n");
    let corpus = parse(&files, Default::default());
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, Default::default())
        .expect("independent authored chronology");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("draft page");
    let page_json = serde_json::to_value(&page).expect("page");
    let search = serde_json::to_value(page.search()).expect("search");
    let graph = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:a"), Default::default())
            .expect("graph"),
    )
    .expect("graph JSON");
    for (field, expected, original) in [
        (
            "description",
            "Literal **summary** — not rendered Markdown",
            "'Literal **summary** — not rendered Markdown'",
        ),
        ("published", "2024-02-29", "'2024-02-29'"),
        (
            "modified",
            "2023-01-01T00:00:00.100Z",
            "'2023-01-01T00:00:00.1+00:00'",
        ),
    ] {
        assert_eq!(page_json["content"][field], expected);
        assert_eq!(search["content"][field], expected);
        assert_eq!(graph["nodes"]["article:a"][field], expected);
        let origin = resolved
            .origin(
                &key("article:a"),
                &[
                    PathSegment::Key("content".into()),
                    PathSegment::Key(field.into()),
                ],
            )
            .expect("retained origin");
        assert_eq!(origin.path, "article.md".parse().expect("path"));
        assert_eq!(
            origin.source_sha256,
            Sha256Digest::of(files[4].1.as_bytes())
        );
        let span = origin.span.expect("exact field span");
        assert_eq!(&files[4].1[span.start..span.end], original);
    }
    let linked = serde_json::to_value(page.json_ld()).expect("JSON-LD");
    assert_eq!(
        linked["description"],
        "Literal **summary** — not rendered Markdown"
    );
    assert_eq!(
        linked["datePublished"],
        json!({"@value":"2024-02-29","@type":"http://www.w3.org/2001/XMLSchema#date"})
    );
    assert_eq!(
        linked["dateModified"],
        json!({"@value":"2023-01-01T00:00:00.100Z","@type":"http://www.w3.org/2001/XMLSchema#dateTime"})
    );
    assert_eq!(linked["creativeWorkStatus"], "draft");
}

#[test]
fn container_dates_do_not_fill_missing_page_facts_and_related_references_stay_shallow() {
    let mut files = fixtures();
    files[0].1 = files[0].1.replace(
        "lifecycle='published'",
        "lifecycle='published'\ndescription='Container only'\npublished='2000-01-01'",
    );
    files[5].1 = files[5].1.replace("lifecycle='archived'", "lifecycle='archived'\ndescription='Foreign metadata must not spread'\npublished='2024-01-01'\nmodified='2024-02-01'");
    let corpus = parse(&files, Default::default());
    let workspace = compilation();
    let resolved =
        authored::resolve(&workspace, &corpus, Default::default()).expect("resolved relationships");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("local page");
    let linked = serde_json::to_value(page.json_ld()).expect("JSON-LD");
    for field in ["description", "datePublished", "dateModified"] {
        assert!(linked.get(field).is_none(), "inferred root {field}");
        assert!(
            linked["gazette:editorialRelated"][0].get(field).is_none(),
            "copied related {field}"
        );
    }
    assert_eq!(
        linked["gazette:editorialRelated"][0]["identifier"],
        "docs:b"
    );
    let foreign = serde_json::to_value(
        resolved
            .project_page(
                &key("docs:b"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("foreign root")
            .json_ld(),
    )
    .expect("foreign JSON-LD");
    assert_eq!(foreign["description"], "Foreign metadata must not spread");
    assert_eq!(foreign["datePublished"]["@value"], "2024-01-01");
    assert_eq!(foreign["dateModified"]["@value"], "2024-02-01");
    for field in ["description", "published", "modified"] {
        assert!(
            resolved
                .origin(
                    &key("article:a"),
                    &[
                        PathSegment::Key("content".into()),
                        PathSegment::Key(field.into())
                    ]
                )
                .is_none(),
            "invented origin"
        );
    }
}

#[test]
fn supported_offset_endpoints_reach_json_ld_without_a_second_normalization() {
    let workspace = compilation();
    for value in ["2024-01-01T00:00:00+14:00", "2024-01-01T00:00:00-14:00"] {
        let mut files = fixtures();
        files[4].1 = files[4].1.replace(
            "lifecycle='draft'",
            &format!("lifecycle='draft'\npublished='{value}'"),
        );
        let corpus = parse(&files, Default::default());
        let resolved = authored::resolve(&workspace, &corpus, Default::default())
            .expect("supported shared date profile");
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("page");
        let linked = serde_json::to_value(page.json_ld()).expect("typed JSON-LD");
        assert_eq!(
            linked["datePublished"],
            json!({"@value":value,"@type":"http://www.w3.org/2001/XMLSchema#dateTime"})
        );
    }
}

#[test]
fn raw_toml_temporal_values_and_metadata_aliases_are_not_a_second_public_schema() {
    let workspace = compilation();
    for field in [
        "published=2024-02-29",
        "published=2024-02-29T00:00:00Z",
        "modified=2024-02-29T00:00:00",
        "published=42",
        "published=true",
        "published=['2024-02-29']",
        "published='2023-02-29'",
        "published='0000-01-01'",
        "modified='2024-01-01T00:00:00+14:01'",
        "modified='2024-02-29T00:00:00.1234567890Z'",
        "date='2024-02-29'",
        "datePublished='2024-02-29'",
        "summary='Not an alias'",
    ] {
        let source = doc(
            "article:a",
            "site:a",
            "draft",
            "",
            "kind='page'\nfamily={kind='article'}",
            "Body\n",
        )
        .replace("lifecycle='draft'", &format!("lifecycle='draft'\n{field}"));
        let files = [("article.md", source)];
        let corpus = parse(&files, Default::default());
        let error = authored::resolve(&workspace, &corpus, Default::default()).expect_err(field);
        let authored::Error::Files(errors) = error else {
            panic!("{field} must fail metadata decoding, not downstream resolution");
        };
        assert_eq!(errors.len(), 1, "{field}");
        assert!(
            matches!(errors[0].error, authored::FileError::Metadata(_)),
            "{field}"
        );
        assert_eq!(errors[0].origin.path, "article.md".parse().expect("path"));
        assert_eq!(
            errors[0].origin.source_sha256,
            Sha256Digest::of(files[0].1.as_bytes())
        );
    }
}
