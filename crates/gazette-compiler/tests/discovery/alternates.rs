//! Native alternate routing and explicitly curated reading paths share one owner.

use super::*;
use std::collections::BTreeMap;

fn with_authored(
    config: &str,
    files: &[(String, String)],
    inspect: impl FnOnce(authored::ResolvedAuthored<'_, '_>),
) {
    let compilation = workspace::compile(config, Default::default()).unwrap();
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    inspect(resolved);
}

#[test]
fn configured_profile_is_closed_named_and_has_no_boolean_compatibility() {
    let config = default_curated();
    let compiled = workspace::compile(&config, Default::default()).unwrap();
    let machine = &compiled
        .workspace
        .publication_for(&key("site:home"))
        .unwrap()
        .machine;
    let llms = machine.llms.as_ref().unwrap();
    assert_eq!(llms.description_from, key("content:first"));
    assert_eq!(
        llms.sections[0].contents,
        [key("content:last"), key("content:first")]
    );
    assert_eq!(
        llms.representation,
        workspace::LlmsRepresentation::MarkdownAlternate
    );
    assert_eq!(
        machine.markdown_alternates.as_ref().unwrap().media_type,
        workspace::MarkdownMediaType::Markdown
    );
    for invalid in [
        config.replace("llms={enabled=true", "llms={extra=true,enabled=true"),
        config.replace(
            "representation='markdown-alternate'",
            "representation='html'",
        ),
        config.replace(
            "directory_page='index.md'",
            "directory_page='../private.md'",
        ),
        config.replace("file_page='append-md'", "file_page='replace-extension'"),
        config.replace("media_type='text/markdown'", "media_type='text/plain'"),
        config.replace("discovery_rel='alternate'", "discovery_rel='canonical'"),
        config.replace("include_human_link=true", "include_human_link='true'"),
        config.replace(
            "{title=\"Articles\",contents=['content:last','content:first']}",
            "['Articles',['content:last','content:first']]",
        ),
    ] {
        assert!(
            workspace::compile(&invalid, Default::default()).is_err(),
            "must reject {invalid}"
        );
    }
    for value in ["true", "false", "[]", "[true]", "{}"] {
        let config = configuration().replace(
            "site='site:home'\npublic_base_url",
            &format!("site='site:home'\nmachine={{llms={value}}}\npublic_base_url"),
        );
        assert!(
            workspace::compile(&config, Default::default()).is_err(),
            "no old llms shape {value}"
        );
    }
}

#[test]
fn directory_file_unicode_and_local_routes_use_the_native_codec_and_exact_bounds() {
    let article = "{kind='page',family={kind='article'}}";
    let files = vec![
        (
            "home/root.md".into(),
            document("content:first", "home", "Root", "published", "/", article),
        ),
        (
            "home/file.md".into(),
            document(
                "content:file",
                "home",
                "File",
                "published",
                "/page.html",
                article,
            ),
        ),
        (
            "home/unicode.md".into(),
            document(
                "content:unicode",
                "home",
                "Unicode",
                "draft",
                "/雪 & quote/",
                article,
            ),
        ),
    ];
    for local in [false, true] {
        with_authored(&default_curated(), &files, |resolved| {
            let owner = if local {
                resolved
                    .with_preview_urls(BTreeMap::from([
                        (
                            key("site:home"),
                            "http://127.0.0.1:4321/mount/".parse().unwrap(),
                        ),
                        (
                            key("site:foreign"),
                            "http://127.0.0.1:4322/".parse().unwrap(),
                        ),
                    ]))
                    .unwrap()
            } else {
                resolved
            };
            let pages = owner.project_site_pages(&key("site:home")).unwrap();
            let planned = pages.markdown_alternates(&[], &[], 1 << 20).unwrap();
            assert_eq!(planned.len(), 3);
            assert_eq!(
                planned
                    .iter()
                    .map(|page| page.output.as_str())
                    .collect::<Vec<_>>(),
                ["index.md", "雪 & quote/index.md", "page.html.md"]
            );
            let base = if local {
                "http://127.0.0.1:4321/mount/"
            } else {
                "https://home.test/mount/"
            };
            assert_eq!(planned[0].html_url, base);
            assert_eq!(planned[0].url, format!("{base}index.md"));
            assert!(
                planned
                    .iter()
                    .any(|page| page.url == format!("{base}page.html.md"))
            );
            assert!(
                planned
                    .iter()
                    .any(|page| page.url == format!("{base}%E9%9B%AA%20%26%20quote/index.md"))
            );
            let exact = serde_json::to_vec(&planned).unwrap().len();
            assert_eq!(pages.markdown_alternates(&[], &[], exact).unwrap(), planned);
            assert_eq!(
                pages
                    .markdown_alternates(&[], &[], exact - 1)
                    .unwrap_err()
                    .code(),
                "discovery_limit"
            );
            assert!(pages.markdown_alternates(&[], &[], 0).is_err());
            let publication = pages.publication();
            let mut profile = publication.machine.markdown_alternates.clone().unwrap();
            let root: gazette_core::publication::routing::RoutePath = "/".parse().unwrap();
            let base = owner.content().base_url(&key("site:home")).unwrap();
            assert_eq!(profile.project(base, &root).unwrap().unwrap(), planned[0]);
            profile.enabled = false;
            assert!(profile.project(base, &root).unwrap().is_none());
        });
    }
}

#[test]
fn curation_preserves_order_and_repetitions_without_becoming_an_inventory() {
    let config = curated(
        &configuration(),
        "content:first",
        &[
            ("Later [path]", &["content:first"]),
            ("Earlier path", &["content:last", "content:first"]),
        ],
    );
    with_pages(&config, &files(), |pages, discovery| {
        let output = String::from_utf8(pages.llms(discovery, 1 << 20).unwrap()).unwrap();
        assert!(output.find("Later \\[path\\]").unwrap() < output.find("Earlier path").unwrap());
        let parsed = ParsedMarkdown::parse(
            &output,
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let facts = parsed.inspect().unwrap();
        assert_eq!(
            facts
                .links
                .iter()
                .map(|link| link.destination.as_str())
                .collect::<Vec<_>>(),
            [
                "https://home.test/mount/z/index.md",
                "https://home.test/mount/z/",
                "https://home.test/mount/a/index.md",
                "https://home.test/mount/a/",
                "https://home.test/mount/z/index.md",
                "https://home.test/mount/z/",
            ]
        );
        assert!(output.contains("> Declared description."));
        for absent in [
            "Foreign secret",
            "foreign.test",
            "retired",
            "Private body",
            "/private/",
            "source:private",
            "## Authors",
        ] {
            assert!(
                !output.contains(absent),
                "not curated public copy: {absent}"
            );
        }
        let exact = output.len();
        assert_eq!(pages.llms(discovery, exact).unwrap(), output.as_bytes());
        assert_eq!(
            pages.llms(discovery, exact - 1).unwrap_err().code(),
            "discovery_limit"
        );
    });
    let one = curated(
        &configuration(),
        "content:first",
        &[("One path", &["content:first"])],
    )
    .replace("include_human_link=true", "include_human_link=false");
    with_pages(&one, &files(), |pages, discovery| {
        let output = String::from_utf8(pages.llms(discovery, 4096).unwrap()).unwrap();
        let facts = ParsedMarkdown::parse(
            &output,
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap()
        .inspect()
        .unwrap();
        assert_eq!(facts.links.len(), 1);
        assert!(
            !output.contains("Alpha archive"),
            "uncurated canonical page is not automatically added"
        );
        assert_eq!(
            pages.markdown_alternates(&[], &[], 4096).unwrap().len(),
            2,
            "alternate coverage remains complete"
        );
    });
}

#[test]
fn llms_keeps_prose_punctuation_readable_and_markdown_meaning_literal() {
    let prose = "A reader's guide: version 2.1, examples (why?), costs + benefits - and results!";
    let config = curated(
        &configuration(),
        "content:first",
        &[(prose, &["content:first"])],
    );
    for description in [
        prose,
        "1. An ordered-looking sentence.",
        "1) Another ordered-looking sentence.",
        "- A bullet-looking sentence.",
        "+ Another bullet-looking sentence.",
        "===",
        ":::component",
        "&. A literal entity prefix.",
        "[a link](https://other.test/) and *emphasis* stay text.",
    ] {
        let files = files()
            .into_iter()
            .map(|(path, source)| {
                (
                    path,
                    source
                        .replace(
                            "title=\"Zulu draft\"",
                            &format!("title={}", serde_json::to_string(prose).unwrap()),
                        )
                        .replace(
                            "description='Declared description.'",
                            &format!(
                                "description={}",
                                serde_json::to_string(description).unwrap()
                            ),
                        ),
                )
            })
            .collect::<Vec<_>>();
        with_pages(&config, &files, |pages, discovery| {
            let direct = pages.llms(discovery, 65_536).unwrap();
            let model = pages
                .llms_model(discovery, &[], &[], 65_536)
                .unwrap()
                .markdown(65_536)
                .unwrap();
            for bytes in [direct, model] {
                let output = String::from_utf8(bytes).unwrap();
                assert!(
                    output.contains(prose),
                    "ordinary punctuation stays readable: {output}"
                );
                if description == prose {
                    assert!(output.contains(&format!("> {prose}")));
                    assert!(!output.contains('\\'));
                }
                if description.starts_with('&') {
                    assert!(output.contains("&amp;. A literal entity prefix."));
                }
                let parsed = ParsedMarkdown::parse(
                    &output,
                    Default::default(),
                    &gazette_adapter_markdown::Comrak,
                )
                .unwrap();
                let facts = parsed.inspect().unwrap();
                assert!(
                    facts.text.contains(description),
                    "description survives: {description}"
                );
                assert!(facts.text.contains(prose));
                assert_eq!(facts.headings.len(), 2);
                assert_eq!(facts.links.len(), 2);
                assert!(facts.raw_html.is_empty());
                let html = parsed.render_html().unwrap();
                assert!(!html.contains("<ol>"));
                assert!(!html.contains("<em>"));
                assert!(!html.contains("<hr"));
                assert_eq!(html.matches("<li>").count(), 1);
            }
        });
    }
}

#[test]
fn missing_foreign_unselected_and_missing_description_refs_fail_without_shrink() {
    for id in ["content:missing", "content:foreign"] {
        let config = curated(
            &configuration(),
            "content:first",
            &[("Chosen", &["content:first", id])],
        );
        with_pages(&config, &files(), |pages, discovery| {
            assert!(
                matches!(pages.llms(discovery, 4096), Err(authored::SitePagesError::LlmsReference(key)) if key.as_str() == id)
            );
        });
    }
    let mut retained = files();
    retained.retain(|(_, text)| !text.contains("id='content:last'"));
    with_pages(&default_curated(), &retained, |pages, discovery| {
        assert_eq!(
            pages.llms(discovery, 4096).unwrap_err().code(),
            "site_llms_reference"
        );
    });
    let mut retained = files();
    retained[0].1 = retained[0]
        .1
        .replace("description='Declared description.'\n", "");
    with_pages(&default_curated(), &retained, |pages, discovery| {
        assert_eq!(
            pages.llms(discovery, 4096).unwrap_err().code(),
            "site_llms_description_required"
        );
    });
    let disabled = default_curated().replace(
        "markdown_alternates={enabled=true",
        "markdown_alternates={enabled=false",
    );
    with_pages(&disabled, &files(), |pages, discovery| {
        assert_eq!(
            pages.llms(discovery, 4096).unwrap_err().code(),
            "site_machine_configuration"
        );
    });
}

#[test]
fn source_bound_discoverability_never_reduces_alternate_materialization() {
    let config = format!(
        "{}\n[[projects]]\nid='project:hidden'\nlifecycle='active'\nvisibility='private'\navailability='unavailable'\n[[placements]]\nsite='site:home'\nproject='project:hidden'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={{source='source:private',target='hidden'}}\n",
        default_curated()
    );
    let mut retained = files();
    retained.push((
        "hidden/private.md".into(),
        document(
            "content:hidden",
            "home",
            "Hidden direct page",
            "published",
            "/direct/",
            "{kind='page',family={kind='documentation'}}",
        ),
    ));
    with_authored(&config, &retained, |resolved| {
        let discovery = resolved.discovery().unwrap();
        let full = resolved.project_site_pages(&key("site:home")).unwrap();
        let discovered = full.discoverable(&discovery).unwrap();
        assert_eq!(full.markdown_alternates(&[], &[], 4096).unwrap().len(), 3);
        assert_eq!(
            discovered
                .markdown_alternates(&[], &[], 4096)
                .unwrap_err()
                .code(),
            "site_machine_configuration"
        );
        assert!(
            !String::from_utf8(full.llms(&discovery, 4096).unwrap())
                .unwrap()
                .contains("Hidden direct page")
        );
    });
    let configured_hidden = config.replace(
        "contents=['content:last','content:first']",
        "contents=['content:hidden']",
    );
    with_authored(&configured_hidden, &retained, |resolved| {
        let pages = resolved.project_site_pages(&key("site:home")).unwrap();
        assert_eq!(
            pages
                .llms(&resolved.discovery().unwrap(), 4096)
                .unwrap_err()
                .code(),
            "site_llms_reference"
        );
    });
}

#[test]
fn aliases_and_syndicated_copies_are_not_canonical_alternate_or_curated_entries() {
    let mut retained = files();
    retained[0].1 = retained[0].1.replace(
        "aliases=['/retired/']",
        "aliases=['/retired/'],mirrors=[{site='site:foreign',path='/mirror/'}]",
    );
    retained.push((
        "home/copy.md".into(),
        document(
            "content:copy",
            "home",
            "Explicit syndicated copy",
            "published",
            "/copy/",
            "{kind='page',family={kind='article'}}",
        )
        .replace("path='/copy/'", "path='/copy/',canonical='content:first'"),
    ));
    with_pages(&default_curated(), &retained, |pages, discovery| {
        let planned = pages.markdown_alternates(&[], &[], 4096).unwrap();
        assert_eq!(planned.len(), 2);
        assert!(planned.iter().all(|page| !page.html_url.contains("retired")
            && !page.html_url.contains("copy")
            && !page.html_url.contains("mirror")));
        assert!(
            !String::from_utf8(pages.llms(discovery, 4096).unwrap())
                .unwrap()
                .contains("copy")
        );
    });
    let config = curated(
        &configuration(),
        "content:first",
        &[("Chosen", &["content:copy"])],
    );
    with_pages(&config, &retained, |pages, discovery| {
        assert_eq!(
            pages.llms(discovery, 4096).unwrap_err().code(),
            "site_llms_reference"
        );
    });
}

#[test]
fn alternate_output_collisions_refuse_the_complete_plan() {
    for route in ["/z/index.md", "/z/index.md/child/"] {
        let mut retained = files();
        retained.push((
            "home/conflict.md".into(),
            document(
                "content:collision",
                "home",
                "Declared route collision",
                "published",
                route,
                "{kind='page',family={kind='utility'}}",
            ),
        ));
        with_pages(&default_curated(), &retained, |pages, _| {
            assert_eq!(
                pages
                    .markdown_alternates(&[], &[], 4096)
                    .unwrap_err()
                    .code(),
                "site_discovery_collision"
            );
        });
    }
    let mut retained = files();
    retained[1].1 = retained[1]
        .1
        .replace("path='/a/'", "path='/a/',aliases=['/z/index.md']");
    with_pages(&default_curated(), &retained, |pages, _| {
        assert_eq!(
            pages
                .markdown_alternates(&[], &[], 4096)
                .unwrap_err()
                .code(),
            "site_discovery_collision"
        );
    });
}

#[test]
fn curated_writer_rejects_independent_discovery_and_control_bearing_section_titles() {
    let config = default_curated();
    let retained = files();
    with_authored(&config, &retained, |owner| {
        let pages = owner.project_site_pages(&key("site:home")).unwrap();
        with_authored(&config, &retained, |other| {
            assert_eq!(
                pages
                    .llms(&other.discovery().unwrap(), 4096)
                    .unwrap_err()
                    .code(),
                "site_discovery_owner"
            );
        });
    });
    for title in ["", "\nInjected heading", "Tab\tpath"] {
        let config = curated(
            &configuration(),
            "content:first",
            &[(title, &["content:first"])],
        );
        with_pages(&config, &retained, |pages, discovery| {
            assert_eq!(
                pages.llms(discovery, 4096).unwrap_err().code(),
                "site_machine_configuration"
            );
        });
    }
}

#[test]
fn alternate_plan_is_deterministic_across_retained_input_order() {
    let mut retained = files();
    let mut first = None;
    with_pages(&default_curated(), &retained, |pages, _| {
        first = Some(pages.markdown_alternates(&[], &[], 4096).unwrap());
    });
    retained.reverse();
    with_pages(&default_curated(), &retained, |pages, _| {
        assert_eq!(
            pages.markdown_alternates(&[], &[], 4096).unwrap(),
            first.unwrap()
        );
    });
}

#[test]
fn generated_taxonomy_and_listing_continuations_receive_alternates_without_new_ids() {
    let configured = configuration().replace("site='site:home'\npublic_base_url", &format!(
        "site='site:home'\nmachine={{markdown_alternates={ALTERNATES}}}\ntaxonomy_pages=[{{taxonomy='taxonomy:topics',route='/topics/',template='taxonomy.html'}}]\nlistings=[{{content='content:index',select={{kind='standalone-articles'}},order={{by='published',direction='descending',undated='last-title-then-id'}},pagination={{page_size=2,next_pages='page/{{number}}/'}}}}]\npublic_base_url"
    ));
    let config = format!("[[taxonomies]]\nid='taxonomy:topics'\nlabel='Topics'\n{configured}");
    let mut retained = files();
    retained.push((
        "home/index.md".into(),
        document(
            "content:index",
            "home",
            "Articles",
            "published",
            "/blog/",
            "{kind='page',family={kind='index'}}",
        )
        .replace("Private body must never become discovery copy.\n", ""),
    ));
    retained.push((
        "home/third.md".into(),
        document(
            "content:third",
            "home",
            "Third",
            "published",
            "/third/",
            "{kind='page',family={kind='article'}}",
        ),
    ));
    with_authored(&config, &retained, |resolved| {
        let discovery = resolved.discovery().unwrap();
        let pages = resolved.project_site_pages(&key("site:home")).unwrap();
        let taxonomies = resolved
            .project_taxonomy_pages(&key("site:home"), &discovery)
            .unwrap();
        let listings = resolved
            .project_listing_pages(&key("site:home"), &discovery)
            .unwrap();
        assert_eq!(listings.len(), 2);
        let count = resolved.content().graph().nodes().count();
        let alternates = pages
            .markdown_alternates(&taxonomies, &listings, 8192)
            .unwrap();
        assert_eq!(alternates.len(), 6);
        assert!(
            alternates
                .iter()
                .any(|page| page.output.as_str() == "blog/page/2/index.md")
        );
        assert!(
            alternates
                .iter()
                .any(|page| page.output.as_str() == "topics/index.md")
        );
        assert_eq!(resolved.content().graph().nodes().count(), count);
        assert_eq!(
            pages
                .markdown_alternates(&[], &listings, 8192)
                .unwrap_err()
                .code(),
            "site_taxonomy_binding"
        );
        assert_eq!(
            pages
                .markdown_alternates(&taxonomies, &listings[..1], 8192)
                .unwrap_err()
                .code(),
            "site_listing_binding"
        );
        let mut changed = listings.clone();
        changed[1].url = "https://injected.test/".into();
        assert_eq!(
            pages
                .markdown_alternates(&taxonomies, &changed, 8192)
                .unwrap_err()
                .code(),
            "site_listing_binding"
        );
    });
}

#[test]
fn complete_llms_catalogue_adds_every_page_family_and_keeps_authored_purposes() {
    let kinds = [
        "{kind='page',family={kind='article'}}",
        "{kind='page',family={kind='documentation'}}",
        "{kind='page',family={kind='project'}}",
        "{kind='page',family={kind='index'}}",
        "{kind='page',family={kind='utility'}}",
        "{kind='collection'}",
        "{kind='journey',collection='content:item-5',members=[]}",
    ];
    let files = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            (
                format!("home/page-{i}.md"),
                document(
                    &format!("content:item-{i}"),
                    "home",
                    &format!("Page {i}"),
                    "published",
                    &format!("/{i}/"),
                    kind,
                ),
            )
        })
        .collect::<Vec<_>>();
    let config = curated(&configuration(), "content:item-0", &[]).replace(
        "llms={enabled=true",
        "llms={coverage='all-pages',enabled=true",
    );
    with_authored(&config, &files, |resolved| {
        let discovery = resolved.discovery().unwrap();
        let pages = resolved.project_site_pages(&key("site:home")).unwrap();
        let model = pages.llms_model(&discovery, &[], &[], 65_536).unwrap();
        assert_eq!(model.pages.len(), kinds.len());
        assert!(model.sections.is_empty());
        for (i, entry) in model.pages.iter().enumerate() {
            assert_eq!(
                entry.id.as_ref().unwrap(),
                &key(&format!("content:item-{i}"))
            );
            assert_eq!(entry.description.as_deref(), Some("Declared description."));
            assert_eq!(entry.url, format!("https://home.test/mount/{i}/index.md"));
        }
        let text = String::from_utf8(model.markdown(65_536).unwrap()).unwrap();
        assert_eq!(text.matches("- [Page").count(), kinds.len());
        assert_eq!(text.matches(": Declared description.").count(), kinds.len());
        assert!(!text.contains("Private body"));
        assert!(pages.llms_model(&discovery, &[], &[], 1).is_err());
        assert!(model.markdown(text.len() - 1).is_err());
        assert_eq!(model.markdown(text.len()).unwrap(), text.as_bytes());
    });
    let missing = files
        .iter()
        .map(|(path, source)| {
            (
                path.clone(),
                source.replace("description='Declared description.'\n", ""),
            )
        })
        .collect::<Vec<_>>();
    with_authored(&config, &missing, |resolved| {
        assert_eq!(
            resolved
                .project_site_pages(&key("site:home"))
                .unwrap()
                .llms_model(&resolved.discovery().unwrap(), &[], &[], 65_536)
                .unwrap_err()
                .code(),
            "site_llms_description_required"
        );
    });
}

#[test]
fn llms_catalogue_keeps_canonical_reading_pages_under_preview_base() {
    let mut config = curated(&configuration(), "content:index", &[]).replace(
        "llms={enabled=true",
        "llms={coverage='all-pages',enabled=true",
    );
    config = config.replace("public_base_url='https://home.test/mount/'", "listings=[{content='content:index',select={kind='standalone-articles'},order={first=[],remaining='title-then-id'},pagination={page_size=1,next_pages='page/{number}/'}}]\npublic_base_url='https://home.test/mount/'");
    let files = vec![
        (
            "home/index.md".into(),
            document(
                "content:index",
                "home",
                "Reading",
                "published",
                "/reading/",
                "{kind='page',family={kind='index'}}",
            )
            .replace("Private body must never become discovery copy.\n", ""),
        ),
        (
            "home/a.md".into(),
            document(
                "content:a",
                "home",
                "A",
                "published",
                "/a/",
                "{kind='page',family={kind='article'}}",
            ),
        ),
        (
            "home/b.md".into(),
            document(
                "content:b",
                "home",
                "B",
                "published",
                "/b/",
                "{kind='page',family={kind='article'}}",
            ),
        ),
    ];
    with_authored(&config, &files, |resolved| {
        let resolved = resolved
            .with_preview_urls(BTreeMap::from([
                (
                    key("site:home"),
                    "http://127.0.0.1:4310/mount/".parse().unwrap(),
                ),
                (
                    key("site:foreign"),
                    "http://127.0.0.1:4311/".parse().unwrap(),
                ),
            ]))
            .unwrap();
        let discovery = resolved.discovery().unwrap();
        let listings = resolved
            .project_listing_pages(&key("site:home"), &discovery)
            .unwrap();
        let pages = resolved.project_site_pages(&key("site:home")).unwrap();
        let model = pages
            .llms_model(&discovery, &[], &listings, 65_536)
            .unwrap();
        assert_eq!(model.pages.len(), 3);
        assert!(model.pages.iter().all(|page| page.page_number.is_none()));
        assert!(model.pages.iter().all(|page| !page.url.contains("/page/")));
        assert!(pages.llms_model(&discovery, &[], &[], 65_536).is_err());
        assert!(
            model
                .pages
                .iter()
                .all(|page| !page.url.contains("home.test"))
        );
    });
}

#[test]
fn curated_theme_model_keeps_missing_optional_entry_descriptions_absent() {
    let files = vec![
        (
            "home/first.md".into(),
            document(
                "content:first",
                "home",
                "First",
                "published",
                "/first/",
                "{kind='page',family={kind='article'}}",
            ),
        ),
        (
            "home/last.md".into(),
            document(
                "content:last",
                "home",
                "Last",
                "published",
                "/last/",
                "{kind='page',family={kind='article'}}",
            )
            .replace("description='Declared description.'\n", ""),
        ),
    ];
    with_authored(&default_curated(), &files, |resolved| {
        let pages = resolved.project_site_pages(&key("site:home")).unwrap();
        let model = pages
            .llms_model(&resolved.discovery().unwrap(), &[], &[], 65_536)
            .unwrap();
        assert!(model.sections[0].pages[0].description.is_none());
        assert_eq!(
            model.sections[0].pages[1].description.as_deref(),
            Some("Declared description.")
        );
    });
}

#[test]
fn llms_omits_alternate_listings_while_markdown_keeps_every_output_view() {
    let mut config = curated(&configuration(), "content:index", &[]).replace(
        "llms={enabled=true",
        "llms={coverage='all-pages',enabled=true",
    );
    config = config.replace("public_base_url='https://home.test/mount/'", "listings=[{content='content:index',select={kind='standalone-articles'},order={by='published',direction='descending',undated='last-title-then-id'},alternate_order={order={by='published',direction='ascending',undated='last-title-then-id'},path='oldest'},pagination={page_size=1,next_pages='page/{number}/'}}]\npublic_base_url='https://home.test/mount/'");
    let retained = vec![
        (
            "home/index.md".into(),
            document(
                "content:index",
                "home",
                "Reading",
                "published",
                "/reading/",
                "{kind='page',family={kind='index'}}",
            )
            .replace("Private body must never become discovery copy.\n", ""),
        ),
        (
            "home/a.md".into(),
            document(
                "content:a",
                "home",
                "A",
                "published",
                "/a/",
                "{kind='page',family={kind='article'}}",
            ),
        ),
        (
            "home/b.md".into(),
            document(
                "content:b",
                "home",
                "B",
                "published",
                "/b/",
                "{kind='page',family={kind='article'}}",
            ),
        ),
    ];
    with_authored(&config, &retained, |resolved| {
        let resolved = resolved
            .with_preview_urls(BTreeMap::from([
                (
                    key("site:home"),
                    "http://127.0.0.1:4320/mount/".parse().unwrap(),
                ),
                (
                    key("site:foreign"),
                    "http://127.0.0.1:4321/".parse().unwrap(),
                ),
            ]))
            .unwrap();
        let discovery = resolved.discovery().unwrap();
        let listings = resolved
            .project_listing_pages(&key("site:home"), &discovery)
            .unwrap();
        let pages = resolved.project_site_pages(&key("site:home")).unwrap();
        let model = pages
            .llms_model(&discovery, &[], &listings, 65_536)
            .unwrap();
        assert_eq!(model.pages.len(), 3);
        assert!(model.pages.iter().all(|page| page.page_number.is_none()));
        assert!(
            model
                .pages
                .iter()
                .all(|page| page.listing_variant.is_none())
        );
        assert!(
            model
                .pages
                .iter()
                .all(|page| !page.url.contains("/oldest/"))
        );
        let alternates = pages.markdown_alternates(&[], &listings, 65_536).unwrap();
        assert_eq!(alternates.len(), 6);
        assert!(
            alternates
                .iter()
                .any(|page| page.html_url == "http://127.0.0.1:4320/mount/reading/oldest/")
        );
        assert_eq!(resolved.content().graph().nodes().count(), 3);
    });
}
