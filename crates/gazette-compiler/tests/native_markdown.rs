//! Configured native defaults and real compiler projections, without native effects.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::{ContentLifecycle, content::ContentKind};

fn compilation(profile: &str) -> workspace::Compilation {
    workspace::compile(&configuration(profile), Default::default()).expect("workspace")
}

fn configuration(profile: &str) -> String {
    format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
        [[brands]]\nid='brand:one'\nname='Example'\n\
        [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/mount/'\n\
        [[projects]]\nid='project:library'\nlifecycle='active'\nvisibility='private'\navailability='unavailable'\n\
        [[sources]]\nid='source:docs'\nlocation={{kind='repository',root='/private/source',commit='1111111111111111111111111111111111111111',subdirectory='docs'}}\n\
        [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:library'\nrender=true\ndiscoverable=false\ndocuments={{source='source:docs',target='native/library'}}\n{profile}\n"
    )
}

const PROFILE: &str = "native_markdown={route_prefix='/docs/library/',lifecycle='published'}";

fn parse<'a>(files: &[(&str, &'a str)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().expect("path"),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained syntax")
}

#[test]
fn configured_native_defaults_lower_without_synthetic_frontmatter_or_subjects() {
    let compilation = compilation(PROFILE);
    let text = "# A **native** `guide`\n\nOriginal body.\n";
    let corpus = parse(&[("native/library/README.md", text)]);
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("native lowering");
    resolved
        .validate_source_bindings()
        .expect("same provenance path");
    let node = resolved
        .content()
        .graph()
        .nodes()
        .next()
        .expect("native node");
    assert_eq!(node.title, "A native guide");
    assert_eq!(node.lifecycle, ContentLifecycle::Published);
    assert!(node.id.as_str().starts_with("native-document:"));
    assert!(matches!(node.structure, ContentKind::Page { .. }));
    assert!(resolved.document(&node.id).is_none());
    assert!(resolved.origin(&node.id, &[]).is_none());
    assert_eq!(
        resolved
            .file(&node.id)
            .expect("original file")
            .body()
            .expect("body")
            .source(),
        text
    );
    let page = resolved
        .project_page(
            &node.id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("native body projection");
    assert!(page.body_html.contains("<strong>native</strong>"));
    assert!(
        page.data.projects.is_empty()
            && page.data.collections.is_empty()
            && page.data.journeys.is_empty()
    );
    assert_eq!(
        page.data.route.expect("shared route").canonical_url,
        "https://example.test/mount/docs/library/"
    );
    let value = serde_json::to_value(page).expect("same model");
    assert_eq!(value["derivation"]["title"]["kind"], "heading");
    assert_eq!(value["provenance"]["project"], "project:library");
    assert_eq!(value["derivation"]["title"]["span"]["start"], 0);
    let serialized = serde_json::to_string(&value).expect("serialized model");
    for private in ["/private/source", "source:docs", "native/library/README.md"] {
        assert!(!serialized.contains(private));
    }
}

#[test]
fn native_language_default_never_overwrites_framed_metadata_or_infers_filename_language() {
    let profile =
        "native_markdown={route_prefix='/docs/library/',lifecycle='published',language='EN-gb'}";
    let compilation = compilation(profile);
    let plain = "# Native guide\n\nUnchanged source.\n";
    let explicit = "+++\n[content]\nid='doc:language'\nsite='site:one'\ntitle='Explicit'\nlifecycle='draft'\nlanguage='HI-in'\nroute={path='/explicit/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Explicit body\n";
    let omitted = "+++\n[content]\nid='doc:omitted'\nsite='site:one'\ntitle='Undeclared'\nlifecycle='archived'\nroute={path='/omitted/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Undeclared body\n";
    let corpus = parse(&[
        ("native/library/guide.fr.md", plain),
        ("native/library/explicit.fr.md", explicit),
        ("native/library/omitted.fr.md", omitted),
    ]);
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("one native default with distinct explicit metadata");
    let field = [
        gazette_compiler::toml::PathSegment::Key("content".into()),
        gazette_compiler::toml::PathSegment::Key("language".into()),
    ];
    let native = resolved
        .content()
        .graph()
        .nodes()
        .find(|node| node.id.as_str().starts_with("native-document:"))
        .expect("plain native document");
    assert_eq!(
        native.language.as_ref().expect("selected default").as_str(),
        "en-GB"
    );
    assert!(native.taxonomies.is_empty());
    assert!(resolved.origin(&native.id, &field).is_none());
    assert!(
        resolved
            .derivation(&native.id)
            .expect("native derivation")
            .language_from_profile
    );
    assert_eq!(
        resolved
            .file(&native.id)
            .expect("native source")
            .body()
            .expect("native body")
            .source(),
        plain
    );
    assert_eq!(
        resolved
            .content()
            .route(&native.id)
            .expect("literal filename route")
            .canonical_url,
        "https://example.test/mount/docs/library/guide.fr/"
    );
    let explicit_id = "doc:language".parse().expect("explicit identity");
    let declared = resolved
        .project_page(
            &explicit_id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("explicit projection");
    assert_eq!(
        declared
            .data
            .content
            .language
            .as_ref()
            .expect("explicit language")
            .as_str(),
        "hi-IN"
    );
    assert_eq!(declared.data.content.lifecycle, ContentLifecycle::Draft);
    let origin = resolved
        .origin(&explicit_id, &field)
        .expect("original language declaration");
    let span = origin.span.expect("exact declaration token");
    assert_eq!(&explicit[span.start..span.end], "'HI-in'");
    let omitted_id = "doc:omitted".parse().expect("omitted identity");
    let omitted_node = resolved
        .content()
        .graph()
        .node(&omitted_id)
        .expect("framed omission");
    assert!(omitted_node.language.is_none());
    assert!(omitted_node.taxonomies.is_empty());
    assert!(resolved.origin(&omitted_id, &field).is_none());
    for (id, original) in [
        (&native.id, plain),
        (&explicit_id, explicit),
        (&omitted_id, omitted),
    ] {
        assert_eq!(
            resolved
                .file(id)
                .expect("retained original")
                .source_sha256(),
            gazette_core::source::Sha256Digest::of(original.as_bytes())
        );
    }
    let without_default = workspace::compile(&configuration(PROFILE), Default::default())
        .expect("profile without language");
    let rebuilt = authored::resolve(&without_default, &corpus, Default::default())
        .expect("no filename language inference");
    let unchanged_identity = rebuilt
        .content()
        .graph()
        .node(&native.id)
        .expect("language does not change identity");
    assert!(unchanged_identity.language.is_none());
    assert!(unchanged_identity.taxonomies.is_empty());
    assert!(
        !rebuilt
            .derivation(&native.id)
            .expect("native derivation")
            .language_from_profile
    );
}

#[test]
fn coordinate_identity_survives_content_pin_mount_and_corpus_prefix_changes() {
    let first = compilation(PROFILE);
    let corpus = parse(&[("native/library/guide.md", "# Original\n")]);
    let original = authored::resolve(&first, &corpus, Default::default()).expect("first");
    let old = original.content().graph().nodes().next().expect("node");
    let changed_config = configuration(PROFILE)
        .replace("/docs/library/", "/different/")
        .replace("/private/source", "/different/source")
        .replace(
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
        )
        .replace("native/library", "moved/logical");
    let changed = workspace::compile(&changed_config, Default::default()).expect("changed config");
    let body = parse(&[(
        "moved/logical/guide.md",
        "# Changed\n\nDifferent content.\n",
    )]);
    let rebuilt = authored::resolve(&changed, &body, Default::default()).expect("rebuilt");
    let new = rebuilt.content().graph().nodes().next().expect("node");
    assert_eq!(old.id, new.id);
    assert_ne!(old.title, new.title);
    assert_ne!(old.route, new.route);
    let renamed = workspace::compile(
        &changed_config.replace("source:docs", "source:renamed"),
        Default::default(),
    )
    .expect("different source identity");
    let renamed = authored::resolve(&renamed, &body, Default::default()).expect("renamed binding");
    assert_ne!(
        new.id,
        renamed.content().graph().nodes().next().expect("node").id
    );
}

#[test]
fn filenames_and_explicit_metadata_keep_distinct_precedence_and_origins() {
    let compilation = compilation(PROFILE);
    let explicit = "+++\n[content]\nid='doc:explicit'\nsite='site:one'\ntitle='Explicit'\nlifecycle='archived'\nroute={path='/chosen/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Other body heading\n";
    let corpus = parse(&[
        ("native/library/Basic Guide.md", "No heading.\n"),
        ("native/library/explicit.md", explicit),
    ]);
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("mixed metadata");
    let explicit_id = "doc:explicit".parse().expect("key");
    let page = resolved
        .project_page(
            &explicit_id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("explicit page");
    assert_eq!(page.data.content.title, "Explicit");
    assert_eq!(page.data.content.lifecycle, ContentLifecycle::Archived);
    assert_eq!(
        page.data.route.expect("explicit route").canonical_url,
        "https://example.test/mount/chosen/"
    );
    assert!(page.data.derivation.is_none());
    let native = resolved
        .content()
        .graph()
        .nodes()
        .find(|node| node.id != explicit_id)
        .expect("native node");
    assert_eq!(native.title, "Basic Guide");
    assert_eq!(
        resolved
            .content()
            .route(&native.id)
            .expect("native route")
            .canonical_url,
        "https://example.test/mount/docs/library/Basic%20Guide/"
    );
    assert!(matches!(
        resolved.derivation(&native.id).expect("derivation").title,
        authored::NativeTitle::Filename
    ));
}

#[test]
fn missing_profiles_ambiguous_frontmatter_and_colliding_routes_never_yield_a_subset() {
    let corpus = parse(&[("native/library/README.md", "# Native\n")]);
    assert!(matches!(
        authored::resolve(&compilation(""), &corpus, Default::default()),
        Err(authored::Error::Files(_))
    ));
    for text in [
        "---\ndraft: true\n---\n# Held\n",
        " +++\ndraft=true\n+++\n# Held\n",
        "\u{feff}---\ndraft: true\n---\n# Held\n",
        "\u{feff}+++\ndraft=true\n+++\n# Held\n",
        "--- # metadata\ndraft: true\n---\n# Held\n",
        "+++ # metadata\ndraft=true\n+++\n# Held\n",
        "\n  \n---\ndraft: true\n---\n# Held\n",
    ] {
        let corpus = parse(&[("native/library/README.md", text)]);
        let error = authored::resolve(&compilation(PROFILE), &corpus, Default::default())
            .expect_err("no metadata-to-body downgrade");
        assert!(
            matches!(error, authored::Error::Files(errors) if matches!(errors[0].error, authored::FileError::Native(authored::NativeError::Framing)))
        );
    }
    let duplicate = parse(&[
        ("native/library/README.md", "# A\n"),
        ("native/library/index.md", "# B\n"),
    ]);
    let error = authored::resolve(&compilation(PROFILE), &duplicate, Default::default())
        .expect_err("route collision");
    assert!(error.to_string().contains("content_route_collision"));
    for profile in [
        "native_markdown={route_prefix='/file.html',lifecycle='published'}",
        "native_markdown={route_prefix='/docs/'}",
        "native_markdown={route_prefix='/docs/',lifecycle='published',unknown=true}",
    ] {
        assert!(workspace::compile(&configuration(profile), Default::default()).is_err());
    }
}

#[test]
fn native_document_links_use_source_context_and_existing_routes_not_output_guessing() {
    let compilation = compilation(PROFILE);
    let corpus = parse(&[
        (
            "native/library/README.md",
            "# Home\n\n[Guide](nested/guide.md)\n\n`[Not a link](absent.md)`\n",
        ),
        (
            "native/library/nested/guide.md",
            "# Guide\n\n[Home](../README.md)\n",
        ),
    ]);
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("graph");
    assert_eq!(resolved.content().graph().nodes().len(), 2);
    for node in resolved.content().graph().nodes() {
        let page = resolved
            .project_page(
                &node.id,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("source-relative link resolution");
        if node.title == "Home" {
            assert!(
                page.body_html
                    .contains("href=\"https://example.test/mount/docs/library/nested/guide/\"")
            );
            assert!(page.body_html.contains("[Not a link](absent.md)"));
        } else {
            assert!(
                page.body_html
                    .contains("href=\"https://example.test/mount/docs/library/\"")
            );
        }
    }
    for target in [
        "absent.md",
        "../outside.md",
        "%2e%2e/outside.md",
        "README.md#unimplemented-anchor",
        "#unknown",
        "?mode=1#unknown",
        "./#unknown",
    ] {
        let text = format!("# Home\n\n[Target]({target})\n");
        let corpus = parse(&[("native/library/README.md", &text)]);
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("nodes");
        let node = resolved.content().graph().nodes().next().expect("node");
        assert_eq!(
            resolved
                .project_page(
                    &node.id,
                    &gazette_core::raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("no broken native link")
                .code(),
            "markdown_link_resolution"
        );
    }
    for (target, expected) in [
        ("?mode=1", "https://example.test/mount/docs/library/?mode=1"),
        ("#", "https://example.test/mount/docs/library/#"),
        ("./#", "https://example.test/mount/docs/library/#"),
        (
            "https://example.test/#external",
            "https://example.test/#external",
        ),
    ] {
        let text = format!("# Home\n\n[Target]({target})\n");
        let corpus = parse(&[("native/library/README.md", &text)]);
        let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("nodes");
        let node = resolved.content().graph().nodes().next().expect("node");
        let page = resolved
            .project_page(
                &node.id,
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("supported retained URL");
        assert!(page.body_html.contains(&format!("href=\"{expected}\"")));
    }
}
