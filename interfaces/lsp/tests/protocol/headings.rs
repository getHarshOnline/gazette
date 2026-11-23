//! Native same-document heading associations through the real framed LSP session.

use super::{
    footnotes::{definition, location, position, references, response},
    *,
};

const PAGE: &str = "file:///selected/headings.md";

#[test]
fn selected_cross_document_targets_follow_unsaved_unicode_sources_and_close_baselines() {
    let caller_uri = "untitled:caller.md";
    let target_uri = "untitled:target.md";
    let caller = format!(
        "{}\n😀 [other](../target.md#shared) [missing](missing.md#shared)\n",
        content("content:caller", "", "kind='page'\nfamily={kind='article'}")
    )
    .replace('\n', "\r\n");
    let target = format!(
        "{}\n# Shared 🦀 {{#shared}}\n\n😀 [local](#shared)\n",
        content("content:target", "", "kind='page'\nfamily={kind='article'}")
    )
    .replace('\n', "\r\n");
    let changed = target.replace("# Shared", "😀 Unsaved prefix\r\n\r\n# Shared");
    let call = position(&caller, "[other]", 0);
    assert_eq!(call["character"], 3, "emoji uses two UTF-16 units");
    let (result, messages) = selected_session(
        selected(&[
            (caller_uri, "notes/caller.md", &caller),
            (target_uri, "target.md", &target),
        ]),
        [
            definition(2, caller_uri, call.clone()),
            references(3, target_uri, position(&target, "# Shared", 0), true),
            open_content(target_uri, 10, &target),
            change(target_uri, 11, &changed),
            definition(4, caller_uri, call.clone()),
            references(5, target_uri, position(&changed, "# Shared", 0), false),
            close(target_uri),
            definition(6, caller_uri, call),
            definition(7, caller_uri, position(&caller, "[missing]", 0)),
            references(8, caller_uri, position(&caller, "[missing]", 0), true),
        ],
        Limits::default(),
    );
    result.expect("real framed cross-source session without native file access");
    let caller_link = location(caller_uri, &caller, "[other](../target.md#shared)", 0);
    let baseline = location(target_uri, &target, "# Shared 🦀 {#shared}", 0);
    assert_eq!(response(&messages, 2)["result"], baseline);
    assert_eq!(
        response(&messages, 3)["result"],
        json!([
            caller_link,
            baseline,
            location(target_uri, &target, "[local](#shared)", 0)
        ])
    );
    assert_eq!(
        response(&messages, 4)["result"],
        location(target_uri, &changed, "# Shared 🦀 {#shared}", 0)
    );
    assert_ne!(response(&messages, 4)["result"], baseline);
    assert_eq!(
        response(&messages, 5)["result"],
        json!([
            caller_link,
            location(target_uri, &changed, "[local](#shared)", 0)
        ])
    );
    assert_eq!(
        response(&messages, 6)["result"],
        baseline,
        "close restores the selected target, never disk"
    );
    assert!(response(&messages, 7)["result"].is_null());
    assert_eq!(response(&messages, 8)["result"], json!([]));
    for id in 2..=8 {
        assert!(
            response(&messages, id).get("error").is_none(),
            "{}",
            response(&messages, id)
        );
    }
}

fn page() -> String {
    format!(
        "{}\n# Repeat\n\n# Repeat\n\n## Custom {{#\"résumé\"}}\n\n😀 [first](#repeat) [second](#repeat-1) [encoded](#r%C3%A9sum%C3%A9) [query](?view=one#repeat)\n",
        content("content:headings", "", "kind='page'\nfamily={kind='article'}")
    ).replace('\n', "\r\n")
}

#[test]
fn heading_definition_and_references_use_allocated_ids_and_full_source_utf16() {
    let source = page();
    let first = position(&source, "[first]", 0);
    assert_eq!(
        first["character"], 3,
        "emoji plus space uses three UTF-16 units"
    );
    let (result, messages) = selected_session(
        selected(&[(PAGE, "headings.md", &source)]),
        [
            definition(2, PAGE, first.clone()),
            definition(3, PAGE, position(&source, "[second]", 0)),
            definition(4, PAGE, position(&source, "[encoded]", 0)),
            definition(5, PAGE, position(&source, "[query]", 0)),
            references(6, PAGE, position(&source, "# Repeat", 0), false),
            references(7, PAGE, first, true),
            references(8, PAGE, position(&source, "# Repeat", 1), true),
        ],
        Limits::default(),
    );
    result.expect("native heading navigation protocol");
    for id in 2..=8 {
        assert!(
            response(&messages, id).get("error").is_none(),
            "unexpected navigation response: {}",
            response(&messages, id)
        );
    }
    let first_heading = location(PAGE, &source, "# Repeat", 0);
    assert_eq!(response(&messages, 2)["result"], first_heading);
    assert_eq!(
        response(&messages, 3)["result"],
        location(PAGE, &source, "# Repeat", 1)
    );
    assert_eq!(
        response(&messages, 4)["result"],
        location(PAGE, &source, "## Custom {#\"résumé\"}", 0)
    );
    assert_eq!(response(&messages, 5)["result"], first_heading);
    let first_link = location(PAGE, &source, "[first](#repeat)", 0);
    let query_link = location(PAGE, &source, "[query](?view=one#repeat)", 0);
    assert_eq!(
        response(&messages, 6)["result"],
        json!([first_link, query_link])
    );
    assert_eq!(
        response(&messages, 7)["result"],
        json!([first_heading, first_link, query_link])
    );
    assert_eq!(
        response(&messages, 8)["result"],
        json!([
            location(PAGE, &source, "# Repeat", 1),
            location(PAGE, &source, "[second](#repeat-1)", 0),
        ])
    );
}

#[test]
fn overlays_reallocate_headings_and_invalid_current_corpus_never_returns_old_targets() {
    let source = page();
    let changed = source
        .replacen("# Repeat\r\n", "# Changed\r\n", 1)
        .replace("[second](#repeat-1)", "[second](#changed)");
    let other_uri = "file:///selected/other.md";
    let other = content("content:other", "", "kind='page'\nfamily={kind='article'}");
    let point = position(&source, "[first]", 0);
    let (result, messages) = selected_session(
        selected(&[
            (PAGE, "headings.md", &source),
            (other_uri, "other.md", &other),
        ]),
        [
            definition(2, PAGE, point.clone()),
            open_content(PAGE, 1, &source),
            change(PAGE, 2, &changed),
            definition(3, PAGE, point.clone()),
            change(PAGE, 1, &source),
            definition(4, PAGE, point.clone()),
            open_content(other_uri, 1, "+++\n[broken\n+++\n# Wrong"),
            definition(5, PAGE, point.clone()),
            references(6, PAGE, point.clone(), true),
            close(other_uri),
            close(PAGE),
            definition(7, PAGE, point),
        ],
        Limits::default(),
    );
    result.expect("current-buffer navigation lifecycle");
    for id in [2, 3, 4, 7] {
        assert!(
            response(&messages, id).get("error").is_none(),
            "unexpected current-source response: {}",
            response(&messages, id)
        );
    }
    assert_eq!(
        response(&messages, 3)["result"],
        location(PAGE, &changed, "# Repeat", 0)
    );
    assert_eq!(
        response(&messages, 4)["result"],
        response(&messages, 3)["result"],
        "stale change is ignored"
    );
    for id in [5, 6] {
        assert_eq!(response(&messages, id)["error"]["code"], -32600);
        assert!(response(&messages, id).get("result").is_none());
    }
    assert_eq!(
        response(&messages, 7)["result"],
        response(&messages, 2)["result"],
        "close restores selected baseline"
    );
}

#[test]
fn external_relative_and_image_links_do_not_infer_local_heading_associations() {
    let source = format!(
        "{}\n# Repeat\n\n[external](https://example.test/#repeat) [relative](other.md#repeat) [absolute](/page/#repeat) ![image](#repeat)\n\nPlain words.\n",
        content(
            "content:headings",
            "",
            "kind='page'\nfamily={kind='article'}"
        )
    );
    let mut requests = Vec::new();
    for (index, needle) in [
        "[external]",
        "[relative]",
        "[absolute]",
        "![image]",
        "Plain words",
    ]
    .into_iter()
    .enumerate()
    {
        let id = i32::try_from(index).expect("bounded fixture request index");
        requests.push(definition(10 + id, PAGE, position(&source, needle, 0)));
        requests.push(references(
            20 + id,
            PAGE,
            position(&source, needle, 0),
            true,
        ));
    }
    let (result, messages) = selected_session(
        selected(&[(PAGE, "headings.md", &source)]),
        requests,
        Limits::default(),
    );
    result.expect("unrelated links remain non-occurrences");
    for id in 10..15 {
        assert!(response(&messages, id)["result"].is_null());
        assert!(response(&messages, id).get("error").is_none());
        assert_eq!(response(&messages, id + 10)["result"], json!([]));
    }
}
