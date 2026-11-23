//! Stable identities follow current retained routes without rewriting prose.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    markdown, workspace,
};
use gazette_core::{
    publication::Key,
    source::{EntryPath, Span},
};

fn key(value: &str) -> Key {
    value.parse().unwrap()
}
fn path(value: &str) -> EntryPath {
    value.parse().unwrap()
}

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/not-opened'}\n\
[[brands]]\nid='brand:one'\nname='Identity fixture'\n\
[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/prefix/'\nauthored=[{source='source:one',target='one'}]\n\
[[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/mount/'\nauthored=[{source='source:one',target='two'}]\n";

fn page(id: &str, site: &str, route: &str, body: &str) -> String {
    format!(
        "+++\n[content]\nid='{id}'\nsite='{site}'\ntitle='Title'\nlifecycle='draft'\n{route}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n{body}"
    )
}

fn corpus<'a>(files: &'a [(String, String)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(name, text)| Input {
            path: path(name),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}

#[test]
fn stable_keys_follow_moved_renamed_cross_site_targets_and_preview_bases() {
    let body = "[[id:collection:target#detail|Visible β label]] [[id:collection:target#gazette-notes-comment|Commentary]]";
    for (source_path, route) in [
        ("two/old/index.md", "/old/"),
        ("two/relocated/index.md", "/new-title/"),
    ] {
        let files = vec![
            (
                "one/index.md".into(),
                page(
                    "content:source",
                    "site:one",
                    "route={path='/source/'}",
                    body,
                ),
            ),
            (
                source_path.into(),
                page(
                    "collection:target",
                    "site:two",
                    &format!("route={{path='{route}'}}"),
                    "# A different title\n\n## Detail {#detail}",
                ),
            ),
            (
                source_path.replace("index.md", "notes.md"),
                "## Commentary {#comment}".into(),
            ),
        ];
        let corpus = corpus(&files);
        let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
        for preview in [false, true] {
            let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
            let resolved = if preview {
                resolved
                    .with_preview_urls(
                        [(
                            key("site:two"),
                            "http://127.0.0.1:42001/mount/".parse().unwrap(),
                        )]
                        .into(),
                    )
                    .unwrap()
            } else {
                resolved
            };
            let page = resolved
                .project_page(
                    &key("content:source"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                )
                .unwrap();
            let base = if preview {
                "http://127.0.0.1:42001/mount"
            } else {
                "https://two.test/mount"
            };
            assert_eq!(
                page.data.links[0].destination,
                format!("{base}{route}#detail")
            );
            assert_eq!(page.data.links[0].content, Some(key("collection:target")));
            assert_eq!(
                page.data.links[1].destination,
                format!("{base}{route}#gazette-notes-comment")
            );
            let references = resolved.publication_references(100, 1 << 20).unwrap();
            assert_eq!(references[0].target, &path(source_path));
            assert_eq!(
                references[1].target,
                &path(&source_path.replace("index.md", "notes.md"))
            );
        }
    }
}

#[test]
fn missing_unrouted_and_fragment_targets_fail_without_namespace_guessing() {
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    for (selector, route) in [
        ("id:target", "route={path='/target/'}"),
        ("id:content:target", "route={path='/target/'}"),
        ("id:collection:target#absent", "route={path='/target/'}"),
        ("id:collection:target#bad%", "route={path='/target/'}"),
        ("id:collection:target", ""),
    ] {
        let files = vec![
            (
                "one/index.md".into(),
                page(
                    "content:source",
                    "site:one",
                    "route={path='/source/'}",
                    &format!("[[{selector}|Keep label]]"),
                ),
            ),
            (
                "two/index.md".into(),
                page("collection:target", "site:two", route, "# Target"),
            ),
        ];
        let corpus = corpus(&files);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let error = resolved
            .project_page(
                &key("content:source"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap_err();
        assert_eq!(error.code(), "markdown_link_resolution", "{selector}");
        assert!(resolved.publication_references(100, 1 << 20).is_err());
    }
}

#[test]
fn escaped_keys_decode_once_and_completion_preserves_native_definitions() {
    let files = vec![
        (
            "one/index.md".into(),
            page(
                "content:source",
                "site:one",
                "route={path='/source/'}",
                "[[id:journey:target%2523%23part#detail|Escaped identity]]",
            ),
        ),
        (
            "two/index.md".into(),
            page(
                "journey:target%23#part",
                "site:two",
                "route={path='/target/'}",
                "# Target\n\n## Detail {#detail}",
            ),
        ),
    ];
    let corpus = corpus(&files);
    let config = workspace::compile(WORKSPACE, Default::default()).unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:source"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        page.data.links[0].content,
        Some(key("journey:target%23#part"))
    );
    let targets = resolved.symbolic_completion_targets(100, 1 << 20).unwrap();
    let identity = targets
        .iter()
        .find(|target| target.replacement == "id:journey:target%2523%23part#detail")
        .unwrap();
    let address = targets
        .iter()
        .find(|target| target.replacement == "site:two.test/target/#detail")
        .unwrap();
    assert_eq!(identity.definition, address.definition);
    assert_eq!(identity.path, address.path);
    assert!(resolved.symbolic_completion_targets(1, 1 << 20).is_err());
    for bad in [
        "id:",
        "id:target",
        "id:content:bad%",
        "id:content:%ff",
        "id:content:%20",
        "id:content:%00",
    ] {
        assert!(
            markdown::IdentityReference::parse(
                bad,
                Span {
                    start: 0,
                    end: bad.len()
                }
            )
            .is_err(),
            "{bad}"
        );
    }
}

#[test]
fn identity_completion_edits_only_recognized_literal_destinations() {
    let source = "Préface β [[id:content:missing#part|Keep label]] [[id:|Incomplete]]\n`[[id:content:code]]`\n[ordinary](id:content:ordinary)";
    let parsed = markdown::ParsedMarkdown::parse(
        source,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let slots = parsed.symbolic_completion_slots().unwrap();
    assert_eq!(
        slots
            .iter()
            .map(|span| span.slice(source).unwrap())
            .collect::<Vec<_>>(),
        ["id:content:missing#part", "id:"]
    );
    let mut edited = source.to_owned();
    edited.replace_range(slots[0].start..slots[0].end, "id:collection:target#detail");
    assert!(edited.contains("[[id:collection:target#detail|Keep label]]"));
}

#[test]
fn shared_domains_do_not_make_exact_identity_targets_ambiguous() {
    let files = vec![
        (
            "one/index.md".into(),
            page(
                "content:source",
                "site:one",
                "route={path='/source/'}",
                "[[id:content:target|Exact target]]",
            ),
        ),
        (
            "two/index.md".into(),
            page(
                "content:target",
                "site:two",
                "route={path='/target/'}",
                "# Target",
            ),
        ),
    ];
    let corpus = corpus(&files);
    let config = workspace::compile(
        &WORKSPACE.replace("two.test", "one.test"),
        Default::default(),
    )
    .unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:source"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        page.data.links[0].destination,
        "https://one.test/mount/target/"
    );
    let targets = resolved.symbolic_completion_targets(100, 1 << 20).unwrap();
    assert!(
        targets
            .iter()
            .any(|target| target.replacement == "id:content:target")
    );
    assert!(
        targets
            .iter()
            .all(|target| target.replacement.starts_with("id:"))
    );
}
