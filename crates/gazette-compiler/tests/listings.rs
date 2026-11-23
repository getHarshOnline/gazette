//! Native retained-corpus proofs for declarative listing selection and pagination.

#[path = "listings/chronology.rs"]
mod chronology;

use std::collections::BTreeMap;

use gazette_compiler::{
    authored::{self, ListingError, ListingPage},
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::{Key, content::Limits};
use serde_json::json;

fn key(value: &str) -> Key {
    value.parse().expect("explicit identity")
}

fn listing(content: &str, selection: &str, order: &str, size: usize) -> String {
    format!(
        "{{content='{content}',select={{kind='{selection}'}},order={{{order}}},pagination={{page_size={size},next_pages='page/{{number}}/'}}}}"
    )
}
const DATES: &str = "by='published',direction='descending',undated='last-title-then-id'";

fn configuration(listings: &[String], extra: &str) -> workspace::Compilation {
    workspace::compile(&format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:retained'\nlocation={{kind='repository',root='/private/listing-source-not-read',commit='1111111111111111111111111111111111111111',subdirectory='content'}}\n\
         [[brands]]\nid='brand:one'\nname='Native listing fixture'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/mount/'\nauthored=[{{source='source:retained',target='one'}},{{source='source:retained',target='descendant'}}]\nlistings=[{}]\n{extra}\n\
         [[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/foreign/'\nauthored=[{{source='source:retained',target='two'}}]\n\
         [[projects]]\nid='project:hidden'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
         [[placements]]\nsite='site:one'\nproject='project:hidden'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={{source='source:retained',target='hidden'}}\n", listings.join(",")), Default::default()).expect("real strict workspace declarations")
}

#[allow(clippy::too_many_arguments)]
fn document(
    id: &str,
    site: &str,
    route: &str,
    title: &str,
    metadata: &str,
    edges: &str,
    structure: &str,
    body: &str,
) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='{site}'\ntitle='{title}'\nlifecycle='published'\nlanguage='en'\nroute={{path='{route}'}}\n{metadata}\nstructure={{{structure}}}\n+++\n{body}"
    )
}
fn index(id: &str, route: &str) -> String {
    document(
        id,
        "site:one",
        route,
        "Existing index",
        "",
        "",
        "kind='page',family={kind='index'}",
        "",
    )
}
fn article(id: &str, title: &str, metadata: &str, edges: &str) -> String {
    document(
        id,
        "site:one",
        &format!("/{}/", id.split(':').next_back().expect("id")),
        title,
        metadata,
        edges,
        "kind='page',family={kind='article'}",
        "# Retained body\n",
    )
}
fn collection(id: &str, title: &str, parent: &str) -> String {
    document(
        id,
        "site:one",
        &format!("/{}/", id.split(':').next_back().expect("id")),
        title,
        "",
        "",
        &format!("kind='collection'{parent}"),
        "Collection intro.\n",
    )
}
fn parse(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(name, text)| Input {
            path: name.parse().expect("logical retained path"),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("actual native frontmatter and Markdown AST")
}
fn project(
    compiled: &workspace::Compilation,
    corpus: &ParsedCorpus<'_>,
) -> Result<Vec<ListingPage>, ListingError> {
    let resolved =
        authored::resolve(compiled, corpus, Default::default()).expect("whole native graph");
    resolved.project_listing_pages(
        &key("site:one"),
        &resolved.discovery().expect("exact bound source discovery"),
    )
}
fn ids(page: &ListingPage) -> Vec<&str> {
    page.items.iter().map(|item| item.id.as_str()).collect()
}

#[test]
fn all_pages_uses_discovery_and_native_pagination_without_creating_editorial_ancestry() {
    let compiled = configuration(
        &[listing("content:search", "all-pages", DATES, 2)],
        "search_page='content:search'",
    );
    let mut files = vec![
        ("one/search.md".into(), index("content:search", "/search/")),
        (
            "one/article.md".into(),
            article(
                "content:article",
                "An article",
                "published='2026-10-28'",
                "",
            ),
        ),
        (
            "one/docs.md".into(),
            document(
                "content:docs",
                "site:one",
                "/docs/",
                "A guide",
                "",
                "",
                "kind='page',family={kind='documentation'}",
                "A guide",
            ),
        ),
        (
            "one/collection.md".into(),
            collection("collection:root", "A collection", ""),
        ),
        (
            "one/journey.md".into(),
            document(
                "journey:one",
                "site:one",
                "/journey/",
                "A journey",
                "",
                "",
                "kind='journey',collection='collection:root',members=[]",
                "",
            ),
        ),
        (
            "hidden/article.md".into(),
            article("content:hidden", "Hidden", "", ""),
        ),
        (
            "two/foreign.md".into(),
            document(
                "content:foreign",
                "site:two",
                "/foreign/",
                "Foreign",
                "",
                "",
                "kind='page',family={kind='article'}",
                "",
            ),
        ),
        (
            "one/copy.md".into(),
            article("content:copy", "A copy", "", "")
                .replace("path='/copy/'", "path='/copy/',canonical='content:article'"),
        ),
        (
            "one/unrouted.md".into(),
            article("content:unrouted", "Unrouted", "", "")
                .replace("route={path='/unrouted/'}\n", ""),
        ),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&key("site:one"), &discovery)
        .unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].total_items, 4);
    assert!(pages.iter().all(|page| page.page_size == 2));
    let search_page = resolved
        .project_page(
            &key("content:search"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        search_page
            .discovery_view(&discovery)
            .unwrap()
            .search()
            .is_none()
    );
    let article_page = resolved
        .project_page(
            &key("content:article"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        article_page
            .discovery_view(&discovery)
            .unwrap()
            .search()
            .is_some()
    );
    assert_eq!(
        pages.iter().flat_map(|page| ids(page)).collect::<Vec<_>>(),
        [
            "content:article",
            "collection:root",
            "content:docs",
            "journey:one"
        ]
    );
    assert_eq!(
        pages[0].next.as_deref(),
        Some("https://one.test/mount/search/page/2/")
    );
    assert!(pages.iter().all(|page| page.subject.is_none()));
    let locations = pages
        .iter()
        .filter_map(|page| page.location_for(&key("content:article")));
    assert!(
        resolved
            .project_browsing_locations_with(&key("content:article"), Some(&discovery), locations)
            .unwrap()
            .is_empty()
    );
    // Adding a retained source record updates the same native selection and page count.
    drop(resolved);
    drop(corpus);
    files.push((
        "descendant/addition.md".into(),
        article("content:added", "Another article", "", ""),
    ));
    let pages = project(&compiled, &parse(&files)).unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].total_items, 5);
    let schema =
        serde_json::to_string(&schemars::schema_for!(workspace::ListingSelection)).unwrap();
    assert!(schema.contains("all-pages"));
}

#[test]
fn nested_generated_indexes_do_not_claim_descendant_pages_as_leaf_companions() {
    let compiled = configuration(
        &[
            listing("content:blog-index", "standalone-articles", DATES, 10),
            listing("content:collections-index", "root-collections", DATES, 10),
        ],
        "",
    );
    let files = vec![
        (
            "one/blog/index.md".into(),
            index("content:blog-index", "/blog/"),
        ),
        (
            "one/blog/entry/index.md".into(),
            article("content:entry", "Standalone", "", "")
                .replace("path='/entry/'", "path='/blog/entry/'"),
        ),
        (
            "one/collections/index.md".into(),
            index("content:collections-index", "/collections/"),
        ),
        (
            "one/collections/inquiry/index.md".into(),
            collection("collection:inquiry", "An inquiry", "")
                .replace("path='/inquiry/'", "path='/collections/inquiry/'"),
        ),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("generated views coexist with independently declared descendant pages");
    assert!(resolved.page_package(&key("content:blog-index")).is_none());
    assert!(
        resolved
            .page_package(&key("content:collections-index"))
            .is_none()
    );
    assert!(resolved.page_package(&key("content:entry")).is_some());
    let pages = resolved
        .project_listing_pages(
            &key("site:one"),
            &resolved.discovery().expect("complete graph"),
        )
        .expect("native derived views");
    assert_eq!(pages.len(), 2);
    assert_eq!(ids(&pages[0]), ["content:entry"]);
    assert_eq!(ids(&pages[1]), ["collection:inquiry"]);
}

#[test]
fn standalone_selection_uses_complete_parentage_before_discovery_and_keeps_descendant_sources() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 2)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/new.md".into(),
            article("content:new", "New", "published='2026-10-05'", ""),
        ),
        (
            "descendant/older.md".into(),
            article("content:older", "Older", "published='2026-10-01'", ""),
        ),
        (
            "one/undated.md".into(),
            article(
                "content:undated",
                "A missing date",
                "modified='2099-01-01'",
                "",
            ),
        ),
        (
            "hidden/parent.md".into(),
            collection("collection:hidden", "Hidden parent", "")
                .replace("path='/hidden/'", "path='/hidden-root/'"),
        ),
        (
            "one/contained.md".into(),
            article(
                "content:contained",
                "Contained",
                "published='2026-10-06'",
                "collections=['collection:hidden']\n",
            ),
        ),
        (
            "hidden/standalone.md".into(),
            article("content:hidden", "Unlisted", "published='2026-10-07'", ""),
        ),
        (
            "one/journey.md".into(),
            document(
                "journey:one",
                "site:one",
                "/journey/",
                "Journey",
                "",
                "",
                "kind='journey',collection='collection:hidden',members=[{content='content:step',primary=true}]",
                "Journey intro.",
            ),
        ),
        (
            "one/step.md".into(),
            article(
                "content:step",
                "Journey member",
                "published='2026-10-08'",
                "",
            ),
        ),
        (
            "two/foreign.md".into(),
            document(
                "content:foreign",
                "site:two",
                "/foreign/",
                "Foreign",
                "published='2026-10-09'",
                "",
                "kind='page',family={kind='article'}",
                "Foreign body.",
            ),
        ),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("complete valid editorial graph");
    let before = json!(resolved.content().graph());
    let pages = resolved
        .project_listing_pages(&key("site:one"), &resolved.discovery().expect("one owner"))
        .expect("shared native listing");
    assert_eq!(pages.len(), 2);
    assert_eq!(ids(&pages[0]), ["content:new", "content:older"]);
    assert_eq!(ids(&pages[1]), ["content:undated"]);
    assert_eq!(pages[0].total_items, 3);
    assert_eq!(pages[0].page_count, 2);
    assert_eq!(
        pages[0].next.as_deref(),
        Some("https://one.test/mount/blog/page/2/")
    );
    assert_eq!(
        pages[1].previous.as_deref(),
        Some("https://one.test/mount/blog/")
    );
    assert!(pages[0].previous.is_none());
    assert!(pages[1].next.is_none());
    assert_eq!(pages[1].output.as_str(), "blog/page/2/index.html");
    assert_eq!(pages[1].content, key("content:index"));
    let location = pages[1]
        .location_for(&key("content:undated"))
        .expect("exact listed item");
    assert_eq!(location.index, &key("content:index"));
    assert_eq!(location.via, &key("content:undated"));
    assert_eq!(location.url, "https://one.test/mount/blog/page/2/");
    assert_eq!(
        (location.page_number, location.page_count),
        (Some(2), Some(2))
    );
    for excluded in [
        "content:contained",
        "content:hidden",
        "content:step",
        "content:foreign",
    ] {
        assert!(
            pages
                .iter()
                .all(|page| page.location_for(&key(excluded)).is_none())
        );
    }

    assert!(
        pages[1].items[0].published.is_none(),
        "modified is never a publication-date fallback"
    );
    assert_eq!(
        json!(resolved.content().graph()),
        before,
        "listing introduces no content or containment"
    );
    let model = json!(pages);
    let graph = resolved
        .project_site_graph(&key("site:one"), Default::default())
        .expect("complete native site graph");
    assert_eq!(
        json!(graph.listings),
        model,
        "one compiler-owned listing query"
    );
    let discovery = resolved.discovery().expect("same resolved source owner");
    let emitted = resolved
        .project_discovery_graph(&key("site:one"), Default::default(), &discovery)
        .expect("same discovery graph");
    assert_eq!(json!(emitted)["listings"], model);
    let text = model.to_string();
    for forbidden in [
        "/private/",
        "source:retained",
        "body",
        "content:hidden",
        "content:contained",
        "content:step",
        "content:foreign",
    ] {
        assert!(
            !text.contains(forbidden),
            "private/structural expansion: {forbidden}"
        );
    }
    let ld = pages[0].json_ld();
    assert_eq!(ld["identifier"], "content:index");
    assert_eq!(
        ld["mainEntity"]["itemListElement"][0]["item"]["identifier"],
        "content:new"
    );
    assert_eq!(ld["mainEntity"]["itemListElement"][1]["position"], 2);
    let route = pages[1].serving_route();
    assert_eq!(route.url, pages[1].url);
    assert_eq!(route.canonical_url, pages[1].url);
    assert!(route.aliases.is_empty() && route.mirrors.is_empty());
}

#[test]
fn documentation_purpose_excludes_guides_without_any_product_landing_registration() {
    let compiled = configuration(
        &[
            listing(
                "content:thoughts",
                "root-collections",
                "first=[],remaining='title-then-id'",
                10,
            ),
            listing("content:blog", "standalone-articles", DATES, 10),
        ],
        "",
    );
    let files = vec![
        (
            "one/thoughts.md".into(),
            index("content:thoughts", "/thoughts/"),
        ),
        ("one/blog.md".into(), index("content:blog", "/blog/")),
        (
            "one/docs.md".into(),
            collection(
                "collection:manual",
                "Product guides",
                ",purpose='documentation'",
            ),
        ),
        (
            "one/guide.md".into(),
            article(
                "content:guide",
                "A product task",
                "",
                "collections=['collection:manual']\n",
            ),
        ),
        (
            "one/independent-guide.md".into(),
            document(
                "content:independent-guide",
                "site:one",
                "/independent-guide/",
                "Independent guide",
                "",
                "",
                "kind='page',family={kind='documentation'}",
                "Read the guide.",
            ),
        ),
        (
            "one/essays.md".into(),
            collection("collection:essays", "Essays", ""),
        ),
        (
            "one/article.md".into(),
            article("content:article", "An editorial article", "", ""),
        ),
    ];
    let corpus = parse(&files);
    let pages = project(&compiled, &corpus).unwrap();
    assert_eq!(
        ids(pages
            .iter()
            .find(|page| page.content == key("content:thoughts"))
            .unwrap()),
        ["collection:essays"]
    );
    assert_eq!(
        ids(pages
            .iter()
            .find(|page| page.content == key("content:blog"))
            .unwrap()),
        ["content:article"]
    );
}

#[test]
fn root_collections_pin_native_candidates_then_sort_titles_and_ids_without_promoting_children() {
    let compiled = configuration(
        &[listing(
            "content:index",
            "root-collections",
            "first=['collection:z','collection:hidden','collection:descendant'],remaining='title-then-id'",
            2,
        )],
        "",
    );
    let files = vec![
        (
            "one/index.md".into(),
            index("content:index", "/collections/"),
        ),
        (
            "one/z.md".into(),
            collection("collection:z", "Last alphabetically", ""),
        ),
        (
            "hidden/hidden.md".into(),
            collection("collection:hidden", "Hidden root", ""),
        ),
        (
            "descendant/root.md".into(),
            collection("collection:descendant", "Descendant source root", ""),
        ),
        ("one/b.md".into(), collection("collection:b", "Same", "")),
        ("one/a.md".into(), collection("collection:a", "Same", "")),
        (
            "one/child.md".into(),
            collection(
                "collection:child",
                "First alphabetically",
                ",parent='collection:hidden'",
            ),
        ),
    ];
    let pages = project(&compiled, &parse(&files))
        .expect("validated hidden pin is filtered, not unresolved");
    assert_eq!(pages.len(), 2);
    assert_eq!(ids(&pages[0]), ["collection:z", "collection:descendant"]);
    assert_eq!(ids(&pages[1]), ["collection:a", "collection:b"]);
    assert_eq!(pages[0].total_items, 4);
}

#[test]
fn preview_urls_reuse_the_validated_base_and_keep_identity_and_site_relative_outputs() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 1)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        ("one/a.md".into(), article("content:a", "A", "", "")),
        ("one/b.md".into(), article("content:b", "B", "", "")),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("native owner")
        .with_preview_urls(BTreeMap::from([(
            key("site:one"),
            "http://127.0.0.1:3456/mount/"
                .parse()
                .expect("loopback prefix"),
        )]))
        .expect("validated preview URL override");
    let pages = resolved
        .project_listing_pages(
            &key("site:one"),
            &resolved.discovery().expect("same selected owner"),
        )
        .expect("preview projection");
    assert_eq!(pages[1].url, "http://127.0.0.1:3456/mount/blog/page/2/");
    assert_eq!(pages[0].items[0].url, "http://127.0.0.1:3456/mount/a/");
    assert_eq!(pages[1].content, key("content:index"));
    assert_eq!(pages[1].output.as_str(), "blog/page/2/index.html");
}

#[test]
fn empty_listing_keeps_one_existing_owner_without_a_continuation() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
    let pages = project(&compiled, &parse(&files)).expect("valid empty index");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_count, 1);
    assert_eq!(pages[0].total_items, 0);
    assert!(pages[0].items.is_empty() && pages[0].next.is_none() && pages[0].previous.is_none());
    assert_eq!(pages[0].route.as_str(), "/blog/");
}

#[test]
fn invalid_owner_body_family_site_route_and_republication_are_not_silently_reinterpreted() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let valid = index("content:index", "/blog/");
    for invalid in [
        format!("{valid}<!-- retained authored body -->"),
        valid.replace("kind='index'", "kind='article'"),
        valid.replace("route={path='/blog/'}", ""),
        index("content:index", "/blog.html"),
    ] {
        let files = vec![("one/index.md".into(), invalid)];
        let error = project(&compiled, &parse(&files)).expect_err("invalid listing owner");
        assert_eq!(error.code(), "listing_binding");
        assert_eq!(error.content(), Some(&key("content:index")));
    }
    let files = vec![(
        "two/index.md".into(),
        valid.replace("site='site:one'", "site='site:two'"),
    )];
    assert_eq!(
        project(&compiled, &parse(&files))
            .expect_err("wrong-site index")
            .code(),
        "listing_binding"
    );
    let files = vec![
        (
            "one/index.md".into(),
            valid.replace(
                "route={path='/blog/'}",
                "route={path='/blog/',canonical='content:original'}",
            ),
        ),
        (
            "one/original.md".into(),
            index("content:original", "/original/"),
        ),
    ];
    assert_eq!(
        project(&compiled, &parse(&files))
            .expect_err("republished index")
            .code(),
        "listing_binding"
    );
}

#[test]
fn priority_unknown_wrong_kind_nested_and_foreign_candidates_refuse_without_a_prefix() {
    for pin in [
        "collection:missing",
        "content:article",
        "collection:child",
        "collection:foreign",
    ] {
        let compiled = configuration(
            &[listing(
                "content:index",
                "root-collections",
                &format!("first=['{pin}'],remaining='title-then-id'"),
                20,
            )],
            "",
        );
        let files = vec![
            (
                "one/index.md".into(),
                index("content:index", "/collections/"),
            ),
            (
                "one/root.md".into(),
                collection("collection:root", "Root", ""),
            ),
            (
                "one/child.md".into(),
                collection("collection:child", "Child", ",parent='collection:root'"),
            ),
            (
                "one/article.md".into(),
                article("content:article", "Article", "", ""),
            ),
            (
                "two/foreign.md".into(),
                collection("collection:foreign", "Foreign", "")
                    .replace("site='site:one'", "site='site:two'"),
            ),
        ];
        let error =
            project(&compiled, &parse(&files)).expect_err("pin must bind an actual candidate");
        assert_eq!(error.code(), "listing_binding");
    }
}

#[test]
fn continuations_cannot_overwrite_primary_alias_mirror_taxonomy_or_file_ancestors() {
    for collision in ["primary", "alias", "mirror", "taxonomy", "ancestor"] {
        let extra = if collision == "taxonomy" {
            "taxonomy_pages=[{taxonomy='taxonomy:topic',route='/blog/page/2/',template='fixture/listing.html'}]"
        } else {
            ""
        };
        let mut compiled_source = configuration(
            &[listing("content:index", "standalone-articles", DATES, 1)],
            "",
        );
        if collision == "taxonomy" {
            // Use the same native configuration builder with a real declared taxonomy.
            let original = format!(
                "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:retained'\nlocation={{kind='repository',root='/private/listing-source-not-read',commit='1111111111111111111111111111111111111111',subdirectory='content'}}\n[[brands]]\nid='brand:one'\nname='Listing'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/mount/'\nauthored=[{{source='source:retained',target='one'}}]\nlistings=[{}]\n{extra}\n[[taxonomies]]\nid='taxonomy:topic'\nlabel='Topics'\nterms=[]\n",
                listing("content:index", "standalone-articles", DATES, 1)
            );
            compiled_source = workspace::compile(&original, Default::default())
                .expect("native generated taxonomy route");
        }
        let a = article("content:a", "A", "", "");
        let mut b = article("content:b", "B", "", "");
        match collision {
            "primary" => b = b.replace("path='/b/'", "path='/blog/page/2/'"),
            "alias" => b = b.replace("path='/b/'", "path='/b/',aliases=['/blog/page/2/']"),
            "mirror" => {
                b = b.replace("site='site:one'", "site='site:two'").replace(
                    "path='/b/'",
                    "path='/b/',mirrors=[{site='site:one',path='/blog/page/2/'}]",
                )
            }
            "ancestor" => b = b.replace("path='/b/'", "path='/blog/page'"),
            _ => {}
        }
        let path_b = if collision == "mirror" {
            "two/b.md"
        } else {
            "one/b.md"
        };
        let files = vec![
            ("one/index.md".into(), index("content:index", "/blog/")),
            ("one/a.md".into(), a),
            (path_b.into(), b),
            ("one/c.md".into(), article("content:c", "C", "", "")),
        ];
        let error =
            project(&compiled_source, &parse(&files)).expect_err("no overwritten existing output");
        assert_eq!(
            error.code(),
            "listing_route_collision",
            "collision={collision}: {error}"
        );
    }
}

#[test]
fn foreign_discovery_owner_is_refused_even_when_graphs_are_equal() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
    let corpus = parse(&files);
    let first = authored::resolve(&compiled, &corpus, Default::default()).expect("first owner");
    let second =
        authored::resolve(&compiled, &corpus, Default::default()).expect("equal independent owner");
    assert!(matches!(
        second.project_listing_pages(
            &key("site:one"),
            &first.discovery().expect("first selection")
        ),
        Err(ListingError::Discovery)
    ));
}

#[test]
fn aggregate_page_and_link_expansion_fail_as_complete_results() {
    let compiled = configuration(
        &[
            listing("content:index", "standalone-articles", DATES, 1),
            listing("content:second", "standalone-articles", DATES, 1),
        ],
        "",
    );
    let mut files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        ("one/second.md".into(), index("content:second", "/second/")),
    ];
    for letter in ["a", "b", "c", "d"] {
        files.push((
            format!("one/{letter}.md"),
            article(&format!("content:{letter}"), letter, "", ""),
        ));
    }
    let corpus = parse(&files);
    let limited = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            nodes: 6,
            ..Default::default()
        },
    )
    .expect("input graph exactly fits");
    assert!(matches!(
        limited.project_listing_pages(
            &key("site:one"),
            &limited.discovery().expect("owner selection")
        ),
        Err(ListingError::Limit("page occurrences"))
    ));
    let limited = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            edges: 3,
            ..Default::default()
        },
    )
    .expect("input graph has no membership edges");
    assert!(matches!(
        limited.project_listing_pages(
            &key("site:one"),
            &limited.discovery().expect("owner selection")
        ),
        Err(ListingError::Limit("link occurrences"))
    ));
}

#[test]
fn republished_copies_never_duplicate_original_listing_references() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/original.md".into(),
            article("content:original", "Original", "", ""),
        ),
        (
            "one/copy.md".into(),
            article("content:copy", "Republished", "", "").replace(
                "path='/copy/'",
                "path='/copy/',canonical='content:original'",
            ),
        ),
    ];
    let pages = project(&compiled, &parse(&files)).expect("original objects only");
    assert_eq!(ids(&pages[0]), ["content:original"]);
    assert_eq!(pages[0].total_items, 1);
}

#[test]
fn authored_lifecycle_is_not_an_implicit_published_only_filter() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/a.md".into(),
            article("content:a", "A", "", "").replace("lifecycle='published'", "lifecycle='draft'"),
        ),
        (
            "one/b.md".into(),
            article("content:b", "B", "", "")
                .replace("lifecycle='published'", "lifecycle='archived'"),
        ),
    ];
    let pages = project(&compiled, &parse(&files))
        .expect("local graph selection preserves lifecycle independently");
    assert_eq!(ids(&pages[0]), ["content:a", "content:b"]);
}

#[test]
fn declared_twenty_item_boundary_has_no_phantom_empty_page() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let mut files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
    for number in 0..20 {
        files.push((
            format!("one/a-{number}.md"),
            article(
                &format!("content:a-{number:02}"),
                &format!("Article {number:02}"),
                "",
                "",
            ),
        ));
    }
    let first = project(&compiled, &parse(&files)).expect("exact full first page");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].items.len(), 20);
    assert!(first[0].next.is_none());
    files.push((
        "one/a-20.md".into(),
        article("content:a-20", "Article 20", "", ""),
    ));
    let next = project(&compiled, &parse(&files)).expect("one additional continuation");
    assert_eq!(next.len(), 2);
    assert_eq!(next[1].items.len(), 1);
    assert_eq!(next[1].page_number, 2);
}

#[test]
fn repeated_long_titles_refuse_before_retaining_an_amplified_listing_set() {
    let compiled = configuration(
        &[
            listing("content:index", "standalone-articles", DATES, 1),
            listing("content:second", "standalone-articles", DATES, 1),
        ],
        "",
    );
    let mut files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        ("one/second.md".into(), index("content:second", "/second/")),
    ];
    for number in 0..30 {
        files.push((
            format!("one/a-{number}.md"),
            article(&format!("content:a-{number:02}"), &"A".repeat(400), "", ""),
        ));
    }
    let corpus = parse(&files);
    let resolved = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            text_bytes: 25_000,
            ..Default::default()
        },
    )
    .expect("input text and route expansion fit independently");
    assert!(
        matches!(
            resolved.project_listing_pages(
                &key("site:one"),
                &resolved.discovery().expect("bound owner")
            ),
            Err(ListingError::Limit(_))
        ),
        "repeating valid titles across output views must not silently truncate"
    );
}

#[test]
fn listing_description_is_authored_optional_metadata_not_an_excerpt() {
    let compiled = configuration(
        &[listing("page:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("page:index", "/blog/")),
        (
            "one/a.md".into(),
            article(
                "article:a",
                "A",
                "description='Exact <authored> question.'",
                "",
            ),
        ),
        ("one/b.md".into(), article("article:b", "B", "", "")),
    ];
    let corpus = parse(&files);
    let pages = project(&compiled, &corpus).expect("listing");
    let model = serde_json::to_value(&pages[0]).unwrap();
    assert_eq!(
        model["items"][0]["description"],
        "Exact <authored> question."
    );
    assert!(
        model["items"][1].get("description").is_none(),
        "body must not become a generated description"
    );
}

fn alternating(size: usize) -> String {
    let primary = listing("content:index", "standalone-articles", DATES, size);
    format!(
        "{},alternate_order={{order={{{}}},path='oldest'}}}}",
        primary.strip_suffix('}').unwrap(),
        DATES.replace("descending", "ascending")
    )
}

#[test]
fn opposite_chronologies_sort_complete_corpus_before_fifteen_item_slices() {
    use workspace::{ListingDirection, ListingVariant};
    let compiled = configuration(&[alternating(15)], "");
    let mut files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
    for number in 1..=31 {
        files.push((
            format!("one/a-{number}.md"),
            article(
                &format!("content:a-{number:02}"),
                &format!("Article {number:02}"),
                &format!("published='2026-01-{number:02}'"),
                "",
            ),
        ));
    }
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&key("site:one"), &discovery)
        .unwrap();
    assert_eq!(pages.len(), 6);
    for variant in [ListingVariant::Primary, ListingVariant::Alternate] {
        let mut views: Vec<_> = pages.iter().filter(|p| p.variant == variant).collect();
        views.sort_by_key(|p| p.page_number);
        assert_eq!(
            views.iter().map(|p| p.items.len()).collect::<Vec<_>>(),
            [15, 15, 1]
        );
        let observed: Vec<_> = views
            .iter()
            .flat_map(|p| p.items.iter().map(|i| i.id.to_string()))
            .collect();
        let expected: Vec<_> = if variant == ListingVariant::Primary {
            (1..=31).rev().collect::<Vec<_>>()
        } else {
            (1..=31).collect::<Vec<_>>()
        }
        .into_iter()
        .map(|n| format!("content:a-{n:02}"))
        .collect();
        assert_eq!(observed, expected);
        for page in views {
            assert_eq!(page.content, key("content:index"));
            assert_eq!(
                page.direction,
                Some(if variant == ListingVariant::Primary {
                    ListingDirection::Descending
                } else {
                    ListingDirection::Ascending
                })
            );
            assert_eq!(page.sorts.len(), 2);
            assert_eq!(page.sorts.iter().filter(|link| link.current).count(), 1);
            assert_eq!(page.sorts[0].url, "https://one.test/mount/blog/");
            assert_eq!(page.sorts[1].url, "https://one.test/mount/blog/oldest/");
            assert!(page.pages.len() <= 7);
            assert_eq!(page.pages.iter().filter(|link| link.current).count(), 1);
            assert_eq!(page.previous.is_none(), page.page_number == 1);
            assert_eq!(page.next.is_none(), page.page_number == page.page_count);
        }
    }
    let site = resolved
        .project_site_pages(&key("site:one"))
        .unwrap()
        .with_listing_pages(&pages)
        .unwrap();
    let sitemap = String::from_utf8(site.sitemap(1 << 20).unwrap()).unwrap();
    assert!(sitemap.contains("https://one.test/mount/blog/oldest/"));
    let projected = resolved
        .project_page(
            &key("content:index"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(projected.data.listing.unwrap().is_primary());
}

#[test]
fn numbered_window_keeps_first_last_current_neighbors_and_explicit_gaps() {
    use workspace::ListingVariant;
    let compiled = configuration(&[alternating(15)], "");
    let mut files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
    for number in 0..151 {
        files.push((
            format!("one/a-{number}.md"),
            article(
                &format!("content:a-{number:03}"),
                &format!("Article {number:03}"),
                "published='2026-10-27'",
                "",
            ),
        ));
    }
    let pages = project(&compiled, &parse(&files)).unwrap();
    for variant in [ListingVariant::Primary, ListingVariant::Alternate] {
        for (current, expected) in [
            (1, vec![1, 2, 3, 11]),
            (6, vec![1, 4, 5, 6, 7, 8, 11]),
            (11, vec![1, 9, 10, 11]),
        ] {
            let page = pages
                .iter()
                .find(|p| p.variant == variant && p.page_number == current)
                .unwrap();
            assert_eq!(
                page.pages.iter().map(|p| p.number).collect::<Vec<_>>(),
                expected
            );
            let mut last = 0;
            for link in &page.pages {
                assert_eq!(link.gap_before, last > 0 && link.number > last + 1);
                assert_eq!(link.current, link.number == current);
                assert!(pages.iter().any(|target| target.variant == variant
                    && target.page_number == link.number
                    && target.url == link.url));
                last = link.number;
            }
        }
    }
    let primary = pages.iter().find(|p| p.is_primary()).unwrap();
    let alternate = pages
        .iter()
        .find(|p| p.variant == ListingVariant::Alternate && p.page_number == 1)
        .unwrap();
    assert_eq!(
        ids(primary),
        ids(alternate),
        "equal authored dates retain identical tie order"
    );
}

#[test]
fn alternate_routes_refuse_collisions_with_existing_outputs_and_primary_continuations() {
    for (alternate, collision) in [
        ("oldest", "/blog/oldest/"),
        ("oldest", "/blog/oldest/page/2/"),
        ("oldest", "/blog/oldest"),
        ("page/2", "/unrelated/"),
    ] {
        let configured = alternating(1).replace("path='oldest'", &format!("path='{alternate}'"));
        let compiled = configuration(&[configured], "");
        let files = vec![
            ("one/index.md".into(), index("content:index", "/blog/")),
            (
                "one/a.md".into(),
                article("content:a", "A", "", "")
                    .replace("path='/a/'", &format!("path='{collision}'")),
            ),
            ("one/b.md".into(), article("content:b", "B", "", "")),
        ];
        assert_eq!(
            project(&compiled, &parse(&files)).unwrap_err().code(),
            "listing_route_collision"
        );
    }
}

#[test]
fn alternate_discovery_rejects_missing_views_changed_ordinals_navigation_and_sort_links() {
    let compiled = configuration(&[alternating(1)], "");
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        ("one/a.md".into(), article("content:a", "A", "", "")),
        ("one/b.md".into(), article("content:b", "B", "", "")),
    ];
    let corpus = parse(&files);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&key("site:one"), &discovery)
        .unwrap();
    let site = resolved.project_site_pages(&key("site:one")).unwrap();
    assert!(site.with_listing_pages(&pages).is_ok());
    for corruption in [
        "missing",
        "ordinal",
        "size",
        "url",
        "gap",
        "sort",
        "direction",
    ] {
        let mut changed = pages.clone();
        match corruption {
            "missing" => {
                changed.pop();
            }
            "ordinal" => changed[0].page_number = 0,
            "size" => changed[0].page_size += 1,
            "url" => changed[0].pages[0].url = "https://other.invalid/".into(),
            "gap" => changed[0].pages[0].gap_before = true,
            "sort" => changed[0].sorts[0].url = changed[0].next.clone().unwrap(),
            "direction" => changed[0].direction = Some(workspace::ListingDirection::Ascending),
            _ => unreachable!(),
        }
        assert!(site.with_listing_pages(&changed).is_err(), "{corruption}");
    }
}

#[test]
fn alternate_policy_is_only_opposite_chronology_and_path_stays_confined() {
    use workspace::ListingDeclaration;
    let valid:ListingDeclaration = serde_json::from_value(json!({"content":"content:index","select":{"kind":"standalone-articles"},"order":{"by":"published","direction":"descending","undated":"last-title-then-id"},"alternate_order":{"order":{"by":"published","direction":"ascending","undated":"last-title-then-id"},"path":"oldest"},"pagination":{"page_size":15,"next_pages":"page/{number}/"}})).unwrap();
    assert!(valid.validate_alternate().is_ok());
    let mut identical = valid.clone();
    identical.alternate_order.as_mut().unwrap().order = identical.order.clone();
    assert!(identical.validate_alternate().is_err());
    let mut priority = valid.clone();
    priority.order = workspace::ListingOrder::Priority {
        first: vec![],
        remaining: workspace::ListingRemaining::TitleThenId,
    };
    assert!(priority.validate_alternate().is_err());
    let mut value = json!(valid);
    value["alternate_order"]["path"] = json!("../outside");
    assert!(serde_json::from_value::<ListingDeclaration>(value).is_err());
}

#[test]
fn alternate_alias_and_mirror_targets_share_the_same_reserved_output_space() {
    for role in ["alias", "mirror"] {
        for target in ["/blog/oldest/", "/blog/oldest/page/2/"] {
            let compiled = configuration(&[alternating(1)], "");
            let mut b = article("content:b", "B", "", "");
            let path = if role == "alias" {
                b = b.replace("path='/b/'", &format!("path='/b/',aliases=['{target}']"));
                "one/b.md"
            } else {
                b = b.replace("site='site:one'", "site='site:two'").replace(
                    "path='/b/'",
                    &format!("path='/b/',mirrors=[{{site='site:one',path='{target}'}}]"),
                );
                "two/b.md"
            };
            let files = vec![
                ("one/index.md".into(), index("content:index", "/blog/")),
                ("one/a.md".into(), article("content:a", "A", "", "")),
                (path.into(), b),
                ("one/c.md".into(), article("content:c", "C", "", "")),
            ];
            assert_eq!(
                project(&compiled, &parse(&files)).unwrap_err().code(),
                "listing_route_collision",
                "{role} {target}"
            );
        }
    }
}

#[test]
fn alternate_views_and_control_links_share_whole_projection_budgets() {
    let compiled = configuration(&[alternating(1)], "");
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        ("one/a.md".into(), article("content:a", "A", "", "")),
        ("one/b.md".into(), article("content:b", "B", "", "")),
    ];
    let corpus = parse(&files);
    let bounded = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            nodes: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        bounded.project_listing_pages(&key("site:one"), &bounded.discovery().unwrap()),
        Err(ListingError::Limit("page occurrences"))
    ));
    // Four items, eight numbered endpoints and eight sort endpoints across four views.
    for (edges, succeeds) in [(20, true), (19, false)] {
        let bounded = authored::resolve(
            &compiled,
            &corpus,
            Limits {
                edges,
                ..Default::default()
            },
        )
        .unwrap();
        let result = bounded.project_listing_pages(&key("site:one"), &bounded.discovery().unwrap());
        assert_eq!(result.is_ok(), succeeds, "edge budget {edges}: {result:?}");
    }
}

#[test]
fn latest_uses_the_complete_discoverable_population_independent_of_slice_and_display_order() {
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/old.md".into(),
            article("content:old", "Old", "published='2020-01-01'", ""),
        ),
        (
            "one/z.md".into(),
            article("content:z", "Alpha", "published='2026-10-30'", ""),
        ),
        (
            "one/a.md".into(),
            article("content:a", "Alpha", "published='2026-10-30'", ""),
        ),
        (
            "one/undated.md".into(),
            article("content:undated", "A first title", "", ""),
        ),
        (
            "hidden/newest.md".into(),
            article(
                "content:hidden-newest",
                "Hidden newest",
                "published='2027-01-01'",
                "",
            ),
        ),
    ];
    let corpus = parse(&files);
    for declaration in [
        alternating(1),
        listing(
            "content:index",
            "standalone-articles",
            "first=['content:old'],remaining='title-then-id'",
            1,
        ),
    ] {
        let compiled = configuration(&[declaration], "");
        let pages = project(&compiled, &corpus).expect("complete native listings");
        assert!(pages.iter().any(|page| ids(page) == ["content:old"]));
        for page in pages {
            let latest = page.latest.expect("dated discoverable item");
            assert_eq!(
                latest.id,
                key("content:a"),
                "equal dates use title then identity"
            );
            assert_eq!(latest.url, "https://one.test/mount/a/");
            assert_eq!(latest.published.unwrap().to_string(), "2026-10-30");
        }
    }
}

#[test]
fn latest_is_absent_without_comparable_publication_chronology() {
    let compiled = configuration(
        &[listing(
            "content:index",
            "standalone-articles",
            "first=[],remaining='title-then-id'",
            1,
        )],
        "",
    );
    for metadata in [
        vec![],
        vec![""],
        vec!["published='2026-10-30'", "published='2026-10-30T12:00:00Z'"],
    ] {
        let mut files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
        for (at, date) in metadata.iter().enumerate() {
            files.push((
                format!("one/article-{at}.md"),
                article(&format!("content:article-{at}"), "Article", date, ""),
            ));
        }
        let corpus = parse(&files);
        let pages = project(&compiled, &corpus).expect("priority order remains valid");
        assert!(pages.iter().all(|page| page.latest.is_none()));
        for page in pages {
            assert!(
                page.item_list().get("itemListOrder").is_none(),
                "editorial priority is not ascending or descending value order"
            );
            assert!(serde_json::to_value(page).unwrap().get("latest").is_none());
        }
    }
}
