//! Explicit browsing is authored Content, independently of editorial residence.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};

fn configuration() -> workspace::Compilation {
    configuration_with("")
}
fn configuration_with(extra: &str) -> workspace::Compilation {
    workspace::compile(
        &format!("[[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[sources]]\nid='source:content'\nlocation={{kind='repository',root='/private/not-read',commit='1111111111111111111111111111111111111111',subdirectory='content'}}\n\
         [[brands]]\nid='brand:one'\nname='Browsing fixture'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/mount/'\nauthored=[{{source='source:content',target='one'}}]\n{extra}"),
        Default::default(),
    )
    .expect("native workspace and exact source binding")
}

fn directory() -> &'static str {
    "+++\n[content]\nid='content:directory'\nsite='site:one'\ntitle='Existing directory'\nlifecycle='draft'\nroute={path='/unrelated-index/'}\nstructure={kind='page',family={kind='index'}}\n+++\n# An authored directory\n"
}

#[test]
fn markdown_and_landing_browsing_share_the_native_metadata_owner() {
    let markdown = "+++\nbrowsing=['content:directory']\n[content]\nid='content:markdown'\nsite='site:one'\ntitle='Markdown project'\nlifecycle='draft'\nroute={path='/different/'}\nstructure={kind='page',family={kind='project'}}\n+++\n# A project\n";
    let landing = "format: gazette-landing/v1\nbrowsing: ['content:directory']\ncontent:\n  id: content:landing\n  site: site:one\n  title: Landing project\n  lifecycle: draft\n  route: {path: /another/}\n  structure: {kind: page, family: {kind: project}}\nsections:\n- heading: {level: 1, text: A project}\n  body: Retained narrative.\n";
    let collection = landing
        .replace("/another/", "/root-product/")
        .replace("content:landing", "collection:product")
        .replace(
            "{kind: page, family: {kind: project}}",
            "{kind: collection}",
        );
    let corpus = ParsedCorpus::parse(
        [
            ("one/directory/index.md", directory()),
            ("one/markdown/index.md", markdown),
            ("one/landing/index.landing.yaml", landing),
            ("one/product/index.landing.yaml", collection.as_str()),
        ]
        .into_iter()
        .map(|(path, text)| Input {
            path: path.parse().expect("retained logical path"),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native TOML and landing syntax");
    let compiled = configuration();
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .expect("explicit browsing must resolve through the shared metadata contract");
    for id in ["content:markdown", "content:landing", "collection:product"] {
        let id = id.parse().unwrap();
        let paths = resolved.project_browsing_locations(&id, None).unwrap();
        let json = serde_json::to_value(&paths).unwrap();
        assert_eq!(json[0]["url"], "https://one.test/mount/unrelated-index/");
        assert_eq!(json[0]["via"], id.as_str());
        assert_eq!(json[0]["kind"], "declared");
        assert!(json[0].get("page_count").is_none());
        assert_eq!(
            resolved
                .content()
                .graph()
                .browsing()
                .iter()
                .filter(|edge| edge.source == id)
                .map(|edge| edge.index.as_str())
                .collect::<Vec<_>>(),
            ["content:directory"]
        );
        assert!(
            resolved.content().graph().editorial_parent(&id).is_none(),
            "browsing never creates an editorial parent"
        );
    }
}

fn parse(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}
fn project_page() -> String {
    "+++\nbrowsing=['content:directory']\n[content]\nid='content:page'\nsite='site:one'\ntitle='Page'\nlifecycle='draft'\nroute={path='/standalone/'}\nstructure={kind='page',family={kind='project'}}\n+++\n# Retained narrative\n".into()
}
const HIDDEN: &str = "[[projects]]\nid='project:hidden'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n[[placements]]\nsite='site:one'\nproject='project:hidden'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={source='source:content',target='hidden'}\n";

#[test]
fn discovery_never_reintroduces_a_hidden_directory_or_hidden_declaring_page() {
    for hidden_index in [true, false] {
        let files = vec![
            (
                (if hidden_index {
                    "hidden/index.md"
                } else {
                    "one/index.md"
                })
                .into(),
                directory().replace("Existing directory", "Secret directory"),
            ),
            (
                (if hidden_index {
                    "one/page.md"
                } else {
                    "hidden/page.md"
                })
                .into(),
                project_page(),
            ),
        ];
        let corpus = parse(&files);
        let compiled = configuration_with(HIDDEN);
        let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
        let discovery = resolved.discovery().unwrap();
        let id = "content:page".parse().unwrap();
        assert_eq!(
            resolved
                .project_browsing_locations(&id, None)
                .unwrap()
                .len(),
            1
        );
        assert!(
            resolved
                .project_browsing_locations(&id, Some(&discovery))
                .unwrap()
                .is_empty()
        );
        let public = serde_json::to_string(
            &resolved
                .project_discovery_graph(
                    &"site:one".parse().unwrap(),
                    Default::default(),
                    &discovery,
                )
                .unwrap(),
        )
        .unwrap();
        assert!(!public.contains("Secret directory") || !hidden_index);
        assert!(
            !public.contains("\"browsing\""),
            "no public relation may expose either suppressed endpoint"
        );
        if hidden_index {
            assert!(!public.contains("content:directory"));
        }
    }
}

#[test]
fn multiple_paths_keep_literal_titles_and_preview_routes_without_ancestry() {
    let second = directory()
        .replace("content:directory", "content:second")
        .replace("Existing directory", "Second <index> & title")
        .replace("/unrelated-index/", "/other-directory/");
    let files = vec![
        ("one/index.md".into(), directory().into()),
        ("one/second.md".into(), second),
        (
            "one/page.md".into(),
            project_page().replace(
                "browsing=['content:directory']",
                "browsing=['content:second','content:directory']",
            ),
        ),
    ];
    let corpus = parse(&files);
    let compiled = configuration();
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .unwrap()
        .with_preview_urls(std::collections::BTreeMap::from([(
            "site:one".parse().unwrap(),
            "http://127.0.0.1:4197/mount/".parse().unwrap(),
        )]))
        .unwrap();
    let id = "content:page".parse().unwrap();
    let paths = resolved.project_browsing_locations(&id, None).unwrap();
    assert_eq!(
        paths.iter().map(|p| p.index.as_str()).collect::<Vec<_>>(),
        ["content:directory", "content:second"]
    );
    assert_eq!(paths[1].title, "Second <index> & title");
    assert_eq!(paths[1].url, "http://127.0.0.1:4197/mount/other-directory/");
    assert!(resolved.content().graph().editorial_parent(&id).is_none());
    let private = serde_json::to_value(
        resolved
            .project_site_graph(&"site:one".parse().unwrap(), Default::default())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(private["browsing"].as_array().unwrap().len(), 2);
    assert!(
        resolved
            .project_browsing_locations(&"content:directory".parse().unwrap(), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn independent_resolved_owner_cannot_supply_discovery_for_browse_paths() {
    let files = vec![
        ("one/index.md".into(), directory().into()),
        ("one/page.md".into(), project_page()),
    ];
    let corpus = parse(&files);
    let compiled = configuration();
    let first = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let second = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let foreign = second.discovery().unwrap();
    assert!(
        first
            .project_browsing_locations(&"content:page".parse().unwrap(), Some(&foreign))
            .is_err()
    );
}

#[test]
fn browsing_schema_and_reference_slot_use_the_single_noneditorial_metadata_field() {
    let schema = serde_json::to_value(authored::declaration_schema()).unwrap();
    assert!(schema["properties"].get("browsing").is_some());
    let source = project_page();
    let authoring = authored::AuthoredAuthoring::parse(&source, Default::default())
        .unwrap()
        .unwrap();
    let at = source.find("content:directory").unwrap();
    let slot = authoring.reference_slot(&source, at).unwrap().unwrap();
    assert_eq!(slot.field, authored::AuthoredReferenceField::BrowsingIndex);
    assert_eq!(
        slot.location.span.slice(&source),
        Some("'content:directory'")
    );
    assert!(authoring.editorial_field(&source, at).unwrap().is_none());
    for invalid in [
        source.replace(
            "browsing=['content:directory']",
            "browsing='content:directory'",
        ),
        source.replace("browsing=", "browse_typo="),
    ] {
        let files = vec![
            ("one/index.md".into(), directory().into()),
            ("one/page.md".into(), invalid),
        ];
        let corpus = parse(&files);
        assert!(authored::resolve(&configuration(), &corpus, Default::default()).is_err());
    }
}

#[test]
fn retained_browsing_view_bytes_have_an_exact_complete_projection_boundary() {
    let files = vec![
        (
            "one/index.md".into(),
            directory().replace(
                "Existing directory",
                &"A precise directory title ".repeat(12),
            ),
        ),
        ("one/page.md".into(), project_page()),
    ];
    let corpus = parse(&files);
    let compiled = configuration();
    let id = "content:page".parse().unwrap();
    let first = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let paths = first.project_browsing_locations(&id, None).unwrap();
    let bytes = serde_json::to_vec(&paths).unwrap().len();
    let exact = authored::resolve(
        &compiled,
        &corpus,
        gazette_core::publication::content::Limits {
            text_bytes: bytes,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        exact.project_browsing_locations(&id, None).unwrap().len(),
        1
    );
    let below = authored::resolve(
        &compiled,
        &corpus,
        gazette_core::publication::content::Limits {
            text_bytes: bytes - 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        below.project_browsing_locations(&id, None),
        Err(authored::ProjectionError::Limit(_))
    ));
}

#[test]
fn generated_and_declared_locations_share_one_complete_occurrence_and_byte_budget() {
    let files = vec![
        (
            "one/index.md".into(),
            directory().replace("# An authored directory\n", ""),
        ),
        (
            "one/page.md".into(),
            project_page().replace("family={kind='project'}", "family={kind='article'}"),
        ),
    ];
    let corpus = parse(&files);
    let compiled = configuration_with(
        "listings=[{content='content:directory',select={kind='standalone-articles'},order={first=[],remaining='title-then-id'},pagination={page_size=1,next_pages='page/{number}/'}}]\n",
    );
    let id = "content:page".parse().unwrap();
    let first = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let one = first.project_browsing_locations(&id, None).unwrap();
    let generated = authored::BrowsingLocation {
        kind: authored::BrowsingKind::Listing,
        page_number: Some(1),
        page_count: Some(2),
        ..one[0].clone()
    };
    let forged = authored::BrowsingLocation {
        url: "https://forged.invalid/",
        ..generated.clone()
    };
    assert!(matches!(
        first.project_browsing_locations_with(&id, None, [forged]),
        Err(authored::ProjectionError::Binding { .. })
    ));
    let supplied = authored::BrowsingLocation {
        title: "Invented title",
        ..generated.clone()
    };
    assert_eq!(
        first
            .project_browsing_locations_with(&id, None, [supplied])
            .unwrap()[0]
            .title,
        "Existing directory"
    );
    let both = first
        .project_browsing_locations_with(&id, None, [generated])
        .unwrap();
    assert_eq!(
        both.len(),
        2,
        "both supplied paths remain explicit without choosing a primary"
    );
    let bytes = serde_json::to_vec(&both).unwrap().len();
    let exact = authored::resolve(
        &compiled,
        &corpus,
        gazette_core::publication::content::Limits {
            text_bytes: bytes,
            edges: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let paths = exact.project_browsing_locations(&id, None).unwrap();
    let generated = authored::BrowsingLocation {
        kind: authored::BrowsingKind::Listing,
        page_number: Some(1),
        page_count: Some(2),
        ..paths[0].clone()
    };
    assert_eq!(
        exact
            .project_browsing_locations_with(&id, None, [generated])
            .unwrap()
            .len(),
        2
    );
    let below = authored::resolve(
        &compiled,
        &corpus,
        gazette_core::publication::content::Limits {
            text_bytes: bytes - 1,
            edges: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let paths = below.project_browsing_locations(&id, None).unwrap();
    let generated = authored::BrowsingLocation {
        kind: authored::BrowsingKind::Listing,
        page_number: Some(1),
        page_count: Some(2),
        ..paths[0].clone()
    };
    assert!(matches!(
        below.project_browsing_locations_with(&id, None, [generated]),
        Err(authored::ProjectionError::Limit(_))
    ));
    let occurrence = authored::resolve(
        &compiled,
        &corpus,
        gazette_core::publication::content::Limits {
            edges: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let paths = occurrence.project_browsing_locations(&id, None).unwrap();
    let generated = authored::BrowsingLocation {
        kind: authored::BrowsingKind::Listing,
        page_number: Some(1),
        page_count: Some(1),
        ..paths[0].clone()
    };
    assert!(matches!(
        occurrence.project_browsing_locations_with(&id, None, [generated]),
        Err(authored::ProjectionError::Limit(_))
    ));
}

#[test]
fn union_cannot_reintroduce_hidden_or_fabricated_generated_endpoints() {
    let files = vec![
        ("hidden/index.md".into(), directory().into()),
        ("one/page.md".into(), project_page()),
    ];
    let corpus = parse(&files);
    let compiled = configuration_with(HIDDEN);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let id = "content:page".parse().unwrap();
    let discovery = resolved.discovery().unwrap();
    let private = resolved.project_browsing_locations(&id, None).unwrap();
    let fabricated = authored::BrowsingLocation {
        kind: authored::BrowsingKind::Listing,
        page_number: Some(1),
        page_count: Some(1),
        url: "https://forged.invalid/",
        ..private[0].clone()
    };
    assert!(
        resolved
            .project_browsing_locations_with(&id, Some(&discovery), [fabricated])
            .unwrap()
            .is_empty(),
        "hidden index stays suppressed through the generated union branch"
    );
    let foreign = "content:unbound".parse().unwrap();
    let fabricated = authored::BrowsingLocation {
        index: &foreign,
        kind: authored::BrowsingKind::Listing,
        page_number: Some(1),
        page_count: Some(1),
        ..private[0].clone()
    };
    assert!(
        matches!(
            resolved.project_browsing_locations_with(&id, None, [fabricated]),
            Err(authored::ProjectionError::Binding { .. })
        ),
        "in-process DTOs do not grant a foreign graph identity authority"
    );
}
