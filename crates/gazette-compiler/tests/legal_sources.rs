//! Explicit legal-source bindings lower real retained documents without inventing a site.

use gazette_compiler::{
    authored::{self, LegalSourceError},
    contracts,
    corpus::{File, Input, ParsedCorpus},
    workspace,
};
use gazette_core::{
    publication::{
        ContentLifecycle, Key,
        content::{ContentKind, Declaration, PageFamily},
    },
    source::Sha256Digest,
};
use serde_json::json;

fn shared_compilation(entry: &str) -> workspace::Compilation {
    workspace::compile(
        &format!(
            "{CONFIGURATION}{}{}",
            publication("site:one", &[binding("content:one-terms", entry)]),
            publication("site:two", &[binding("content:two-terms", entry)])
        ),
        Default::default(),
    )
    .expect("explicit shared legal source")
}

#[test]
fn shared_legal_symbolic_references_keep_native_resolution_and_single_source_occurrences() {
    let compiled = shared_compilation("terms.md");
    let text = format!(
        "{}\n[[site:two.test/terms/#terms|Same owner terms]]\n",
        source("").replace("# Owner’s terms", "# Terms")
    );
    let corpus = bound_corpus(&compiled, &text);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let references = resolved
        .publication_references(1, 4096)
        .expect("one retained source wikilink, not two copied occurrences");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].path, references[0].target);
    assert_eq!(
        references[0].location.span.slice(&text),
        Some("[[site:two.test/terms/#terms|Same owner terms]]")
    );
    for id in ["content:one-terms", "content:two-terms"] {
        let page = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert!(
            page.body_html
                .contains("href=\"https://two.test/mount/terms/#terms\"")
        );
        assert_eq!(page.data.links[0].content, Some(key("content:two-terms")));
    }
    let broken = text.replace("#terms|", "#absent|");
    let corpus = bound_corpus(&compiled, &broken);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    assert!(
        matches!(resolved.publication_references(4, 4096), Err(authored::ProjectionError::Document {origin,..}) if origin.source_sha256==Sha256Digest::of(broken.as_bytes()) && origin.path.as_str()=="__legal/source:legal/terms.md")
    );
}

fn bound_corpus<'s>(compiled: &workspace::Compilation, text: &'s str) -> ParsedCorpus<'s> {
    let source = compiled
        .workspace
        .legal_sources()
        .next()
        .expect("legal view");
    let path = source
        .logical_path(&source.legal_binding().unwrap().entry)
        .expect("private coordinate");
    ParsedCorpus::parse(
        [Input {
            path,
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained source syntax")
}

#[test]
fn resolved_shared_legal_outputs_borrow_one_file_and_keep_private_provenance() {
    let compiled = shared_compilation("terms.md");
    let text = source("");
    let corpus = bound_corpus(&compiled, &text);
    let resolved =
        authored::resolve(&compiled, &corpus, Default::default()).expect("complete native graph");
    resolved
        .validate_source_bindings()
        .expect("ID-bound legal provenance");
    let first = key("content:one-terms");
    let second = key("content:two-terms");
    assert!(std::ptr::eq(
        resolved.file(&first).unwrap(),
        resolved.file(&second).unwrap()
    ));
    assert!(std::ptr::eq(
        resolved.document(&first).unwrap(),
        resolved.document(&second).unwrap()
    ));
    assert_eq!(resolved.source_path(&first), resolved.source_path(&second));
    let path = resolved.source_path(&first).unwrap();
    assert_eq!(compiled.workspace.legal_bindings(path).count(), 2);
    assert!(
        compiled.workspace.content_source(path).is_none(),
        "legal is not a fabricated tree placement"
    );
    for (id, site, address) in [
        (&first, "site:one", "https://one.test/mount/terms/"),
        (&second, "site:two", "https://two.test/mount/terms/"),
    ] {
        let provenance = resolved.source_binding(id).unwrap();
        assert_eq!(provenance.source_key(), &key("source:legal"));
        assert_eq!(provenance.site, &key(site));
        assert_eq!(&provenance.legal_binding().unwrap().id, id);
        assert!(provenance.tree_binding().is_none());
        assert_eq!(provenance.entry(path), Some("terms.md"));
        assert_eq!(
            provenance
                .logical_path(&"terms.md".parse().unwrap())
                .unwrap(),
            *path
        );
        assert!(
            provenance
                .logical_path(&"asset.png".parse().unwrap())
                .is_err()
        );
        let page = resolved
            .project_page(
                id,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert_eq!(page.data.route.unwrap().url, address);
        assert!(page.body_html.contains("<strong>legal prose</strong>"));
        assert_eq!(
            page.data.source.source_sha256,
            Sha256Digest::of(text.as_bytes())
        );
        assert_eq!(page.data.source.path, *path);
        assert!(page.data.notes.is_none());
        assert!(resolved.page_package(id).is_none());
        let value = serde_json::to_value(&page).unwrap();
        assert_eq!(value["provenance"], json!({"site":site}));
        assert_eq!(value["content"]["id"], id.as_str());
        let serialized = serde_json::to_string(&page).unwrap();
        for private in [
            "__legal",
            "source:legal",
            "content:owner-terms",
            "/private/legal-root-never-read",
        ] {
            assert!(
                !serialized.contains(private),
                "private coordinate leaked: {private}"
            );
        }
        for field in ["id", "site", "route", "structure"] {
            assert!(
                resolved
                    .origin(
                        id,
                        &[
                            gazette_compiler::toml::PathSegment::Key("content".into()),
                            gazette_compiler::toml::PathSegment::Key(field.into())
                        ]
                    )
                    .is_none(),
                "derived {field} has no source declaration"
            );
        }
        for (field, literal) in [
            ("title", "'Terms of use'"),
            ("description", "'Owner-held terms.'"),
            ("lifecycle", "'draft'"),
            ("language", "'en'"),
        ] {
            let origin = resolved
                .origin(
                    id,
                    &[
                        gazette_compiler::toml::PathSegment::Key("content".into()),
                        gazette_compiler::toml::PathSegment::Key(field.into()),
                    ],
                )
                .expect("real reduced scalar declaration");
            assert_eq!(origin.path, *path);
            assert_eq!(origin.source_sha256, Sha256Digest::of(text.as_bytes()));
            assert_eq!(origin.span.unwrap().slice(&text), Some(literal));
        }
    }
}

#[test]
fn selected_legal_index_is_not_an_article_package_and_siblings_are_not_pages_or_assets() {
    let compiled = shared_compilation("terms/index.md");
    let text = source("");
    let unrelated = "+++\n[unknown]\nretained=true\n+++\n# Unselected sibling\n";
    let corpus = ParsedCorpus::parse(
        [
            Input {
                path: "__legal/source:legal/terms/index.md".parse().unwrap(),
                bytes: text.as_bytes(),
            },
            Input {
                path: "__legal/source:legal/terms/notes.md".parse().unwrap(),
                bytes: b"# No implicit commentary\n",
            },
            Input {
                path: "__legal/source:legal/other.md".parse().unwrap(),
                bytes: unrelated.as_bytes(),
            },
            Input {
                path: "__legal/source:legal/terms/image.png".parse().unwrap(),
                bytes: b"opaque unselected companion",
            },
        ],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("only selected legal source participates");
    assert_eq!(resolved.content().graph().nodes().len(), 2);
    assert!(resolved.page_packages().is_empty());
    for site in ["site:one", "site:two"] {
        let assets = resolved.assets_for(&key(site)).unwrap();
        assert!(assets.entries().next().is_none());
        assert!(assets.uncompiled().is_empty());
    }
}

#[test]
fn legal_source_missing_unframed_and_metadata_identity_failures_are_exact() {
    let compiled = compile(&[binding("content:one-terms", "terms.md")]);
    let empty =
        ParsedCorpus::parse([], Default::default(), &gazette_adapter_markdown::Comrak).unwrap();
    assert!(
        matches!(authored::resolve(&compiled, &empty, Default::default()), Err(authored::Error::LegalInput{path,..}) if path.as_str()=="__legal/source:legal/terms.md")
    );
    for text in [
        "# Unframed owner body\n".to_owned(),
        source("site='site:one'\n"),
        source("").replace("content:owner-terms", "content:different-owner"),
    ] {
        let corpus = bound_corpus(&compiled, &text);
        let error = authored::resolve(&compiled, &corpus, Default::default())
            .expect_err("no successful partial legal publication");
        if text.starts_with('#') {
            assert!(matches!(error, authored::Error::LegalInput { .. }));
        } else {
            let authored::Error::Files(errors) = error else {
                panic!("retained legal metadata failure")
            };
            assert_eq!(errors.len(), 1);
            assert_eq!(
                errors[0].origin.path.as_str(),
                "__legal/source:legal/terms.md"
            );
            assert_eq!(
                errors[0].origin.source_sha256,
                Sha256Digest::of(text.as_bytes())
            );
            assert!(matches!(&errors[0].error, authored::FileError::Legal(_)));
            if text.contains("content:different-owner") {
                assert_eq!(
                    errors[0].origin.span.unwrap().slice(&text),
                    Some("'content:different-owner'")
                );
            }
        }
    }
}

#[test]
fn legal_output_amplification_has_independent_node_and_byte_bounds() {
    let compiled = shared_compilation("terms.md");
    let text = source("");
    let corpus = bound_corpus(&compiled, &text);
    for limits in [
        gazette_core::publication::content::Limits {
            nodes: 1,
            ..Default::default()
        },
        gazette_core::publication::content::Limits {
            text_bytes: text.len() + 64,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            authored::resolve(&compiled, &corpus, limits),
            Err(authored::Error::Structure {
                error: gazette_core::publication::content::Error::Limit(_),
                ..
            })
        ));
    }
}

#[test]
fn legal_routes_and_ids_conflicting_with_authored_pages_fail_whole_graph_resolution() {
    for (id, route) in [
        ("content:one-terms", "/other/"),
        ("content:other", "/terms/"),
    ] {
        let configuration = format!(
            "{CONFIGURATION}{}authored=[{{source='source:legal',target='authored'}}]\n",
            publication("site:one", &[binding("content:one-terms", "terms.md")])
        );
        let compiled = workspace::compile(&configuration, Default::default()).unwrap();
        let text = source("");
        let page = format!(
            "+++\n[content]\nid='{id}'\nsite='site:one'\ntitle='Authored page'\nlifecycle='draft'\n[content.route]\npath='{route}'\n[content.structure]\nkind='page'\nfamily={{kind='utility'}}\n+++\n# Authored page\n"
        );
        let corpus = ParsedCorpus::parse(
            [
                Input {
                    path: "__legal/source:legal/terms.md".parse().unwrap(),
                    bytes: text.as_bytes(),
                },
                Input {
                    path: "authored/other.md".parse().unwrap(),
                    bytes: page.as_bytes(),
                },
            ],
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let error = authored::resolve(&compiled, &corpus, Default::default())
            .expect_err("legal output conflicts cannot win over authored identities/routes");
        let authored::Error::Structure { declarations, .. } = error else {
            panic!("whole graph diagnostic")
        };
        assert!(
            declarations
                .values()
                .flatten()
                .any(
                    |origin| origin.path.as_str() == "__legal/source:legal/terms.md"
                        && origin.span.is_none()
                )
        );
        assert!(
            declarations
                .values()
                .flatten()
                .any(|origin| origin.path.as_str() == "authored/other.md")
        );
    }
}

#[test]
fn legal_body_assets_are_not_implicitly_admitted_and_errors_retain_source_identity() {
    let compiled = compile(&[binding("content:one-terms", "terms.md")]);
    let text = format!("{}\n![Unselected companion](image.png)\n", source(""));
    let corpus = bound_corpus(&compiled, &text);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let error = resolved
        .project_page(
            &key("content:one-terms"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("legal binding is not an asset tree");
    let authored::ProjectionError::Document { origin, .. } = error else {
        panic!("exact body diagnostic")
    };
    assert_eq!(origin.path.as_str(), "__legal/source:legal/terms.md");
    assert_eq!(origin.source_sha256, Sha256Digest::of(text.as_bytes()));
}

#[test]
fn private_legal_namespace_is_reserved_and_source_key_components_are_unambiguous() {
    for target in [
        "__legal",
        "__legal/source-legal",
        "__legal/source-legal/terms.md",
        "/%5f%5flegal/normalized/",
    ] {
        let configuration = format!(
            "{CONFIGURATION}{}authored=[{{source='source:legal',target='{target}'}}]\n",
            publication("site:one", &[binding("content:one-terms", "terms.md")])
        );
        let workspace::Error::Invalid(errors) =
            workspace::compile(&configuration, Default::default())
                .expect_err("internal corpus namespace is not an authored target")
        else {
            panic!("workspace diagnostic")
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_content_overlap")
        );
    }
    let placed = format!(
        "{CONFIGURATION}{}\n[[projects]]\nid='project:docs'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[placements]]\nsite='site:one'\nproject='project:docs'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={{source='source:legal',target='__legal/placed'}}\n",
        publication("site:one", &[binding("content:one-terms", "terms.md")])
    );
    let error = workspace::compile(&placed, Default::default())
        .expect_err("placement cannot overwrite private legal namespace");
    let workspace::Error::Invalid(errors) = error else {
        panic!("workspace diagnostic, got {error:?}")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_content_overlap")
    );
    let mut paths = std::collections::BTreeSet::new();
    for source_key in [
        "source:a/b",
        "source:a%2Fb",
        "source:a\\b",
        "source:a",
        "source:café",
    ] {
        let configuration = format!(
            "{CONFIGURATION}{}",
            publication("site:one", &[binding("content:one-terms", "terms.md")])
        )
        .replace("source:legal", source_key);
        let compiled = workspace::compile(&configuration, Default::default()).unwrap();
        let source = compiled.workspace.legal_sources().next().unwrap();
        let path = source.logical_path(&"terms.md".parse().unwrap()).unwrap();
        assert_eq!(source.entry(&path), Some("terms.md"));
        assert!(
            paths.insert(path),
            "distinct exact keys cannot alias the private coordinate"
        );
    }
}

const CONFIGURATION: &str = "\
[[principals]]\nid='person:publisher'\nkind='person'\n\
[[sites]]\nid='site:one'\npublisher='person:publisher'\ndeployable=false\n\
[[sites]]\nid='site:two'\npublisher='person:publisher'\ndeployable=false\n\
[[sources]]\nid='source:legal'\nlocation={kind='filesystem',root='/private/legal-root-never-read'}\n\
[[brands]]\nid='brand:one'\nname='One'\n";

fn key(value: &str) -> Key {
    value.parse().expect("explicit identity")
}

fn binding(id: &str, entry: &str) -> String {
    format!(
        "{{source='source:legal',entry='{entry}',source_content='content:owner-terms',id='{id}',route='/terms/',family='utility'}}"
    )
}

fn publication(site: &str, bindings: &[String]) -> String {
    format!(
        "[[brands.publications]]\nsite='{site}'\npublic_base_url='https://{}.test/mount/'\nlegal=[{}]\n",
        site.split(':').next_back().expect("site label"),
        bindings.join(",")
    )
}

fn compile(bindings: &[String]) -> workspace::Compilation {
    workspace::compile(
        &format!("{CONFIGURATION}{}", publication("site:one", bindings)),
        Default::default(),
    )
    .expect("strict native legal workspace")
}

fn source(extra: &str) -> String {
    format!(
        "+++\n[content]\nid='content:owner-terms'\ntitle='Terms of use'\ndescription='Owner-held terms.'\nlifecycle='draft'\nlanguage='en'\n{extra}+++\n\n# Owner’s terms\n\nExact retained **legal prose**, not an engine-authored notice.\n"
    )
}

fn corpus(text: &str) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        [Input {
            path: "retained/terms.md".parse().expect("logical path"),
            bytes: text.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real retained TOML and Comrak body")
}

fn document<'a, 's>(
    corpus: &'a ParsedCorpus<'s>,
) -> &'a gazette_compiler::document::ParsedDocument<'s> {
    let File::Document(document) = corpus
        .files()
        .get("retained/terms.md")
        .expect("selected entry")
    else {
        panic!("explicit reduced source remains a framed document");
    };
    document
}

#[test]
fn one_retained_owner_document_lowers_to_two_independent_native_utility_pages() {
    let configuration = format!(
        "{CONFIGURATION}{}{}",
        publication("site:one", &[binding("content:one-terms", "terms.md")]),
        publication("site:two", &[binding("content:two-terms", "terms.md")])
    );
    let compiled = workspace::compile(&configuration, Default::default())
        .expect("cross-site source reuse is explicit and valid");
    assert_eq!(compiled.workspace.content_bindings().count(), 0);
    let text = source("");
    let corpus = corpus(&text);
    let retained = document(&corpus);
    let original_html = retained.render_html().expect("original retained body");
    let mut nodes = Vec::new();
    for site in ["site:one", "site:two"] {
        let publication = compiled
            .workspace
            .publication_for(&key(site))
            .expect("publication");
        let lowered =
            authored::lower_legal_source(retained, &publication.site, &publication.legal[0])
                .expect("explicit legal lowering");
        assert!(std::ptr::eq(lowered.document, retained));
        assert_eq!(lowered.source_content, key("content:owner-terms"));
        assert_eq!(lowered.document.source(), text);
        assert_eq!(
            lowered.document.source_sha256(),
            Sha256Digest::of(text.as_bytes())
        );
        assert_eq!(lowered.document.render_html().unwrap(), original_html);
        assert_eq!(lowered.content.id, publication.legal[0].id);
        assert_eq!(lowered.content.site, publication.site);
        assert_eq!(lowered.content.lifecycle, ContentLifecycle::Draft);
        assert_eq!(lowered.content.title, "Terms of use");
        assert_eq!(
            lowered.content.description.as_deref(),
            Some("Owner-held terms.")
        );
        assert_eq!(lowered.content.language.as_ref().unwrap().as_str(), "en");
        assert_eq!(
            lowered.content.structure,
            ContentKind::Page {
                family: PageFamily::Utility
            }
        );
        assert!(lowered.content.attributions.is_empty());
        assert!(lowered.content.taxonomies.is_empty());
        assert!(lowered.content.keywords.is_empty());
        assert!(lowered.content.schema_type.is_none());
        assert!(lowered.content.license.is_none());
        assert!(lowered.content.published.is_none());
        assert!(lowered.content.modified.is_none());
        let route = lowered
            .content
            .route
            .as_ref()
            .expect("explicit output route");
        assert_eq!(route.path.as_str(), "/terms/");
        assert!(route.canonical.is_none());
        assert!(route.aliases.is_empty());
        assert!(route.mirrors.is_empty());
        nodes.push(lowered.content);
    }
    let graph = compiled
        .resolve_content(
            Declaration {
                browsing: vec![],
                nodes,
                memberships: vec![],
                related: vec![],
                projects: vec![],
            },
            Default::default(),
        )
        .expect("ordinary utility nodes use the existing native graph and routes");
    assert_eq!(graph.graph().nodes().len(), 2);
    assert!(graph.graph().node(&key("content:owner-terms")).is_none());
    assert_eq!(
        graph.route(&key("content:one-terms")).unwrap().url,
        "https://one.test/mount/terms/"
    );
    assert_eq!(
        graph.route(&key("content:two-terms")).unwrap().url,
        "https://two.test/mount/terms/"
    );
    assert!(
        authored::resolve(&compiled, &corpus, Default::default()).is_err(),
        "an unbound reduced document is not silently admitted as an ordinary authored page"
    );
}

#[test]
fn source_identity_mismatch_keeps_original_digest_and_exact_identity_span() {
    let compiled = compile(&[binding("content:one-terms", "terms.md")]);
    let mut binding = compiled
        .workspace
        .publication_for(&key("site:one"))
        .unwrap()
        .legal[0]
        .clone();
    binding.source_content = key("content:different-source");
    let text = source("");
    let corpus = corpus(&text);
    let error = authored::lower_legal_source(document(&corpus), &key("site:one"), &binding)
        .expect_err("no substitution by title, filename or output identity");
    assert_eq!(error.code(), "legal_source_identity");
    let LegalSourceError::Identity {
        expected,
        actual,
        source_sha256,
        span,
    } = error
    else {
        panic!("identity diagnostic");
    };
    assert_eq!(expected, binding.source_content);
    assert_eq!(actual, key("content:owner-terms"));
    assert_eq!(source_sha256, Sha256Digest::of(text.as_bytes()));
    let span = span.expect("original id field");
    assert_eq!(&text[span.start..span.end], "'content:owner-terms'");
}

#[test]
fn reduced_profile_rejects_page_fields_unknown_tables_and_invalid_native_values() {
    let compiled = compile(&[binding("content:one-terms", "terms.md")]);
    let binding = &compiled
        .workspace
        .publication_for(&key("site:one"))
        .unwrap()
        .legal[0];
    for extra in [
        "site='site:one'\n",
        "route={path='/terms/'}\n",
        "structure={kind='page',family={kind='utility'}}\n",
        "attributions=[]\n",
        "published='2026-01-01'\n",
        "keywords=['legal']\n",
        "license='https://example.test/license'\n",
        "unknown=true\n",
        "[body]\ntext='replacement'\n",
    ] {
        let text = source(extra);
        let corpus = corpus(&text);
        let error = authored::lower_legal_source(document(&corpus), &key("site:one"), binding)
            .expect_err("only the reduced source profile is consumed");
        assert_eq!(error.code(), "legal_source_metadata", "{extra}");
        assert!(std::error::Error::source(&error).is_some());
        let LegalSourceError::Metadata {
            source_sha256,
            span,
            ..
        } = error
        else {
            panic!("metadata diagnostic");
        };
        assert_eq!(source_sha256, Sha256Digest::of(text.as_bytes()));
        if let Some(span) = span {
            assert!(span.start >= document(&corpus).metadata_span().start);
            assert!(span.end <= document(&corpus).metadata_span().end);
        }
    }
    for (from, to) in [
        ("lifecycle='draft'", "lifecycle='approved'"),
        ("language='en'", "language='not a language'"),
    ] {
        let text = source("").replace(from, to);
        let corpus = corpus(&text);
        assert!(
            authored::lower_legal_source(document(&corpus), &key("site:one"), binding).is_err()
        );
    }
}

#[test]
fn omitted_description_and_language_do_not_acquire_defaults() {
    let compiled = compile(&[binding("content:one-terms", "terms.md")]);
    let text = source("")
        .replace("description='Owner-held terms.'\n", "")
        .replace("language='en'\n", "");
    let corpus = corpus(&text);
    let lowered = authored::lower_legal_source(
        document(&corpus),
        &key("site:one"),
        &compiled
            .workspace
            .publication_for(&key("site:one"))
            .unwrap()
            .legal[0],
    )
    .unwrap();
    assert!(lowered.content.description.is_none());
    assert!(lowered.content.language.is_none());
    assert_eq!(lowered.content.lifecycle, ContentLifecycle::Draft);
}

#[test]
fn legal_bindings_use_native_confined_paths_closed_family_and_plural_tables() {
    let declared = binding("content:one-terms", "legal/terms.md");
    let compiled = compile(std::slice::from_ref(&declared));
    assert_eq!(
        compiled
            .workspace
            .publication_for(&key("site:one"))
            .unwrap()
            .legal[0]
            .entry
            .as_str(),
        "legal/terms.md"
    );
    for invalid in [
        declared.replace("family='utility'", "family='article'"),
        declared.replace("family='utility'", "family='collection'"),
        declared.replace("family='utility'", "family={kind='utility'}"),
        declared.replace("entry='legal/terms.md'", "entry='../terms.md'"),
        declared.replace("entry='legal/terms.md'", "entry='/terms.md'"),
        declared.replace("entry='legal/terms.md'", "entry='legal/./terms.md'"),
        declared.replace("route='/terms/'", "route='terms/'"),
        declared.replace("route='/terms/'", "route='/../terms/'"),
        declared.replace("route='/terms/'", "route='/terms/?q=one'"),
        declared.replace("family='utility'", "family='utility',target='invented'"),
    ] {
        assert!(
            workspace::compile(
                &format!(
                    "{CONFIGURATION}{}",
                    publication("site:one", std::slice::from_ref(&invalid))
                ),
                Default::default()
            )
            .is_err(),
            "{invalid}"
        );
    }
    let tables = format!(
        "{CONFIGURATION}[[brands.publications]]\nsite='site:one'\n[[brands.publications.legal]]\nsource='source:legal'\nentry='terms.md'\nsource_content='content:owner-terms'\nid='content:one-terms'\nroute='/terms/'\nfamily='utility'\n"
    );
    let parsed = workspace::compile(&tables, Default::default()).expect("native array tables");
    assert_eq!(
        parsed
            .workspace
            .publication_for(&key("site:one"))
            .unwrap()
            .legal
            .len(),
        1
    );
    for value in [
        binding("content:one-terms", "terms.md"),
        "[['source:legal','terms.md']]".into(),
    ] {
        let singular =
            format!("{CONFIGURATION}[[brands.publications]]\nsite='site:one'\nlegal={value}\n");
        assert!(workspace::compile(&singular, Default::default()).is_err());
    }
}

#[test]
fn source_references_duplicate_output_ids_and_duplicate_site_bindings_fail_statically() {
    let first = binding("content:one-terms", "terms.md");
    for (bindings, code) in [
        (
            vec![first.replace("source='source:legal'", "source='source:missing'")],
            "workspace_reference",
        ),
        (
            vec![
                first.clone(),
                first.replace("entry='terms.md'", "entry='privacy.md'"),
            ],
            "workspace_duplicate",
        ),
        (
            vec![
                first.clone(),
                first.replace("id='content:one-terms'", "id='content:duplicate-body'"),
            ],
            "workspace_legal_binding_duplicate",
        ),
    ] {
        let error = workspace::compile(
            &format!("{CONFIGURATION}{}", publication("site:one", &bindings)),
            Default::default(),
        )
        .expect_err("complete validation before IO");
        let workspace::Error::Invalid(errors) = error else {
            panic!("static workspace diagnostic")
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == code && error.field.contains(".legal."))
        );
    }
    let conflicting = format!(
        "{CONFIGURATION}{}{}",
        publication("site:one", std::slice::from_ref(&first)),
        publication("site:two", &[first])
    );
    let workspace::Error::Invalid(errors) = workspace::compile(&conflicting, Default::default())
        .expect_err("IDs remain workspace-global")
    else {
        panic!("duplicate diagnostic")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_duplicate"
                && error.key.as_ref() == Some(&key("content:one-terms")))
    );
}

#[test]
fn lowered_source_text_uses_the_existing_whole_graph_scalar_validators() {
    let compiled = compile(&[binding("content:one-terms", "terms.md")]);
    let binding = &compiled
        .workspace
        .publication_for(&key("site:one"))
        .unwrap()
        .legal[0];
    for (from, to, code) in [
        ("title='Terms of use'", "title='   '", "content_title"),
        (
            "description='Owner-held terms.'",
            "description='   '",
            "content_description",
        ),
    ] {
        let text = source("").replace(from, to);
        let corpus = corpus(&text);
        let lowered = authored::lower_legal_source(document(&corpus), &key("site:one"), binding)
            .expect("typed source lowering does not duplicate whole-graph rules");
        let error = compiled
            .resolve_content(
                Declaration {
                    browsing: vec![],
                    nodes: vec![lowered.content],
                    memberships: vec![],
                    related: vec![],
                    projects: vec![],
                },
                Default::default(),
            )
            .expect_err("native graph remains authoritative for scalar validity");
        let gazette_core::publication::content::Error::Invalid(errors) = error else {
            panic!("native graph scalar diagnostic");
        };
        assert!(errors.iter().any(|error| error.code == code));
    }
}

#[test]
fn native_schemas_describe_reduced_sources_and_explicit_legal_bindings() {
    let schema = serde_json::to_value(authored::legal_source_schema()).expect("source schema");
    let validator = jsonschema::validator_for(&schema).expect("native source schema compiles");
    let source = json!({"content":{"id":"content:owner-terms","title":"Terms","description":"Owner text","lifecycle":"draft","language":"en"}});
    assert!(validator.is_valid(&source));
    let mut forbidden = source.clone();
    forbidden["content"]["site"] = json!("site:corporate");
    assert!(!validator.is_valid(&forbidden));
    forbidden = source;
    forbidden["content"]["structure"] = json!({"kind":"page","family":{"kind":"utility"}});
    assert!(!validator.is_valid(&forbidden));
    let schema = serde_json::to_value(contracts::workspace()).expect("workspace schema");
    assert_eq!(
        schema["$defs"]["BrandPublication"]["properties"]["legal"]["type"],
        "array"
    );
    assert!(
        schema["$defs"]["LegalBinding"]["properties"]
            .get("target")
            .is_none()
    );
    assert_eq!(
        schema["$defs"]["LegalBinding"]["properties"]["aliases"]["type"],
        "array"
    );
    assert!(
        !schema["$defs"]["LegalBinding"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "aliases")
    );
    assert_eq!(
        schema["$defs"]["LegalBinding"]["additionalProperties"],
        false
    );
}

#[test]
fn legal_binding_aliases_redirect_released_addresses_through_the_shared_route_checks() {
    let declared = binding("content:one-terms", "terms.md").replace(
        "route='/terms/'",
        "route='/terms/',aliases=['/tnc-policy/']",
    );
    let compiled = compile(std::slice::from_ref(&declared));
    let selected = compiled
        .workspace
        .publication_for(&key("site:one"))
        .unwrap();
    assert_eq!(selected.legal[0].aliases[0].as_str(), "/tnc-policy/");
    let text = source("");
    let corpus = corpus(&text);
    let lowered =
        authored::lower_legal_source(document(&corpus), &selected.site, &selected.legal[0])
            .unwrap();
    let route = lowered.content.route.as_ref().unwrap();
    assert_eq!(route.aliases[0].as_str(), "/tnc-policy/");
    let graph = compiled
        .resolve_content(
            Declaration {
                browsing: vec![],
                nodes: vec![lowered.content],
                memberships: vec![],
                related: vec![],
                projects: vec![],
            },
            Default::default(),
        )
        .expect("a legal alias uses the ordinary route compiler");
    let resolved = graph.route(&key("content:one-terms")).unwrap();
    assert_eq!(resolved.aliases.len(), 1);
    assert_eq!(
        resolved.aliases[0].url,
        "https://one.test/mount/tnc-policy/"
    );
    assert_eq!(resolved.aliases[0].output.as_str(), "tnc-policy/index.html");

    for aliases in ["['/terms/']", "['/old/','/old/']", "['/terms/index.html']"] {
        let invalid = binding("content:one-terms", "terms.md").replace(
            "route='/terms/'",
            &format!("route='/terms/',aliases={aliases}"),
        );
        let compiled = compile(std::slice::from_ref(&invalid));
        let selected = compiled
            .workspace
            .publication_for(&key("site:one"))
            .unwrap();
        let lowered =
            authored::lower_legal_source(document(&corpus), &selected.site, &selected.legal[0])
                .unwrap();
        let error = compiled
            .resolve_content(
                Declaration {
                    browsing: vec![],
                    nodes: vec![lowered.content],
                    memberships: vec![],
                    related: vec![],
                    projects: vec![],
                },
                Default::default(),
            )
            .expect_err("self, duplicate and overlapping aliases fail");
        let gazette_core::publication::content::Error::Invalid(errors) = error else {
            panic!("route diagnostic for {aliases}");
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == "content_route_collision"),
            "{aliases}: {errors:?}"
        );
    }
    let relative = binding("content:one-terms", "terms.md")
        .replace("route='/terms/'", "route='/terms/',aliases=['tnc-policy/']");
    assert!(
        workspace::compile(
            &format!("{CONFIGURATION}{}", publication("site:one", &[relative])),
            Default::default()
        )
        .is_err(),
        "an alias is a native site-relative route path"
    );
}
