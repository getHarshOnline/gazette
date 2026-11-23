//! Local authored views consume the shared graph and retained document policy.

use std::error::Error as _;

use gazette_compiler::{
    authored::{self, ContentView, ProjectionError},
    corpus::{self, Input, ParsedCorpus},
    document, markdown, toml, workspace,
};
use gazette_core::{
    publication::{
        ContentLifecycle, Key,
        content::{self, ProjectRelation},
    },
    source::Sha256Digest,
};
use serde_json::json;

#[path = "authored_projection/json_ld.rs"]
mod json_ld;

#[path = "authored_projection/article_metadata.rs"]
mod article_metadata;
#[path = "authored_projection/editorial.rs"]
mod editorial;
#[path = "authored_projection/license.rs"]
mod license;
#[path = "authored_projection/navigation.rs"]
mod navigation;
#[path = "authored_projection/site_chrome.rs"]
mod site_chrome;
#[path = "authored_projection/summary.rs"]
mod summary;

fn key(value: &str) -> Key {
    value.parse().expect("key")
}

fn compilation() -> workspace::Compilation {
    workspace::compile("[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=true\n[[projects]]\nid='project:p'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n[[projects]]\nid='project:q'\nlifecycle='planned'\nvisibility='public'\navailability='gated'\n[[sources]]\nid='source:private'\nlocation={kind='repository',root='/private/operator-only',commit='0000000000000000000000000000000000000000',subdirectory='secret'}\n", toml::Limits::default()).expect("workspace")
}

fn doc(id: &str, site: &str, state: &str, edges: &str, structure: &str, body: &str) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='{site}'\ntitle='Same title'\nlifecycle='{state}'\n[content.structure]\n{structure}\n+++\n{body}"
    )
}

fn fixtures() -> Vec<(&'static str, String)> {
    vec![
        (
            "root.md",
            doc(
                "collection:root",
                "site:a",
                "published",
                "projects=[{project='project:p',kind='context'}]\n",
                "kind='collection'",
                "# Root\n",
            ),
        ),
        (
            "child.md",
            doc(
                "collection:child",
                "site:a",
                "published",
                "",
                "kind='collection'\nparent='collection:root'",
                "# Child\n",
            ),
        ),
        (
            "journey.md",
            doc(
                "journey:z",
                "site:a",
                "archived",
                "projects=[{project='project:q',kind='context'}]\n",
                "kind='journey'\ncollection='collection:child'\nmembers=[{content='docs:local',primary=true},{content='article:a',primary=true}]",
                "# Journey\n",
            ),
        ),
        (
            "standalone.md",
            doc(
                "journey:a",
                "site:a",
                "draft",
                "",
                "kind='journey'\ncollection='collection:root'\nmembers=[{content='article:single',primary=true}]",
                "Standalone\n",
            ),
        ),
        (
            "article.md",
            doc(
                "article:a",
                "site:a",
                "draft",
                "related=['docs:b']\nprojects=[{project='project:p',kind='about'}]\n",
                "kind='page'\nfamily={kind='article'}",
                "# नमस्ते\n\nBody **strong**.\n",
            ),
        ),
        (
            "docs.md",
            doc(
                "docs:b",
                "site:b",
                "archived",
                "",
                "kind='page'\nfamily={kind='documentation'}",
                "# Docs\n",
            ),
        ),
        (
            "empty.md",
            doc(
                "article:empty",
                "site:b",
                "published",
                "related=['article:a']\n",
                "kind='page'\nfamily={kind='article'}",
                "Empty\n",
            ),
        ),
        (
            "local.md",
            doc(
                "docs:local",
                "site:a",
                "archived",
                "",
                "kind='page'\nfamily={kind='documentation'}",
                "# Local docs\n",
            ),
        ),
        (
            "single.md",
            doc(
                "article:single",
                "site:a",
                "published",
                "",
                "kind='page'\nfamily={kind='article'}",
                "Single\n",
            ),
        ),
    ]
}

fn parse<'a>(files: &'a [(&str, String)], limits: corpus::Limits) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().expect("path"),
            bytes: bytes.as_bytes(),
        }),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained syntax")
}

fn ids(nodes: &[ContentView<'_>]) -> Vec<Key> {
    nodes.iter().map(|view| view.node.id.clone()).collect()
}

#[test]
fn site_chrome_resolves_ordered_cross_site_and_no_script_page_destinations() {
    let workspace = workspace::compile(
        "[composition]\n\
         navigation=[{label='Remote docs',destination='content:remote'}]\n\
         [composition.chrome]\n\
         skip_to_content_label='Skip'\n\
         navigation_label='Primary'\n\
         footer_label='Footer'\n\
         [composition.chrome.regions]\n\
         'region:preface'='A deliberate opening note'\n\
         [composition.chrome.shell]\nclose_label='Close'\n\
         [composition.chrome.shell.search]\n\
         trigger_label='Search'\ntitle='Search'\nfield_label='Query'\n\
         loading_message='Loading'\nempty_message='Empty'\nunavailable_message='Unavailable'\n\
         no_script_destination='content:search'\n\
         [composition.chrome.shell.settings]\n\
         trigger_label='Settings'\ntitle='Settings'\ncolor_mode_label='Color'\n\
         color_mode_system_label='System'\ncolor_mode_light_label='Light'\ncolor_mode_dark_label='Dark'\n\
         motion_label='Motion'\nmotion_system_label='System'\nmotion_reduce_label='Reduce'\nmotion_allow_label='Allow'\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n\
         [[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='One'\n\
         [[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/'\n\
         [[brands.publications]]\nsite='site:b'\npublic_base_url='https://b.test/'\n",
        toml::Limits::default(),
    )
    .expect("workspace chrome");
    let remote = "+++\n[content]\nid='content:remote'\nsite='site:b'\ntitle='Remote'\nlifecycle='published'\nroute={path='/remote/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\nRemote\n".to_owned();
    let search = "+++\n[content]\nid='content:search'\nsite='site:a'\ntitle='Search'\nlifecycle='published'\nroute={path='/search/'}\nstructure={kind='page',family={kind='article'}}\n+++\nSearch\n".to_owned();
    let files = [("remote.md", remote), ("search.md", search)];
    let corpus = parse(&files, corpus::Limits::default());
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("resolved authored corpus");
    let chrome = resolved
        .site_chrome(&key("site:a"))
        .expect("valid chrome destinations")
        .expect("selected chrome");
    assert_eq!(chrome.navigation[0].label, "Remote docs");
    assert_eq!(
        chrome
            .regions
            .get(&key("region:preface"))
            .map(String::as_str),
        Some("A deliberate opening note")
    );
    assert_eq!(chrome.navigation[0].destination.content.site, key("site:b"));
    assert_eq!(
        chrome.navigation[0].destination.route.canonical_url,
        "https://b.test/remote/"
    );
    let fallback = chrome
        .shell
        .search
        .as_ref()
        .and_then(|search| search.no_script_destination.as_ref())
        .expect("resolved no-script page");
    assert_eq!(fallback.content.id, key("content:search"));
    assert_eq!(fallback.route.canonical_url, "https://a.test/search/");
    assert!(
        resolved
            .site_chrome(&key("site:missing"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn leaf_projection_borrows_exact_graph_catalog_and_explicit_typed_relationships() {
    let files = fixtures();
    let corpus = parse(&files, corpus::Limits::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default())
        .expect("resolved corpus");
    let view = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("local draft view");
    assert!(std::ptr::eq(
        view.data.content.node,
        resolved
            .content()
            .graph()
            .node(&key("article:a"))
            .expect("node")
    ));
    assert_eq!(view.data.content.lifecycle, ContentLifecycle::Draft);
    assert_eq!(view.data.site.id, &key("site:a"));
    assert_eq!(view.data.site.publisher, &key("owner:one"));
    assert_eq!(view.data.source.path, "article.md".parse().expect("path"));
    assert_eq!(
        view.data.source.source_sha256,
        Sha256Digest::of(files[4].1.as_bytes())
    );
    assert_eq!(view.data.source.span, None);
    assert_eq!(
        view.body_html,
        "<h1 id=\"nmste\">नमस्ते</h1>\n<p>Body <strong>strong</strong>.</p>\n"
    );
    assert!(view.data.collections.is_empty());
    assert_eq!(
        view.data
            .journeys
            .iter()
            .map(|membership| (&membership.journey.id, membership.primary))
            .collect::<Vec<_>>(),
        [(&key("journey:z"), true)]
    );
    assert_eq!(ids(&view.data.related), [key("docs:b")]);
    assert!(view.data.steps.is_empty());
    assert!(view.data.children.is_empty());
    assert_eq!(view.data.projects.len(), 2);
    assert_eq!(view.data.projects[0].project.id, key("project:p"));
    assert_eq!(
        view.data.projects[0].relationship.relation(),
        ProjectRelation::About
    );
    assert_eq!(view.data.projects[0].relationship.evidence().len(), 2);
    assert_eq!(
        view.data.projects[1].relationship.relation(),
        ProjectRelation::Mentions
    );
    for project in &view.data.projects {
        assert!(std::ptr::eq(
            project.project,
            resolved
                .content()
                .project(&project.project.id)
                .expect("joined catalog project")
        ));
        assert!(std::ptr::eq(
            project.relationship,
            &resolved
                .content()
                .graph()
                .project_relations(&key("article:a"))
                .expect("links")[&project.project.id]
        ));
    }
    let json = serde_json::to_value(&view).expect("serialization");
    assert_eq!(
        json["site"],
        json!({"id":"site:a","publisher":"owner:one","publisher_principal":{"id":"owner:one","kind":"person"},"roles":[]})
    );
    assert!(
        json.get("source").is_none(),
        "source evidence stays outside the template model"
    );
    assert_eq!(json["projects"][0]["project"]["visibility"], "private");
    let encoded = serde_json::to_string(&view).expect("serialization");
    for private in [
        "operator-only",
        "source:private",
        "source_location",
        "deployable",
        "canonical_url",
        "authors",
        "rights",
    ] {
        assert!(
            !encoded.contains(private),
            "leaked or invented field {private}"
        );
    }
}

#[test]
fn collection_children_and_journey_steps_keep_structure_order_and_related_foreign_states() {
    let mut files = fixtures();
    files[0].1 = files[0].1.replace(
        "kind='collection'",
        "kind='collection'\nmember_order=['journey:a']",
    );
    let corpus = parse(&files, corpus::Limits::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default())
        .expect("resolved corpus");
    let root = resolved
        .project_page(
            &key("collection:root"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("collection");
    assert_eq!(
        ids(&root.data.children),
        [key("journey:a"), key("collection:child")]
    );
    assert!(
        root.data
            .children
            .iter()
            .all(|child| child.resolved_route.is_none()),
        "unrouted child remains in its structural position without an invented address"
    );
    assert!(root.data.collections.is_empty());
    assert!(root.data.projects.is_empty());
    assert_eq!(root.data.project_declarations.len(), 1);
    assert_eq!(
        root.data.project_declarations[0].project.id,
        key("project:p")
    );
    assert_eq!(
        root.data.project_declarations[0].declaration.kind,
        content::ProjectDeclarationKind::Context
    );
    let child = resolved
        .project_page(
            &key("collection:child"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("child");
    assert_eq!(ids(&child.data.collections), [key("collection:root")]);
    assert_eq!(ids(&child.data.children), [key("journey:z")]);
    let journey = resolved
        .project_page(
            &key("journey:z"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("journey");
    assert_eq!(ids(&journey.data.collections), [key("collection:child")]);
    assert_eq!(
        ids(&journey.data.steps),
        [key("docs:local"), key("article:a")]
    );
    assert_eq!(
        journey.data.steps[0].node.lifecycle,
        ContentLifecycle::Archived
    );
    assert_eq!(
        journey.data.steps[1].node.lifecycle,
        ContentLifecycle::Draft
    );
    assert_eq!(journey.data.steps[0].node.site, key("site:a"));
    assert_eq!(journey.data.steps[1].node.site, key("site:a"));
    assert_eq!(
        journey.data.steps[0].node.title,
        journey.data.steps[1].node.title
    );
    assert_ne!(journey.data.steps[0].node.id, journey.data.steps[1].node.id);
    assert!(journey.data.projects.is_empty());
    assert!(journey.data.journeys.is_empty());
    let docs = resolved
        .project_page(
            &key("docs:local"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("docs");
    assert!(
        docs.data.collections.is_empty(),
        "journey collection is not leaf membership"
    );
    assert!(
        docs.data.related.is_empty(),
        "editorial links are outgoing only"
    );
    assert_eq!(docs.data.projects.len(), 2);
    assert!(docs.data.project_declarations.is_empty());
    assert!(
        docs.data
            .projects
            .iter()
            .all(|project| project.relationship.relation() == ProjectRelation::Mentions)
    );
    let empty = resolved
        .project_page(
            &key("article:empty"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("unconnected page");
    assert!(
        empty.data.projects.is_empty(),
        "editorial proximity cannot inherit projects"
    );
    let standalone = resolved
        .project_page(
            &key("journey:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("singleton journey");
    assert_eq!(ids(&standalone.data.collections), [key("collection:root")]);
    assert_eq!(ids(&standalone.data.steps), [key("article:single")]);
    let foreign = resolved
        .project_page(
            &key("docs:b"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("related foreign page");
    assert_eq!(foreign.data.site.id, &key("site:b"));
    assert!(foreign.data.projects.is_empty());
    assert!(foreign.data.journeys.is_empty());
}

#[test]
fn unrelated_corpus_order_does_not_change_any_projected_output() {
    let files = fixtures();
    let mut reordered = fixtures();
    reordered.reverse();
    let first = parse(&files, corpus::Limits::default());
    let second = parse(&reordered, corpus::Limits::default());
    let compilation = compilation();
    let first = authored::resolve(&compilation, &first, content::Limits::default()).expect("first");
    let second =
        authored::resolve(&compilation, &second, content::Limits::default()).expect("reordered");
    for node in first.content().graph().nodes() {
        assert_eq!(
            serde_json::to_value(
                first
                    .project_page(
                        &node.id,
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak
                    )
                    .expect("first view")
            )
            .expect("json"),
            serde_json::to_value(
                second
                    .project_page(
                        &node.id,
                        &gazette_core::raster::unavailable,
                        &gazette_adapter_markdown::Comrak
                    )
                    .expect("second view")
            )
            .expect("json")
        );
    }
}

#[test]
fn raw_html_render_failure_retains_original_error_and_complete_document_origin() {
    let raw = doc(
        "article:raw",
        "site:a",
        "published",
        "",
        "kind='page'\nfamily={kind='article'}",
        "<script>unsafe()</script>\n",
    )
    .replace('\n', "\r\n");
    let files = vec![("unicode/अ.md", raw)];
    let corpus = parse(&files, corpus::Limits::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default())
        .expect("raw HTML is syntactically valid");
    let error = resolved
        .project_page(
            &key("article:raw"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("no trusted raw HTML inferred");
    assert_eq!(error.code(), "markdown_raw_html_unavailable");
    assert!(error.source().is_some());
    let ProjectionError::Document { origin, error } = error else {
        panic!("original rendering error lost")
    };
    let document::Error::Markdown {
        error: markdown::Error::RawHtml(span),
        offset,
    } = *error
    else {
        panic!("original document error lost")
    };
    assert_eq!(origin.path, "unicode/अ.md".parse().expect("path"));
    assert_eq!(origin.span, None);
    assert_eq!(
        origin.source_sha256,
        Sha256Digest::of(files[0].1.as_bytes())
    );
    assert_eq!(
        &files[0].1[offset + span.start..offset + span.end],
        "<script>unsafe()</script>"
    );
}

#[test]
fn unknown_identity_and_original_output_limit_never_return_partial_views() {
    let files = fixtures();
    let compilation = compilation();
    let mut limits = corpus::Limits::default();
    limits.document.markdown.output_bytes = 1;
    let corpus = parse(&files, limits);
    let resolved = authored::resolve(&compilation, &corpus, content::Limits::default())
        .expect("resolution independent of output budget");
    let error = resolved
        .project_page(
            &key("article:missing"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("exact lookup only");
    assert!(matches!(error,ProjectionError::UnknownContent(id) if id==key("article:missing")));
    let error = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("retained output ceiling");
    assert_eq!(error.code(), "markdown_limit");
    let ProjectionError::Document { error, .. } = error else {
        panic!("document error lost")
    };
    assert!(matches!(
        *error,
        document::Error::Markdown {
            error: markdown::Error::Limit(_),
            ..
        }
    ));
}

#[test]
fn search_borrows_complete_page_metadata_without_html_or_private_source_coordinates() {
    let config = "[[principals]]\nid='owner:one'\nkind='person'\n\
        [[principals]]\nid='principal:writer'\nkind='person'\n\
        [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n\
        [[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n\
        [[projects]]\nid='project:p'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
        [[projects]]\nid='project:q'\nlifecycle='planned'\nvisibility='public'\navailability='gated'\n\
        [[sources]]\nid='source:private'\nlocation={kind='repository',root='/private/search-proof',commit='1111111111111111111111111111111111111111',subdirectory='secret'}\n\
        [[brands]]\nid='brand:one'\nname='Explicit brand'\n\
        [[brands.publications]]\nsite='site:a'\nauthored=[{source='source:private',target='home'}]\npublic_base_url='https://one.test/mount/'\n\
        [[taxonomies]]\nid='taxonomy:topic'\nlabel='Topic'\nlanguage='EN-us'\nterms=[{id='term:chosen',label='Chosen label'},{id='term:unused',label='Unrelated secret'}]\n";
    let compilation = workspace::compile(config, Default::default()).expect("real metadata owners");
    let mut files = fixtures();
    files[4].0 = "home/article.md";
    files[4].1 = files[4].1.replace(
        "lifecycle='draft'",
        "lifecycle='draft'\nlanguage='en-US'\ntaxonomies=[{taxonomy='taxonomy:topic',term='term:chosen'}]\nattributions=[{principal='principal:writer',role='author'}]\nroute={path='/article/',aliases=['/old/']}",
    );
    let corpus = parse(&files, Default::default());
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("one semantic graph");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("real page projection");
    let search = serde_json::to_value(page.search()).expect("search serialization");
    let mut metadata = serde_json::to_value(&page).expect("page serialization");
    let object = metadata.as_object_mut().expect("flattened page object");
    assert!(object.remove("body_html").is_some());
    assert!(object.remove("body_text").is_some());
    assert!(object.remove("opening").is_some());
    assert!(
        search.get("opening").is_none(),
        "search must not carry HTML header fragments"
    );
    object.insert("body".into(), json!(page.body_text));
    assert_eq!(
        search, metadata,
        "every metadata field is shared, not manually copied"
    );
    assert_eq!(search["body"], "नमस्ते\nBody strong.\n");
    assert_eq!(search["content"]["lifecycle"], "draft");
    assert_eq!(search["content"]["language"], "en-US");
    assert_eq!(
        search["classifications"][0]["taxonomy"]["language"],
        "en-US"
    );
    assert_eq!(
        search["classifications"][0]["term"],
        json!({"id":"term:chosen","label":"Chosen label"})
    );
    assert_eq!(
        search["attributions"][0]["principal"]["id"],
        "principal:writer"
    );
    assert_eq!(search["attributions"][0]["role"], "author");
    assert_eq!(search["site"]["publisher"], "owner:one");
    assert_eq!(
        search["route"]["canonical_url"],
        "https://one.test/mount/article/"
    );
    assert_eq!(search["provenance"], json!({"site":"site:a"}));
    assert_eq!(search["projects"][0]["project"]["lifecycle"], "done");
    assert_eq!(search["projects"][0]["project"]["visibility"], "private");
    assert_eq!(
        search["projects"][0]["project"]["availability"],
        "unavailable"
    );
    assert_eq!(
        page.data.projects[0].relationship.relation(),
        ProjectRelation::About
    );
    assert_eq!(
        page.data.projects[1].relationship.relation(),
        ProjectRelation::Mentions
    );
    assert_eq!(search["collections"], json!([]));
    assert_eq!(search["journeys"][0]["journey"]["id"], "journey:z");
    assert_eq!(search["journeys"][0]["primary"], true);
    assert_eq!(search["related"][0]["lifecycle"], "archived");
    assert_eq!(
        search["project_declarations"][0]["declaration"]["kind"],
        "about"
    );
    let encoded = search.to_string();
    for private in [
        "body_html",
        "body_text",
        "home/article.md",
        "/private/search-proof",
        "source:private",
        "Unrelated secret",
        "<strong>",
    ] {
        assert!(
            !encoded.contains(private),
            "unexpected search field or private data: {private}"
        );
    }
    assert_eq!(
        serde_json::to_value(page.search()).expect("repeat borrowed projection"),
        search
    );
}
