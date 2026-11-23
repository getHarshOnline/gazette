//! Session addresses recompile shared projections without changing publication configuration.

use std::collections::BTreeMap;

use gazette_compiler::{
    authored::{self, ResolvedAuthored, Selection, SelectionError},
    corpus::{File, Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        Key,
        content::{self, Disposition},
        routing::PublicBaseUrl,
    },
    source::EntryPath,
};

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}
fn path(value: &str) -> EntryPath {
    value.parse().expect("fixture path")
}
fn urls(values: &[(&str, &str)]) -> BTreeMap<Key, PublicBaseUrl> {
    values
        .iter()
        .map(|(site, url)| (key(site), url.parse().expect("typed base URL")))
        .collect()
}

fn compilation() -> workspace::Compilation {
    workspace::compile("[[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
        [[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
        [sites.composition]\ntheme={package='theme:preview',palette='palette:plain'}\n\
        [[sites]]\nid='site:unbound'\npublisher='owner:one'\ndeployable=false\n\
        [[sources]]\nid='source:one'\nlocation={kind='repository',root='/private/preview-fixture',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
        [[theme_packages]]\npackage='theme:preview'\nsource='source:one'\nmanifest='theme.toml'\n\
        [[brands]]\nid='brand:one'\nname='One'\n\
        [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/pr%C3%A9fix/'\n\
        authored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]\n\
        machine={feeds={identity_base='https://identity.test/stable/',channels=[{id='feed:one',channel='collection:one',path='/feed.json',format='json',scope={kind='site'}}]}}\n\
        [[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/other/'\n\
        authored=[{source='source:one',target='two',static_assets={route_prefix='/media/'}}]\n", Default::default()).expect("native workspace configuration")
}

fn files(body: &str) -> Vec<(String, String)> {
    vec![
        ("one/home.md".into(), "+++\n[content]\nid='collection:one'\nsite='site:one'\ntitle='Home'\nlifecycle='published'\nroute={path='/'}\nstructure={kind='collection'}\n+++\n# Home\n".into()),
        ("one/page.md".into(), format!("+++\ncollections=['collection:one']\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='published'\nroute={{path='/article/',aliases=['/old.html']}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n{body}")),
        ("two/page.md".into(), "+++\n[content]\nid='content:two'\nsite='site:two'\ntitle='Two'\nlifecycle='published'\nroute={path='/foreign/',aliases=['/old-foreign.html']}\nstructure={kind='page',family={kind='article'}}\n+++\n# Foreign\n".into()),
        ("one/image.png".into(), "opaque retained image bytes".into()),
        ("two/image.png".into(), "other retained image bytes".into()),
    ]
}

fn corpus(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(name, source)| Input {
            path: path(name),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native retained corpus")
}

fn resolve<'a, 's>(
    compiled: &'a workspace::Compilation,
    corpus: &'a ParsedCorpus<'s>,
) -> ResolvedAuthored<'a, 's> {
    authored::resolve(compiled, corpus, Default::default()).expect("shared publication graph")
}

fn feed(resolved: &ResolvedAuthored<'_, '_>) -> serde_json::Value {
    let pages = resolved
        .content()
        .graph()
        .nodes()
        .filter(|node| node.site == key("site:one"))
        .map(|node| {
            (
                node.id.clone(),
                resolved
                    .project_page(
                        &node.id,
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak,
                    )
                    .expect("real page projection"),
            )
        })
        .collect();
    let settings = resolved
        .content()
        .compilation()
        .workspace
        .publication_for(&key("site:one"))
        .expect("publication")
        .machine
        .feeds
        .as_ref()
        .expect("configured feed");
    let bytes = resolved
        .project_feed(&key("site:one"), &settings.channels[0], &pages)
        .expect("native feed projection")
        .render(1 << 20)
        .expect("complete JSON feed")
        .expect("configured retained output");
    serde_json::from_slice(&bytes).expect("native feed bytes")
}

fn all(resolved: &ResolvedAuthored<'_, '_>) -> Selection {
    Selection {
        content: resolved
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: resolved
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| matches!(file, File::Asset(_)))
            .map(|(path, _)| (path.clone(), Disposition::Include))
            .collect(),
    }
}

#[test]
fn routes_aliases_assets_links_and_feeds_share_session_urls_without_mutating_identity() {
    let compiled = compilation();
    let before = serde_json::to_vec(&compiled).expect("original full compilation");
    let files = files("[foreign](https://two.test/other/foreign/) ![local](image.png)\n");
    let corpus = corpus(&files);
    let original = resolve(&compiled, &corpus);
    assert!(original.content().preview_urls().is_none());
    let original_feed = feed(&original);
    let asset_path = path("one/image.png");
    assert_eq!(
        original
            .assets_for(&key("site:one"))
            .expect("prime original cache")
            .get(&asset_path)
            .expect("original asset")
            .url,
        "https://one.test/pr%C3%A9fix/media/image.png"
    );
    let output = original
        .content()
        .route(&key("content:one"))
        .expect("original route")
        .output
        .clone();
    let projected = original
        .with_preview_urls(urls(&[("site:one", "http://127.0.0.1:42001/pr%C3%A9fix/")]))
        .expect("selected loopback projection");
    assert!(std::ptr::eq(projected.content().compilation(), &compiled));
    assert_eq!(
        projected
            .content()
            .preview_urls()
            .expect("explicit session marker")
            .len(),
        1
    );
    let route = projected
        .content()
        .route(&key("content:one"))
        .expect("same route compiler");
    assert_eq!(
        route.canonical_url,
        "http://127.0.0.1:42001/pr%C3%A9fix/article/"
    );
    assert_eq!(
        route.aliases[0].url,
        "http://127.0.0.1:42001/pr%C3%A9fix/old.html"
    );
    assert_eq!(route.output, output);
    assert_eq!(route.aliases[0].output, path("old.html"));
    assert_eq!(
        projected
            .content()
            .base_url(&key("site:two"))
            .expect("explicit foreign fallback")
            .as_str(),
        "https://two.test/other/"
    );
    let asset = projected
        .assets_for(&key("site:one"))
        .expect("cache rebuilt")
        .get(&asset_path)
        .expect("same asset");
    assert_eq!(
        asset.url,
        "http://127.0.0.1:42001/pr%C3%A9fix/media/image.png"
    );
    assert_eq!(asset.content(), b"opaque retained image bytes");
    let page = projected
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("same Markdown resolver");
    assert!(page.body_html.contains("https://two.test/other/foreign/"));
    assert!(
        page.body_html
            .contains("http://127.0.0.1:42001/pr%C3%A9fix/media/image.png")
    );
    let preview_feed = feed(&projected);
    assert_eq!(
        preview_feed["feed_url"],
        "http://127.0.0.1:42001/pr%C3%A9fix/feed.json"
    );
    assert_eq!(
        preview_feed["home_page_url"],
        "http://127.0.0.1:42001/pr%C3%A9fix/"
    );
    assert_eq!(preview_feed["items"][0]["url"], route.canonical_url);
    assert_eq!(
        preview_feed["_gazette_feed"]["id"],
        original_feed["_gazette_feed"]["id"]
    );
    assert_eq!(
        preview_feed["items"][0]["id"],
        original_feed["items"][0]["id"]
    );
    assert_eq!(
        serde_json::to_vec(&compiled).expect("unchanged full compilation"),
        before
    );
    let second = projected
        .with_preview_urls(urls(&[
            ("site:one", "http://[::1]:42003/pr%C3%A9fix/"),
            ("site:two", "http://127.0.0.1:42002/other/"),
        ]))
        .expect("new session replaces previous bindings");
    assert_eq!(
        second
            .assets_for(&key("site:one"))
            .expect("no stale cached first-session URL")
            .get(&asset_path)
            .expect("asset")
            .url,
        "http://[::1]:42003/pr%C3%A9fix/media/image.png"
    );
    assert_eq!(
        second
            .content()
            .route(&key("content:two"))
            .expect("second-site projection")
            .canonical_url,
        "http://127.0.0.1:42002/other/foreign/"
    );
    assert!(
        second
            .project_page(
                &key("content:one"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("owned hyperlinks follow the selected site bindings")
            .body_html
            .contains("http://127.0.0.1:42002/other/foreign/")
    );
}

#[test]
fn body_links_share_bounded_destination_facts_with_the_machine_projection() {
    use gazette_core::markdown::{LinkDestinationKind, LinkRelationship};

    let compiled = compilation();
    let mut fixture_files = files(
        "[foreign](https://two.test/other/foreign/) [download](image.png) \
         [outside](https://outside.test/path) [missing](/missing/)\n",
    );
    fixture_files[1].1 = fixture_files[1].1.replace(
        "+++\ncollections",
        "+++\nrelated=['content:two']\ncollections",
    );
    let parsed = corpus(&fixture_files);
    let resolved = resolve(&compiled, &parsed);
    let page = resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("annotated page");
    assert_eq!(page.data.links.len(), 4);
    let foreign = &page.data.links[0];
    assert_eq!(foreign.address, "content:one#link-1");
    assert_eq!(foreign.kind, LinkDestinationKind::CrossSite);
    assert_eq!(foreign.content.as_ref().unwrap(), &key("content:two"));
    assert_eq!(foreign.site.as_ref().unwrap(), &key("site:two"));
    assert_eq!(foreign.relationships, [LinkRelationship::Related]);
    let presentation = foreign.presentation.as_ref().expect("known site Theme");
    assert_eq!(presentation.theme.as_str(), "theme:preview");
    assert_eq!(
        presentation.palette.as_ref().unwrap(),
        &key("palette:plain")
    );
    let download = &page.data.links[1];
    assert_eq!(download.kind, LinkDestinationKind::Resource);
    assert_eq!(download.site.as_ref().unwrap(), &key("site:one"));
    assert!(download.content.is_none() && download.relationships.is_empty());
    let outside = &page.data.links[2];
    assert_eq!(outside.kind, LinkDestinationKind::External);
    assert!(outside.site.is_none() && outside.content.is_none());
    let missing = &page.data.links[3];
    assert_eq!(missing.kind, LinkDestinationKind::InternalNative);
    assert_eq!(missing.site.as_ref().unwrap(), &key("site:one"));
    assert!(missing.content.is_none() && missing.relationships.is_empty());
    for link in &page.data.links {
        assert!(
            page.body_html
                .contains(&format!("data-gazette-link=\"{}\"", link.address))
        );
    }
    assert!(
        page.body_html
            .contains("data-gazette-destination-kind=\"cross-site\"")
    );
    assert!(
        page.body_html
            .contains("data-gazette-destination-relationships=\"related\"")
    );
    assert!(
        page.body_html
            .contains("data-gazette-destination-theme=\"theme:preview\"")
    );
    assert!(
        page.body_html
            .contains("data-gazette-destination-palette=\"palette:plain\"")
    );
    assert!(
        page.body_html
            .contains("data-gazette-destination-kind=\"resource\"")
    );
    let public = serde_json::to_string(&page).expect("public page model");
    for private in [
        "/private/preview-fixture",
        "source:one",
        "one/page.md",
        "span",
    ] {
        assert!(
            !public.contains(private),
            "private coordinate leaked: {private}"
        );
    }
    let repeated = resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("repeat annotated page");
    assert_eq!(repeated.data.links, page.data.links);
    assert_eq!(repeated.body_html, page.body_html);

    let unsafe_files = files("[unsafe](javascript:alert(1))\n");
    let unsafe_corpus = corpus(&unsafe_files);
    let unsafe_resolved = resolve(&compiled, &unsafe_corpus);
    let error = unsafe_resolved
        .project_page(
            &key("content:one"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("unsafe destination remains rejected before annotation");
    assert_eq!(error.code(), "markdown_unsafe_url");
}

#[test]
fn selection_checks_the_same_preview_asset_and_content_addresses() {
    let compiled = compilation();
    for (body, expected) in [
        (
            "![foreign](http://127.0.0.1:42002/other/media/image.png)\n",
            "selection_asset",
        ),
        (
            "[foreign](http://127.0.0.1:42002/other/foreign/)\n",
            "selection_excluded_link",
        ),
        (
            "[foreign alias](http://127.0.0.1:42002/other/old-foreign.html)\n",
            "selection_excluded_link",
        ),
        (
            "![original asset](https://two.test/other/media/image.png?v=1#retained)\n",
            "selection_asset",
        ),
        (
            "[original content](https://two.test/other/foreign/)\n",
            "selection_excluded_link",
        ),
        (
            "[original alias](https://two.test/other/old-foreign.html)\n",
            "selection_excluded_link",
        ),
    ] {
        let files = files(body);
        let corpus = corpus(&files);
        let projected = resolve(&compiled, &corpus)
            .with_preview_urls(urls(&[
                ("site:one", "http://127.0.0.1:42001/pr%C3%A9fix/"),
                ("site:two", "http://127.0.0.1:42002/other/"),
            ]))
            .expect("preview before candidate closure");
        let mut selection = all(&projected);
        if expected == "selection_asset" {
            selection
                .assets
                .insert(path("two/image.png"), Disposition::Exclude);
        } else {
            selection
                .content
                .insert(key("content:two"), Disposition::Exclude);
        }
        let error = projected
            .into_selected(&selection)
            .expect_err("excluded known preview target cannot become an external untracked URL");
        assert_eq!(error.code(), expected);
        assert!(matches!(
            error,
            SelectionError::Asset { .. } | SelectionError::Link { .. }
        ));
    }
    let files = files("![local](image.png)\n");
    let corpus = corpus(&files);
    let projected = resolve(&compiled, &corpus)
        .with_preview_urls(urls(&[("site:one", "http://127.0.0.1:42001/pr%C3%A9fix/")]))
        .expect("preview");
    let selected = projected
        .into_selected(&all(&resolve(&compiled, &corpus)))
        .expect("complete explicit candidate");
    assert_eq!(
        selected
            .content()
            .route(&key("content:one"))
            .expect("same candidate route")
            .canonical_url,
        "http://127.0.0.1:42001/pr%C3%A9fix/article/"
    );
    assert!(selected.content().preview_urls().is_some());
}

#[test]
fn invalid_preview_bindings_fail_without_fallback_or_configuration_mutation() {
    let compiled = compilation();
    let before = serde_json::to_vec(&compiled).expect("original configuration");
    let files = files("Body\n");
    let corpus = corpus(&files);
    for (site, base, expected) in [
        (
            "site:absent",
            "http://127.0.0.1:42001/pr%C3%A9fix/",
            "content_preview_site",
        ),
        (
            "site:unbound",
            "http://127.0.0.1:42001/",
            "content_preview_base",
        ),
        (
            "site:one",
            "https://127.0.0.1:42001/pr%C3%A9fix/",
            "content_preview_url",
        ),
        (
            "site:one",
            "http://localhost:42001/pr%C3%A9fix/",
            "content_preview_url",
        ),
        (
            "site:one",
            "http://192.0.2.1:42001/pr%C3%A9fix/",
            "content_preview_url",
        ),
        (
            "site:one",
            "http://127.0.0.1:0/pr%C3%A9fix/",
            "content_preview_url",
        ),
        (
            "site:one",
            "http://127.0.0.1:42001/changed/",
            "content_preview_url",
        ),
    ] {
        let error = resolve(&compiled, &corpus)
            .with_preview_urls(urls(&[(site, base)]))
            .expect_err("invalid serving address refused");
        assert!(
            matches!(error, content::Error::Invalid(ref diagnostics) if diagnostics.len() == 1 && diagnostics[0].code == expected),
            "{error:?}"
        );
    }
    assert_eq!(
        serde_json::to_vec(&compiled).expect("unchanged original configuration"),
        before
    );
}

#[test]
fn preview_binding_count_and_text_are_bounded_before_retaining_projection() {
    let compiled = compilation();
    let files = files("Body\n");
    let corpus = corpus(&files);
    for bindings in [
        BTreeMap::new(),
        (0..129)
            .map(|index| {
                (
                    key(&format!("site:{index}")),
                    "http://127.0.0.1:42001/".parse().expect("loopback base"),
                )
            })
            .collect(),
    ] {
        assert!(matches!(
            resolve(&compiled, &corpus).with_preview_urls(bindings),
            Err(content::Error::Limit("preview URL sites"))
        ));
    }
    let resolved = authored::resolve(
        &compiled,
        &corpus,
        content::Limits {
            text_bytes: 2048,
            ..Default::default()
        },
    )
    .expect("bounded ordinary graph");
    let excessive = format!("http://127.0.0.1:42001/{}/", "a".repeat(4096));
    assert!(matches!(
        resolved.with_preview_urls(urls(&[("site:one", &excessive)])),
        Err(content::Error::Limit("preview URL bytes"))
    ));
}
