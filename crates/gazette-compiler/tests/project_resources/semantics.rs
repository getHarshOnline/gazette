//! A navigation label does not determine the linked work's relationship or type.
use super::*;
use serde_json::json;

#[test]
fn mentions_index_about_its_project_joins_its_actual_listing_identity() {
    for relation in ["about", "mentions"] {
        let config = config(
            "ecosystem",
            "kind='software-application'\nresources={landing='page:product',mentions='page:blogs'}",
        );
        let mut files = files();
        files[2].1 = files[2].1.replacen(
            "+++\n",
            &format!("+++\nprojects=[{{project='project:one',kind='{relation}'}}]\n"),
            1,
        );
        let corpus = parse(&files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let product = resolved
            .project_page(
                &key("page:product"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let index = resolved
            .project_page(
                &key("page:blogs"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let product = json!(product.json_ld().public());
        let index = json!(index.json_ld().public());
        assert_eq!(index["@type"], "CollectionPage");
        let linked = product["about"][0]["subjectOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|work| work["identifier"] == "page:blogs");
        assert_eq!(linked.is_some(), relation == "about");
        if let Some(linked) = linked {
            assert_eq!(linked["@id"], index["@id"]);
            assert_eq!(linked["@type"], index["@type"]);
        }
        let discovery = resolved.discovery().unwrap();
        let pages = resolved
            .project_listing_pages(&key("site:one"), &discovery)
            .unwrap();
        let generated = json!(
            pages
                .iter()
                .find(|page| page.is_primary())
                .unwrap()
                .public_json_ld()
        );
        assert_eq!(generated["@id"], index["@id"]);
        assert_eq!(generated["about"]["@id"], product["about"][0]["@id"]);
    }
}
