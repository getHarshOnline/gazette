//! One retained excerpt and reference mapping, shared by human and machine views.

use super::{doc, key, parse};
use gazette_compiler::{authored, workspace};
use serde_json::Value;

fn compilation() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Explicit'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://example.test/mount/'\n",
        Default::default(),
    ).expect("explicit canonical base")
}

fn source(body: &str, routed: bool) -> String {
    doc("article:a", "site:a", "draft", "", "kind='page'\nfamily={kind='article'}", body)
        .replace("lifecycle='draft'", if routed {
            "lifecycle='draft'\ndescription='Explicit description, not the excerpt'\nroute={path='/article.html'}"
        } else {
            "lifecycle='draft'\ndescription='Explicit description, not the excerpt'"
        })
}

#[test]
fn retained_summary_projects_plain_search_and_abstract_without_description_fallback() {
    let body = "# Heading\n\nLead **strong** [next](next/?mode=1) and [here](#heading).[^note]\n\n<!-- more -->\n\nRemaining body.\n\n[^note]: Footnote excluded from excerpt.\n";
    let files = [("article.md", source(body, true))];
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("resolved");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete body and summary");
    let summary = page.summary.as_ref().expect("explicit body cutoff");
    assert_eq!(
        body[summary.marker.start..summary.marker.end].trim(),
        "<!-- more -->"
    );
    assert!(summary.html.contains("<strong>strong</strong>"));
    assert!(summary.text.contains("Lead strong next and here."));
    assert!(!summary.text.contains("Footnote excluded"));
    assert!(!summary.html.contains("footnote"));
    assert!(!summary.html.contains("Remaining body"));
    assert!(page.body_html.contains("Remaining body"));
    assert!(page.body_html.contains("Footnote excluded"));
    assert!(!page.body_text.contains("<!-- more -->"));
    for destination in [
        "https://example.test/mount/next/?mode=1",
        "https://example.test/mount/article.html#heading",
    ] {
        let href = format!("href=\"{destination}\"");
        assert!(page.body_html.contains(&href));
        assert!(summary.html.contains(&href));
    }
    let continuation = summary
        .continuation
        .strip_prefix("https://example.test/mount/article.html#")
        .expect("portable canonical continuation");
    assert!(page.body_html.contains(&format!("id=\"{continuation}\"")));
    let page_json = serde_json::to_value(&page).expect("page JSON");
    let search = serde_json::to_value(page.search()).expect("search");
    let ld = serde_json::to_value(page.json_ld()).expect("JSON-LD");
    assert_eq!(page_json["summary"]["text"], search["summary"]);
    assert_eq!(ld["abstract"], search["summary"]);
    assert_eq!(ld["description"], "Explicit description, not the excerpt");
    assert_eq!(search["content"], page_json["content"]);
    assert!(search["summary"].is_string());
    assert!(search.get("html").is_none());
    assert!(search.get("body_html").is_none());
    let repeated = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("retained repeat");
    assert_eq!(serde_json::to_value(repeated).expect("repeat"), page_json);
}

#[test]
fn unrouted_excerpt_preserves_relative_coordinates_and_absence_is_not_body_summarization() {
    let compilation = compilation();
    for (body, marked) in [
        ("Lead [next](next/)\n\n<!-- more -->\n\nTail\n", true),
        ("Lead without a cutoff.\n", false),
        ("`<!-- more -->` is a code example.\n", false),
    ] {
        let files = [("article.md", source(body, false))];
        let corpus = parse(&files, Default::default());
        let resolved =
            authored::resolve(&compilation, &corpus, Default::default()).expect("resolved");
        let page = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("unrouted projection");
        let json: Value = serde_json::to_value(&page).expect("page");
        let search = serde_json::to_value(page.search()).expect("search");
        let ld = serde_json::to_value(page.json_ld()).expect("LD");
        if marked {
            let summary = page.summary.as_ref().expect("cutoff");
            assert!(summary.continuation.starts_with('#'));
            assert!(summary.html.contains("href=\"next/\""));
            assert!(page.body_html.contains("href=\"next/\""));
        } else {
            assert!(json.get("summary").is_none());
            assert!(search.get("summary").is_none());
            assert!(ld.get("abstract").is_none());
        }
        assert_eq!(ld["description"], "Explicit description, not the excerpt");
    }
}

#[test]
fn late_unsafe_or_unresolvable_content_never_returns_a_successful_prefix() {
    let compilation = compilation();
    for (tail, code) in [
        (
            "<script>not admitted</script>\n",
            "markdown_raw_html_unavailable",
        ),
        ("[Bad](javascript:alert)\n", "markdown_unsafe_url"),
        ("[Missing](#absent)\n", "markdown_link_resolution"),
        ("[Encoded](%2e%2e/escape/)\n", "markdown_link_resolution"),
        ("<!-- more -->\n", "markdown_summary"),
    ] {
        let body = format!("Valid prefix.\n\n<!-- more -->\n\n{tail}");
        let files = [("article.md", source(&body, true))];
        let corpus = parse(&files, Default::default());
        let resolved =
            authored::resolve(&compilation, &corpus, Default::default()).expect("metadata");
        assert_eq!(
            resolved
                .project_page(
                    &key("article:a"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("no partial page")
                .code(),
            code
        );
    }
}
