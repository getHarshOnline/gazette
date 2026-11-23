//! Discovery is a source-bound view of one native owner, not a graph mutation.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::Key;
use serde_json::{Value, json};

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn configuration() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={kind='repository',root='/private/discovery-fixture-not-read',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
         [[brands]]\nid='brand:one'\nname='Discovery fixture'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\nauthored=[{source='source:one',target='authored'}]\n\
         [[projects]]\nid='project:one'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:one'\nrender=true\ndiscoverable=false\ndocuments={source='source:one',target='project'}\n",
        Default::default(),
    ).expect("native workspace, without opening declared sources")
}

fn document(id: &str, path: &str, structure: &str) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='site:one'\ntitle='Retained Unicode Ω'\ndescription='Explicit description'\npublished='2026-09-10'\nmodified='2026-09-11T10:00:00Z'\nlanguage='fr-ca'\nlifecycle='archived'\nattributions=[{{principal='owner:one',role='author'}}]\nroute={{path='{path}'}}\nstructure={{{structure}}}\n+++\nRetained body.\n"
    )
}

fn corpus<'a>(files: &[(&str, &'a str)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().expect("exact logical source"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native Markdown and front matter")
}

const PAGE: &str = "kind='page',family={kind='documentation'}";

#[test]
fn missing_source_binding_refuses_discovery_without_disabling_private_structural_inspection() {
    let compiled = configuration();
    let text = document("content:one", "/guide/", PAGE);
    let corpus = corpus(&[("unbound/guide.md", &text)]);
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("private structural graph does not invent source binding");
    let before = serde_json::to_value(resolved.content().graph()).expect("original complete graph");
    let private = resolved
        .project_site_graph(&key("site:one"), Default::default())
        .expect("private graph inspection remains available");
    assert_eq!(
        private.nodes[&key("content:one")].title,
        "Retained Unicode Ω"
    );
    assert_eq!(
        resolved
            .project_site_pages(&key("site:one"))
            .expect("private routed pages")
            .pages()
            .len(),
        1
    );
    assert!(
        matches!(resolved.discovery(), Err(authored::SourceBindingError::Missing(id)) if id == key("content:one"))
    );
    assert_eq!(
        serde_json::to_value(resolved.content().graph()).expect("same graph after refusal"),
        before
    );
}

#[test]
fn equivalent_owners_and_foreign_lookalikes_cannot_borrow_another_discovery_selection() {
    let compiled = configuration();
    let text = document("content:one", "/guide/", PAGE);
    let corpus = corpus(&[("authored/guide.md", &text)]);
    let first =
        authored::resolve(&compiled, &corpus, Default::default()).expect("first actual owner");
    let second = authored::resolve(&compiled, &corpus, Default::default())
        .expect("independently resolved equivalent owner");
    assert_eq!(
        json!(first.content().graph()),
        json!(second.content().graph())
    );
    let discovery = first.discovery().expect("first source-bound discovery");
    assert!(discovery.matches(&first));
    assert!(!discovery.matches(&second));
    let node = first
        .content()
        .graph()
        .node(&key("content:one"))
        .expect("native node");
    let lookalike = node.clone();
    assert_eq!(&lookalike, node);
    assert!(discovery.owns(node));
    assert!(discovery.node(node).is_some());
    assert!(!discovery.owns(&lookalike));
    assert!(discovery.node(&lookalike).is_none());
    let foreign = second
        .content()
        .graph()
        .node(&key("content:one"))
        .expect("equivalent foreign node");
    assert!(discovery.node(foreign).is_none());
    let pages = second
        .project_site_pages(&key("site:one"))
        .expect("second complete pages");
    assert!(matches!(
        pages.discoverable(&discovery),
        Err(authored::SitePagesError::Discovery)
    ));
    assert!(matches!(
        second.project_discovery_graph(&key("site:one"), Default::default(), &discovery),
        Err(authored::SiteGraphError::Discovery)
    ));
    let error = second
        .project_discovery_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect_err("foreign owner refused before a successful page");
    assert_eq!(error.code(), "authored_projection_binding");
    let own = second.discovery().expect("independent correct owner");
    assert_eq!(
        pages
            .discoverable(&own)
            .expect("positive control")
            .pages()
            .len(),
        1
    );
}

#[test]
fn project_only_materialization_can_have_a_complete_empty_discovery_set() {
    let compiled = configuration();
    let text = document("content:one", "/guide/", PAGE);
    let corpus = corpus(&[("project/guide.md", &text)]);
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("exact project provenance");
    let discovery = resolved
        .discovery()
        .expect("false is valid source-bound selection");
    let node = resolved
        .content()
        .graph()
        .node(&key("content:one"))
        .expect("native node retained");
    assert!(discovery.owns(node));
    assert!(!discovery.contains(&node.id));
    assert!(discovery.node(node).is_none());
    let full = resolved
        .project_site_pages(&key("site:one"))
        .expect("full materialization selection");
    assert_eq!(full.pages().len(), 1);
    let original = full.sitemap(1 << 20).expect("full private routes");
    assert!(
        std::str::from_utf8(&original)
            .expect("XML")
            .contains("https://one.example/mount/guide/")
    );
    let visible = full
        .discoverable(&discovery)
        .expect("empty discovery is not an empty-site error");
    assert_eq!(visible.pages().len(), 0);
    let sitemap = visible.sitemap(1 << 20).expect("valid empty sitemap");
    assert!(
        !std::str::from_utf8(&sitemap)
            .expect("XML")
            .contains("<loc>")
    );
    let page = resolved
        .project_discovery_page(
            &node.id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("direct page still renders");
    assert!(page.body_html.contains("Retained body"));
    assert!(page.discovery_view(&discovery).is_some());
    assert!(page.listing(&discovery).is_none());
    assert_eq!(
        full.sitemap(1 << 20).expect("full set remains unchanged"),
        original
    );
}

#[test]
fn authored_eligibility_preserves_scalar_metadata_but_not_implicit_structural_expansion() {
    let compiled = configuration();
    let home = document("collection:home", "/", "kind='collection'");
    let child = document(
        "collection:child",
        "/child/",
        "kind='collection',parent='collection:home'",
    );
    let page = document("content:one", "/guide/", PAGE).replacen(
        "+++\n",
        "+++\nprojects=[{project='project:one',kind='about'}]\n",
        1,
    );
    let journey = document(
        "journey:one",
        "/journey/",
        "kind='journey',collection='collection:home',members=[{content='content:one',primary=true}]",
    );
    let corpus = corpus(&[
        ("authored/home.md", &home),
        ("authored/child.md", &child),
        ("authored/guide.md", &page),
        ("authored/journey.md", &journey),
    ]);
    let resolved =
        authored::resolve(&compiled, &corpus, Default::default()).expect("full authored graph");
    let discovery = resolved
        .discovery()
        .expect("authored source remains eligible despite private project subject");
    for node in resolved.content().graph().nodes() {
        assert!(discovery.contains(&node.id));
        let native = json!(node);
        let projected = json!(discovery.node(node).expect("same native eligible node"));
        for field in [
            "id",
            "site",
            "title",
            "description",
            "published",
            "modified",
            "language",
            "lifecycle",
            "attributions",
            "route",
        ] {
            assert!(
                native.get(field).is_some(),
                "nonempty scalar fixture {field}"
            );
            assert_eq!(
                projected[field], native[field],
                "exact authored metadata {field}"
            );
        }
        assert_eq!(projected["structure"]["kind"], native["structure"]["kind"]);
        if node.id == key("content:one") {
            assert_eq!(
                projected["structure"]["family"],
                native["structure"]["family"]
            );
        }
        for endpoint in ["parent", "collection", "members"] {
            assert!(
                projected["structure"].get(endpoint).is_none(),
                "no aggregate structural re-expansion"
            );
        }
    }
    let original: Value = json!(
        resolved
            .content()
            .graph()
            .node(&key("journey:one"))
            .expect("original journey")
    );
    assert_eq!(original["structure"]["collection"], "collection:home");
    assert_eq!(
        original["structure"]["members"][0]["content"],
        "content:one"
    );
    assert!(!resolved.content().compilation().workspace.placements()[0].discoverable);
}
