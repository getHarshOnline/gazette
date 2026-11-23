//! Alias fragments retain every section and its own links when native IDs repeat.
use super::*;
use crate::rustdoc::html::{Node, append_implementations};
use crate::rustdoc::type_impls::Implementation;

fn row(html: &str) -> Implementation {
    Implementation {
        html: html.into(),
        is_trait: false,
    }
}

#[test]
fn repeated_section_ids_keep_distinct_destinations_and_links() {
    let source = document("<div id=\"examples\">Page introduction</div>", false);
    let mut page = parse(source.as_bytes(), "sample/type.First.html", 64 << 10).unwrap();
    append_implementations(&mut page, vec![
        row("<h3 id=\"examples\">First</h3><a href=\"#examples\">First link</a><h3 id=\"examples-1\">Nested</h3><a href=\"#examples-1\">Nested link</a>"),
        row("<h3 id=\"examples\">Second</h3><a href=\"#examples\">Second link</a><a href=\"sample/struct.Record.html\">Record</a>"),
        row("<h3 id=\"examples\">Third</h3><a href=\"#examples\">Third link</a>"),
    ], "sample/type.First.html", "type.impl/sample/struct.Record.js", 64 << 10).unwrap();
    let mut ids = std::collections::BTreeSet::new();
    let mut links = Vec::new();
    for node in &page.nodes {
        if let Node::Start { attributes, .. } = node {
            if let Some(id) = attributes.get("id") {
                assert!(ids.insert(id.clone()), "duplicate {id}");
            }
            if let Some(href) = attributes.get("href") {
                links.push(href.as_str());
            }
        }
    }
    assert_eq!(
        links,
        [
            "#examples-2",
            "#examples-1",
            "#examples-3",
            "../sample/struct.Record.html",
            "#examples-4"
        ]
    );
    assert_eq!(ids, page.anchors);
    assert!(page.hrefs.contains("../sample/struct.Record.html"));
}

#[test]
fn alias_fragments_share_active_content_and_aggregate_limits() {
    let source = document("", false);
    for (body, expected) in [
        (
            "<script>act()</script>",
            "unsupported Rustdoc notable-trait fragment",
        ),
        (
            "<h3 onclick=\"act()\">Title</h3>",
            "unsupported Rustdoc active or script-dependent attribute",
        ),
        (
            "<h1>Second title</h1>",
            "unsupported Rustdoc notable-trait fragment",
        ),
    ] {
        let mut page = parse(source.as_bytes(), "sample/type.First.html", 64 << 10).unwrap();
        assert!(matches!(
            append_implementations(
                &mut page,
                vec![row(body)],
                "sample/type.First.html",
                "type.impl/sample/struct.Record.js",
                64 << 10
            ),
            Err(crate::Error::Invalid { reason, .. }) if reason == expected
        ));
    }
    let mut page = parse(source.as_bytes(), "sample/type.First.html", 64 << 10).unwrap();
    let maximum = page.decoded + 20;
    assert!(matches!(
        append_implementations(
            &mut page,
            vec![row("<p>123456789012345</p>"), row("<p>123456789012345</p>")],
            "sample/type.First.html",
            "type.impl/sample/struct.Record.js",
            maximum
        ),
        Err(crate::Error::Invalid {
            reason: "Rustdoc decoded-byte ceiling exceeded",
            ..
        })
    ));
}

#[test]
fn upstream_collection_link_correction_requires_exact_native_context() {
    const OWNER: &str = "type.impl/alloc/collections/btree/map/struct.BTreeMap.js";
    const LINK: &str = "index.html#insert-and-complex-keys";
    const FIXED: &str =
        "https://doc.rust-lang.org/1.94.0/std/collections/index.html#insert-and-complex-keys";
    let source = document("<a href=\"https://doc.rust-lang.org/1.94.0/alloc/collections/btree/map/struct.BTreeMap.html\">BTreeMap</a>",false)
        .replace("rustdoc struct", "rustdoc type")
        .replace("1.94.0 (qualified fixture)", "1.94.0 (4a4ef493e 2026-03-02)");
    let fragment = format!(
        "<section id=\"method.insert\"><p>Original explanation. <a href=\"{LINK}\">Module-level documentation</a>.</p></section>"
    );
    for case in 0..7 {
        let mut page = parse(source.as_bytes(), "sample/type.Lookup.html", 64 << 10).unwrap();
        let mut implementation = row(&fragment);
        let owner = if case == 1 {
            "type.impl/other/struct.BTreeMap.js"
        } else {
            OWNER
        };
        match case {
            2 => page.generator = "1.95.0 (unqualified)".into(),
            3 => page.hrefs.clear(),
            4 => implementation.html = fragment.replace("method.insert", "method.get"),
            5 => implementation.is_trait = true,
            6 => implementation.html = fragment.replace(LINK, "index.html#another-section"),
            _ => (),
        }
        append_implementations(
            &mut page,
            vec![implementation],
            "sample/type.Lookup.html",
            owner,
            64 << 10,
        )
        .unwrap();
        assert_eq!(page.hrefs.contains(FIXED), case == 0, "case {case}");
        if case != 0 {
            let fragment = if case == 6 {
                "another-section"
            } else {
                "insert-and-complex-keys"
            };
            assert!(page.hrefs.contains(&format!("../index.html#{fragment}")));
        }
    }
}
