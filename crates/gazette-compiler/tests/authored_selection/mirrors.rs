//! Candidate exclusion cannot leave a foreign redirect to excluded meaning.

use super::*;

fn mirrored() -> String {
    page("content:source", "published", "/guide/", "Canonical body\n").replace(
        "path='/guide/'",
        "path='/guide/',mirrors=[{site='site:two',path='/mirror/'}]",
    )
}

#[test]
fn excluded_canonical_owner_removes_its_mirror_without_copying_a_foreign_node() {
    let files = vec![
        ("authored/one/source.md".into(), mirrored()),
        (
            "authored/two/other.md".into(),
            page(
                "content:other",
                "published",
                "/other/",
                "Independent treatment\n",
            )
            .replace("site='site:one'", "site='site:two'"),
        ),
    ];
    let corpus = parse(&files);
    let compiled = compilation();
    let original = resolve(&compiled, &corpus);
    assert_eq!(
        original
            .project_site_pages(&key("site:two"))
            .unwrap()
            .mirrors()
            .len(),
        1
    );
    let mut selection = all(&original);
    selection
        .content
        .insert(key("content:source"), Disposition::Exclude);
    let selected = original.into_selected(&selection).unwrap();
    let site = selected.project_site_pages(&key("site:two")).unwrap();
    assert!(site.mirrors().is_empty());
    assert_eq!(site.pages().next().unwrap().node.id, key("content:other"));
}

#[test]
fn selected_body_cannot_link_to_an_excluded_canonical_object_via_its_mirror() {
    let files = vec![
        ("authored/one/source.md".into(), mirrored()),
        (
            "authored/one/other.md".into(),
            page(
                "content:other",
                "published",
                "/other/",
                "[Guide](https://other.test/mirror/)\n",
            ),
        ),
    ];
    let corpus = parse(&files);
    let compiled = compilation();
    let original = resolve(&compiled, &corpus);
    let mut selection = all(&original);
    selection
        .content
        .insert(key("content:source"), Disposition::Exclude);
    assert!(
        matches!(original.into_selected(&selection), Err(SelectionError::Link { content, target, .. }) if content == key("content:other") && target == key("content:source"))
    );
}

#[test]
fn preview_does_not_disguise_an_excluded_mirrors_original_public_address() {
    for destination in [
        "https://other.test/mirror/",
        "http://127.0.0.1:32002/mirror/",
    ] {
        let files = vec![
            ("authored/one/source.md".into(), mirrored()),
            (
                "authored/one/other.md".into(),
                page(
                    "content:other",
                    "published",
                    "/other/",
                    &format!("[Guide]({destination})\n"),
                ),
            ),
        ];
        let corpus = parse(&files);
        let compiled = compilation();
        // Only the foreign mirror site's base changes, not the canonical home.
        let original = resolve(&compiled, &corpus)
            .with_preview_urls(
                [(key("site:two"), "http://127.0.0.1:32002/".parse().unwrap())].into(),
            )
            .unwrap();
        let mut selection = all(&original);
        selection
            .content
            .insert(key("content:source"), Disposition::Exclude);
        assert!(
            matches!(original.into_selected(&selection), Err(SelectionError::Link { target, .. }) if target == key("content:source"))
        );
    }
}
