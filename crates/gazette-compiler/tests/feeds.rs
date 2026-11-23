//! Standards projections consume actual retained workspace, content and page models.

use std::collections::{BTreeMap, BTreeSet};

use gazette_compiler::{
    authored::{self, FeedError, PageProjection, ResolvedAuthored},
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::Key;
use serde_json::{Value, json};
use xml::reader::{EventReader, XmlEvent};

#[path = "feeds/discovery.rs"]
mod discovery;

#[path = "feeds/selection.rs"]
mod configured_selection;

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn selection(format: &str, scope: &str, channel: &str) -> String {
    format!(
        "{{id='feed:main',channel='{channel}',path='/updates.{format}',format='{format}',scope={scope}}}"
    )
}

fn configuration(channels: &str) -> String {
    format!(
        "[[principals]]\nid='principal:publisher'\nkind='organization'\nname='Publisher only'\n\
         [[principals]]\nid='principal:writer'\nkind='person'\nname='Writer & साथी'\n\
         [[principals]]\nid='principal:unnamed'\nkind='person'\n\
         [[sites]]\nid='site:home'\npublisher='principal:publisher'\ndeployable=false\n\
         [[sites]]\nid='site:foreign'\npublisher='principal:publisher'\ndeployable=false\n\
         [[projects]]\nid='project:one'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
         [[sources]]\nid='source:private'\nlocation={{kind='repository',root='/private/feed-fixture',commit='1111111111111111111111111111111111111111',subdirectory='secret'}}\n\
         [[brands]]\nid='brand:one'\nname='Not a feed title'\n\
         [[brands.publications]]\nsite='site:home'\npublic_base_url='https://home.test/mount/'\nauthored=[{{source='source:private',target='home'}}]\n\
         machine={{feeds={{identity_base='https://ids.test/publications/',channels=[{channels}]}}}}\n\
         [[brands.publications]]\nsite='site:foreign'\npublic_base_url='https://foreign.test/'\nauthored=[{{source='source:private',target='foreign'}}]\n"
    )
}

fn document(
    id: &str,
    site: &str,
    title: &str,
    route: &str,
    state: &str,
    edges: &str,
    structure: &str,
) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='{site}'\ntitle={}\n\
         description='Explicit description & <literal>'\nlanguage='en-US'\n\
         published='2024-02-29T01:02:03+05:30'\nmodified='2024-03-01T04:05:06Z'\n\
         lifecycle='{state}'\nattributions=[{{principal='principal:writer',role='author'}}]\n\
         route={{path='{route}'}}\nstructure={structure}\n+++\nBefore **bold** & café.\n\n<!-- more -->\n\nAfter *tail*.\n",
        serde_json::to_string(title).expect("quoted title")
    )
}

fn files() -> Vec<(String, String)> {
    let page = "{kind='page',family={kind='article'}}";
    [
        ("home/channel.md", document("collection:home", "site:home", "News <&> Café", "/", "published", "", "{kind='collection'}")),
        ("home/first.md", document("content:first", "site:home", "First <&> title", "/z/", "draft", "projects=[{project='project:one',kind='about'}]\n", page)),
        ("home/last.md", document("content:last", "site:home", "Archived title", "/a/", "archived", "", page)),
        ("home/nested.md", document("collection:nested", "site:home", "Nested", "/collection/", "published", "", "{kind='collection',parent='collection:home'}")),
        ("home/deep.md", document("content:deep", "site:home", "Deep", "/deep/", "published", "collections=['collection:nested']\n", page)),
        ("home/journey.md", document("journey:ordered", "site:home", "Ordered", "/trail/", "published", "", "{kind='journey',collection='collection:home',members=[{content='content:first',primary=true},{content='content:last',primary=true}]}")),
        ("foreign/page.md", document("content:foreign", "site:foreign", "Foreign page", "/elsewhere/", "published", "", page)),
    ].into_iter().map(|(path, source)| (path.into(), source)).collect()
}

fn with_pages<T>(
    config: &str,
    files: &[(String, String)],
    inspect: impl FnOnce(
        &ResolvedAuthored<'_, '_>,
        &workspace::Compilation,
        &BTreeMap<Key, PageProjection<'_>>,
    ) -> T,
) -> T {
    let compiled = workspace::compile(config, Default::default()).expect("native workspace");
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().expect("corpus path"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained Markdown/TOML");
    let resolved =
        authored::resolve(&compiled, &corpus, Default::default()).expect("whole semantic graph");
    let pages = resolved
        .content()
        .graph()
        .nodes()
        .filter(|node| node.site == key("site:home"))
        .map(|node| {
            (
                node.id.clone(),
                resolved
                    .project_page(
                        &node.id,
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak,
                    )
                    .expect("shared local page"),
            )
        })
        .collect();
    inspect(&resolved, &compiled, &pages)
}

fn rendered(
    config: &str,
    files: &[(String, String)],
    maximum: usize,
) -> Result<Vec<u8>, FeedError> {
    with_pages(config, files, |resolved, compiled, pages| {
        let feeds = compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .as_ref()
            .expect("configured feeds");
        resolved
            .project_feed(&key("site:home"), &feeds.channels[0], pages)?
            .render(maximum)
            .map(|output| {
                output.expect("this explicit fixture selects empty output rather than omission")
            })
    })
}

fn json_feed(config: &str, files: &[(String, String)]) -> Value {
    serde_json::from_slice(&rendered(config, files, 1024 * 1024).expect("complete JSON feed"))
        .expect("native JSON decoding")
}

fn item_ids(value: &Value) -> Vec<&str> {
    value["items"]
        .as_array()
        .expect("complete item sequence")
        .iter()
        .map(|item| item["id"].as_str().expect("item identity"))
        .collect()
}

#[derive(Default)]
struct XmlFields {
    values: BTreeMap<String, Vec<String>>,
    namespaces: BTreeMap<String, BTreeSet<String>>,
}

fn xml_fields(bytes: &[u8]) -> XmlFields {
    let mut fields = XmlFields::default();
    let mut stack = Vec::<(String, String)>::new();
    for event in EventReader::new(bytes) {
        match event.expect("native whole XML parse") {
            XmlEvent::StartElement { name, .. } => {
                stack.push((name.local_name, String::new()));
                let path = stack
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                fields
                    .namespaces
                    .entry(path)
                    .or_default()
                    .insert(name.namespace.unwrap_or_default());
            }
            XmlEvent::Characters(text) | XmlEvent::CData(text) => {
                stack.last_mut().expect("element text").1.push_str(&text)
            }
            XmlEvent::EndElement { .. } => {
                let path = stack
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let (_, text) = stack.pop().expect("balanced end");
                fields.values.entry(path).or_default().push(text);
            }
            _ => (),
        }
    }
    assert!(stack.is_empty());
    fields
}

#[test]
fn site_collection_and_journey_scopes_preserve_complete_membership_and_distinct_ordering() {
    let files = files();
    for (scope, channel, expected) in [
        (
            "{kind='site'}",
            "collection:home",
            vec![
                "content:last",
                "collection:nested",
                "content:deep",
                "journey:ordered",
                "content:first",
            ],
        ),
        (
            "{kind='collection',recursive=false}",
            "collection:home",
            vec!["collection:nested", "journey:ordered"],
        ),
        (
            "{kind='collection',recursive=true}",
            "collection:home",
            vec![
                "content:last",
                "collection:nested",
                "content:deep",
                "journey:ordered",
                "content:first",
            ],
        ),
        (
            "{kind='journey'}",
            "journey:ordered",
            vec!["content:first", "content:last"],
        ),
    ] {
        let config = configuration(&selection("json", scope, channel));
        let feed = json_feed(&config, &files);
        let ids = expected
            .iter()
            .map(|id| {
                format!(
                    "https://ids.test/publications/#content={}",
                    id.replace(':', "%3A")
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(item_ids(&feed), ids, "{scope}");
        assert!(!item_ids(&feed).iter().any(|id| id.contains("foreign")));
        let mut reversed = files.clone();
        reversed.reverse();
        assert_eq!(
            rendered(&config, &files, 1024 * 1024).expect("original"),
            rendered(&config, &reversed, 1024 * 1024).expect("reordered corpus")
        );
    }
}

#[test]
fn explicit_feed_configuration_is_sorted_strict_and_does_not_infer_default_channels() {
    let first = selection("json", "{kind='site'}", "collection:home");
    let second = first
        .replace("feed:main", "feed:aaa")
        .replace("updates.json", "other.json");
    let compiled = workspace::compile(
        &configuration(&format!("{first},{second}")),
        Default::default(),
    )
    .expect("two channels");
    let feeds = compiled
        .workspace
        .publication_for(&key("site:home"))
        .expect("site")
        .machine
        .feeds
        .as_ref()
        .expect("feeds");
    assert_eq!(
        feeds
            .channels
            .iter()
            .map(|feed| feed.id.as_str())
            .collect::<Vec<_>>(),
        ["feed:aaa", "feed:main"]
    );
    for (channels, expected) in [
        (String::new(), "workspace_feeds_empty"),
        (format!("{first},{first}"), "workspace_feed_duplicate"),
        (
            format!("{first},{}", first.replace("feed:main", "feed:other")),
            "workspace_feed_duplicate",
        ),
        (
            first.replace("/updates.json", "/directory/"),
            "workspace_feed_path",
        ),
    ] {
        let workspace::Error::Invalid(errors) =
            workspace::compile(&configuration(&channels), Default::default())
                .expect_err("invalid selection")
        else {
            panic!("semantic feed configuration failure")
        };
        assert!(
            errors.iter().any(|error| error.code == expected),
            "{expected}: {errors:?}"
        );
    }
    for invalid in [
        first.replace("format='json'", "format='yaml'"),
        first.replace("kind='site'", "kind='site',recursive=true"),
        first.replace("kind='site'", "kind='journey',recursive=false"),
        first.replace("kind='site'", "kind='collection'"),
        first.replace("format='json'", "format='json',title='Duplicate owner'"),
        "['feed:main','collection:home','/updates.json','json',{kind='site'}]".into(),
    ] {
        assert!(matches!(
            workspace::compile(&configuration(&invalid), Default::default()),
            Err(workspace::Error::Toml(_))
        ));
    }
    let positional = configuration(&first).replace(
        &format!("feeds={{identity_base='https://ids.test/publications/',channels=[{first}]}}"),
        &format!("feeds=['https://ids.test/publications/', [{first}]]"),
    );
    assert!(
        positional.contains("feeds=["),
        "fixture reaches the exact nested boundary"
    );
    assert!(matches!(
        workspace::compile(&positional, Default::default()),
        Err(workspace::Error::Toml(_))
    ));
    let plain = "[[principals]]\nid='principal:publisher'\nkind='person'\n[[sites]]\nid='site:home'\npublisher='principal:publisher'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Brand'\n[[brands.publications]]\nsite='site:home'\n";
    let compiled = workspace::compile(plain, Default::default()).expect("feeds omitted");
    assert!(
        compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .is_none()
    );
}

#[test]
fn json_feed_uses_exact_shared_metadata_and_full_body_without_author_or_date_inheritance() {
    let config = configuration(&selection("json", "{kind='journey'}", "journey:ordered"));
    let mut files = files();
    files[1].1 = files[1].1.replace("published='2024-02-29T01:02:03+05:30'", "published='2024-02-29'")
        .replace("modified='2024-03-01T04:05:06Z'\n", "")
        .replace("attributions=[{principal='principal:writer',role='author'}]", "attributions=[{principal='principal:writer',role='author'},{principal='principal:unnamed',role='author'}]");
    with_pages(&config, &files, |resolved, compiled, pages| {
        let settings = compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("site")
            .machine
            .feeds
            .as_ref()
            .expect("feeds");
        let bytes = resolved
            .project_feed(&key("site:home"), &settings.channels[0], pages)
            .expect("complete selection")
            .render(1024 * 1024)
            .expect("JSON feed")
            .expect("configured retained output");
        let feed: Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(feed["version"], "https://jsonfeed.org/version/1.1");
        assert_eq!(feed["title"], "Ordered");
        assert_eq!(feed["home_page_url"], "https://home.test/mount/trail/");
        assert_eq!(feed["feed_url"], "https://home.test/mount/updates.json");
        assert_eq!(
            feed["_gazette_feed"],
            json!({"id":"https://ids.test/publications/#feed=feed%3Amain"})
        );
        assert!(
            feed.get("authors").is_none(),
            "channel author must not become item default"
        );
        assert!(
            feed.get("language").is_none(),
            "channel language must not become item default"
        );
        assert_eq!(
            feed["_gazette"],
            serde_json::to_value(&pages[&key("journey:ordered")].data).expect("channel PageData")
        );
        let items = feed["items"].as_array().expect("ordered items");
        assert_eq!(items.len(), 2);
        for (item, id) in items.iter().zip(["content:first", "content:last"]) {
            let page = &pages[&key(id)];
            assert_eq!(
                item["_gazette"],
                serde_json::to_value(&page.data).expect("exact PageData")
            );
            assert_eq!(item["content_html"], page.body_html);
            assert_eq!(item["content_text"], page.body_text);
            assert_eq!(
                item["summary"],
                page.summary.as_ref().expect("explicit summary").text
            );
            assert_eq!(item["language"], "en-US");
            assert!(
                item["content_html"]
                    .as_str()
                    .expect("full HTML")
                    .contains("After <em>tail</em>")
            );
        }
        assert_eq!(items[0]["title"], "First <&> title");
        assert_eq!(items[0]["authors"], json!([{"name":"Writer & साथी"}]));
        assert_eq!(
            items[0]["_gazette"]["attributions"]
                .as_array()
                .expect("all roles")
                .len(),
            2
        );
        assert!(
            items[0].get("date_published").is_none(),
            "date-only is not midnight"
        );
        assert!(
            items[0].get("date_modified").is_none(),
            "missing date is not channel date"
        );
        assert_eq!(items[0]["_gazette"]["content"]["published"], "2024-02-29");
        assert_eq!(items[0]["_gazette"]["content"]["lifecycle"], "draft");
        assert_eq!(items[1]["_gazette"]["content"]["lifecycle"], "archived");
        assert_eq!(items[1]["date_published"], "2024-02-29T01:02:03+05:30");
        assert_eq!(items[1]["date_modified"], "2024-03-01T04:05:06Z");
        let text = std::str::from_utf8(&bytes).expect("UTF-8");
        for private in [
            "/private/feed-fixture",
            "source:private",
            "home/first.md",
            "Foreign page",
        ] {
            assert!(!text.contains(private), "unscoped private value {private}");
        }
    });
}

#[test]
fn atom_and_rss_are_native_decodable_xml_with_exact_metadata_and_escaped_html() {
    let files = files();
    for format in ["atom", "rss"] {
        let config = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
        with_pages(&config, &files, |resolved, compiled, pages| {
            let settings = compiled
                .workspace
                .publication_for(&key("site:home"))
                .expect("site")
                .machine
                .feeds
                .as_ref()
                .expect("feeds");
            let output = resolved
                .project_feed(&key("site:home"), &settings.channels[0], pages)
                .expect("selection")
                .render(1024 * 1024)
                .expect("complete XML feed")
                .expect("configured retained output");
            let parsed = xml_fields(&output);
            let (channel, entry, body, id) = if format == "atom" {
                ("feed", "feed.entry", "content", "id")
            } else {
                ("rss.channel", "rss.channel.item", "description", "guid")
            };
            assert_eq!(parsed.values[&format!("{channel}.title")], ["Ordered"]);
            assert_eq!(
                parsed.namespaces[&format!("{channel}.metadata")],
                BTreeSet::from(["urn:gazette:syndication".into()])
            );
            assert_eq!(
                serde_json::from_str::<Value>(&parsed.values[&format!("{channel}.metadata")][0])
                    .expect("channel extension"),
                serde_json::to_value(&pages[&key("journey:ordered")].data).expect("channel data")
            );
            assert_eq!(
                parsed.values[&format!("{entry}.title")],
                ["First <&> title", "Archived title"]
            );
            assert_eq!(
                parsed.values[&format!("{entry}.{id}")],
                [
                    "https://ids.test/publications/#content=content%3Afirst",
                    "https://ids.test/publications/#content=content%3Alast"
                ]
            );
            for (index, key_value) in ["content:first", "content:last"].iter().enumerate() {
                let page = &pages[&key(key_value)];
                assert_eq!(
                    parsed.values[&format!("{entry}.{body}")][index],
                    page.body_html
                );
                assert_eq!(
                    serde_json::from_str::<Value>(
                        &parsed.values[&format!("{entry}.metadata")][index]
                    )
                    .expect("entry extension"),
                    serde_json::to_value(&page.data).expect("exact entry PageData")
                );
            }
            if format == "atom" {
                assert_eq!(
                    parsed.namespaces["feed"],
                    BTreeSet::from(["http://www.w3.org/2005/Atom".into()])
                );
                assert_eq!(
                    parsed.values["feed.id"],
                    ["https://ids.test/publications/#feed=feed%3Amain"]
                );
                assert_eq!(parsed.values["feed.updated"], ["2024-03-01T04:05:06Z"]);
                assert_eq!(
                    parsed.values["feed.entry.updated"],
                    ["2024-03-01T04:05:06Z", "2024-03-01T04:05:06Z"]
                );
                assert_eq!(
                    parsed.values["feed.entry.author.name"],
                    ["Writer & साथी", "Writer & साथी"]
                );
            } else {
                assert_eq!(
                    parsed.values["rss.channel.id"],
                    ["https://ids.test/publications/#feed=feed%3Amain"]
                );
                assert_eq!(
                    parsed.namespaces["rss.channel.id"],
                    BTreeSet::from(["urn:gazette:syndication".into()])
                );
                assert_eq!(
                    parsed.values["rss.channel.description"],
                    ["Explicit description & <literal>"]
                );
                assert_eq!(
                    parsed.values["rss.channel.item.creator"],
                    ["Writer & साथी", "Writer & साथी"]
                );
                assert_eq!(
                    parsed.namespaces["rss.channel.item.creator"],
                    BTreeSet::from(["http://purl.org/dc/elements/1.1/".into()])
                );
                assert!(
                    !parsed.values.contains_key("rss.channel.item.author"),
                    "name is not an email address"
                );
            }
        });
    }
}

#[test]
fn stable_item_identity_survives_route_feed_path_format_and_serving_base_changes() {
    let files = files();
    let config = configuration(&selection("json", "{kind='journey'}", "journey:ordered"));
    let before = json_feed(&config, &files);
    let changed = config
        .replace("https://home.test/mount/", "https://moved.test/new/")
        .replace("/updates.json", "/renamed.json");
    let mut moved_files = files.clone();
    moved_files[1].1 = moved_files[1].1.replace("path='/z/'", "path='/moved/'");
    let after = json_feed(&changed, &moved_files);
    assert_eq!(item_ids(&before), item_ids(&after));
    assert_ne!(before["items"][0]["url"], after["items"][0]["url"]);
    assert_ne!(before["feed_url"], after["feed_url"]);
    assert_eq!(before["_gazette_feed"], after["_gazette_feed"]);
    let another_feed = json_feed(&config.replace("feed:main", "feed:other"), &files);
    assert_eq!(
        item_ids(&before),
        item_ids(&another_feed),
        "one item namespace across independently named feeds"
    );
    assert_ne!(before["_gazette_feed"], another_feed["_gazette_feed"]);
    for format in ["atom", "rss"] {
        let config = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
        let xml = xml_fields(
            &rendered(&config, &files, 1024 * 1024).expect("same identities in other wire format"),
        );
        let field = if format == "atom" {
            "feed.entry.id"
        } else {
            "rss.channel.item.guid"
        };
        assert_eq!(xml.values[field], item_ids(&before));
    }
}

#[test]
fn foreign_members_wrong_kind_or_missing_pages_fail_without_a_successful_subset() {
    let rejects_containment =
        |config: &str, files: &[(String, String)], source: &str, target: &str| {
            let compiled = workspace::compile(config, Default::default()).expect("workspace");
            let corpus = ParsedCorpus::parse(
                files.iter().map(|(path, source)| Input {
                    path: path.parse().expect("retained path"),
                    bytes: source.as_bytes(),
                }),
                Default::default(),
                &gazette_adapter_markdown::Comrak,
            )
            .expect("complete retained corpus");
            let authored::Error::Structure {
                error: gazette_core::publication::content::Error::Invalid(errors),
                declarations,
            } = authored::resolve(&compiled, &corpus, Default::default())
                .expect_err("foreign containment fails before feed selection")
            else {
                panic!("original structural failure required")
            };
            assert!(
                errors
                    .iter()
                    .any(|error| error.code == "content_containment_site"
                        && error.source == key(source)
                        && error.target == Some(key(target))),
                "{errors:?}"
            );
            let origin = &declarations[&key(source)][0];
            let (_, bytes) = files
                .iter()
                .find(|(path, _)| path == origin.path.as_str())
                .expect("bound declaring bytes");
            assert_eq!(
                origin.source_sha256,
                gazette_core::source::Sha256Digest::of(bytes.as_bytes())
            );
        };
    let config = configuration(&selection("json", "{kind='journey'}", "journey:ordered"));
    let mut foreign = files();
    foreign[5].1 = foreign[5]
        .1
        .replace("content='content:last'", "content='content:foreign'");
    rejects_containment(&config, &foreign, "journey:ordered", "content:foreign");
    let mut foreign_collection = files();
    foreign_collection[6].1 = foreign_collection[6].1.replace(
        "[content]\n",
        "collections=['collection:home']\n[content]\n",
    );
    let collection = configuration(&selection(
        "json",
        "{kind='collection',recursive=false}",
        "collection:home",
    ));
    rejects_containment(
        &collection,
        &foreign_collection,
        "content:foreign",
        "collection:home",
    );
    let mut no_route = files();
    no_route[1].1 = no_route[1].1.replace("route={path='/z/'}\n", "");
    assert!(
        matches!(rendered(&config, &no_route, 1024*1024), Err(FeedError::Binding {content, ..}) if content == key("content:first"))
    );
    for scope in ["{kind='collection',recursive=false}", "{kind='journey'}"] {
        let config = configuration(&selection("json", scope, "content:first"));
        assert!(
            matches!(rendered(&config, &files(), 1024*1024), Err(FeedError::Binding {content, ..}) if content == key("content:first"))
        );
    }
    with_pages(&config, &files(), |resolved, compiled, pages| {
        let selected = &compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("site")
            .machine
            .feeds
            .as_ref()
            .expect("feeds")
            .channels[0];
        let mut incomplete = BTreeMap::new();
        for id in ["journey:ordered", "content:first"] {
            incomplete.insert(
                key(id),
                resolved
                    .project_page(
                        &key(id),
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak,
                    )
                    .expect("available page"),
            );
        }
        assert!(
            matches!(resolved.project_feed(&key("site:home"), selected, &incomplete), Err(FeedError::Binding {content, ..}) if content == key("content:last"))
        );
        let mut unconfigured = selected.clone();
        unconfigured.id = key("feed:not-selected");
        assert!(
            matches!(resolved.project_feed(&key("site:home"), &unconfigured, pages), Err(FeedError::Configuration(id)) if id == key("feed:not-selected"))
        );
    });
}

#[test]
fn missing_required_atom_and_rss_facts_are_exact_errors_not_synthesized_defaults() {
    for (format, file_index, before, after, content_id, field) in [
        (
            "atom",
            5,
            "modified='2024-03-01T04:05:06Z'\n",
            "",
            "journey:ordered",
            "modified timestamp",
        ),
        (
            "atom",
            1,
            "modified='2024-03-01T04:05:06Z'",
            "modified='2024-03-01'",
            "content:first",
            "modified timestamp",
        ),
        (
            "atom",
            1,
            "attributions=[{principal='principal:writer',role='author'}]",
            "attributions=[]",
            "content:first",
            "named author",
        ),
        (
            "atom",
            1,
            "principal='principal:writer',role='author'",
            "principal='principal:unnamed',role='author'",
            "content:first",
            "author name",
        ),
        (
            "rss",
            5,
            "description='Explicit description & <literal>'\n",
            "",
            "journey:ordered",
            "description",
        ),
    ] {
        let config = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
        let mut files = files();
        assert!(files[file_index].1.contains(before));
        files[file_index].1 = files[file_index].1.replace(before, after);
        let error = rendered(&config, &files, 1024 * 1024)
            .expect_err("no invented format-required metadata");
        let FeedError::Missing {
            feed,
            content,
            field: actual,
        } = error
        else {
            panic!("expected exact missing field for {format}/{content_id}")
        };
        assert_eq!(feed, key("feed:main"));
        assert_eq!(content, key(content_id));
        assert_eq!(actual, field);
    }
}

#[test]
fn rss_omits_unrepresentable_dates_but_keeps_exact_typed_editorial_facts() {
    let config = configuration(&selection("rss", "{kind='journey'}", "journey:ordered"));
    let mut files = files();
    files[1].1 = files[1].1.replace(
        "published='2024-02-29T01:02:03+05:30'",
        "published='2024-02-29'",
    );
    files[2].1 = files[2].1.replace(
        "published='2024-02-29T01:02:03+05:30'",
        "published='2024-02-29T01:02:03.123456789+05:30'",
    );
    let parsed = xml_fields(
        &rendered(&config, &files, 1024 * 1024)
            .expect("lossless extension with optional standard dates"),
    );
    assert!(!parsed.values.contains_key("rss.channel.item.pubDate"));
    let metadata = &parsed.values["rss.channel.item.metadata"];
    assert_eq!(
        serde_json::from_str::<Value>(&metadata[0]).expect("date-only metadata")["content"]["published"],
        "2024-02-29"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&metadata[1]).expect("fractional metadata")["content"]["published"],
        "2024-02-29T01:02:03.123456789+05:30"
    );
}

#[test]
fn an_empty_explicit_journey_remains_an_empty_feed_instead_of_falling_back_to_the_site() {
    let mut files = files();
    files[5].1 = files[5].1.replace(
        "members=[{content='content:first',primary=true},{content='content:last',primary=true}]",
        "members=[]",
    );
    let config = configuration(&selection("json", "{kind='journey'}", "journey:ordered"));
    assert_eq!(json_feed(&config, &files)["items"], json!([]));
    for format in ["atom", "rss"] {
        let config = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
        let xml =
            xml_fields(&rendered(&config, &files, 1024 * 1024).expect("empty explicit channel"));
        let entry = if format == "atom" {
            "feed.entry"
        } else {
            "rss.channel.item"
        };
        assert!(!xml.values.contains_key(entry));
    }
}

#[test]
fn all_formats_enforce_exact_complete_output_boundaries() {
    let files = files();
    for format in ["json", "atom", "rss"] {
        let config = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
        let baseline = rendered(&config, &files, 1024 * 1024).expect("baseline feed");
        assert_eq!(
            rendered(&config, &files, baseline.len()).expect("exact bound"),
            baseline
        );
        assert!(
            matches!(
                rendered(&config, &files, baseline.len() - 1),
                Err(FeedError::Limit)
            ),
            "{format}"
        );
        assert!(
            matches!(rendered(&config, &files, 0), Err(FeedError::Limit)),
            "{format}"
        );
    }
}

#[test]
fn xml_transport_preserves_cr_and_rejects_xml_invalid_codepoints_at_the_actual_wire_boundary() {
    let files = files();
    for format in ["atom", "rss"] {
        let config = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
        with_pages(&config, &files, |resolved, compiled, _| {
            let selected = &compiled
                .workspace
                .publication_for(&key("site:home"))
                .expect("publication")
                .machine
                .feeds
                .as_ref()
                .expect("feeds")
                .channels[0];
            let mut pages = ["journey:ordered", "content:first", "content:last"]
                .into_iter()
                .map(|id| {
                    (
                        key(id),
                        resolved
                            .project_page(
                                &key(id),
                                &gazette_core::raster::unavailable,
                                &gazette_adapter_markdown::Comrak,
                            )
                            .expect("actual retained projection"),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            // Parser normalization must not hide a wire-writer defect. Keep real
            // graph/metadata bindings and inject only the public rendered payload.
            let body = format!("<p>First\rSecond & café {}</p>\r", "&<💡>é".repeat(1800));
            pages
                .get_mut(&key("content:first"))
                .expect("first item")
                .body_html = body.clone();
            let output = resolved
                .project_feed(&key("site:home"), selected, &pages)
                .expect("bound projections")
                .render(1024 * 1024)
                .expect("CR-preserving XML")
                .expect("configured retained output");
            let parsed = xml_fields(&output);
            let field = if format == "atom" {
                "feed.entry.content"
            } else {
                "rss.channel.item.description"
            };
            assert_eq!(
                parsed.values[field][0], body,
                "XML normalization must not change CR"
            );
            assert_eq!(
                parsed.values[field][1],
                pages[&key("content:last")].body_html
            );
            for invalid in ['\0', '\u{b}', '\u{fffe}', '\u{ffff}'] {
                pages
                    .get_mut(&key("content:first"))
                    .expect("first item")
                    .body_html = format!("<p>Before{invalid}After</p>");
                let feed = resolved
                    .project_feed(&key("site:home"), selected, &pages)
                    .expect("same semantic bindings");
                assert!(
                    matches!(feed.render(1024 * 1024), Err(FeedError::Encoding(_))),
                    "{format} accepted XML-invalid U+{:04X}",
                    u32::from(invalid)
                );
            }
            pages
                .get_mut(&key("content:first"))
                .expect("first item")
                .body_html = body;
            assert_eq!(
                resolved
                    .project_feed(&key("site:home"), selected, &pages)
                    .expect("restored payload")
                    .render(1024 * 1024)
                    .expect("failure has no retained writer effects")
                    .expect("configured retained output"),
                output
            );
        });
    }
}

#[test]
fn feed_identity_uniqueness_spans_publications_but_not_distinct_feed_ids_or_namespaces() {
    let home = configuration(&selection("json", "{kind='site'}", "collection:home"));
    let foreign = selection("json", "{kind='site'}", "content:foreign");
    let append = |identity: &str, channel: &str| {
        format!("{home}machine={{feeds={{identity_base='{identity}',channels=[{channel}]}}}}\n")
    };
    let error = workspace::compile(
        &append("https://ids.test/publications/", &foreign),
        Default::default(),
    )
    .expect_err("same namespaced feed identity cannot name two publications");
    let workspace::Error::Invalid(errors) = error else {
        panic!("expected identity collision")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_feed_duplicate"
                && error.key.as_ref() == Some(&key("feed:main")))
    );
    for (namespace, channel) in [
        (
            "https://ids.test/publications/",
            foreign.replace("feed:main", "feed:foreign"),
        ),
        ("https://other-ids.test/publications/", foreign.clone()),
    ] {
        let compiled = workspace::compile(&append(namespace, &channel), Default::default())
            .expect("distinct identity pair");
        for site in ["site:home", "site:foreign"] {
            assert_eq!(
                compiled
                    .workspace
                    .publication_for(&key(site))
                    .expect("configured publication")
                    .machine
                    .feeds
                    .as_ref()
                    .expect("retained channel")
                    .channels
                    .len(),
                1
            );
        }
    }
}

#[test]
fn empty_atom_channel_requires_its_own_named_author_without_publisher_fallback() {
    let config = configuration(&selection("atom", "{kind='journey'}", "journey:ordered"));
    let mut files = files();
    files[5].1 = files[5].1.replace("members=[{content='content:first',primary=true},{content='content:last',primary=true}]", "members=[]")
        .replace("attributions=[{principal='principal:writer',role='author'}]", "attributions=[]");
    let error = rendered(&config, &files, 1024 * 1024).expect_err("publisher is not an author");
    let FeedError::Missing {
        feed,
        content,
        field,
    } = error
    else {
        panic!("expected exact missing author")
    };
    assert_eq!(feed, key("feed:main"));
    assert_eq!(content, key("journey:ordered"));
    assert_eq!(field, "named author");
}
