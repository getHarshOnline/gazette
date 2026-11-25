//! Discovery projections exercise the retained corpus and canonical route resolver.

#[path = "discovery/alternates.rs"]
mod alternates;
#[path = "discovery/markdown_context.rs"]
mod markdown_context;
#[path = "discovery/sitemap.rs"]
mod sitemap;

use std::convert::Infallible;

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    markdown::{LinkKind, ParsedMarkdown},
    workspace, xml,
};
use gazette_core::publication::{Key, content::Limits};

fn key(value: &str) -> Key {
    value.parse().expect("namespaced fixture identity")
}

fn configuration() -> String {
    "[[principals]]\nid='principal:publisher'\nkind='person'\n\
     [[sites]]\nid='site:home'\npublisher='principal:publisher'\ndeployable=false\n\
     [[sites]]\nid='site:foreign'\npublisher='principal:publisher'\ndeployable=false\n\
     [[sources]]\nid='source:private'\n\
     location={kind='repository',root='/private/discovery-fixture',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
     [[extensions]]\nid='schema:custom'\nschema={type='object',additionalProperties=false}\n\
     [[brands]]\nid='brand:one'\nname='Explicit publication'\n\
     [[brands.publications]]\nsite='site:home'\npublic_base_url='https://home.test/mount/'\n\
     authored=[{source='source:private',target='home'}]\n\
     [[brands.publications]]\nsite='site:foreign'\npublic_base_url='https://foreign.test/'\n\
     authored=[{source='source:private',target='foreign'}]\n"
        .into()
}

const ALTERNATES: &str = "{enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}";

fn curated(config: &str, description: &str, sections: &[(&str, &[&str])]) -> String {
    let sections = sections
        .iter()
        .map(|(title, ids)| {
            format!(
                "{{title={},contents=[{}]}}",
                serde_json::to_string(title).unwrap(),
                ids.iter()
                    .map(|id| format!("'{id}'"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    config.replace("site='site:home'\npublic_base_url", &format!(
        "site='site:home'\nmachine={{markdown_alternates={ALTERNATES},llms={{enabled=true,description_from='{description}',representation='markdown-alternate',include_human_link=true,sections=[{sections}]}}}}\npublic_base_url"
    ))
}

fn default_curated() -> String {
    curated(
        &configuration(),
        "content:first",
        &[("Articles", &["content:last", "content:first"])],
    )
}

fn document(
    id: &str,
    site: &str,
    title: &str,
    state: &str,
    route: &str,
    structure: &str,
) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='site:{site}'\ntitle={}\n\
         description='Declared description.'\nlifecycle='{state}'\nroute={{path='{route}'}}\nstructure={structure}\n+++\n\
         Private body must never become discovery copy.\n",
        serde_json::to_string(title).expect("TOML-compatible quoted fixture title")
    )
}

fn attributed(document: String, declarations: &str) -> String {
    document.replacen(
        "route={",
        &format!("attributions=[{declarations}]\nroute={{"),
        1,
    )
}

fn files() -> Vec<(String, String)> {
    let article = "{kind='page',family={kind='article'}}";
    vec![
        (
            "home/z.md".into(),
            document(
                "content:first",
                "home",
                "Zulu draft",
                "draft",
                "/z/",
                article,
            )
            .replace("path='/z/'", "path='/z/',aliases=['/retired/']"),
        ),
        (
            "home/a.md".into(),
            document(
                "content:last",
                "home",
                "Alpha archive",
                "archived",
                "/a/",
                article,
            ),
        ),
        (
            "foreign/page.md".into(),
            document(
                "content:foreign",
                "foreign",
                "Foreign secret",
                "published",
                "/foreign/",
                article,
            ),
        ),
    ]
}

fn with_pages(
    config: &str,
    files: &[(String, String)],
    inspect: impl FnOnce(&authored::SitePages<'_>, &authored::Discovery<'_>),
) {
    let compiled =
        workspace::compile(config, Default::default()).expect("real workspace compilation");
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().expect("retained corpus path"),
            bytes: bytes.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real Markdown/TOML parsing");
    let resolved =
        authored::resolve(&compiled, &corpus, Limits::default()).expect("bound content graph");
    let pages = resolved
        .project_site_pages(&key("site:home"))
        .expect("local routed pages");
    for page in pages.pages() {
        assert!(std::ptr::eq(
            page.route,
            resolved
                .content()
                .route(&page.node.id)
                .expect("same route owner")
        ));
    }
    let discovery = resolved.discovery().expect("exact discovery owner");
    inspect(&pages, &discovery);
}

fn xml_values(bytes: &[u8]) -> Vec<String> {
    let mut values = Vec::new();
    xml::inspect(bytes, Default::default(), |kind, value| {
        if kind == xml::TextKind::Text {
            values.push(value.to_owned());
        }
        Ok::<_, Infallible>(())
    })
    .expect("generated sitemap is strict XML");
    values
}

#[test]
fn exact_taxonomy_pages_join_existing_machine_writers_without_authored_identities() {
    let config = format!(
        "[[taxonomies]]\nid='taxonomy:topics'\nlabel='Topics [all]'\n\
         [[taxonomies.terms]]\nid='term:empty'\nlabel='Empty & explicit'\n{}",
        configuration().replace(
            "site='site:home'\npublic_base_url",
            "site='site:home'\nmachine={sitemap=true,sitemap_urls_per_file=2}\n\
             taxonomy_pages=[\
             {taxonomy='taxonomy:topics',route='/topics/',template='taxonomy.html'},\
             {taxonomy='taxonomy:topics',term='term:empty',route='/topics/empty/',template='term.html'}]\n\
             public_base_url",
        )
    );
    let compiled =
        workspace::compile(&config, Default::default()).expect("explicit taxonomy routes");
    let files = files();
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, body)| Input {
            path: path.parse().unwrap(),
            bytes: body.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native source corpus");
    let resolved =
        authored::resolve(&compiled, &corpus, Limits::default()).expect("shared resolved owner");
    let discovery = resolved
        .discovery()
        .expect("exact source discovery selection");
    let pages = resolved
        .project_site_pages(&key("site:home"))
        .unwrap()
        .discoverable(&discovery)
        .unwrap();
    let mut listings = resolved
        .project_taxonomy_pages(&key("site:home"), &discovery)
        .expect("complete listing projections");
    listings.reverse();
    let joined = pages
        .with_taxonomy_pages(&listings)
        .expect("same owner declarations");
    assert_eq!(
        joined.pages().len(),
        2,
        "derived listings are not authored objects"
    );
    let expected = [
        "https://home.test/mount/a/",
        "https://home.test/mount/topics/",
        "https://home.test/mount/topics/empty/",
        "https://home.test/mount/z/",
    ];
    let sitemap = joined.sitemap(1 << 20).expect("single writer union");
    assert_eq!(xml_values(&sitemap), expected);
    let tree = joined
        .sitemaps(Default::default())
        .expect("same union partitioned");
    let mut union = Vec::new();
    for path in ["sitemap-00001.xml", "sitemap-00002.xml"] {
        union.extend(xml_values(tree.file(&path.parse().unwrap()).unwrap()));
    }
    assert_eq!(union, expected);
    assert_eq!(
        joined.llms(&discovery, 1 << 20).unwrap_err().code(),
        "site_machine_configuration",
        "taxonomy discovery does not invent curated entrypoints"
    );
    assert_eq!(
        joined
            .with_taxonomy_pages(&listings)
            .unwrap()
            .sitemap(1 << 20)
            .unwrap(),
        sitemap
    );
    assert_eq!(
        joined
            .discoverable(&discovery)
            .unwrap()
            .sitemap(1 << 20)
            .unwrap(),
        pages.sitemap(1 << 20).unwrap(),
        "new discovery never retains stale derived joins"
    );
    assert!(
        pages.with_taxonomy_pages(&listings[..1]).is_err(),
        "no silent set shrink"
    );
    let saved = listings[0].model.canonical_url.clone();
    listings[0].model.canonical_url = "https://injected.invalid/".into();
    assert_eq!(
        pages.with_taxonomy_pages(&listings).unwrap_err().code(),
        "site_taxonomy_binding"
    );
    listings[0].model.canonical_url = saved;
    listings[0].model.title = "injected label";
    assert_eq!(
        pages.with_taxonomy_pages(&listings).unwrap_err().code(),
        "site_taxonomy_binding"
    );
}

#[test]
fn reading_guide_uses_navigation_order_and_authored_descriptions() {
    let config = curated(
        &configuration().replace(
            "id='site:home'\npublisher",
            "id='site:home'\ncomposition={navigation=[\
             {label='Last',destination='content:last'},\
             {label='First',destination='content:first'},\
             {label='Again',destination='content:last'}]}\npublisher",
        ),
        "content:first",
        &[],
    )
    .replace("sections=[]", "coverage='navigation'");
    with_pages(&config, &files(), |pages, discovery| {
        let guide = pages.llms_model(discovery, &[], &[], 1 << 20).unwrap();
        assert!(guide.pages.is_empty());
        assert_eq!(guide.sections.len(), 1);
        let entries = &guide.sections[0].pages;
        let ids = entries
            .iter()
            .map(|page| page.id.as_ref().unwrap().as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["content:last", "content:first"]);
        assert_eq!(
            entries[0].description.as_deref(),
            Some("Declared description.")
        );
        assert_eq!(
            entries[1].description, None,
            "The introduction already uses this description."
        );
    });
}

#[test]
fn machine_selection_is_explicit_site_local_and_strict() {
    for (setting, expected) in [
        ("", (false, false, false, false)),
        ("machine={}\n", (false, false, false, false)),
        ("machine={sitemap=true}\n", (true, false, false, false)),
        (
            "machine={llms={enabled=true,description_from='content:first',representation='markdown-alternate',include_human_link=true,sections=[]}}\n",
            (false, true, false, false),
        ),
        ("machine={search='lexical'}\n", (false, false, true, false)),
        ("machine={search='english'}\n", (false, false, true, false)),
        ("machine={json_ld=true}\n", (false, false, false, true)),
        (
            "machine={sitemap=true,llms={enabled=true,description_from='content:first',representation='markdown-alternate',include_human_link=true,sections=[]},search='lexical',json_ld=true}\n",
            (true, true, true, true),
        ),
    ] {
        let config = configuration().replace(
            "site='site:home'\npublic_base_url",
            &format!("site='site:home'\n{setting}public_base_url"),
        );
        let compiled =
            workspace::compile(&config, Default::default()).expect("selected machine outputs");
        let home = &compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("home binding")
            .machine;
        assert_eq!(
            (
                home.sitemap,
                home.llms.is_some(),
                home.search.is_some(),
                home.json_ld,
            ),
            expected
        );
        let foreign = &compiled
            .workspace
            .publication_for(&key("site:foreign"))
            .expect("foreign binding")
            .machine;
        assert!(
            !foreign.sitemap
                && foreign.llms.is_none()
                && foreign.search.is_none()
                && !foreign.json_ld,
            "no selection inheritance between sites"
        );
    }
    let english = configuration().replace(
        "site='site:home'\npublic_base_url",
        "site='site:home'\nmachine={search='english'}\npublic_base_url",
    );
    let compiled = workspace::compile(&english, Default::default()).expect("English profile");
    assert_eq!(
        compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("home binding")
            .machine
            .search,
        Some(workspace::SearchConfiguration::Profile(
            workspace::SearchProfile::English
        ))
    );
    for invalid in [
        "{sitemap=true,extra=true}",
        "{llms='true'}",
        "{search='true'}",
        "{search=1}",
        "{json_ld='true'}",
        "{json_ld=1}",
        "{json_ld=true,json_ld=false}",
        "{search='lexical',search='english'}",
        "{sitemap=true,sitemap=false}",
        "{path='elsewhere.xml'}",
        "[true,false]",
        "true",
    ] {
        let config = configuration().replace(
            "site='site:home'\npublic_base_url",
            &format!("site='site:home'\nmachine={invalid}\npublic_base_url"),
        );
        assert!(
            workspace::compile(&config, Default::default()).is_err(),
            "must reject {invalid}"
        );
    }
}

#[test]
fn projections_keep_all_local_states_in_canonical_order_without_aliases_or_foreign_content() {
    with_pages(&default_curated(), &files(), |pages, discovery| {
        assert_eq!(pages.pages().len(), 2);
        assert_eq!(
            pages
                .pages()
                .map(|page| page.node.id.to_string())
                .collect::<Vec<_>>(),
            ["content:last", "content:first"]
        );
        let sitemap = pages.sitemap(1 << 20).expect("sitemap");
        assert_eq!(
            xml_values(&sitemap),
            ["https://home.test/mount/a/", "https://home.test/mount/z/"]
        );
        let xml = std::str::from_utf8(&sitemap).expect("XML UTF-8");
        assert!(xml.contains("xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\""));
        assert_eq!(xml.matches("<url>").count(), 2);
        assert!(!xml.contains("lastmod"));
        let llms = String::from_utf8(pages.llms(discovery, 1 << 20).expect("llms")).expect("UTF-8");
        assert_eq!(
            llms,
            "# Explicit publication\n\n> Declared description.\n\n## Articles\n\n- [Alpha archive](https://home.test/mount/a/index.md) ([HTML](https://home.test/mount/a/))\n- [Zulu draft](https://home.test/mount/z/index.md) ([HTML](https://home.test/mount/z/))\n"
        );
        for excluded in [
            "Foreign secret",
            "foreign.test",
            "retired",
            "Private body",
            "/private/discovery-fixture",
            "author",
            "contact",
        ] {
            assert!(
                !xml.contains(excluded) && !llms.contains(excluded),
                "unexpected inference/disclosure: {excluded}"
            );
        }
    });
}

#[test]
fn curation_does_not_infer_author_sections_or_other_roles() {
    let config = format!(
        "{}\n{}",
        configuration(),
        "[[principals]]\nid='principal:alpha'\nkind='person'\nname='Shared [name]'\n\
         [[principals]]\nid='principal:zeta'\nkind='person'\nname='Shared [name]'\n\
         [[principals]]\nid='principal:rights'\nkind='organization'\nname='Rights canary'\n\
         [[principals]]\nid='principal:foreign'\nkind='person'\nname='Foreign canary'\n"
    );
    let article = "{kind='page',family={kind='article'}}";
    let files = vec![
        (
            "home/z.md".into(),
            attributed(
                document("content:zeta", "home", "Zulu", "published", "/z/", article),
                "{principal='principal:zeta',role='author'},\
                 {principal='principal:rights',role='rights-holder'}",
            ),
        ),
        (
            "home/a.md".into(),
            attributed(
                document(
                    "content:alpha",
                    "home",
                    "Alpha",
                    "published",
                    "/a/",
                    article,
                ),
                "{principal='principal:alpha',role='author'},\
                 {principal='principal:zeta',role='author'}",
            ),
        ),
        (
            "foreign/page.md".into(),
            attributed(
                document(
                    "content:foreign",
                    "foreign",
                    "Foreign",
                    "published",
                    "/foreign/",
                    article,
                ),
                "{principal='principal:foreign',role='author'}",
            ),
        ),
    ];
    let config = curated(
        &config,
        "content:alpha",
        &[("Chosen", &["content:zeta", "content:alpha"])],
    );
    with_pages(&config, &files, |pages, discovery| {
        let output = String::from_utf8(pages.llms(discovery, 1 << 20).expect("named authors"))
            .expect("UTF-8 llms");
        assert!(!output.contains("## Authors"));
        assert!(!output.contains("Shared"));
        assert!(output.contains("## Chosen"));
        for absent in [
            "principal:alpha",
            "principal:zeta",
            "Rights canary",
            "Foreign canary",
            "principal:publisher",
        ] {
            assert!(!output.contains(absent), "unscoped author output: {absent}");
        }
    });
}

#[test]
fn unnamed_author_is_not_invented_or_required_by_curated_entrypoints() {
    let config = format!(
        "{}\n[[principals]]\nid='principal:unnamed'\nkind='person'\n",
        configuration()
    );
    let files = vec![(
        "home/page.md".into(),
        attributed(
            document(
                "content:one",
                "home",
                "One",
                "published",
                "/one/",
                "{kind='page',family={kind='article'}}",
            ),
            "{principal='principal:unnamed',role='author'}",
        ),
    )];
    let config = curated(&config, "content:one", &[("Chosen", &["content:one"])]);
    with_pages(&config, &files, |pages, discovery| {
        let output = String::from_utf8(
            pages
                .llms(discovery, 1 << 20)
                .expect("explicit curated page"),
        )
        .unwrap();
        assert!(!output.contains("principal:unnamed"));
        assert!(!output.contains("## Authors"));
        assert!(output.contains("[One]"));
    });
}

#[test]
fn corpus_input_order_does_not_change_discovery_bytes() {
    let mut files = files();
    let mut first = None;
    with_pages(&default_curated(), &files, |pages, discovery| {
        first = Some((
            pages.sitemap(1 << 20).expect("sitemap"),
            pages.llms(discovery, 1 << 20).expect("llms"),
        ));
    });
    files.reverse();
    with_pages(&default_curated(), &files, |pages, discovery| {
        assert_eq!(
            (
                pages.sitemap(1 << 20).expect("sitemap"),
                pages.llms(discovery, 1 << 20).expect("llms")
            ),
            first.expect("first complete output")
        );
    });
}

#[test]
fn each_writer_accepts_exact_byte_budget_and_rejects_one_less_without_truncation() {
    with_pages(&default_curated(), &files(), |pages, discovery| {
        let sitemap = pages.sitemap(usize::MAX).expect("bounded sitemap");
        let llms = pages.llms(discovery, 1 << 20).expect("bounded llms");
        assert_eq!(
            pages.sitemap(sitemap.len()).expect("exact sitemap bytes"),
            sitemap
        );
        assert_eq!(
            pages.llms(discovery, llms.len()).expect("exact llms bytes"),
            llms
        );
        assert_eq!(
            pages
                .sitemap(sitemap.len() - 1)
                .expect_err("one byte short")
                .code(),
            "discovery_limit"
        );
        assert_eq!(
            pages
                .llms(discovery, llms.len() - 1)
                .expect_err("one byte short")
                .code(),
            "discovery_limit"
        );
        assert!(pages.sitemap(0).is_err());
        assert!(pages.llms(discovery, 0).is_err());
    });
}

#[test]
fn curation_preserves_declared_section_order_across_actual_families() {
    let kinds = [
        ("Articles", "{kind='page',family={kind='article'}}"),
        (
            "Documentation",
            "{kind='page',family={kind='documentation'}}",
        ),
        ("Projects", "{kind='page',family={kind='project'}}"),
        ("Indexes", "{kind='page',family={kind='index'}}"),
        ("Utility pages", "{kind='page',family={kind='utility'}}"),
        (
            "Extensions",
            "{kind='page',family={kind='extension',schema='schema:custom',data={}}}",
        ),
        ("Collections", "{kind='collection'}"),
        (
            "Journeys",
            "{kind='journey',collection='content:item-6',members=[]}",
        ),
    ];
    let files = kinds
        .iter()
        .enumerate()
        .map(|(index, (label, structure))| {
            (
                format!("home/unrelated-{index}.md"),
                document(
                    &format!("content:item-{index}"),
                    "home",
                    label,
                    "published",
                    &format!("/{index}/"),
                    structure,
                ),
            )
        })
        .collect::<Vec<_>>();
    let ids = (0..kinds.len())
        .map(|i| format!("content:item-{i}"))
        .collect::<Vec<_>>();
    let sections = kinds
        .iter()
        .enumerate()
        .map(|(i, (label, _))| (*label, vec![ids[i].as_str()]))
        .collect::<Vec<_>>();
    let borrowed = sections
        .iter()
        .map(|(title, ids)| (*title, ids.as_slice()))
        .collect::<Vec<_>>();
    let config = curated(&configuration(), "content:item-0", &borrowed);
    with_pages(&config, &files, |pages, discovery| {
        let output = String::from_utf8(pages.llms(discovery, 1 << 20).expect("all families"))
            .expect("UTF-8");
        let headings = output
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .collect::<Vec<_>>();
        let expected = kinds.map(|(label, _)| label);
        assert_eq!(headings, expected);
        for (index, (label, _)) in kinds.iter().enumerate() {
            assert!(output.contains(&format!(
                "## {label}\n\n- [{label}](https://home.test/mount/{index}/index.md)"
            )));
        }
    });
}

#[test]
fn hostile_titles_remain_plain_markdown_text_and_only_canonical_links_exist() {
    let title = "雪 [trap](https://evil.test/) # heading <script> & *strong* `code` \\ ![image](x)";
    let name = "品牌 # [brand](https://evil.test/) <b>";
    let config = configuration().replace(
        "name='Explicit publication'",
        &format!(
            "name={}",
            serde_json::to_string(name).expect("quoted brand")
        ),
    );
    let files = vec![(
        "home/page.md".into(),
        document(
            "content:one",
            "home",
            title,
            "draft",
            "/雪 & quote/",
            "{kind='page',family={kind='article'}}",
        ),
    )];
    let config = curated(&config, "content:one", &[("Chosen", &["content:one"])]);
    with_pages(&config, &files, |pages, discovery| {
        let canonical = pages
            .pages()
            .next()
            .expect("page")
            .route
            .canonical_url
            .clone();
        assert_eq!(
            xml_values(&pages.sitemap(1 << 20).expect("sitemap")),
            std::slice::from_ref(&canonical)
        );
        let output = String::from_utf8(pages.llms(discovery, 1 << 20).expect("escaped llms"))
            .expect("UTF-8");
        let parsed = ParsedMarkdown::parse(
            &output,
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("actual Markdown consumer");
        let inspection = parsed.inspect().expect("source facts");
        assert!(inspection.raw_html.is_empty());
        assert!(
            inspection
                .links
                .iter()
                .all(|link| link.kind == LinkKind::Link)
        );
        let links = inspection
            .links
            .iter()
            .map(|link| link.destination.clone())
            .collect::<Vec<_>>();
        let html = parsed.render_html().expect("safe generated Markdown");
        assert!(
            !html.contains("<code>"),
            "literal title must not become code markup"
        );
        assert_eq!(links, [format!("{canonical}index.md"), canonical]);
        assert_eq!(
            inspection.headings.len(),
            2,
            "only configured brand and actual group headings"
        );
        assert!(inspection.text.contains(name));
        assert!(inspection.text.contains(title));
    });
}

#[test]
fn sitemap_enforces_protocol_url_character_boundary_on_resolved_urls() {
    for length in [2047, 2048] {
        let prefix = "https://home.test/mount/";
        let route = format!("/{}", "a".repeat(length - prefix.len()));
        let files = vec![(
            "home/long.md".into(),
            document(
                "content:long",
                "home",
                "Long canonical URL",
                "published",
                &route,
                "{kind='page',family={kind='article'}}",
            ),
        )];
        let config = curated(
            &configuration(),
            "content:long",
            &[("Chosen", &["content:long"])],
        );
        with_pages(&config, &files, |pages, discovery| {
            assert_eq!(
                pages
                    .pages()
                    .next()
                    .expect("page")
                    .route
                    .canonical_url
                    .chars()
                    .count(),
                length
            );
            if length == 2047 {
                assert_eq!(
                    xml_values(&pages.sitemap(1 << 20).expect("maximum valid URL")).len(),
                    1
                );
            } else {
                assert_eq!(
                    pages
                        .sitemap(1 << 20)
                        .expect_err("2048 characters invalid")
                        .code(),
                    "discovery_limit"
                );
                assert!(
                    pages.llms(discovery, 1 << 20).is_ok(),
                    "sitemap protocol bound is not a generic route limit"
                );
            }
        });
    }
}

#[test]
fn url_valid_host_punctuation_cannot_change_markdown_link_destinations() {
    for base in [
        "https://a)b.test/mount/",
        "https://a(b.test/mount/",
        "https://a&copy;.test/mount/",
        "https://a&amp;copy;.test/mount/",
        "https://a&copy;).test/(mount)/",
    ] {
        let config = default_curated().replace("https://home.test/mount/", base);
        with_pages(&config, &files(), |pages, discovery| {
            let expected = pages
                .pages()
                .map(|page| page.route.canonical_url.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                xml_values(&pages.sitemap(1 << 20).expect("escaped XML")),
                expected
            );
            let llms = String::from_utf8(
                pages
                    .llms(discovery, 1 << 20)
                    .expect("escaped destinations"),
            )
            .expect("UTF-8");
            let actual =
                ParsedMarkdown::parse(&llms, Default::default(), &gazette_adapter_markdown::Comrak)
                    .expect("actual Markdown consumer")
                    .inspect()
                    .expect("destination facts")
                    .links
                    .into_iter()
                    .filter(|link| link.kind == LinkKind::Link)
                    .map(|link| link.destination)
                    .collect::<Vec<_>>();
            let pairs = expected
                .iter()
                .flat_map(|url| [format!("{url}index.md"), url.clone()])
                .collect::<Vec<_>>();
            assert_eq!(
                actual, pairs,
                "exact serving Markdown/HTML pairs for {base}"
            );
        });
    }
}
