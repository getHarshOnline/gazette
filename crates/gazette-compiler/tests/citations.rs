//! Authored citation text and retained downloads share the native source/selection contract.
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        Key,
        content::{Disposition, Limits},
    },
    source::EntryPath,
};
use serde_json::json;
fn key(s: &str) -> Key {
    s.parse().unwrap()
}
fn path(s: &str) -> EntryPath {
    s.parse().unwrap()
}
fn workspace(extra: &str) -> workspace::Compilation {
    workspace::compile(&format!("[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={{kind='repository',root='/private/retained',commit='1111111111111111111111111111111111111111',subdirectory='docs'}}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/mount/'\nauthored=[{{source='source:one',target='authored',static_assets={{route_prefix='/media/'}}{extra}}}]\n"), Default::default()).unwrap()
}
fn page(citation: &str) -> String {
    format!(
        "+++\n[content]\nid='content:page'\nsite='site:one'\ntitle='Example'\nlifecycle='published'\nroute={{path='/article/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n{citation}\n+++\nA plain body without headings or links.\n"
    )
}
fn corpus<'a>(files: &[(&str, &'a [u8])]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}
#[test]
fn citation_optional_forms_preserve_plain_text_and_do_not_generate_metadata() {
    let w = workspace("");
    for citation in [
        "",
        "citation={text='<script> & \"quotation\"'}",
        "citation={formats=[{label='Custom',text='Writer. Title.'}]}",
        "citation={pdf='https://files.example/paper'}",
    ] {
        let text = page(citation);
        let c = corpus(&[("authored/page.md", text.as_bytes())]);
        let r = authored::resolve(&w, &c, Default::default()).unwrap();
        let p = r
            .project_page(
                &key("content:page"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let v = json!(p);
        assert_eq!(v.get("citation").is_some(), !citation.is_empty());
        assert!(v["content"].get("schema_type").is_none());
        assert!(
            json!(p.json_ld()).get("citation").is_none(),
            "how-to-cite is not a cited work"
        );
        if citation.contains("<script>") {
            assert_eq!(v["citation"]["text"], "<script> & \"quotation\"");
        }
        assert_eq!(json!(p.search())["citation"], v["citation"]);
        if citation.contains("https://files.example/paper") {
            assert_eq!(
                v["citation"]["pdf_destination"],
                json!({
                    "kind":"external", "content":null, "site":null, "relationships":[], "presentation":null
                })
            );
        } else {
            assert!(v["citation"].get("pdf_destination").is_none());
        }
        assert!(v["citation"].get("bibtex_destination").is_none());
        let again = r
            .project_page(
                &key("content:page"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert_eq!(v, json!(again));
    }
}
#[test]
fn citation_downloads_are_resolved_retained_assets_and_selected_build_obligations() {
    let w = workspace("");
    let text =
        page("citation={text='An authored citation.',bibtex='paper.bib',pdf='paper.pdf#page=2'}");
    let c = corpus(&[
        ("authored/page.md", text.as_bytes()),
        ("authored/paper.bib", b"@article{one,title={Example}}"),
        ("authored/paper.pdf", b"opaque PDF fixture"),
        ("authored/unused.pdf", b"unused"),
    ]);
    let r = authored::resolve(&w, &c, Default::default()).unwrap();
    let p = r
        .project_page(
            &key("content:page"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        json!(p)["citation"]["bibtex"],
        "https://example.test/mount/media/paper.bib"
    );
    assert_eq!(
        json!(p)["citation"]["pdf"],
        "https://example.test/mount/media/paper.pdf#page=2"
    );
    for field in ["bibtex_destination", "pdf_destination"] {
        assert_eq!(
            json!(p)["citation"][field],
            json!({
                "kind":"resource", "content":null, "site":"site:one", "relationships":[], "presentation":null
            })
        );
    }
    assert_eq!(p.data.assets.len(), 2);
    let selection = r.non_draft_selection().unwrap();
    assert_eq!(
        selection.assets[&path("authored/paper.bib")],
        Disposition::Include
    );
    assert_eq!(
        selection.assets[&path("authored/paper.pdf")],
        Disposition::Include
    );
    assert_eq!(
        selection.assets[&path("authored/unused.pdf")],
        Disposition::Exclude
    );
    let mut missing = selection.clone();
    missing
        .assets
        .insert(path("authored/paper.bib"), Disposition::Exclude);
    assert!(r.into_selected(&missing).is_err());
}
#[test]
fn citation_rejects_unsafe_missing_or_escaping_downloads_with_frontmatter_origin() {
    let w = workspace("");
    for value in [
        "javascript:alert(1)",
        "data:text/html,bad",
        "mailto:writer@example.test",
        "missing.pdf",
        "%2e%2e/escape.pdf",
        "folder%2fpaper.pdf",
        "wrong.txt",
        "#heading",
    ] {
        let text = page(&format!("citation={{pdf='{value}'}}"));
        let c = corpus(&[("authored/page.md", text.as_bytes())]);
        let r = authored::resolve(&w, &c, Default::default()).unwrap();
        let e = r
            .project_page(
                &key("content:page"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap_err();
        let authored::ProjectionError::Citation(e) = e else {
            panic!("citation diagnostic required")
        };
        assert_eq!(e.field, "pdf");
        assert_eq!(
            e.origin.unwrap().span.unwrap().slice(&text),
            Some(format!("'{value}'").as_str())
        );
        assert!(r.non_draft_selection().is_err());
    }
}
#[test]
fn citation_validation_and_limits_reject_empty_duplicate_and_unbounded_declarations() {
    let w = workspace("");
    for citation in [
        "citation={}",
        "citation={text=' '}",
        "citation={formats=[{label='APA',text='a'},{label='APA',text='b'}]}",
        "citation={formats=[{label=' ',text='b'}]}",
        "citation={pdf=' x.pdf'}",
    ] {
        let text = page(citation);
        let c = corpus(&[("authored/page.md", text.as_bytes())]);
        assert!(
            authored::resolve(&w, &c, Default::default()).is_err(),
            "{citation}"
        );
    }
    let text = page(&format!("citation={{text='{}'}}", "x".repeat(4096)));
    let c = corpus(&[("authored/page.md", text.as_bytes())]);
    assert!(
        authored::resolve(
            &w,
            &c,
            Limits {
                text_bytes: 1024,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn citation_preview_and_discovery_borrow_one_resolved_view() {
    let w = workspace("");
    let text = page("citation={bibtex='paper.bib'}");
    let c = corpus(&[
        ("authored/page.md", text.as_bytes()),
        ("authored/paper.bib", b"@article{one}"),
    ]);
    let r = authored::resolve(&w, &c, Default::default())
        .unwrap()
        .with_preview_urls(
            [(
                key("site:one"),
                "http://127.0.0.1:4919/mount/".parse().unwrap(),
            )]
            .into_iter()
            .collect(),
        )
        .unwrap();
    let p = r
        .project_page(
            &key("content:page"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        json!(p)["citation"]["bibtex"],
        "http://127.0.0.1:4919/mount/media/paper.bib"
    );
    assert_eq!(
        json!(p)["citation"]["bibtex_destination"],
        json!({
            "kind":"resource", "content":null, "site":"site:one", "relationships":[], "presentation":null
        })
    );
    let d = r.discovery().unwrap();
    let view = p.discovery_view(&d).unwrap();
    assert_eq!(json!(view)["citation"], json!(p)["citation"]);
    assert_eq!(json!(view.data())["citation"], json!(p)["citation"]);
    assert_eq!(
        json!(view.search().unwrap())["citation"],
        json!(p)["citation"]
    );
    assert_eq!(
        json!(p.listing(&d).unwrap())["content"]["citation"]["bibtex"],
        "paper.bib"
    );
}

#[test]
fn citation_download_cannot_expose_a_retained_only_asset() {
    let w = workspace(",retained_only=['private.pdf']");
    let text = page("citation={pdf='private.pdf'}");
    let c = corpus(&[
        ("authored/page.md", text.as_bytes()),
        ("authored/private.pdf", b"private"),
    ]);
    let r = authored::resolve(&w, &c, Default::default()).unwrap();
    assert_eq!(
        r.project_page(
            &key("content:page"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak
        )
        .unwrap_err()
        .code(),
        "authored_citation"
    );
    assert!(r.non_draft_selection().is_err());
}

#[test]
fn citation_foreign_site_download_keeps_the_existing_destination_contract() {
    let configured = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n[[brands.publications]]\nsite='site:two'\npublic_base_url='https://foreign.example/'\n";
    let w = workspace::compile(configured, Default::default()).unwrap();
    let text = page("citation={pdf='https://foreign.example/paper.pdf#page=3'}");
    let c = corpus(&[("page.md", text.as_bytes())]);
    let r = authored::resolve(&w, &c, Default::default()).unwrap();
    let p = r
        .project_page(
            &key("content:page"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        json!(p)["citation"]["pdf"],
        "https://foreign.example/paper.pdf#page=3"
    );
    assert_eq!(
        json!(p)["citation"]["pdf_destination"],
        json!({
            "kind":"cross-site", "content":null, "site":"site:two", "site_name":"Example", "relationships":[], "presentation":null
        })
    );
    assert!(p.data.assets.is_empty());
}
