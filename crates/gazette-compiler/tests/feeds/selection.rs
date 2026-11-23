//! Explicit publication filtering preserves native selection, bindings and page meaning.

use super::*;

fn filtered(channels: &str, lifecycle: &str, omit: bool) -> String {
    configuration(channels).replacen(
        "feeds={identity_base=",
        &format!("feeds={{selection={{kind='articles',residence='all',lifecycle={lifecycle},require_published_date=true}},omit_when_empty={omit},identity_base="),
        1,
    )
}

fn channel(format: &str) -> String {
    format!(
        "{{id='feed:main',channel='collection:home',path='/updates.{format}',format='{format}'}}"
    )
}

fn article(id: &str, route: &str) -> String {
    document(
        id,
        "site:home",
        id,
        route,
        "published",
        "",
        "{kind='page',family={kind='article'}}",
    )
}

#[test]
fn configured_articles_select_authored_dates_and_lifecycle_not_landing_or_file_facts() {
    let config = filtered(&channel("json"), "['published']", true);
    let mut sources = files();
    sources.extend([
        (
            "home/calendar.md".into(),
            article("content:calendar", "/calendar/")
                .replace("2024-02-29T01:02:03+05:30", "2024-02-29"),
        ),
        (
            "home/undated.md".into(),
            article("content:undated", "/undated/")
                .replace("published='2024-02-29T01:02:03+05:30'\n", ""),
        ),
        (
            "home/standalone.md".into(),
            article("content:standalone", "/standalone/"),
        ),
        (
            "home/docs.md".into(),
            article("content:documentation", "/documentation/")
                .replace("family={kind='article'}", "family={kind='documentation'}"),
        ),
    ]);
    with_pages(&config, &sources, |resolved, compiled, pages| {
        let feeds = compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .as_ref()
            .expect("feeds");
        assert!(
            feeds.channels[0].scope.is_none(),
            "consumer shape has no implicit channel scope declaration"
        );
        let bytes = resolved
            .project_feed(&key("site:home"), &feeds.channels[0], pages)
            .expect("bound candidates")
            .render(1 << 20)
            .expect("wire")
            .expect("three published dated articles");
        let value: Value = serde_json::from_slice(&bytes).expect("JSON Feed");
        let items = value["items"].as_array().expect("entries");
        assert_eq!(
            items
                .iter()
                .map(|item| item["_gazette"]["content"]["id"]
                    .as_str()
                    .expect("identity"))
                .collect::<Vec<_>>(),
            ["content:calendar", "content:deep", "content:standalone"]
        );
        assert_eq!(items[0]["_gazette"]["content"]["published"], "2024-02-29");
        assert!(
            items[0].get("date_published").is_none(),
            "calendar precision must not be invented as a timestamp"
        );
        for item in items {
            let id = item["_gazette"]["content"]["id"]
                .as_str()
                .expect("retained identity");
            let page = &pages[&key(id)];
            assert_eq!(item["content_html"], page.body_html);
            assert_eq!(item["content_text"], page.body_text);
            assert_eq!(
                item["_gazette"]["content"],
                serde_json::to_value(&page.data.content).expect("native content")
            );
        }
        assert!(
            pages.contains_key(&key("content:undated")),
            "filtering is not graph/body deletion"
        );
        assert!(
            pages.contains_key(&key("content:first")),
            "draft HTML remains retained"
        );
    });
}

#[test]
fn article_family_examples_in_documentation_do_not_enter_editorial_feeds() {
    let config = filtered(&channel("json"), "['published']", true);
    let mut sources = files();
    sources.extend([
        (
            "home/manual.md".into(),
            document(
                "collection:manual",
                "site:home",
                "Manual",
                "/manual/",
                "published",
                "",
                "{kind='collection',purpose='documentation'}",
            ),
        ),
        (
            "home/example.md".into(),
            article("content:example", "/manual/example/").replacen(
                "+++\n",
                "+++\ncollections=['collection:manual']\n",
                1,
            ),
        ),
    ]);
    with_pages(&config, &sources, |resolved, compiled, pages| {
        let feeds = compiled
            .workspace
            .publication_for(&key("site:home"))
            .unwrap()
            .machine
            .feeds
            .as_ref()
            .unwrap();
        let bytes = resolved
            .project_feed(&key("site:home"), &feeds.channels[0], pages)
            .unwrap()
            .render(1 << 20)
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            value["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["_gazette"]["content"]["id"] != "content:example")
        );
        assert!(pages[&key("content:example")].data.content.documentation);
        assert_eq!(
            pages[&key("content:example")].data.content.node.structure,
            gazette_core::publication::content::ContentKind::Page {
                family: gazette_core::publication::content::PageFamily::Article
            }
        );
    });
}

#[test]
fn explicit_collection_and_journey_scopes_survive_article_filtering() {
    let mut sources = files();
    sources[1].1 = sources[1]
        .1
        .replace("lifecycle='draft'", "lifecycle='published'");
    sources[2].1 = sources[2]
        .1
        .replace("lifecycle='archived'", "lifecycle='published'");
    sources.push((
        "home/direct.md".into(),
        article("content:direct", "/direct/").replace(
            "[content]\n",
            "collections=['collection:home']\n[content]\n",
        ),
    ));
    for (scope, owner, expected) in [
        (
            "{kind='collection',recursive=false}",
            "collection:home",
            vec!["content:direct"],
        ),
        (
            "{kind='collection',recursive=true}",
            "collection:home",
            vec![
                "content:last",
                "content:deep",
                "content:direct",
                "content:first",
            ],
        ),
        (
            "{kind='journey'}",
            "journey:ordered",
            vec!["content:first", "content:last"],
        ),
    ] {
        let config = filtered(&selection("json", scope, owner), "['published']", true);
        let value = json_feed(&config, &sources);
        assert_eq!(
            value["items"]
                .as_array()
                .expect("entries")
                .iter()
                .map(|item| item["_gazette"]["content"]["id"]
                    .as_str()
                    .expect("identity"))
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn empty_output_is_explicit_absence_in_all_formats_and_never_skips_channel_binding() {
    let sources = files()
        .into_iter()
        .map(|(path, source)| {
            (
                path,
                source.replace("lifecycle='published'", "lifecycle='draft'"),
            )
        })
        .collect::<Vec<_>>();
    for format in ["atom", "rss", "json"] {
        let config = filtered(&channel(format), "['published']", true);
        with_pages(&config, &sources, |resolved, compiled, pages| {
            let feeds = compiled
                .workspace
                .publication_for(&key("site:home"))
                .expect("publication")
                .machine
                .feeds
                .as_ref()
                .expect("feeds");
            let feed = resolved
                .project_feed(&key("site:home"), &feeds.channels[0], pages)
                .expect("all retained candidates bind");
            assert!(
                feed.render(0)
                    .expect("no bytes allocated for omission")
                    .is_none()
            );
        });
        let retained = config.replace("omit_when_empty=true", "omit_when_empty=false");
        let bytes = rendered(&retained, &sources, 1 << 20).expect("explicit empty output");
        if format == "json" {
            let value: Value = serde_json::from_slice(&bytes).expect("whole JSON");
            assert!(
                value["items"]
                    .as_array()
                    .expect("empty sequence")
                    .is_empty()
            );
        } else {
            let fields = xml_fields(&bytes);
            assert!(!fields.values.contains_key(if format == "atom" {
                "feed.entry.id"
            } else {
                "rss.channel.item.guid"
            }));
        }
        with_pages(
            &config.replace("channel='collection:home'", "channel='content:absent'"),
            &sources,
            |resolved, compiled, pages| {
                let feeds = compiled
                    .workspace
                    .publication_for(&key("site:home"))
                    .expect("publication")
                    .machine
                    .feeds
                    .as_ref()
                    .expect("feeds");
                assert!(
                    matches!(resolved.project_feed(&key("site:home"), &feeds.channels[0], pages), Err(FeedError::Binding { content, .. }) if content == key("content:absent"))
                );
            },
        );
    }
}

#[test]
fn lifecycle_filters_cannot_hide_an_unbound_original_candidate() {
    let config = filtered(&channel("json"), "['published']", true);
    with_pages(&config, &files(), |resolved, compiled, pages| {
        let partial = pages
            .keys()
            .filter(|id| **id != key("content:first"))
            .map(|id| {
                (
                    id.clone(),
                    resolved
                        .project_page(
                            id,
                            &gazette_core::raster::unavailable,
                            &gazette_adapter_markdown::Comrak,
                        )
                        .expect("exact page"),
                )
            })
            .collect();
        let feeds = compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .as_ref()
            .expect("feeds");
        assert!(
            matches!(resolved.project_feed(&key("site:home"), &feeds.channels[0], &partial), Err(FeedError::Binding { content, .. }) if content == key("content:first"))
        );
    });
}

#[test]
fn selector_shape_and_lifecycle_are_closed_and_channel_scope_is_never_inferred_without_it() {
    let config = filtered(&channel("json"), "['published']", true);
    for source in [
        config.replace("['published']", "[]"),
        config.replace("['published']", "['published','published']"),
    ] {
        let errors = workspace::compile(&source, Default::default())
            .expect_err("nonempty distinct explicit states");
        assert!(format!("{errors:?}").contains("workspace_feed_lifecycle"));
    }
    let errors = workspace::compile(&configuration(&channel("json")), Default::default())
        .expect_err("no unspecified selection");
    assert!(format!("{errors:?}").contains("workspace_feed_scope_required"));
    for source in [
        config.replace("kind='articles'", "kind='pages'"),
        config.replace("residence='all'", "residence='inferred'"),
        config.replace("['published']", "['released']"),
        config.replace(
            "require_published_date=true",
            "require_published_date=true,guess_date=true",
        ),
    ] {
        assert!(
            workspace::compile(&source, Default::default()).is_err(),
            "no undeclared policy: {source}"
        );
    }
}

#[test]
fn configured_filter_and_discovery_are_intersected_without_rewriting_authored_links() {
    let config = discovery::configured("json").replacen("feeds={identity_base=", "feeds={selection={kind='articles',residence='all',lifecycle=['published','archived'],require_published_date=true},omit_when_empty=true,identity_base=", 1);
    let mut sources = discovery::selected_files();
    sources[1].1 = sources[1]
        .1
        .replace("lifecycle='draft'", "lifecycle='published'");
    with_pages(&config, &sources, |resolved, compiled, pages| {
        let feeds = compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .as_ref()
            .expect("feeds");
        let discovery = resolved.discovery().expect("source selection");
        let private = resolved
            .project_feed(&key("site:home"), &feeds.channels[0], pages)
            .expect("private native graph")
            .render(1 << 20)
            .expect("wire")
            .expect("entries");
        let output = resolved
            .project_discovery_feed(&key("site:home"), &feeds.channels[0], pages, &discovery)
            .expect("shared discovery")
            .render(1 << 20)
            .expect("wire")
            .expect("last retained");
        let private: Value = serde_json::from_slice(&private).expect("private JSON");
        let output: Value = serde_json::from_slice(&output).expect("filtered JSON");
        assert_eq!(item_ids(&private).len(), 2);
        assert_eq!(item_ids(&output).len(), 1);
        assert_eq!(
            output["items"][0]["_gazette"]["content"]["id"],
            "content:last"
        );
        assert_eq!(
            output["items"][0]["content_html"],
            pages[&key("content:last")].body_html
        );
        assert_eq!(
            output["items"][0]["_gazette"]["links"][0]["content"], "content:first",
            "explicit prose links remain authored meaning"
        );
    });
}

#[test]
fn publication_selector_cannot_restore_an_explicitly_excluded_article() {
    use gazette_compiler::authored::Selection;
    use gazette_core::publication::content::Disposition;

    let config = filtered(&channel("json"), "['published']", true);
    let compiled = workspace::compile(&config, Default::default()).expect("explicit policy");
    let mut sources = files();
    sources[1].1 = sources[1]
        .1
        .replace("lifecycle='draft'", "lifecycle='published'");
    sources.push((
        "home/excluded.md".into(),
        article("content:excluded", "/excluded/"),
    ));
    let corpus = ParsedCorpus::parse(
        sources.iter().map(|(path, source)| Input {
            path: path.parse().expect("native entry"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained corpus");
    let private =
        authored::resolve(&compiled, &corpus, Default::default()).expect("complete source graph");
    let selection = Selection {
        content: private
            .content()
            .graph()
            .nodes()
            .map(|node| {
                (
                    node.id.clone(),
                    if node.id == key("content:excluded") {
                        Disposition::Exclude
                    } else {
                        Disposition::Include
                    },
                )
            })
            .collect(),
        assets: BTreeMap::new(),
    };
    let selected = private
        .into_selected(&selection)
        .expect("native closed candidate selection");
    let pages = selected
        .content()
        .graph()
        .nodes()
        .filter(|node| node.site == key("site:home"))
        .map(|node| {
            (
                node.id.clone(),
                selected
                    .project_page(
                        &node.id,
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak,
                    )
                    .expect("selected page"),
            )
        })
        .collect();
    let feeds = compiled
        .workspace
        .publication_for(&key("site:home"))
        .expect("publication")
        .machine
        .feeds
        .as_ref()
        .expect("feeds");
    let output = selected
        .project_feed(&key("site:home"), &feeds.channels[0], &pages)
        .expect("selected feed")
        .render(1 << 20)
        .expect("wire")
        .expect("selected articles");
    let value: Value = serde_json::from_slice(&output).expect("complete wire decoding");
    assert_eq!(item_ids(&value).len(), 2);
    assert!(
        value["items"]
            .as_array()
            .expect("entries")
            .iter()
            .all(|item| item["_gazette"]["content"]["id"] != "content:excluded")
    );
    assert!(
        selected
            .content()
            .graph()
            .node(&key("content:excluded"))
            .is_none()
    );
    assert_eq!(
        selected.corpus().files().len(),
        corpus.files().len(),
        "retained sources are not erased"
    );
}
