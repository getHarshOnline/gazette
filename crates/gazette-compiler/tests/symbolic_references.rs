//! Symbolic wikilinks bind retained syntax to configured sites, routes and anchors.

use std::collections::BTreeMap;

use gazette_compiler::{
    authored::{self, ProjectionError, ResolvedAuthored, Selection, SelectionError},
    corpus::{Input, ParsedCorpus},
    document, markdown, workspace,
};
use gazette_core::{
    markdown::{LinkDestinationKind, LinkRelationship},
    publication::{Key, content::Disposition, routing::PublicBaseUrl},
    source::{EntryPath, Sha256Digest},
};
use serde_json::json;

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("retained logical source path")
}

fn assert_no_source_spans(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            assert!(!fields.contains_key("span"), "private source coordinates");
            fields.values().for_each(assert_no_source_spans);
        }
        serde_json::Value::Array(values) => values.iter().for_each(assert_no_source_spans),
        _ => {}
    }
}

fn configuration() -> &'static str {
    "[[principals]]\nid='owner:one'\nkind='person'\n\
     [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
     [[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n\
     [[sources]]\nid='source:retained'\nlocation={kind='repository',root='/private/symbolic-fixture',commit='1111111111111111111111111111111111111111',subdirectory='content'}\n\
     [[brands]]\nid='brand:one'\nname='Explicit symbolic bindings'\n\
     [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/mount/'\nauthored=[{source='source:retained',target='one'}]\n\
     [[brands.publications]]\nsite='site:two'\npublic_base_url='https://two.test/other/'\nauthored=[{source='source:retained',target='two'}]\n"
}

fn compilation(config: &str) -> workspace::Compilation {
    workspace::compile(config, Default::default()).expect("explicit domain and source bindings")
}

fn page(id: &str, site: &str, route: &str, edges: &str, body: &str) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='{site}'\ntitle='{id}'\nlifecycle='published'\nroute={{path='{route}'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n{body}"
    )
}

fn files(body: &str, notes: Option<&str>) -> Vec<(String, String)> {
    let mut files = vec![
        (
            "one/article/index.md".into(),
            page(
                "content:article",
                "site:one",
                "/article/",
                "related=['content:foreign']\n",
                body,
            ),
        ),
        (
            "one/local.md".into(),
            page(
                "content:local",
                "site:one",
                "/local/",
                "",
                "# Local\n\nRetained local target.\n",
            ),
        ),
        (
            "two/foreign.md".into(),
            page(
                "content:foreign",
                "site:two",
                "/foreign/",
                "",
                "# Foreign\n\nRetained remote target.\n",
            ),
        ),
    ];
    if let Some(notes) = notes {
        files.push(("one/article/notes.md".into(), notes.into()));
    }
    files
}

fn parse(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes: bytes.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("actual Comrak parsing, not a symbolic source scanner")
}

fn resolve<'a, 's>(
    compiled: &'a workspace::Compilation,
    corpus: &'a ParsedCorpus<'s>,
) -> ResolvedAuthored<'a, 's> {
    authored::resolve(compiled, corpus, Default::default()).expect("retained publication graph")
}

fn project<'a>(resolved: &'a ResolvedAuthored<'_, '_>) -> authored::PageProjection<'a> {
    resolved
        .project_page(
            &key("content:article"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("bound symbolic references render through the normal page projection")
}

fn assert_failure(
    config: &str,
    body: &str,
    notes: Option<&str>,
    occurrence: &str,
    expected_code: &str,
) {
    let files = files(body, notes);
    let corpus = parse(&files);
    let compiled = compilation(config);
    let resolved = resolve(&compiled, &corpus);
    let error = resolved
        .project_page(
            &key("content:article"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("unproved symbolic reference returns no partial page");
    assert_eq!(error.code(), expected_code, "{occurrence}: {error}");
    let ProjectionError::Document { origin, error } = error else {
        panic!("source-bound Markdown failure, not a synthesized DTO: {error:?}");
    };
    let source_path = if notes.is_some() {
        "one/article/notes.md"
    } else {
        "one/article/index.md"
    };
    assert_eq!(origin.path, path(source_path));
    assert_eq!(
        origin.source_sha256,
        Sha256Digest::of(
            files
                .iter()
                .find(|(name, _)| name == source_path)
                .expect("original input")
                .1
                .as_bytes()
        )
    );
    let document::Error::Markdown { error, .. } = *error else {
        panic!("retained Markdown diagnostic");
    };
    let span = match error {
        markdown::Error::LinkResolution { span, .. } | markdown::Error::UnsafeUrl(span) => span,
        other => panic!("located reference failure: {other:?}"),
    };
    let original_body = notes.unwrap_or(body);
    assert_eq!(
        &original_body[span.start..span.end],
        occurrence,
        "original source construct survives resolution"
    );
}

#[test]
fn authored_pages_and_notes_reject_relative_markdown_files() {
    for target in [
        "../local.md",
        "../../Docs/private.md",
        "../local.MARKDOWN?view=reader#heading",
        "../local%2Emd",
    ] {
        let link = format!("[Private source]({target})");
        assert_failure(
            configuration(),
            &link,
            None,
            &link,
            "markdown_link_resolution",
        );
        assert_failure(
            configuration(),
            "# Article\n",
            Some(&link),
            &link,
            "markdown_link_resolution",
        );
        let unbound = configuration()
            .replace("authored=[{source='source:retained',target='one'}]\n", "")
            .replace("authored=[{source='source:retained',target='two'}]\n", "");
        assert_failure(&unbound, &link, None, &link, "markdown_link_resolution");
    }
}

#[test]
fn one_site_workspace_resolves_a_symbolic_reference_without_a_foreign_inventory() {
    let config = configuration().replace(
        "[[sites]]\nid='site:two'\npublisher='owner:one'\ndeployable=false\n",
        "",
    );
    let config = config
        .split("[[brands.publications]]\nsite='site:two'")
        .next()
        .expect("one declared publication");
    let mut files = files("[[site:one.test/local/#local|Single site]]\n", None);
    files.retain(|(name, _)| name != "two/foreign.md");
    files[0].1 = files[0].1.replace("related=['content:foreign']\n", "");
    let corpus = parse(&files);
    let compiled = compilation(config);
    let resolved = resolve(&compiled, &corpus);
    let page = project(&resolved);
    assert_eq!(page.data.links.len(), 1);
    assert_eq!(
        page.data.links[0].content.as_ref(),
        Some(&key("content:local"))
    );
    assert_eq!(
        page.data.links[0].destination,
        "https://one.test/mount/local/#local"
    );
    assert!(page.body_html.contains("Single site"));
}

#[test]
fn retained_wikilinks_resolve_single_and_cross_site_routes_and_share_public_annotations() {
    let body = "# Article\n\n[[site:one.test/local/#local|Local label]] [[site:two.test/foreign/#foreign|Foreign label]]\n\n`[[site:absent.test/no/|Code]]`\n";
    let files = files(body, None);
    let corpus = parse(&files);
    let retained = corpus.files()[&path("one/article/index.md")]
        .body()
        .expect("retained AST");
    let before = retained.inspect().expect("native link facts");
    assert_eq!(before.links.len(), 2, "code is not a symbolic reference");
    assert_eq!(before.links[0].destination, "site:one.test/local/#local");
    assert_eq!(
        before.links[1].destination,
        "site:two.test/foreign/#foreign"
    );
    assert_eq!(
        &body[before.links[0].span.start..before.links[0].span.end],
        "[[site:one.test/local/#local|Local label]]"
    );
    let compiled = compilation(configuration());
    let resolved = resolve(&compiled, &corpus);
    let page = project(&resolved);
    assert_eq!(page.data.links.len(), 2);
    for (index, id, site, kind, url, label) in [
        (
            0,
            "content:local",
            "site:one",
            LinkDestinationKind::InternalNative,
            "https://one.test/mount/local/#local",
            "Local label",
        ),
        (
            1,
            "content:foreign",
            "site:two",
            LinkDestinationKind::CrossSite,
            "https://two.test/other/foreign/#foreign",
            "Foreign label",
        ),
    ] {
        let link = &page.data.links[index];
        assert_eq!(link.destination, url);
        assert_eq!(link.kind, kind);
        assert_eq!(link.content.as_ref(), Some(&key(id)));
        assert_eq!(link.site.as_ref(), Some(&key(site)));
        assert!(page.body_html.contains(&format!("href=\"{url}\"")));
        assert!(page.body_html.contains(label));
        assert!(
            page.body_html
                .contains(&format!("data-gazette-link=\"{}\"", link.address))
        );
    }
    assert_eq!(
        page.data.links[1].relationships,
        [LinkRelationship::Related]
    );
    assert!(
        page.body_html
            .contains("data-gazette-destination-kind=\"cross-site\"")
    );
    let model = serde_json::to_value(&page).expect("actual human model");
    let search = serde_json::to_value(page.search()).expect("actual machine projection");
    assert_eq!(model["links"], search["links"]);
    assert_eq!(model["content"], search["content"]);
    let public = serde_json::to_string(&model).expect("public JSON");
    for private in [
        "/private/symbolic-fixture",
        "one/article/index.md",
        "two/foreign.md",
    ] {
        assert!(!public.contains(private), "private evidence {private}");
    }
    // The existing heading-navigation contract retains Markdown construct ranges.
    // New public link annotations must not carry their private occurrence evidence.
    assert_no_source_spans(&model["links"]);
    assert!(model.get("source").is_none());
    assert_eq!(retained.inspect().expect("retained AST unchanged"), before);
    assert_eq!(retained.source(), body);
}

#[test]
fn domain_selectors_keep_declared_routes_but_serve_validated_preview_bases_and_prefixes() {
    let files = files(
        "[[site:one.test/local/|Local]] [[site:two.test/foreign/#foreign|Foreign]]\n",
        None,
    );
    let corpus = parse(&files);
    let compiled = compilation(configuration());
    let original_identity = serde_json::to_vec(&compiled).expect("original workspace");
    let public = resolve(&compiled, &corpus);
    let original_route = public
        .content()
        .route(&key("content:foreign"))
        .expect("target route")
        .output
        .clone();
    let bases: BTreeMap<Key, PublicBaseUrl> = [
        (
            key("site:one"),
            "http://127.0.0.1:42001/mount/"
                .parse()
                .expect("validated preview base"),
        ),
        (
            key("site:two"),
            "http://127.0.0.1:42002/other/"
                .parse()
                .expect("validated preview base"),
        ),
    ]
    .into_iter()
    .collect();
    let preview = public
        .with_preview_urls(bases)
        .expect("native preview URL validation");
    let page = project(&preview);
    assert_eq!(
        page.data.links[0].destination,
        "http://127.0.0.1:42001/mount/local/"
    );
    assert_eq!(
        page.data.links[1].destination,
        "http://127.0.0.1:42002/other/foreign/#foreign"
    );
    assert_eq!(
        page.data.links[1].content.as_ref(),
        Some(&key("content:foreign"))
    );
    assert_eq!(
        preview
            .content()
            .route(&key("content:foreign"))
            .expect("same route")
            .output,
        original_route
    );
    assert_eq!(
        serde_json::to_vec(&compiled).expect("workspace unchanged"),
        original_identity
    );
    // A preview listener is a serving address, never another declared domain.
    assert_failure(
        configuration(),
        "[[site:127.0.0.1/local/|Listener]]\n",
        None,
        "[[site:127.0.0.1/local/|Listener]]",
        "markdown_link_resolution",
    );
}

#[test]
fn notes_share_symbolic_resolution_without_becoming_another_body_or_identity() {
    let notes = "# Detail\n\n[[site:two.test/foreign/#foreign|Notes target]]\n";
    let files = files("# Article\n\nContinuous contribution.\n", Some(notes));
    let corpus = parse(&files);
    let compiled = compilation(configuration());
    let resolved = resolve(&compiled, &corpus);
    assert_eq!(resolved.content().graph().nodes().len(), 3);
    let page = project(&resolved);
    let notes = page.data.notes.as_ref().expect("retained companion");
    assert!(page.data.links.is_empty());
    assert!(!page.body_html.contains("Notes target"));
    assert_eq!(notes.links.len(), 1);
    let link = &notes.links[0];
    assert_eq!(link.destination, "https://two.test/other/foreign/#foreign");
    assert_eq!(link.content.as_ref(), Some(&key("content:foreign")));
    assert_eq!(link.kind, LinkDestinationKind::CrossSite);
    assert_eq!(link.relationships, [LinkRelationship::Related]);
    assert!(
        notes
            .body_html
            .contains(&format!("data-gazette-link=\"{}\"", link.address))
    );
    assert!(notes.body_html.contains("Notes target"));
    let model = serde_json::to_value(&page).expect("actual article model");
    let search = serde_json::to_value(page.search()).expect("same owning search document");
    assert_eq!(model["notes"], search["notes"]);
    assert_eq!(search["content"]["id"], "content:article");
}

#[test]
fn symbolic_aliases_and_embedded_notes_anchors_bind_the_same_primary_target() {
    let body = "[[site:two.test/old-foreign.html#foreign|Alias]] [[site:two.test/foreign/#gazette-notes-detail|Embedded notes]] [[site:two.test/foreign/#fn-gazette-notes-note-6e|Note footnote]]\n";
    let mut files = files(body, None);
    files[2].0 = "two/foreign/index.md".into();
    files[2].1 = files[2].1.replace(
        "route={path='/foreign/'}",
        "route={path='/foreign/',aliases=['/old-foreign.html']}",
    );
    files.push((
        "two/foreign/notes.md".into(),
        "# Detail\n\nSupplementary text[^n].\n\n[^n]: Retained note footnote.\n".into(),
    ));
    let corpus = parse(&files);
    let compiled = compilation(configuration());
    let resolved = resolve(&compiled, &corpus);
    let page = project(&resolved);
    assert_eq!(page.data.links.len(), 3);
    for (link, fragment) in page.data.links.iter().zip([
        "#foreign",
        "#gazette-notes-detail",
        "#fn-gazette-notes-note-6e",
    ]) {
        assert_eq!(
            link.destination,
            format!("https://two.test/other/foreign/{fragment}")
        );
        assert_eq!(link.content.as_ref(), Some(&key("content:foreign")));
        assert_eq!(link.kind, LinkDestinationKind::CrossSite);
        assert!(
            page.body_html
                .contains(&format!("href=\"{}\"", link.destination))
        );
    }
    assert!(
        !page
            .body_html
            .contains("href=\"https://two.test/other/old-foreign.html")
    );
    let target = resolved
        .project_page(
            &key("content:foreign"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("target notes use the same retained anchor namespace");
    let notes = target
        .data
        .notes
        .as_ref()
        .expect("embedded target fragment");
    assert!(notes.body_html.contains("id=\"gazette-notes-detail\""));
    assert!(notes.body_html.contains("id=\"fn-gazette-notes-note-6e\""));
    let model = serde_json::to_value(&page).unwrap();
    let search = serde_json::to_value(page.search()).unwrap();
    assert_eq!(model["links"], search["links"]);
    assert_eq!(
        resolved.content().graph().nodes().len(),
        3,
        "notes do not become another symbolic target identity"
    );
}

#[test]
fn unknown_and_ambiguous_configured_domains_cannot_choose_a_target() {
    let unknown = "[[site:absent.test/foreign/|Unknown]]";
    assert_failure(
        configuration(),
        &format!("नमस्ते\r\n\r\n{unknown}\r\n"),
        None,
        unknown,
        "markdown_link_resolution",
    );
    let ambiguous = configuration().replace("https://two.test/other/", "https://one.test/other/");
    let occurrence = "[[site:one.test/local/|Ambiguous]]";
    assert_failure(
        &ambiguous,
        occurrence,
        None,
        occurrence,
        "markdown_link_resolution",
    );
}

#[test]
fn exact_routes_and_retained_fragments_are_required_in_primary_and_notes_sources() {
    for occurrence in [
        "[[site:two.test/missing/|No route]]",
        "[[site:two.test/foreign|Not the exact route]]",
        "[[site:two.test/other/foreign/|Prefix is not the route]]",
        "[[site:two.test/foreign/#missing|No fragment]]",
        "[[site:two.test/foreign/#bad%|Malformed fragment]]",
    ] {
        assert_failure(
            configuration(),
            occurrence,
            None,
            occurrence,
            "markdown_link_resolution",
        );
        assert_failure(
            configuration(),
            "# Article\n",
            Some(occurrence),
            occurrence,
            "markdown_link_resolution",
        );
    }
}

#[test]
fn malformed_selectors_queries_and_traversal_are_not_normalized_into_real_routes() {
    for occurrence in [
        "[[site://two.test/foreign/|Authority syntax]]",
        "[[site:two.test|No route]]",
        "[[site:two.test/foreign/?mode=1|Query]]",
        "[[site:two.test/foreign/../foreign/|Traversal]]",
        "[[site:two.test/%2e%2e/foreign/|Encoded traversal]]",
        "[[site:two.test/foreign%2F|Encoded separator]]",
        "[[https://two.test/other/foreign/|Unsupported WikiLink scheme]]",
    ] {
        assert_failure(
            configuration(),
            occurrence,
            None,
            occurrence,
            "markdown_link_resolution",
        );
    }
}

#[test]
fn symbolic_links_in_main_or_notes_cannot_bypass_excluded_target_selection() {
    let occurrence = "[[site:two.test/foreign/#foreign|Excluded]]";
    for in_notes in [false, true] {
        let files = files(
            if in_notes { "# Article\n" } else { occurrence },
            in_notes.then_some(occurrence),
        );
        let corpus = parse(&files);
        let compiled = compilation(configuration());
        let resolved = resolve(&compiled, &corpus);
        let mut selection = Selection {
            content: resolved
                .content()
                .graph()
                .nodes()
                .map(|node| (node.id.clone(), Disposition::Include))
                .collect(),
            assets: BTreeMap::new(),
        };
        let selected = resolve(&compiled, &corpus)
            .into_selected(&selection)
            .expect("complete candidate uses the same symbolic resolver");
        assert_eq!(
            project(&selected).data.content.node.id,
            key("content:article")
        );
        selection
            .content
            .insert(key("content:foreign"), Disposition::Exclude);
        let error = resolved
            .into_selected(&selection)
            .expect_err("no dangling target, including companion links");
        assert!(
            matches!(error, SelectionError::Link { ref content, ref target, .. } if content == &key("content:article") && target == &key("content:foreign")),
            "{error:?}"
        );
    }
}

#[test]
fn ordinary_markdown_site_scheme_stays_unsafe_and_malformed_wikilink_text_is_not_scanned() {
    let ordinary = "[Ordinary](site:two.test/foreign/)";
    assert_failure(
        configuration(),
        ordinary,
        None,
        ordinary,
        "markdown_unsafe_url",
    );
    let literal = "[[site:absent.test/missing/|Unclosed";
    let files = files(literal, None);
    let corpus = parse(&files);
    assert!(
        corpus.files()[&path("one/article/index.md")]
            .body()
            .unwrap()
            .inspect()
            .unwrap()
            .links
            .is_empty()
    );
    let compiled = compilation(configuration());
    let resolved = resolve(&compiled, &corpus);
    assert!(
        project(&resolved).body_html.contains(literal),
        "only actual parser-recognized constructs are symbolic"
    );
}

#[test]
fn unused_footnote_symbolic_failures_are_checked_before_normalized_output() {
    let occurrence = "[[site:absent.test/foreign/|Unused target]]";
    let body = format!("Visible body.\n\n[^unused]: {occurrence}\n");
    let files = files(&body, None);
    let corpus = parse(&files);
    let inspection = corpus.files()[&path("one/article/index.md")]
        .body()
        .unwrap()
        .inspect()
        .expect("whole-source facts");
    assert_eq!(inspection.links.len(), 1);
    assert!(
        !inspection.links[0].emitted,
        "unused definition is retained, not rendered"
    );
    assert_failure(
        configuration(),
        &body,
        None,
        occurrence,
        "markdown_link_resolution",
    );
    let good_files = self::files(
        "Visible body.\n\n[^unused]: [[site:two.test/foreign/#foreign|Retained unused target]]\n",
        None,
    );
    let good_corpus = parse(&good_files);
    let compiled = compilation(configuration());
    let resolved = resolve(&compiled, &good_corpus);
    let page = project(&resolved);
    assert!(!page.body_html.contains("Retained unused target"));
    assert!(
        page.data.links.is_empty(),
        "non-emitted source evidence is not a public annotation"
    );
    assert_eq!(
        serde_json::to_value(page.search()).unwrap()["content"]["id"],
        json!("content:article")
    );
}
