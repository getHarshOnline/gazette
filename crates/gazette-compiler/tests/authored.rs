//! Whole-corpus structural lowering, not full authored-schema or publication admission.

use gazette_compiler::{
    authored::{self, Error},
    corpus::{self, File, Input, ParsedCorpus},
    toml::{self, PathSegment},
    workspace,
};
use gazette_core::{
    publication::{ContentLifecycle, Key, content},
    source::Sha256Digest,
};

fn key(value: &str) -> Key {
    value.parse().expect("fixture key")
}

fn compilation() -> workspace::Compilation {
    workspace::compile(
        "[[principals]]\nid='owner:publisher'\nkind='person'\n\
         [[sites]]\nid='site:alpha'\npublisher='owner:publisher'\ndeployable=false\n\
         [[sites]]\nid='site:beta'\npublisher='owner:publisher'\ndeployable=true\n",
        toml::Limits::default(),
    )
    .expect("real workspace TOML")
}

fn document(id: &str, site: &str, lifecycle: &str, edges: &str, structure: &str) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='{site}'\ntitle='Guide'\n\
         lifecycle='{lifecycle}'\n[content.structure]\n{structure}\n+++\n# Body\n"
    )
}

fn leaf(id: &str) -> String {
    document(
        id,
        "site:alpha",
        "published",
        "",
        "kind='page'\nfamily={kind='documentation'}",
    )
}

#[test]
fn page_template_retains_exact_origin_and_is_removed_with_excluded_content() {
    let selected = leaf("content:selected")
        .replacen(
            "+++\n",
            "+++\n# Résumé 🦀\ntemplate='layouts/Article.html'\n",
            1,
        )
        .replace('\n', "\r\n");
    let default = leaf("content:default");
    let corpus = parse(
        &[
            ("selected.md", selected.as_bytes()),
            ("default.md", default.as_bytes()),
        ],
        corpus::Limits::default(),
    );
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap();
    let id = key("content:selected");
    assert_eq!(
        resolved.template(&id).unwrap().as_str(),
        "layouts/Article.html"
    );
    assert!(resolved.template(&key("content:default")).is_none());
    assert!(resolved.template(&key("content:unknown")).is_none());
    let field = [PathSegment::Key("template".into())];
    let origin = resolved.origin(&id, &field).unwrap();
    assert_eq!(origin.source_sha256, Sha256Digest::of(selected.as_bytes()));
    let span = origin.span.unwrap();
    assert_eq!(span.start, selected.find("'layouts/Article.html'").unwrap());
    assert_eq!(&selected[span.start..span.end], "'layouts/Article.html'");
    assert!(resolved.origin(&key("content:default"), &field).is_none());
    assert!(
        serde_json::to_value(resolved.content().graph().node(&id).unwrap())
            .unwrap()
            .get("template")
            .is_none()
    );
    let selection = authored::Selection {
        content: [
            (id.clone(), content::Disposition::Exclude),
            (key("content:default"), content::Disposition::Include),
        ]
        .into_iter()
        .collect(),
        assets: Default::default(),
    };
    let candidate = resolved.into_selected(&selection).unwrap();
    assert!(candidate.template(&id).is_none());
    assert!(candidate.origin(&id, &field).is_none());
}

#[test]
fn page_template_rejects_non_paths_and_non_strings_in_native_lowering() {
    for value in [
        "''",
        "'../outside.html'",
        "'/absolute.html'",
        "false",
        "['page.html']",
        "{path='page.html'}",
    ] {
        let source = leaf("content:one").replacen("+++\n", &format!("+++\ntemplate={value}\n"), 1);
        let corpus = parse(&[("page.md", source.as_bytes())], corpus::Limits::default());
        let workspace = compilation();
        let failure = authored::resolve(&workspace, &corpus, content::Limits::default())
            .expect_err("exact retained entry required");
        let Error::Files(files) = failure else {
            panic!("native metadata admission expected")
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].origin.path.as_str(), "page.md");
        assert_eq!(
            files[0].origin.source_sha256,
            Sha256Digest::of(source.as_bytes())
        );
        assert!(matches!(files[0].error, authored::FileError::Metadata(_)));
    }
}

fn parse<'a>(files: &[(&str, &'a [u8])], limits: corpus::Limits) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().expect("logical file path"),
            bytes,
        }),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
    .expect("complete syntax corpus")
}

#[test]
fn page_resource_order_is_preserved_but_inputs_are_a_single_normalized_set() {
    let source = document(
        "content:one",
        "site:alpha",
        "draft",
        "[[resources]]\ninputs=['z.scss','a.scss']\nbuild={kind='stylesheet',entry='z.scss'}\noutput='/z.css'\n[[resources]]\ninputs=['main.ts']\nbuild={kind='browser-module',entry='main.ts'}\noutput='/a.js'\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let corpus = parse(&[("page.md", source.as_bytes())], corpus::Limits::default());
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("structural declarations");
    let resources = resolved.resources(&key("content:one"));
    assert_eq!(
        resources
            .iter()
            .map(|resource| resource.output.as_str())
            .collect::<Vec<_>>(),
        ["/z.css", "/a.js"]
    );
    assert_eq!(
        resources[0]
            .inputs
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["a.scss", "z.scss"]
    );
    assert_eq!(resources[0].build.entry().as_str(), "z.scss");
    assert_eq!(
        resolved
            .document(&key("content:one"))
            .expect("retained syntax")
            .source_sha256(),
        Sha256Digest::of(source.as_bytes())
    );
    assert!(resolved.resources(&key("content:absent")).is_empty());
}

#[test]
fn resource_declaration_limits_fail_during_lowering_not_as_a_partial_graph() {
    let source = document(
        "content:one",
        "site:alpha",
        "draft",
        "[[resources]]\ninputs=['one.ts']\nbuild={kind='browser-module',entry='one.ts'}\noutput='/one.js'\n[[resources]]\ninputs=['two.ts']\nbuild={kind='browser-module',entry='two.ts'}\noutput='/two.js'\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let corpus = parse(&[("page.md", source.as_bytes())], corpus::Limits::default());
    let workspace = compilation();
    for (limits, expected) in [
        (
            content::Limits {
                nodes: 1,
                ..Default::default()
            },
            "page resource declarations",
        ),
        (
            content::Limits {
                text_bytes: 1,
                ..Default::default()
            },
            "page resource declaration bytes",
        ),
    ] {
        let Error::Files(failures) =
            authored::resolve(&workspace, &corpus, limits).expect_err("complete bound failure")
        else {
            panic!("resource lowering, not later graph failure")
        };
        assert_eq!(failures.len(), 1);
        assert!(
            matches!(&failures[0].error, authored::FileError::Resource(gazette_core::resource::Error::Limit(reason)) if *reason == expected)
        );
        assert_eq!(failures[0].origin.path.as_str(), "page.md");
        assert_eq!(
            failures[0].origin.source_sha256,
            Sha256Digest::of(source.as_bytes())
        );
        assert!(failures[0].origin.span.is_some());
    }
}

#[test]
fn complete_markdown_set_resolves_mixed_relations_and_retains_every_source() {
    let collection = document(
        "collection:guides",
        "site:alpha",
        "published",
        "",
        "kind='collection'",
    );
    let journey = document(
        "journey:learn",
        "site:alpha",
        "published",
        "",
        "kind='journey'\ncollection='collection:guides'\n\
         members=[{content='content:local-archived',primary=true},{content='content:draft',primary=true}]",
    );
    let draft = document(
        "content:draft",
        "site:alpha",
        "draft",
        "related=['content:archived']\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let archived = document(
        "content:archived",
        "site:beta",
        "archived",
        "",
        "kind='page'\nfamily={kind='documentation'}",
    );
    let assets: &[u8] = &[0xff, 0, 0x80, b'{'];
    let local_archived = document(
        "content:local-archived",
        "site:alpha",
        "archived",
        "",
        "kind='page'\nfamily={kind='documentation'}",
    );
    let collected = document(
        "content:collected",
        "site:alpha",
        "published",
        "collections=['collection:guides']\n",
        "kind='page'\nfamily={kind='article'}",
    );
    let files = [
        ("z-collection.md", collection.as_bytes()),
        ("b-journey.md", journey.as_bytes()),
        ("c-draft.md", draft.as_bytes()),
        ("a-archived.md", archived.as_bytes()),
        ("assets/opaque.bin", assets),
        ("d-local-archived.md", local_archived.as_bytes()),
        ("e-collected.md", collected.as_bytes()),
    ];
    let corpus = parse(&files, corpus::Limits::default());
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("whole-set structural resolution");
    assert!(std::ptr::eq(resolved.corpus(), &corpus));
    assert!(std::ptr::eq(resolved.content().compilation(), &workspace));
    assert_eq!(resolved.corpus().files().len(), 7);
    let graph = resolved.content().graph();
    assert_eq!(graph.nodes().len(), 6);
    let view = graph
        .journey_view(&key("journey:learn"))
        .expect("ordered journey");
    assert_eq!(
        view.steps
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        [key("content:local-archived"), key("content:draft")]
    );
    assert_eq!(view.steps[0].lifecycle, ContentLifecycle::Archived);
    assert_eq!(view.steps[1].lifecycle, ContentLifecycle::Draft);
    assert_eq!(
        graph
            .primary_journey(&key("content:draft"))
            .expect("explicit primary")
            .id,
        key("journey:learn")
    );
    assert_eq!(
        graph
            .memberships()
            .iter()
            .next()
            .expect("collection membership")
            .content,
        key("content:collected")
    );
    assert_eq!(
        graph
            .related()
            .iter()
            .next()
            .expect("separate editorial edge")
            .target,
        key("content:archived")
    );
    assert_eq!(
        resolved
            .content()
            .site_for(&key("content:draft"))
            .expect("exact home")
            .id,
        key("site:alpha")
    );
    assert_eq!(
        resolved
            .content()
            .site_for(&key("journey:learn"))
            .expect("site-local journey home")
            .id,
        key("site:alpha")
    );
    assert_eq!(
        resolved
            .content()
            .site_for(&key("content:archived"))
            .expect("foreign editorial target keeps its home")
            .id,
        key("site:beta")
    );
    assert!(graph.editorial_parent(&key("content:archived")).is_none());
    assert_eq!(
        graph
            .editorial_parent(&key("content:local-archived"))
            .expect("containing journey")
            .id,
        key("journey:learn")
    );
    for (id, source) in [
        ("content:draft", &draft),
        ("content:archived", &archived),
        ("journey:learn", &journey),
        ("collection:guides", &collection),
        ("content:local-archived", &local_archived),
        ("content:collected", &collected),
    ] {
        let retained = resolved.document(&key(id)).expect("source association");
        assert_eq!(retained.source(), source);
        assert_eq!(
            retained.source_sha256(),
            Sha256Digest::of(source.as_bytes())
        );
        assert_eq!(
            retained
                .body()
                .render_html()
                .expect("retained body projection"),
            "<h1 id=\"body\">Body</h1>\n"
        );
    }
    match &resolved.corpus().files()["assets/opaque.bin"] {
        File::Asset(bytes) => {
            assert_eq!(*bytes, assets);
            assert!(std::ptr::eq(bytes.as_ptr(), assets.as_ptr()));
        }
        other => panic!("asset must not disappear or become a document: {other:?}"),
    }
    assert!(resolved.document(&key("content:unknown")).is_none());
    let mut reverse = files;
    reverse.reverse();
    let reordered = parse(&reverse, corpus::Limits::default());
    let other = authored::resolve(&workspace, &reordered, content::Limits::default())
        .expect("same whole set");
    assert_eq!(
        serde_json::to_value(graph).expect("graph projection"),
        serde_json::to_value(other.content().graph()).expect("reordered graph projection")
    );
}

#[test]
fn unknown_and_missing_fields_fail_the_complete_set_in_path_order() {
    let valid = leaf("content:valid");
    let root_unknown = valid.replacen("+++\n", "+++\nauthor='not-supported-yet'\n", 1);
    let missing_title = valid.replace("title='Guide'\n", "");
    let nested_unknown = valid.replace(
        "kind='page'",
        "kind='page'\ntemplate='not-a-structural-field'",
    );
    let corpus = parse(
        &[
            ("z.md", root_unknown.as_bytes()),
            ("a.md", missing_title.as_bytes()),
            ("m.md", nested_unknown.as_bytes()),
            ("valid.md", valid.as_bytes()),
        ],
        corpus::Limits::default(),
    );
    match authored::resolve(&compilation(), &corpus, content::Limits::default())
        .expect_err("unsupported fields never silently drop")
    {
        Error::Files(errors) => {
            assert_eq!(
                errors
                    .iter()
                    .map(|error| error.origin.path.as_str())
                    .collect::<Vec<_>>(),
                ["a.md", "m.md", "z.md"]
            );
            for error in errors {
                assert!(matches!(
                    error.error,
                    authored::FileError::Metadata(toml::Error::Invalid { .. })
                ));
            }
        }
        other => panic!("expected complete file-schema failures: {other:?}"),
    }
}

#[test]
fn unframed_markdown_is_accounted_for_without_a_successful_subset() {
    let valid = leaf("content:valid");
    let native = "# Unbound native documentation\r\nRésumé\r\n";
    let corpus = parse(
        &[
            ("valid.md", valid.as_bytes()),
            ("native.md", native.as_bytes()),
        ],
        corpus::Limits::default(),
    );
    match authored::resolve(&compilation(), &corpus, content::Limits::default())
        .expect_err("native binding required")
    {
        Error::Files(errors) => {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].origin.path.as_str(), "native.md");
            assert_eq!(
                errors[0].origin.source_sha256,
                Sha256Digest::of(native.as_bytes())
            );
            assert!(errors[0].origin.span.is_none());
            assert!(matches!(
                errors[0].error,
                authored::FileError::NativeBindingRequired
            ));
        }
        other => panic!("unframed file is not a graph failure: {other:?}"),
    }
}

#[test]
fn duplicate_content_identity_retains_both_full_document_origins() {
    let first = leaf("content:duplicate");
    let second = first.replace("Guide", "Other title").replace('\n', "\r\n");
    let corpus = parse(
        &[("z.md", second.as_bytes()), ("a.md", first.as_bytes())],
        corpus::Limits::default(),
    );
    match authored::resolve(&compilation(), &corpus, content::Limits::default())
        .expect_err("duplicate exact identity")
    {
        Error::Structure {
            error: content::Error::Invalid(errors),
            declarations,
        } => {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].code, "content_duplicate");
            assert_eq!(errors[0].source, key("content:duplicate"));
            let origins = &declarations[&key("content:duplicate")];
            assert_eq!(origins.len(), 2);
            for (origin, path, source, start) in [
                (&origins[0], "a.md", &first, 4),
                (&origins[1], "z.md", &second, 5),
            ] {
                assert_eq!(origin.path.as_str(), path);
                assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
                let span = origin.span.expect("declaring metadata coordinates");
                assert_eq!(span.start, start);
                assert!(source[span.start..span.end].starts_with("[content]"));
                assert!(!source[span.start..span.end].contains("# Body"));
            }
        }
        other => panic!("duplicate must retain structural diagnostics and both origins: {other:?}"),
    }
}

#[test]
fn original_graph_and_workspace_site_diagnostics_keep_their_classes_and_origins() {
    let missing_collection =
        leaf("content:broken").replacen("+++\n", "+++\ncollections=['collection:missing']\n", 1);
    let missing_site = leaf("content:broken").replace("site:alpha", "owner:publisher");
    for (source, code, target) in [
        (
            missing_collection,
            "content_reference",
            "collection:missing",
        ),
        (missing_site, "content_site_reference", "owner:publisher"),
    ] {
        let corpus = parse(
            &[("broken.md", source.as_bytes())],
            corpus::Limits::default(),
        );
        match authored::resolve(&compilation(), &corpus, content::Limits::default())
            .expect_err("original structural failure")
        {
            Error::Structure {
                error: content::Error::Invalid(errors),
                declarations,
            } => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].code, code);
                assert_eq!(errors[0].source, key("content:broken"));
                assert_eq!(errors[0].target, Some(key(target)));
                assert_eq!(
                    declarations[&key("content:broken")][0].source_sha256,
                    Sha256Digest::of(source.as_bytes())
                );
            }
            other => panic!("original diagnostic class required: {other:?}"),
        }
    }
}

#[test]
fn editorial_residence_failures_retain_exact_whole_document_declarations() {
    let workspace = compilation();
    for (edges, structure, expected) in [
        (
            "collections=['collection:one']\n",
            "kind='journey'\ncollection='collection:one'\nmembers=[{content='content:page',primary=true}]",
            vec![
                (
                    "content_multiple_residence",
                    "content:page",
                    Some("collection:one"),
                ),
                (
                    "content_multiple_residence",
                    "content:page",
                    Some("journey:one"),
                ),
            ],
        ),
        (
            "collections=['collection:one','collection:two']\n",
            "kind='journey'\ncollection='collection:one'\nmembers=[]",
            vec![
                (
                    "content_multiple_residence",
                    "content:page",
                    Some("collection:one"),
                ),
                (
                    "content_multiple_residence",
                    "content:page",
                    Some("collection:two"),
                ),
            ],
        ),
        (
            "",
            "kind='journey'\nmembers=[{content='content:page',primary=true}]",
            vec![("journey_collection_required", "journey:one", None)],
        ),
        (
            "",
            "kind='journey'\ncollection='collection:one'\nmembers=[{content='content:page',primary=false}]",
            vec![(
                "journey_member_not_primary",
                "journey:one",
                Some("content:page"),
            )],
        ),
    ] {
        let files = [
            (
                "one.md",
                document(
                    "collection:one",
                    "site:alpha",
                    "published",
                    "",
                    "kind='collection'",
                ),
            ),
            (
                "two.md",
                document(
                    "collection:two",
                    "site:alpha",
                    "draft",
                    "",
                    "kind='collection'",
                ),
            ),
            (
                "journey.md",
                document("journey:one", "site:alpha", "published", "", structure),
            ),
            (
                "page.md",
                document(
                    "content:page",
                    "site:alpha",
                    "archived",
                    edges,
                    "kind='page'\nfamily={kind='article'}",
                ),
            ),
        ];
        let inputs = files
            .iter()
            .map(|(path, source)| (*path, source.as_bytes()))
            .collect::<Vec<_>>();
        let corpus = parse(&inputs, Default::default());
        let Error::Structure {
            error: content::Error::Invalid(errors),
            declarations,
        } = authored::resolve(&workspace, &corpus, Default::default())
            .expect_err("no partial editorial graph")
        else {
            panic!("original structural diagnostics required")
        };
        assert_eq!(
            errors
                .iter()
                .map(|error| (
                    error.code,
                    error.source.as_str(),
                    error.target.as_ref().map(Key::as_str)
                ))
                .collect::<Vec<_>>(),
            expected
        );
        for (id, path, source) in [
            ("collection:one", files[0].0, &files[0].1),
            ("collection:two", files[1].0, &files[1].1),
            ("journey:one", files[2].0, &files[2].1),
            ("content:page", files[3].0, &files[3].1),
        ] {
            let origins = &declarations[&key(id)];
            assert_eq!(origins.len(), 1);
            assert_eq!(origins[0].path.as_str(), path);
            assert_eq!(
                origins[0].source_sha256,
                Sha256Digest::of(source.as_bytes())
            );
            let span = origins[0].span.expect("bound declaration coordinates");
            assert!(source[span.start..span.end].contains(&format!("id='{id}'")));
            assert!(!source[span.start..span.end].contains("# Body"));
        }
    }
}

#[test]
fn whole_graph_resource_limits_retain_declaration_evidence() {
    let source = leaf("content:bounded");
    let corpus = parse(
        &[("bounded.md", source.as_bytes())],
        corpus::Limits::default(),
    );
    for (limits, resource) in [
        (
            content::Limits {
                nodes: 0,
                ..content::Limits::default()
            },
            "nodes",
        ),
        (
            content::Limits {
                text_bytes: 0,
                ..content::Limits::default()
            },
            "text bytes",
        ),
    ] {
        match authored::resolve(&compilation(), &corpus, limits)
            .expect_err("complete graph ceiling")
        {
            Error::Structure {
                error: content::Error::Limit(actual),
                declarations,
            } => {
                assert_eq!(actual, resource);
                assert_eq!(declarations[&key("content:bounded")].len(), 1);
            }
            other => panic!("resource failures must not become file failures: {other:?}"),
        }
    }
}

#[test]
fn origins_use_crlf_unicode_full_source_bytes_and_never_invent_default_locations() {
    let source = document(
        "collection:unicode",
        "site:alpha",
        "published",
        "",
        "kind='collection'",
    )
    .replace("Guide", "Résumé 🦀")
    .replace("# Body", "# Résumé 🦀")
    .replace('\n', "\r\n");
    let corpus = parse(
        &[("é/guide.md", source.as_bytes())],
        corpus::Limits::default(),
    );
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("Unicode structural source");
    let id = key("collection:unicode");
    let path = |segments: &[&str]| {
        segments
            .iter()
            .map(|segment| PathSegment::Key((*segment).into()))
            .collect::<Vec<_>>()
    };
    for (field, token) in [
        (path(&["content", "title"]), "'Résumé 🦀'"),
        (path(&["content", "structure", "kind"]), "'collection'"),
    ] {
        let origin = resolved.origin(&id, &field).expect("explicit exact field");
        let span = origin.span.expect("explicit field span");
        assert_eq!(span.start, source.find(token).expect("fixture token"));
        assert_eq!(&source[span.start..span.end], token);
        assert_eq!(origin.path.as_str(), "é/guide.md");
        assert_eq!(origin.source_sha256, Sha256Digest::of(source.as_bytes()));
    }
    for field in [
        path(&["collections"]),
        path(&["related"]),
        path(&["content", "structure", "parent"]),
        path(&["content", "missing"]),
        vec![PathSegment::Index(0)],
    ] {
        assert!(resolved.origin(&id, &field).is_none());
    }
    assert!(
        resolved
            .origin(&key("content:unknown"), &path(&["content", "title"]))
            .is_none()
    );
}

#[test]
fn body_projection_failure_does_not_remove_document_coordinates_or_structural_result() {
    let source = leaf("content:bounded");
    let mut limits = corpus::Limits::default();
    limits.document.markdown.output_bytes = 0;
    let corpus = parse(&[("bounded.md", source.as_bytes())], limits);
    assert!(
        corpus.inspect().is_err(),
        "inspection must not emit a partial report"
    );
    let workspace = compilation();
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default())
        .expect("structural lowering does not require a body projection");
    let id = key("content:bounded");
    let retained = resolved.document(&id).expect("source remains addressable");
    assert!(retained.inspect().is_err());
    assert_eq!(
        retained.source_sha256(),
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(
        &source[retained.body_span().start..retained.body_span().end],
        "# Body\n"
    );
    assert_eq!(retained.metadata_span().start, 4);
    let origin = resolved
        .origin(
            &id,
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("title".into()),
            ],
        )
        .expect("origin independent of projection budgets");
    let span = origin.span.expect("title coordinates");
    assert_eq!(&source[span.start..span.end], "'Guide'");
}

#[test]
fn authored_template_wins_without_rewriting_source_provenance() {
    let source =
        leaf("content:selected").replacen("+++\n", "+++\ntemplate='source/article.html'\n", 1);
    let unbound = leaf("content:unbound");
    let corpus = parse(
        &[
            ("selected.md", source.as_bytes()),
            ("unbound.md", unbound.as_bytes()),
        ],
        corpus::Limits::default(),
    );
    let workspace = workspace::compile("[[principals]]\nid='owner:publisher'\nkind='person'\n[[sites]]\nid='site:alpha'\npublisher='owner:publisher'\ndeployable=false\n[sites.composition.page_templates]\n'content:selected'='world/project.html'\n", toml::Limits::default()).unwrap();
    let resolved = authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap();
    let id = key("content:selected");
    assert_eq!(
        resolved.selected_template(&id).unwrap().as_str(),
        "source/article.html"
    );
    assert_eq!(
        resolved.template(&id).unwrap().as_str(),
        "source/article.html"
    );
    assert!(
        resolved
            .origin(&id, &[PathSegment::Key("template".into())])
            .is_some()
    );
    assert!(
        resolved
            .selected_template(&key("content:unbound"))
            .is_none()
    );
    assert!(
        resolved
            .selected_template(&key("content:unknown"))
            .is_none()
    );
    let selected = resolved
        .into_selected(&authored::Selection {
            content: [
                (id.clone(), content::Disposition::Exclude),
                (key("content:unbound"), content::Disposition::Include),
            ]
            .into_iter()
            .collect(),
            assets: Default::default(),
        })
        .unwrap();
    assert!(
        selected.selected_template(&id).is_none(),
        "workspace binding must not resurrect excluded content"
    );
}

#[test]
fn page_template_bindings_reject_unknown_and_foreign_content() {
    let source = leaf("content:one");
    let corpus = parse(&[("one.md", source.as_bytes())], corpus::Limits::default());
    for (site, id, expected) in [
        (
            "site:alpha",
            "content:missing",
            "content_page_template_reference",
        ),
        ("site:beta", "content:one", "content_page_template_site"),
    ] {
        let config = String::from(
            "[[principals]]\nid='owner:publisher'\nkind='person'\n[[sites]]\nid='site:alpha'\npublisher='owner:publisher'\ndeployable=false\n[[sites]]\nid='site:beta'\npublisher='owner:publisher'\ndeployable=false\n",
        );
        // Place the binding explicitly on its composing site.
        let config = if site == "site:alpha" {
            config.replace("[[sites]]\nid='site:beta'", &format!("[sites.composition.page_templates]\n'{id}'='world/page.html'\n[[sites]]\nid='site:beta'"))
        } else {
            format!("{config}[sites.composition.page_templates]\n'{id}'='world/page.html'\n")
        };
        let workspace = workspace::compile(&config, toml::Limits::default()).unwrap();
        let error = authored::resolve(&workspace, &corpus, content::Limits::default()).unwrap_err();
        let Error::Structure {
            error: content::Error::Invalid(diagnostics),
            ..
        } = error
        else {
            panic!("expected structural reference failure")
        };
        assert!(diagnostics.iter().any(|d| d.code == expected));
    }
}
