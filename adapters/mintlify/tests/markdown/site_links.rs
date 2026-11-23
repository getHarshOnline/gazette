//! Cross-module page and fragment links after complete target assembly.

use super::*;

#[test]
fn site_root_links_join_selected_modules_and_validate_anchors_after_complete_assembly() {
    use gazette_core::markdown::{Limits as MarkdownLimits, Profile};
    use gazette_ports::markdown::Markdown;

    let first_bytes = b"# First\n\n[Client][api] and [Again](/second/library/types/Client.mdx#client).\n\n[api]: /second/library/types/Client.md#client\n\n```text\n[Code](/missing/library)\n```\n";
    let client_bytes = b"# Client\n\n[Back](/first/library.md#first)\n";
    let first = Input::new(first_bytes);
    let second = directory(
        &[
            ("selected/types/Client.md", client_bytes),
            ("hidden.md", b"# Unselected"),
        ],
        Some("selected"),
    );
    let output = assemble(&[("first", &first), ("second", &second)], Limits::default())
        .expect("forward and reverse cross-module links");
    let repeated = assemble(&[("first", &first), ("second", &second)], Limits::default())
        .expect("identical retained input");
    assert_eq!(output.inventory(), repeated.inventory());
    let reordered = assemble(&[("second", &second), ("first", &first)], Limits::default())
        .expect("module declaration order is not link visibility");
    for (path, expected) in [
        (
            "first/library.md",
            vec![
                "/second/library/types/Client#client",
                "/second/library/types/Client#client",
            ],
        ),
        (
            "second/library/types/Client.md",
            vec!["/first/library#first"],
        ),
    ] {
        let path = path.parse().expect("emitted page path");
        let bytes = output.file(&path).expect("emitted page");
        assert_eq!(Some(bytes), reordered.file(&path));
        let parsed = Comrak
            .parse(
                std::str::from_utf8(bytes).expect("UTF-8 page"),
                Profile::Publication,
                MarkdownLimits::default(),
            )
            .expect("native link facts");
        assert_eq!(
            parsed
                .syntax()
                .links
                .iter()
                .map(|link| link.destination.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let text = std::str::from_utf8(
        output
            .file(&"first/library.md".parse().expect("first page"))
            .expect("first bytes"),
    )
    .expect("UTF-8");
    assert!(text.contains("[Code](/missing/library)"));
    assert_eq!(
        first.tree.file(&"reference.json".parse().expect("source")),
        Some(first_bytes.as_slice())
    );
    assert_eq!(
        second
            .tree
            .file(&"selected/types/Client.md".parse().expect("source")),
        Some(client_bytes.as_slice())
    );
    let configuration = |tree: &CapturedSource| -> Value {
        serde_json::from_slice(
            tree.file(&"docs.json".parse().expect("configuration path"))
                .expect("configuration bytes"),
        )
        .expect("configuration JSON")
    };
    assert_eq!(
        configuration(&output)["navigation"]["dropdowns"][0]["dropdown"],
        "first"
    );
    assert_eq!(
        configuration(&reordered)["navigation"]["dropdowns"][0]["dropdown"],
        "second"
    );
}

#[test]
fn local_and_cross_module_fragments_cannot_name_missing_or_nonemitted_anchors() {
    let target = directory(
        &[("selected/Target.md", b"# Target\n\n## Stable {#explicit}\n")],
        Some("selected"),
    );
    for destination in [
        "/second/library/Target#missing",
        "/second/library/Target#Target",
    ] {
        let source = format!("# First\n\n[Target]({destination})\n");
        let first = Input::new(source.as_bytes());
        assert!(
            assemble(&[("first", &first), ("second", &target)], Limits::default()).is_err(),
            "accepted missing or wrong-case heading anchor {destination}"
        );
    }
    let source = Input::new(b"# First\n\n[Target](/second/library/Target#explicit)\n");
    assemble(
        &[("first", &source), ("second", &target)],
        Limits::default(),
    )
    .expect("exact explicit cross-module anchor");

    let missing_self = Input::new(b"# First\n\n[Self](#missing)\n");
    assert!(assemble(&[("first", &missing_self)], Limits::default()).is_err());
}

#[test]
fn site_root_links_require_exact_emitted_pages_not_aliases_or_companions() {
    let target = directory(
        &[
            ("selected/Target.md", PAGE),
            ("Hidden.md", PAGE),
            ("asset.png", b"not a published page"),
        ],
        Some("selected"),
    );
    for destination in [
        "/second/library/Missing",
        "/second/library/target",
        "/second/library/Hidden",
        "/second/library/asset.png",
        "/unselected/library/Target",
        "/second/library",
        "/docs.json",
        "/artifacts/second/library/Target",
        "/second//library/Target",
        "/second/library/Target/",
        "/second/library/./Target",
        "/first/../second/library/Target",
        "/second/library/Target?view=all",
        "/second/library/%54arget",
        "//second/library/Target",
        "/second/library/Target#",
    ] {
        let source = format!("[Target]({destination})\n");
        let first = Input::new(source.as_bytes());
        assert!(
            assemble(&[("first", &first), ("second", &target)], Limits::default()).is_err(),
            "accepted non-page or non-exact route {destination}"
        );
        assert_eq!(
            first
                .tree
                .file(&"reference.json".parse().expect("source path")),
            Some(source.as_bytes())
        );
    }
    let image = Input::new(b"![Not an image](/second/library/Target.md)\n");
    assert!(assemble(&[("first", &image), ("second", &target)], Limits::default()).is_err());
}

#[test]
fn complete_site_page_visibility_does_not_expand_relative_reference_scope() {
    let first = directory(
        &[("Start.md", b"[Other](../../second/library/Target.md)\n")],
        None,
    );
    let second = directory(&[("Target.md", PAGE)], None);
    assert!(assemble(&[("first", &first), ("second", &second)], Limits::default()).is_err());
}
