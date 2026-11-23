//! Article companions use retained Markdown and the owning page's publication model.

use gazette_compiler::{
    authored::{
        self, PageCompanionKind, ProjectionError, ResolvedAuthored, Selection, SelectionError,
    },
    corpus::{self, File, Input, ParsedCorpus},
    document, markdown, workspace,
};
use gazette_core::{
    publication::{
        Key,
        content::{self, Disposition},
    },
    source::{EntryPath, Sha256Digest},
};

fn key(value: &str) -> Key {
    value.parse().expect("exact fixture identity")
}

fn path(value: &str) -> EntryPath {
    value.parse().expect("retained logical path")
}

fn compilation() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:one'\nlocation={kind='repository',root='/private/notes-fixture',commit='1111111111111111111111111111111111111111',subdirectory='content'}\n\
         [[brands]]\nid='brand:one'\nname='Notes fixture'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n\
         authored=[{source='source:one',target='authored/one',static_assets={route_prefix='/media/'}}]\n",
        Default::default(),
    ).expect("one explicit authored source and publication")
}

fn page(id: &str, route: &str, family: &str, body: &str) -> Vec<u8> {
    format!(
        "+++\n[content]\nid='{id}'\nsite='site:one'\ntitle='Article'\n\
         lifecycle='published'\nroute={{path='{route}'}}\n\
         structure={{kind='page',family={{kind='{family}'}}}}\n+++\n{body}"
    )
    .into_bytes()
}

fn files(notes: &str) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "authored/one/article/index.md".into(),
            page(
                "content:article",
                "/article/",
                "article",
                "# Shared\n\nMain contribution[^n].\n\n[^n]: Main footnote.\n",
            ),
        ),
        (
            "authored/one/article/notes.md".into(),
            notes.as_bytes().to_vec(),
        ),
        (
            "authored/one/other.md".into(),
            page(
                "content:other",
                "/other/",
                "article",
                "# Other\n\nOther contribution.\n",
            ),
        ),
    ]
}

fn parse(files: &[(String, Vec<u8>)], limits: corpus::Limits) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
    .expect("complete retained article and notes corpus")
}

fn resolve<'a, 's>(
    compilation: &'a workspace::Compilation,
    corpus: &'a ParsedCorpus<'s>,
) -> ResolvedAuthored<'a, 's> {
    authored::resolve(compilation, corpus, content::Limits::default())
        .expect("notes belong to their article without an invented page")
}

fn selection(resolved: &ResolvedAuthored<'_, '_>, disposition: Disposition) -> Selection {
    Selection {
        content: resolved
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), disposition))
            .collect(),
        assets: resolved
            .corpus()
            .files()
            .iter()
            .filter(|(_, file)| matches!(file, File::Asset(_) | File::Notebook(_)))
            .map(|(path, _)| (path.clone(), disposition))
            .collect(),
    }
}

#[test]
fn capture_root_article_bundle_keeps_notes_one_identity_and_bare_resource_inputs() {
    let notes_source = "# Detail\n\nRoot supplementary **detail**. [local](#detail)\n";
    for with_program in [false, true] {
        let mut files = vec![
            (
                "authored/one/index.md".into(),
                page(
                    "content:root",
                    "/deliberate/",
                    "article",
                    "# Article\n\nContinuous root contribution.\n",
                ),
            ),
            (
                "authored/one/notes.md".into(),
                notes_source.as_bytes().to_vec(),
            ),
        ];
        if with_program {
            files.push((
                "authored/one/index.ts".into(),
                b"import './script/dependency.ts';".to_vec(),
            ));
            files.push((
                "authored/one/script/dependency.ts".into(),
                b"export const retained = 42;".to_vec(),
            ));
        }
        let corpus = parse(&files, Default::default());
        let compilation = compilation();
        let resolved = resolve(&compilation, &corpus);
        let id = key("content:root");
        assert_eq!(
            resolved.content().graph().nodes().len(),
            1,
            "notes and resources do not manufacture identities"
        );
        let package = resolved
            .page_package(&id)
            .expect("selected capture root is an article package");
        assert_eq!(package.root(), None);
        assert_eq!(package.document(), &path("authored/one/index.md"));
        let notes = package
            .notes()
            .expect("root companion is not silently suppressed");
        assert_eq!(notes.path(), &path("authored/one/notes.md"));
        assert_eq!(notes.relative(), &path("notes.md"));
        assert_eq!(
            notes.source_sha256(),
            Some(Sha256Digest::of(notes_source.as_bytes()))
        );
        let resources = resolved.resources(&id);
        assert_eq!(resources.len(), usize::from(with_program));
        if with_program {
            assert_eq!(resources[0].build.entry(), &path("index.ts"));
            assert_eq!(
                resources[0].inputs,
                [path("index.ts"), path("script/dependency.ts")]
            );
            assert_eq!(resources[0].output.as_str(), "/deliberate/index.js");
        }
        let route = resolved
            .content()
            .route(&id)
            .expect("authored route is retained");
        assert_eq!(route.canonical_url, "https://example.test/deliberate/");
        assert_eq!(route.output, path("deliberate/index.html"));
        let projection = resolved
            .project_page(
                &id,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("real retained root bundle projection");
        assert!(
            projection
                .body_html
                .contains("Continuous root contribution.")
        );
        assert!(!projection.body_html.contains("Root supplementary"));
        let notes = projection
            .data
            .notes
            .as_ref()
            .expect("root notes reach the public model");
        assert!(
            notes
                .body_html
                .contains("Root supplementary <strong>detail</strong>")
        );
        assert!(notes.body_html.contains("id=\"gazette-notes-detail\""));
        assert_eq!(notes.source().unwrap().path, path("authored/one/notes.md"));
        let model = serde_json::to_value(&projection).expect("normal page model");
        let opening = projection.opening().expect("leading article H1");
        assert_eq!(
            [opening.heading_html, opening.remainder_html].concat(),
            projection.body_html
        );
        assert!(opening.heading_html.contains("id=\"article\""));
        assert!(!opening.remainder_html.contains("Root supplementary"));
        assert_eq!(model["opening"]["heading_html"], opening.heading_html);
        assert!(
            model["notes"]["body_html"]
                .as_str()
                .unwrap()
                .contains("gazette-notes-detail")
        );
        let search =
            serde_json::to_value(projection.search()).expect("sole owning search document");
        assert_eq!(search["content"]["id"], "content:root");
        assert_eq!(search["notes"], model["notes"]);
        assert_eq!(search["body"], projection.body_text);
        let public = serde_json::to_string(&model).unwrap();
        for private in [
            "authored/one/index.md",
            "authored/one/notes.md",
            "/private/notes-fixture",
        ] {
            assert!(
                !public.contains(private),
                "no private capture coordinate {private}"
            );
        }
    }
}

#[test]
fn capture_root_missing_note_anchor_reports_the_retained_companion_instead_of_ignoring_it() {
    let notes_source = "# Detail\n\n[broken](#missing)\n";
    let files = vec![
        (
            "authored/one/index.md".into(),
            page("content:root", "/deliberate/", "article", "# Article\n"),
        ),
        (
            "authored/one/notes.md".into(),
            notes_source.as_bytes().to_vec(),
        ),
    ];
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let error = resolved
        .project_page(
            &key("content:root"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("capture-root notes are compiled, not silently dropped");
    assert_eq!(error.code(), "markdown_link_resolution");
    let ProjectionError::Document { origin, error } = error else {
        panic!("original notes diagnostic");
    };
    assert_eq!(origin.path, path("authored/one/notes.md"));
    assert_eq!(
        origin.source_sha256,
        Sha256Digest::of(notes_source.as_bytes())
    );
    let document::Error::Markdown {
        error: markdown::Error::LinkResolution { span, .. },
        ..
    } = *error
    else {
        panic!("located missing-note-anchor error");
    };
    assert_eq!(&notes_source[span.start..span.end], "[broken](#missing)");
}

#[test]
fn rich_notes_keep_the_article_identity_and_their_own_rendered_namespace_and_provenance() {
    let notes_source = "# Shared\n\nSupplementary **detail**[^n].\n\n[local](#shared) [other](https://example.test/other/#other)\n\n![Diagram](diagram.png)\n\n| Claim | Evidence |\n| --- | --- |\n| Notes | Retained |\n\n```rust\nlet retained = 42;\n```\n\n[^n]: Notes footnote.\n";
    let mut files = files(notes_source);
    files.push((
        "authored/one/article/diagram.png".into(),
        b"retained opaque PNG source".to_vec(),
    ));
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:article");
    assert_eq!(resolved.content().graph().nodes().len(), 2);
    assert_eq!(
        resolved.source_path(&id),
        Some(&path("authored/one/article/index.md"))
    );
    let package = resolved.page_package(&id).expect("article package");
    let companion = package
        .companions()
        .iter()
        .find(|companion| companion.kind() == PageCompanionKind::Notes)
        .expect("plain notes are typed companions");
    assert_eq!(companion.path(), &path("authored/one/article/notes.md"));
    assert_eq!(companion.relative(), &path("notes.md"));
    assert_eq!(
        companion.source_sha256(),
        Some(Sha256Digest::of(notes_source.as_bytes()))
    );
    let projection = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("the rich notes renderer retains native Markdown features");
    let notes = projection
        .data
        .notes
        .as_ref()
        .expect("separate supplementary projection");
    assert_eq!(
        notes.source().unwrap().path,
        path("authored/one/article/notes.md")
    );
    assert_eq!(notes.source().unwrap().span, None);
    assert_eq!(
        notes.source().unwrap().source_sha256,
        Sha256Digest::of(notes_source.as_bytes())
    );
    assert_eq!(notes.source_sha256, companion.source_sha256());
    assert_eq!(
        projection.data.source.source_sha256,
        Sha256Digest::of(&files[0].1)
    );
    assert!(projection.body_html.contains("id=\"shared\""));
    assert!(projection.body_html.contains("id=\"fn-note-6e\""));
    assert!(!projection.body_html.contains("Supplementary"));
    assert!(!projection.body_text.contains("Supplementary"));
    assert_eq!(projection.data.headings[0].id, "shared");
    assert_eq!(notes.headings[0].id, "gazette-notes-shared");
    assert_eq!(
        notes.headings[0].level, 2,
        "supplementary H1 is below the article heading"
    );
    assert_eq!(
        corpus.files()[&path("authored/one/article/notes.md")]
            .body()
            .unwrap()
            .headings()
            .unwrap()[0]
            .level,
        1,
        "the retained notes syntax is unchanged"
    );
    assert_eq!(notes.headings[0].fragment, "#gazette-notes-shared");
    assert!(notes.body_html.contains("id=\"gazette-notes-shared\""));
    assert!(notes.body_html.contains("id=\"fn-gazette-notes-note-6e\""));
    assert!(
        notes
            .body_html
            .contains("href=\"#fn-gazette-notes-note-6e\"")
    );
    assert!(notes.body_html.contains("<strong>detail</strong>"));
    assert!(notes.body_html.contains("<table>"));
    assert!(notes.body_html.contains("language-rust"));
    assert!(
        notes
            .body_html
            .contains("https://example.test/article/diagram.png")
    );
    assert!(notes.body_text.contains("Notes footnote."));
    assert!(notes.body_text.contains("let retained = 42;"));
    assert!(notes.components.is_empty());
    assert!(
        notes
            .links
            .iter()
            .any(|link| link.destination.ends_with("#gazette-notes-shared"))
    );
    assert!(notes.links.iter().any(
        |link| link.destination == "https://example.test/other/#other"
            && link.content.as_ref() == Some(&key("content:other"))
    ));
    assert!(
        projection.data.links.is_empty(),
        "notes links remain in their own fragment model"
    );
    assert_eq!(projection.data.assets.len(), 1);
    assert_eq!(
        projection.data.assets[0].url,
        "https://example.test/article/diagram.png"
    );
    let public = serde_json::to_value(notes).expect("public notes data");
    assert_eq!(
        public["source_sha256"],
        serde_json::to_value(companion.source_sha256()).unwrap()
    );
    assert!(public.get("source").is_none());
    assert!(
        !serde_json::to_string(&public)
            .unwrap()
            .contains("authored/one")
    );
    let search = serde_json::to_value(projection.search()).expect("one owning search document");
    assert_eq!(search["body"], projection.body_text);
    let pages = resolved
        .project_site_pages(&key("site:one"))
        .expect("routed site");
    assert_eq!(
        pages.pages().len(),
        2,
        "notes have no independent route or search record"
    );
}

#[test]
fn notes_selection_follows_the_owner_and_cannot_hide_a_link_to_excluded_content() {
    let files = files("# Detail\n\n[other](https://example.test/other/#other)\n");
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let included = selection(&resolved, Disposition::Include);
    assert_eq!(included.content.len(), 2);
    assert!(
        included.assets.is_empty(),
        "Markdown notes have no separate asset disposition"
    );
    let selected = resolved
        .into_selected(&included)
        .expect("owner includes notes");
    assert!(
        selected
            .project_page(
                &key("content:article"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("selected notes")
            .data
            .notes
            .is_some()
    );
    let resolved = resolve(&compilation, &corpus);
    let mut exclude_target = selection(&resolved, Disposition::Include);
    exclude_target
        .content
        .insert(key("content:other"), Disposition::Exclude);
    let error = resolved
        .into_selected(&exclude_target)
        .expect_err("notes links are checked with the owning article");
    assert!(
        matches!(error, SelectionError::Link { ref content, ref target, .. } if content == &key("content:article") && target == &key("content:other")),
        "exact owner and excluded target: {error:?}"
    );
    let resolved = resolve(&compilation, &corpus);
    let mut exclude_owner = selection(&resolved, Disposition::Include);
    exclude_owner
        .content
        .insert(key("content:article"), Disposition::Exclude);
    let selected = resolved
        .into_selected(&exclude_owner)
        .expect("notes leave with their owner");
    assert!(selected.page_package(&key("content:article")).is_none());
    assert_eq!(selected.content().graph().nodes().len(), 1);
    assert!(
        selected
            .project_page(
                &key("content:other"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("unrelated page remains")
            .data
            .notes
            .is_none()
    );
    let resolved = resolve(&compilation, &corpus);
    let mut invented = selection(&resolved, Disposition::Include);
    invented
        .assets
        .insert(path("authored/one/article/notes.md"), Disposition::Exclude);
    assert!(
        matches!(resolved.into_selected(&invented), Err(SelectionError::Asset { path: ref rejected, .. }) if rejected == &path("authored/one/article/notes.md"))
    );
}

#[test]
fn notes_rich_components_use_the_selected_catalog_and_own_distinct_occurrence_addresses() {
    use gazette_core::markdown::{ComponentBinding, Error, Fragment};
    use gazette_ports::markdown::ComponentRenderer;

    struct RetainedBody;
    impl ComponentRenderer for RetainedBody {
        fn render(
            &self,
            binding: &ComponentBinding,
            body: &Fragment,
            _: bool,
        ) -> Result<String, Error> {
            let context = binding
                .context
                .as_ref()
                .expect("compiler-proved article host");
            Ok(format!(
                "<aside data-address=\"{}\">{}</aside>",
                context.address, body.html
            ))
        }
    }

    let mut files = files("# Notes\n\n:::callout\nSupplementary **meaning**.\n:::\n");
    files[0].1 = page(
        "content:article",
        "/article/",
        "article",
        "# Article\n\n:::callout\nMain **meaning**.\n:::\n",
    );
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let manifest = gazette_compiler::theme_package::ParsedManifest::parse(
        "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.callout]\ntemplate='callout.html'\n",
        Default::default(),
    ).expect("the native theme parser selects the component contract");
    let projection = resolved
        .project_page_with(
            &key("content:article"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            Some(markdown::ComponentPresentation {
                definitions: manifest.components(),
                renderer: &RetainedBody,
            }),
        )
        .expect("notes and article reuse the selected rich rendering path");
    let notes = projection
        .data
        .notes
        .as_ref()
        .expect("rendered note component");
    assert_eq!(projection.data.components.len(), 1);
    assert_eq!(notes.components.len(), 1);
    let main_context = projection.data.components[0].context.as_ref().unwrap();
    let note_context = notes.components[0].context.as_ref().unwrap();
    assert_eq!(main_context.address, "content:article#component-1");
    assert_eq!(
        note_context.address,
        "content:article#component-gazette-notes-1"
    );
    assert_eq!(note_context.host, main_context.host);
    assert_eq!(note_context.host.content, key("content:article"));
    assert_eq!(notes.components[0].name.as_str(), "callout");
    assert!(
        notes
            .body_html
            .contains("Supplementary <strong>meaning</strong>.")
    );
    assert!(notes.body_text.contains("Supplementary meaning."));
    assert!(!projection.body_html.contains("Supplementary"));
    assert!(notes.body_html.contains(&note_context.address));
}

#[test]
fn a_broken_notes_fragment_reports_the_exact_notes_source_and_original_markdown_range() {
    let source = "# Detail\r\n\r\nनोट [broken](#missing)\r\n";
    let files = files(source);
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let error = resolved
        .project_page(
            &key("content:article"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("missing note anchor cannot produce a partial page");
    assert_eq!(error.code(), "markdown_link_resolution");
    let ProjectionError::Document { origin, error } = error else {
        panic!("notes renderer lost its document origin")
    };
    assert_eq!(origin.path, path("authored/one/article/notes.md"));
    assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
    assert_eq!(origin.span, None);
    let document::Error::Markdown {
        error: markdown::Error::LinkResolution { span, .. },
        offset,
    } = *error
    else {
        panic!("original note link failure lost")
    };
    assert_eq!(offset, 0, "plain notes retain complete-file coordinates");
    assert_eq!(&source[span.start..span.end], "[broken](#missing)");
}

#[test]
fn orphan_nonarticle_framed_and_nested_notes_refuse_the_complete_authored_set() {
    for (owner_path, family, note_path, note_bytes) in [
        (
            "authored/one/article.md",
            "article",
            "authored/one/article/notes.md",
            b"# Orphan\n".to_vec(),
        ),
        (
            "authored/one/article/index.md",
            "utility",
            "authored/one/article/notes.md",
            b"# Wrong owner\n".to_vec(),
        ),
        (
            "authored/one/article/index.md",
            "article",
            "authored/one/article/notes.md",
            page(
                "content:notes",
                "/notes/",
                "article",
                "# Independent page\n",
            ),
        ),
        (
            "authored/one/article/index.md",
            "article",
            "authored/one/article/nested/notes.md",
            b"# Nested\n".to_vec(),
        ),
    ] {
        let files = vec![
            (
                owner_path.into(),
                page("content:article", "/article/", family, "# Article\n"),
            ),
            (note_path.into(), note_bytes),
        ];
        let corpus = parse(&files, Default::default());
        let compilation = compilation();
        let error = authored::resolve(&compilation, &corpus, content::Limits::default())
            .expect_err("only a plain sibling note of an article package is admitted");
        let authored::Error::Files(failures) = error else {
            panic!("note shape failure must retain a source owner")
        };
        assert!(!failures.is_empty());
        assert!(
            failures
                .iter()
                .any(|failure| failure.origin.path == path(owner_path)
                    || failure.origin.path == path(note_path)),
            "known original source must explain the refusal: {failures:?}"
        );
    }
}

#[test]
fn notes_preserve_original_rendering_safety_and_their_independent_output_ceiling() {
    for (source, expected) in [
        (
            "# Notes\n\n<script>unsafe()</script>\n",
            "markdown_raw_html_unavailable",
        ),
        (
            "# Notes\n\n[unsafe](javascript:alert)\n",
            "markdown_unsafe_url",
        ),
    ] {
        let files = files(source);
        let corpus = parse(&files, Default::default());
        let compilation = compilation();
        let resolved = resolve(&compilation, &corpus);
        let error = resolved
            .project_page(
                &key("content:article"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("supplementary content has the ordinary admission policy");
        assert_eq!(error.code(), expected);
        let ProjectionError::Document { origin, .. } = error else {
            panic!("note safety error lost its source")
        };
        assert_eq!(origin.path, path("authored/one/article/notes.md"));
        assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
    }
    let source = format!("# Notes\n\n{}\n", "Long supplementary text. ".repeat(100));
    let files = files(&source);
    let limits = corpus::Limits {
        document: document::Limits {
            markdown: markdown::Limits {
                output_bytes: 1024,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let corpus = parse(&files, limits);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let error = resolved
        .project_page(
            &key("content:article"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("notes cannot bypass their retained output ceiling");
    assert_eq!(error.code(), "markdown_limit");
    let ProjectionError::Document { origin, .. } = error else {
        panic!("bounded notes render lost its source")
    };
    assert_eq!(origin.path, path("authored/one/article/notes.md"));
}

#[test]
fn notes_count_towards_corpus_limits_and_reordering_keeps_the_projection_identical() {
    let files = files("# Notes\n\nRetained supplementary record.\n");
    let bytes: usize = files.iter().map(|(_, bytes)| bytes.len()).sum();
    let limits = corpus::Limits {
        source_bytes: bytes - 1,
        ..Default::default()
    };
    let error = ParsedCorpus::parse(
        files.iter().map(|(name, bytes)| Input {
            path: path(name),
            bytes,
        }),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
    .expect_err("notes bytes are ordinary retained corpus input");
    assert!(matches!(
        error,
        corpus::Error::SourceBytesLimit { maximum, attempted: Some(attempted) }
            if maximum == bytes - 1 && attempted == bytes
    ));
    let first = parse(&files, Default::default());
    let mut reordered = files.clone();
    reordered.reverse();
    let second = parse(&reordered, Default::default());
    let compilation = compilation();
    let first = resolve(&compilation, &first);
    let second = resolve(&compilation, &second);
    let project = |resolved: &ResolvedAuthored<'_, '_>| {
        serde_json::to_value(
            resolved
                .project_page(
                    &key("content:article"),
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                )
                .expect("complete notes projection"),
        )
        .expect("deterministic serialized page")
    };
    assert_eq!(project(&first), project(&second));
}

#[test]
fn reserved_notes_anchors_cannot_collide_and_heading_releveling_preserves_authored_hierarchy() {
    for anchor in [
        "gazette-notes-shared",
        "fn-gazette-notes-note-6e",
        "fnref-gazette-notes-note-6e",
    ] {
        let mut collision = files("# Shared\n");
        collision[0].1 = page(
            "content:article",
            "/article/",
            "article",
            &format!("# Main {{#{anchor}}}\n"),
        );
        let corpus = parse(&collision, Default::default());
        let compilation = compilation();
        let resolved = resolve(&compilation, &corpus);
        let error = resolved
            .project_page(
                &key("content:article"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("primary heading cannot occupy the notes namespace");
        assert!(
            matches!(error, ProjectionError::Binding { ref id, .. } if id == &key("content:article"))
        );
    }
    let compilation = compilation();
    for (source, expected) in [
        ("# Notes\n\n## Child\n\n###### Deep\n", vec![2, 3, 6]),
        ("## Notes\n\n### Child\n", vec![2, 3]),
        (
            "# Notes\n\n##### Deep\n\n###### Sibling after saturation\n",
            vec![2, 6, 6],
        ),
    ] {
        let files = files(source);
        let corpus = parse(&files, Default::default());
        let resolved = resolve(&compilation, &corpus);
        let projection = resolved
            .project_page(
                &key("content:article"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("subordinate heading levels stay within HTML's six levels");
        let headings = &projection.data.notes.as_ref().unwrap().headings;
        assert_eq!(
            headings
                .iter()
                .map(|heading| heading.level)
                .collect::<Vec<_>>(),
            expected
        );
        if headings
            .iter()
            .map(|heading| heading.level)
            .collect::<Vec<_>>()
            == vec![2, 6, 6]
        {
            assert_eq!(
                headings
                    .iter()
                    .map(|heading| heading.parent)
                    .collect::<Vec<_>>(),
                vec![None, Some(0), Some(0)]
            );
        }
        assert_eq!(headings[1].parent, Some(0));
        assert!(
            headings
                .iter()
                .all(|heading| heading.id.starts_with("gazette-notes-"))
        );
    }
}

#[test]
fn an_asset_referenced_only_by_notes_keeps_its_original_binding_and_is_required_by_selection() {
    let bytes = b"shared retained PNG bytes";
    let mut files = files("# Notes\n\n![Shared evidence](../shared.png)\n");
    files.push(("authored/one/shared.png".into(), bytes.to_vec()));
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let id = key("content:article");
    let package = resolved.page_package(&id).expect("article bundle");
    assert_eq!(
        package.companions().len(),
        1,
        "the external sibling asset is not claimed by the bundle"
    );
    assert_eq!(package.companions()[0].kind(), PageCompanionKind::Notes);
    let projection = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("notes resolve assets through the owning publication binding");
    let notes = projection.data.notes.as_ref().expect("notes fragment");
    assert!(
        notes
            .body_html
            .contains("https://example.test/media/shared.png")
    );
    assert!(!projection.body_html.contains("shared.png"));
    assert_eq!(
        projection.data.assets.len(),
        1,
        "notes-only dependencies reach the shared page asset model"
    );
    let asset = projection.data.assets[0];
    assert_eq!(asset.url, "https://example.test/media/shared.png");
    assert_eq!(asset.source().path, path("authored/one/shared.png"));
    assert_eq!(asset.source().source_sha256, Sha256Digest::of(bytes));
    assert_eq!(asset.sha256, Sha256Digest::of(bytes));
    assert_eq!(asset.content(), bytes);
    let mut selected = selection(&resolved, Disposition::Include);
    selected
        .assets
        .insert(path("authored/one/shared.png"), Disposition::Exclude);
    let error = resolved
        .into_selected(&selected)
        .expect_err("a note dependency cannot disappear from an included page");
    assert!(
        matches!(error, SelectionError::Asset { path: ref rejected, .. } if rejected == &path("authored/one/shared.png")),
        "exact required input: {error:?}"
    );
    let resolved = resolve(&compilation, &corpus);
    let selected = selection(&resolved, Disposition::Include);
    let selected = resolved
        .into_selected(&selected)
        .expect("complete notes and asset selection");
    let projection = selected
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("selected notes keep the existing asset transport");
    assert_eq!(
        projection.data.assets[0].url,
        "https://example.test/media/shared.png"
    );
}
