//! Discovery feed selection reuses complete native pages and preserves authored payloads.

use super::*;

pub(super) fn configured(format: &str) -> String {
    let mut value = configuration(&selection(format, "{kind='journey'}", "journey:ordered"));
    value.push_str("\n[[placements]]\nrole='primary'\nsite='site:home'\nproject='project:one'\nrender=true\ndiscoverable=false\ndocuments={source='source:private',target='hidden'}\n");
    value
}

pub(super) fn selected_files() -> Vec<(String, String)> {
    let mut values = files();
    values[1].0 = "hidden/first.md".into();
    values[2]
        .1
        .push_str("\nIntentional [first](https://home.test/mount/z/) remains authored text.\n");
    values
}

#[test]
fn emitted_feed_filters_items_not_authored_links_and_private_feed_remains_complete() {
    for format in ["json", "atom", "rss"] {
        let config = configured(format);
        let files = selected_files();
        with_pages(&config, &files, |resolved, compiled, pages| {
            let configuration = &compiled
                .workspace
                .publication_for(&key("site:home"))
                .expect("publication")
                .machine
                .feeds
                .as_ref()
                .expect("feeds")
                .channels[0];
            let discovery = resolved.discovery().expect("source-bound eligibility");
            let private = resolved
                .project_feed(&key("site:home"), configuration, pages)
                .expect("private complete feed");
            let emitted = resolved
                .project_discovery_feed(&key("site:home"), configuration, pages, &discovery)
                .expect("discovery feed");
            let private_bytes = private
                .render(1 << 20)
                .expect("original wire")
                .expect("configured retained output");
            let bytes = emitted
                .render(1 << 20)
                .expect("filtered wire")
                .expect("configured retained output");
            let text = std::str::from_utf8(&bytes).expect("feed UTF-8");
            assert!(
                text.contains("Intentional"),
                "original retained body still emitted"
            );
            let inspect_metadata = |metadata: &Value| {
                for field in [
                    "content",
                    "children",
                    "collections",
                    "journeys",
                    "breadcrumbs",
                    "related",
                    "steps",
                ] {
                    if let Some(value) = metadata.get(field) {
                        assert!(
                            !serde_json::to_string(value)
                                .expect("generated metadata field")
                                .contains("content:first"),
                            "suppressed endpoint returned through {field}: {value:#}"
                        );
                    }
                }
                assert!(
                    metadata.get("body_html").is_none(),
                    "metadata does not duplicate standard body"
                );
                let id = metadata["content"]["id"]
                    .as_str()
                    .expect("listed content identity");
                assert_eq!(
                    metadata["links"],
                    serde_json::to_value(&pages[&key(id)].data.links)
                        .expect("original authored link evidence")
                );
                if id == "content:last" {
                    assert_eq!(metadata["links"][0]["content"], "content:first");
                    assert_eq!(
                        metadata["links"][0]["destination"],
                        "https://home.test/mount/z/"
                    );
                    assert_eq!(metadata["links"][0]["kind"], "internal-native");
                }
            };
            if format == "json" {
                let original: Value =
                    serde_json::from_slice(&private_bytes).expect("private JSON feed");
                let projected: Value = serde_json::from_slice(&bytes).expect("emitted JSON feed");
                assert_eq!(
                    original["items"].as_array().expect("original items").len(),
                    2
                );
                assert_eq!(
                    projected["items"].as_array().expect("listed items").len(),
                    1
                );
                let mut original_item = original["items"][1].clone();
                let mut listed_item = projected["items"][0].clone();
                original_item
                    .as_object_mut()
                    .expect("original item object")
                    .remove("_gazette");
                listed_item
                    .as_object_mut()
                    .expect("listed item object")
                    .remove("_gazette");
                assert_eq!(
                    listed_item, original_item,
                    "standard feed meaning and body are unchanged"
                );
                for metadata in [&projected["_gazette"], &projected["items"][0]["_gazette"]] {
                    inspect_metadata(metadata);
                }
                assert!(
                    !item_ids(&projected)
                        .iter()
                        .any(|id| id.contains("content%3Afirst"))
                );
                assert!(
                    projected["_gazette"]["content"]["structure"]
                        .get("members")
                        .is_none()
                );
                assert!(
                    projected["items"][0]["content_html"]
                        .as_str()
                        .expect("authored HTML")
                        .contains("https://home.test/mount/z/")
                );
            } else {
                let fields = xml_fields(&bytes);
                let original = xml_fields(&private_bytes);
                let element = if format == "atom" {
                    "feed.entry.id"
                } else {
                    "rss.channel.item.guid"
                };
                assert_eq!(
                    fields.values[element].len() + 1,
                    original.values[element].len()
                );
                let mut metadata_count = 0;
                for (path, values) in &fields.values {
                    if path.ends_with(".metadata") {
                        for metadata in values {
                            metadata_count += 1;
                            let value: Value = serde_json::from_str(metadata)
                                .expect("XML metadata is shared JSON projection");
                            inspect_metadata(&value);
                        }
                    }
                }
                assert_eq!(
                    metadata_count, 2,
                    "channel and retained item metadata checked"
                );
                assert!(
                    !fields.values[element]
                        .iter()
                        .any(|id| id.contains("content%3Afirst"))
                );
            }
            assert_eq!(
                emitted
                    .render(bytes.len())
                    .expect("exact wire cap")
                    .expect("configured retained output"),
                bytes
            );
            assert!(matches!(
                emitted.render(bytes.len() - 1),
                Err(FeedError::Limit)
            ));
            assert_eq!(
                private
                    .render(1 << 20)
                    .expect("unchanged private feed")
                    .expect("configured retained output"),
                private_bytes
            );
        });
    }
}

#[test]
fn hidden_channel_and_missing_original_hidden_page_are_explicit_failures() {
    let config = configured("json");
    let mut files = selected_files();
    with_pages(&config, &files, |resolved, compiled, pages| {
        let configuration = &compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .as_ref()
            .expect("feeds")
            .channels[0];
        let discovery = resolved.discovery().expect("eligibility");
        // A partial page map is not permission to skip validation of the original feed scope.
        let partial = pages
            .iter()
            .filter(|(id, _)| **id != key("content:first"))
            .map(|(id, _)| {
                (
                    id.clone(),
                    resolved
                        .project_page(
                            id,
                            &gazette_core::raster::unavailable,
                            &gazette_adapter_markdown::Comrak,
                        )
                        .expect("same original page"),
                )
            })
            .collect();
        assert!(
            matches!(resolved.project_discovery_feed(&key("site:home"), configuration, &partial, &discovery), Err(FeedError::Binding { content, .. }) if content == key("content:first"))
        );
    });
    files[5].0 = "hidden/journey.md".into();
    with_pages(&config, &files, |resolved, compiled, pages| {
        let configuration = &compiled
            .workspace
            .publication_for(&key("site:home"))
            .expect("publication")
            .machine
            .feeds
            .as_ref()
            .expect("feeds")
            .channels[0];
        let discovery = resolved.discovery().expect("eligibility");
        assert!(matches!(
            resolved.project_discovery_feed(&key("site:home"), configuration, pages, &discovery),
            Err(FeedError::Discovery)
        ));
        resolved
            .project_feed(&key("site:home"), configuration, pages)
            .expect("private inspection still permits hidden channel");
    });
}
