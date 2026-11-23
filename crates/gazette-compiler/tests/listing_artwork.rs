//! Owner-authored artwork resolves through retained selection and native static assets.
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};

const CONFIG: &str = "principals=[{id='owner:one',kind='person'}]\nsites=[{id='site:one',publisher='owner:one',deployable=false}]\nsources=[{id='source:one',location={kind='filesystem',root='/not-read-one'}},{id='source:two',location={kind='filesystem',root='/not-read-two'}}]\n[[brands]]\nid='brand:one'\nname='Artwork fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.test/press/'\nauthored=[{source='source:one',target='one',static_assets={route_prefix='/assets/one/'}},{source='source:two',target='two',static_assets={route_prefix='/assets/two/'}}]\nlistings=[{content='content:index',select={kind='root-collections'},order={first=[],remaining='title-then-id'},pagination={page_size=1,next_pages='page/{number}/'}}]\n";
fn config() -> workspace::Compilation {
    workspace::compile(CONFIG, Default::default()).unwrap()
}
fn index() -> String {
    "+++\n[content]\nid='content:index'\nsite='site:one'\ntitle='Directory'\nlifecycle='published'\nroute={path='/directory/'}\nstructure={kind='page',family={kind='index'}}\n+++\n".into()
}
fn collection() -> String {
    "+++\n[listing_artwork]\ndefault={source='source:two',entry='media/default.webp'}\nlight={source='source:two',entry='media/light.png'}\n[content]\nid='collection:one'\nsite='site:one'\ntitle='First collection'\nlifecycle='published'\nroute={path='/first/'}\nstructure={kind='collection'}\n+++\nA retained account.\n".into()
}
fn yaml() -> String {
    "format: gazette-landing/v1\nlisting_artwork:\n  default: {source: 'source:two', entry: media/default.webp}\ncontent:\n  id: collection:two\n  site: site:one\n  title: Second collection\n  lifecycle: published\n  route: {path: /second/}\n  structure: {kind: collection}\nsections:\n- body: Another retained account.\n".into()
}
fn files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("one/index.md".into(), index().into_bytes()),
        ("one/first/index.md".into(), collection().into_bytes()),
        ("two/second/index.landing.yaml".into(), yaml().into_bytes()),
        (
            "two/media/default.webp".into(),
            b"opaque retained WebP transport".to_vec(),
        ),
        (
            "two/media/light.png".into(),
            b"opaque retained PNG transport".to_vec(),
        ),
    ]
}
fn parse(files: &[(String, Vec<u8>)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().unwrap(),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}
#[test]
fn toml_and_yaml_owner_selections_reach_existing_ordered_slices_with_native_urls() {
    let files = files();
    let corpus = parse(&files);
    let config = config();
    let resolved = authored::resolve(&config, &corpus, Default::default())
        .expect("strict shared Metadata accepts owner artwork");
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
        .unwrap();
    let json = serde_json::to_value(&pages).unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        json[0]["items"][0]["artwork"]["default"],
        "https://one.test/press/assets/two/media/default.webp"
    );
    assert_eq!(
        json[0]["items"][0]["artwork"]["light"],
        "https://one.test/press/assets/two/media/light.png"
    );
    assert!(json[0]["items"][0]["artwork"].get("dark").is_none());
    assert!(json[1]["items"][0]["artwork"].get("light").is_none());
}

#[test]
fn schema_and_original_toml_yaml_diagnostics_share_strict_native_fields() {
    let schema = serde_json::to_string(&authored::declaration_schema()).unwrap();
    assert!(schema.contains("listing_artwork") && schema.contains("default"));
    for invalid in [
        collection().replace(
            "default={source='source:two',entry='media/default.webp'}\n",
            "",
        ),
        collection().replace(
            "entry='media/default.webp'",
            "entry='media/default.webp',guess=true",
        ),
        collection().replace("entry='media/default.webp'", "entry='../default.webp'"),
        collection().replace(
            "default={source='source:two',entry='media/default.webp'}",
            "default='https://example.test/default.webp'",
        ),
    ] {
        let mut files = files();
        files[1].1 = invalid.into_bytes();
        let corpus = parse(&files);
        assert!(matches!(
            authored::resolve(&config(), &corpus, Default::default()),
            Err(authored::Error::Files(_))
        ));
    }
    for (position, source) in [
        (
            1,
            collection().replace("source='source:two'", "source='source:absent'"),
        ),
        (2, yaml().replace("source:two", "source:absent")),
    ] {
        let mut files = files();
        files[position].1 = source.into_bytes();
        let corpus = parse(&files);
        let Err(authored::Error::Artwork(error)) =
            authored::resolve(&config(), &corpus, Default::default())
        else {
            panic!("native artwork must refuse absent selected source")
        };
        assert_eq!(error.variant, "default");
        let origin = error.origin.unwrap();
        let span = origin.span.expect("original field location");
        assert!(
            std::str::from_utf8(&files[position].1).unwrap()[span.start..span.end]
                .contains("source:absent")
        );
    }
}

#[test]
fn selected_source_retention_file_type_and_transport_mapping_are_required() {
    for (entry, bytes) in [
        ("media/absent.webp", None),
        ("media/paper.pdf", Some(b"%PDF".to_vec())),
        ("second/index.landing.yaml", None),
    ] {
        let mut files = files();
        files[1].1 = collection()
            .replace("media/default.webp", entry)
            .into_bytes();
        if let Some(bytes) = bytes {
            files.push((format!("two/{entry}"), bytes));
        }
        let corpus = parse(&files);
        assert!(matches!(
            authored::resolve(&config(), &corpus, Default::default()),
            Err(authored::Error::Artwork(_))
        ));
    }
    for source in [
        CONFIG.replace(
            "static_assets={route_prefix='/assets/two/'}",
            "retained_only=['media/default.webp'],static_assets={route_prefix='/assets/two/'}",
        ),
        CONFIG.replace(",static_assets={route_prefix='/assets/two/'}", ""),
    ] {
        let files = files();
        let corpus = parse(&files);
        let compiled = workspace::compile(&source, Default::default()).unwrap();
        assert!(matches!(
            authored::resolve(&compiled, &corpus, Default::default()),
            Err(authored::Error::Artwork(_))
        ));
    }
    // A configured source declaration alone grants no content binding/acquisition.
    let source = CONFIG.replace(
        "sources=[",
        "sources=[{id='source:unused',location={kind='filesystem',root='/must-not-be-read'}},",
    );
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    let mut files = files();
    files[1].1 = collection()
        .replace("source='source:two'", "source='source:unused'")
        .into_bytes();
    let corpus = parse(&files);
    assert!(matches!(
        authored::resolve(&compiled, &corpus, Default::default()),
        Err(authored::Error::Artwork(_))
    ));
}

#[test]
fn candidate_dependency_closure_keeps_exact_assets_without_selecting_other_files() {
    use gazette_core::publication::content::Disposition;
    let mut files = files();
    files.push((
        "two/media/unused.webp".into(),
        b"unneeded opaque image".to_vec(),
    ));
    let corpus = parse(&files);
    let compiled = config();
    let first = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    assert_eq!(
        first
            .authoring_asset_paths()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["two/media/default.webp", "two/media/light.png"],
        "editor analysis retains required artwork without absorbing the media library"
    );
    let mut selection = first.non_draft_selection().unwrap();
    assert_eq!(
        selection.assets["two/media/default.webp"],
        Disposition::Include
    );
    assert_eq!(
        selection.assets["two/media/light.png"],
        Disposition::Include
    );
    assert_eq!(
        selection.assets["two/media/unused.webp"],
        Disposition::Exclude
    );
    let selected = first.into_selected(&selection).unwrap();
    assert!(
        selected
            .assets_for(&"site:one".parse().unwrap())
            .unwrap()
            .get(&"two/media/unused.webp".parse().unwrap())
            .is_none()
    );
    selection
        .assets
        .insert("two/media/light.png".parse().unwrap(), Disposition::Exclude);
    let fresh = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    assert!(matches!(
        fresh.into_selected(&selection),
        Err(authored::SelectionError::Artwork(_))
    ));
    // Excluding the declaring owner does not require or expose its unique variant.
    selection
        .content
        .insert("collection:one".parse().unwrap(), Disposition::Exclude);
    let fresh = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let selected = fresh.into_selected(&selection).unwrap();
    let discovery = selected.discovery().unwrap();
    let pages = selected
        .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
        .unwrap();
    let json = serde_json::to_string(&pages).unwrap();
    assert!(!json.contains("light.png") && !json.contains("collection:one"));
}

#[test]
fn preview_mount_and_no_artwork_use_existing_url_and_absence_contracts() {
    use std::collections::BTreeMap;
    let files = files();
    let corpus = parse(&files);
    let compiled = config();
    let resolved = authored::resolve(&compiled, &corpus, Default::default())
        .unwrap()
        .with_preview_urls(BTreeMap::from([(
            "site:one".parse().unwrap(),
            "http://127.0.0.1:4198/press/".parse().unwrap(),
        )]))
        .unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
        .unwrap();
    let json = serde_json::to_value(&pages).unwrap();
    assert_eq!(
        json[0]["items"][0]["artwork"]["default"],
        "http://127.0.0.1:4198/press/assets/two/media/default.webp"
    );
    assert_eq!(
        pages[1].url,
        "http://127.0.0.1:4198/press/directory/page/2/"
    );
    let mut absent = self::files();
    let original = collection();
    let artwork =
        &original[original.find("[listing_artwork]").unwrap()..original.find("[content]").unwrap()];
    absent[1].1 = original.replace(artwork, "").into_bytes();
    let corpus = parse(&absent);
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
        .unwrap();
    assert!(
        serde_json::to_value(&pages).unwrap()[0]["items"][0]
            .get("artwork")
            .is_none()
    );
}

#[test]
fn discovery_filters_owner_before_artwork_and_borrowed_selection_stays_bound() {
    let old = "{source='source:two',target='two',static_assets={route_prefix='/assets/two/'}}";
    let source = CONFIG.replace(&format!(",{old}"), "")
        + "\n[[projects]]\nid='project:hidden'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n[[placements]]\nsite='site:one'\nproject='project:hidden'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={source='source:two',target='two',static_assets={route_prefix='/assets/two/'}}\n";
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    let mut files = files();
    files[2].1 = yaml()
        .replace("media/default.webp", "media/secret.webp")
        .into_bytes();
    files.push((
        "two/media/secret.webp".into(),
        b"opaque private-owner decoration".to_vec(),
    ));
    let corpus = parse(&files);
    let first = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let second = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = first.discovery().unwrap();
    let json = serde_json::to_string(
        &first
            .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
            .unwrap(),
    )
    .unwrap();
    assert!(json.contains("default.webp"));
    assert!(
        !json.contains("secret.webp")
            && !json.contains("collection:two")
            && !json.contains("source:two")
            && !json.contains("\"entry\"")
            && !json.contains("provenance")
    );
    let graph = serde_json::to_string(
        &first
            .project_discovery_graph(&"site:one".parse().unwrap(), Default::default(), &discovery)
            .unwrap(),
    )
    .unwrap();
    assert!(
        !graph.contains("secret.webp") && !graph.contains("listing_artwork"),
        "artwork must not expand shallow graph/discovery node serialization"
    );
    let foreign = second.discovery().unwrap();
    assert!(matches!(
        first.project_listing_pages(&"site:one".parse().unwrap(), &foreign),
        Err(authored::ListingError::Discovery)
    ));
}

#[test]
fn rendition_occurrences_share_the_existing_complete_listing_budget() {
    use gazette_core::publication::content::Limits;
    let files = files();
    let corpus = parse(&files);
    let compiled = config();
    let id = "site:one".parse().unwrap();
    // Two item references, three rendition references and four numbered navigation links.
    let exact = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            edges: 9,
            ..Default::default()
        },
    )
    .unwrap();
    let discovery = exact.discovery().unwrap();
    assert_eq!(
        exact.project_listing_pages(&id, &discovery).unwrap().len(),
        2
    );
    let below = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            edges: 8,
            ..Default::default()
        },
    )
    .unwrap();
    let discovery = below.discovery().unwrap();
    assert!(matches!(
        below.project_listing_pages(&id, &discovery),
        Err(authored::ListingError::Limit(_))
    ));
    let complete = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = complete.discovery().unwrap();
    let pages = complete.project_listing_pages(&id, &discovery).unwrap();
    let bytes = pages
        .iter()
        .map(|page| serde_json::to_vec(page).unwrap().len())
        .sum::<usize>();
    let exact = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            text_bytes: bytes,
            ..Default::default()
        },
    )
    .unwrap();
    let discovery = exact.discovery().unwrap();
    assert_eq!(
        exact.project_listing_pages(&id, &discovery).unwrap().len(),
        2
    );
    let below = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            text_bytes: bytes - 1,
            ..Default::default()
        },
    )
    .unwrap();
    let discovery = below.discovery().unwrap();
    assert!(matches!(
        below.project_listing_pages(&id, &discovery),
        Err(authored::ListingError::Limit(_))
    ));
}

#[test]
fn optional_variants_are_exact_and_package_mapping_retains_native_precedence() {
    for (light, dark) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut files = files();
        let mut page = collection();
        if !light {
            page = page.replace("light={source='source:two',entry='media/light.png'}\n", "");
        }
        if dark {
            page = page.replace(
                "[content]",
                "dark={source='source:two',entry='media/dark.jpg'}\n[content]",
            );
            files.push((
                "two/media/dark.jpg".into(),
                b"opaque retained JPEG transport".to_vec(),
            ));
        }
        files[1].1 = page.into_bytes();
        let corpus = parse(&files);
        let compiled = config();
        let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
        let discovery = resolved.discovery().unwrap();
        let json = serde_json::to_value(
            resolved
                .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
                .unwrap(),
        )
        .unwrap();
        let art = &json[0]["items"][0]["artwork"];
        assert_eq!(art.get("light").is_some(), light);
        assert_eq!(art.get("dark").is_some(), dark);
    }
    let mut files = files();
    files[1].1 = collection()
        .replace(
            "default={source='source:two',entry='media/default.webp'}",
            "default={source='source:one',entry='package/चित्र frame.webp'}",
        )
        .into_bytes();
    files.push(("one/package/index.md".into(),"+++\n[content]\nid='content:package'\nsite='site:one'\ntitle='Package'\nlifecycle='published'\nroute={path='/package/'}\nstructure={kind='page',family={kind='utility'}}\n+++\nAn ordinary page.\n".as_bytes().to_vec()));
    files.push((
        "one/package/चित्र frame.webp".into(),
        b"retained package image".to_vec(),
    ));
    let corpus = parse(&files);
    let compiled = config();
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let pages = resolved
        .project_listing_pages(&"site:one".parse().unwrap(), &discovery)
        .unwrap();
    let json = serde_json::to_value(pages).unwrap();
    let url = json[0]["items"][0]["artwork"]["default"].as_str().unwrap();
    assert!(
        url.starts_with("https://one.test/press/package/")
            && url.contains("%20frame.webp")
            && !url.contains("assets/one")
    );
    let asset = resolved
        .assets_for(&"site:one".parse().unwrap())
        .unwrap()
        .get(&"one/package/चित्र frame.webp".parse().unwrap())
        .unwrap();
    assert_eq!(asset.url, url);
    assert_eq!(asset.content(), b"retained package image");
}

#[test]
fn foreign_and_ambiguous_content_bindings_do_not_grant_image_authority() {
    let source = CONFIG
        .replace(
            "sources=[",
            "sources=[{id='source:foreign',location={kind='filesystem',root='/not-read-foreign'}},",
        )
        .replace(
            "sites=[",
            "sites=[{id='site:other',publisher='owner:one',deployable=false},",
        )
        + "\n[[brands]]\nid='brand:other'\nname='Other'\n[[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.test/'\nauthored=[{source='source:foreign',target='foreign',static_assets={route_prefix='/assets/foreign/'}}]\n";
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    let mut files = files();
    files[1].1 = collection()
        .replace("source='source:two'", "source='source:foreign'")
        .into_bytes();
    files.push((
        "foreign/media/default.webp".into(),
        b"foreign image".to_vec(),
    ));
    files.push(("foreign/media/light.png".into(), b"foreign image".to_vec()));
    let corpus = parse(&files);
    assert!(matches!(
        authored::resolve(&compiled, &corpus, Default::default()),
        Err(authored::Error::Artwork(_))
    ));
    let source=CONFIG.replace("authored=[","authored=[{source='source:two',target='two-alias',static_assets={route_prefix='/assets/alias/'}},");
    {
        let compiled = workspace::compile(&source, Default::default()).unwrap();
        let files = self::files();
        let corpus = parse(&files);
        let Err(authored::Error::Artwork(error)) =
            authored::resolve(&compiled, &corpus, Default::default())
        else {
            panic!("ambiguous same-site binding cannot be chosen by first-match")
        };
        assert!(error.reason.contains("ambiguous"));
    }
}

#[test]
fn all_declared_renditions_share_the_existing_metadata_record_ceiling() {
    use gazette_core::publication::content::Limits;
    let mut files = files();
    files[1].1 = collection()
        .replace(
            "[content]",
            "dark={source='source:two',entry='media/default.webp'}\n[content]",
        )
        .into_bytes();
    let corpus = parse(&files);
    let compiled = config();
    // Three content nodes and four actual rendition declarations. The duplicate
    // image coordinate is still a separate retained declaration and occurrence.
    assert!(
        authored::resolve(
            &compiled,
            &corpus,
            Limits {
                nodes: 4,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        authored::resolve(
            &compiled,
            &corpus,
            Limits {
                nodes: 3,
                ..Default::default()
            }
        ),
        Err(authored::Error::Files(failures)) if failures.iter().any(|failure| matches!(&failure.error, authored::FileError::Resource(gazette_core::resource::Error::Limit("authored artwork rendition declarations"))))
    ));
}

fn nested_cards() -> Vec<(String, Vec<u8>)> {
    let mut entries = files();
    let node = |id: &str, path: &str, structure: &str, metadata: &str| {
        format!(
        "+++\n{metadata}\n[content]\nid='{id}'\nsite='site:one'\ntitle='{id}'\nlifecycle='published'\nroute={{path='/{path}/'}}\nstructure={{{structure}}}\n+++\nAn authored account.\n"
    ).into_bytes()
    };
    entries.push((
        "one/child.md".into(),
        node(
            "collection:child",
            "child",
            "kind='collection',parent='collection:one'",
            "",
        ),
    ));
    entries.push(("one/journey.md".into(), node("journey:ordered", "ordered", "kind='journey',collection='collection:child',members=[{content='page:b',primary=true},{content='page:a',primary=true}]", "")));
    for id in ["a", "b"] {
        entries.push((
            format!("one/{id}.md"),
            node(
                &format!("page:{id}"),
                id,
                "kind='page',family={kind='article'}",
                "",
            ),
        ));
    }
    entries.push((
        "one/unrelated.md".into(),
        node(
            "page:unrelated",
            "unrelated",
            "kind='page',family={kind='article'}",
            "",
        ),
    ));
    entries
}

#[test]
fn structural_cards_inherit_only_the_nearest_editorial_artwork_and_keep_order() {
    let entries = nested_cards();
    let corpus = parse(&entries);
    let compiled = config();
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let expected = "https://one.test/press/assets/two/media/default.webp";
    for (id, field, ids) in [
        ("collection:one", "children", vec!["collection:child"]),
        ("collection:child", "children", vec!["journey:ordered"]),
        ("journey:ordered", "steps", vec!["page:b", "page:a"]),
    ] {
        let page = resolved
            .project_discovery_page(
                &id.parse().unwrap(),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .unwrap();
        let view = serde_json::to_value(page.discovery_view(&discovery).unwrap()).unwrap();
        let cards = view[field].as_array().unwrap();
        assert_eq!(
            cards
                .iter()
                .map(|card| card["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(
            cards
                .iter()
                .all(|card| card["artwork"]["default"] == expected)
        );
        assert!(
            view["content"].get("artwork").is_none(),
            "presentation does not rewrite the owner's semantic metadata"
        );
        if field == "children" {
            let schema =
                serde_json::to_value(schemars::schema_for!(authored::DiscoveryNode<'_>)).unwrap();
            let validator = jsonschema::validator_for(&schema).unwrap();
            assert!(cards.iter().all(|card| validator.is_valid(card)));
        }
    }
    let unrelated = resolved
        .project_page(
            &"page:unrelated".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(unrelated.data.content.artwork.is_none());
}

#[test]
fn a_structural_card_uses_its_own_artwork_before_the_containing_collection() {
    let mut entries = nested_cards();
    let child = entries
        .iter_mut()
        .find(|(path, _)| path == "one/child.md")
        .unwrap();
    child.1 = String::from_utf8(child.1.clone())
        .unwrap()
        .replacen(
            "+++\n",
            "+++\n[listing_artwork]\ndefault={source='source:two',entry='media/light.png'}\n",
            1,
        )
        .into_bytes();
    let corpus = parse(&entries);
    let compiled = config();
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &"journey:ordered".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(page.data.steps.iter().all(|card| {
        card.artwork
            .as_ref()
            .unwrap()
            .default
            .ends_with("/media/light.png")
    }));
}

#[test]
fn discovery_does_not_inherit_artwork_through_a_hidden_ancestor() {
    let mut entries = nested_cards();
    entries[1].0 = "hidden/root.md".into();
    let corpus = parse(&entries);
    let source = CONFIG.replace(
        "sources=[",
        "sources=[{id='source:hidden',location={kind='filesystem',root='/not-read-hidden'}},",
    ) + "\n[[projects]]\nid='project:hidden'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[placements]]\nrole='primary'\nsite='site:one'\nproject='project:hidden'\nrender=true\ndiscoverable=false\ndocuments={source='source:hidden',target='hidden'}\n";
    let compiled = workspace::compile(&source, Default::default()).unwrap();
    let resolved = authored::resolve(&compiled, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    assert!(!discovery.contains(&"collection:one".parse().unwrap()));
    let id = "journey:ordered".parse().unwrap();
    let private = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(private.data.steps.iter().all(|card| card.artwork.is_some()));
    let public = resolved
        .project_discovery_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .unwrap();
    assert!(public.data.steps.iter().all(|card| card.artwork.is_none()));
}

#[test]
fn structural_artwork_ancestor_visits_share_a_page_budget() {
    use gazette_core::publication::content::Limits;
    let mut entries = files();
    for index in 0..20 {
        entries.push((format!("one/member-{index}.md"), format!("+++\ncollections=['collection:one']\n[content]\nid='member:{index}'\nsite='site:one'\ntitle='Member'\nlifecycle='published'\nroute={{path='/member-{index}/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\nAccount.\n").into_bytes()));
    }
    let corpus = parse(&entries);
    let compiled = config();
    let resolved = authored::resolve(
        &compiled,
        &corpus,
        Limits {
            edges: 30,
            ..Default::default()
        },
    )
    .unwrap();
    let result = resolved.project_page(
        &"collection:one".parse().unwrap(),
        &gazette_core::raster::unavailable,
        &gazette_adapter_markdown::Comrak,
    );
    assert!(matches!(
        result,
        Err(authored::ProjectionError::Limit(
            "structural artwork ancestry visits"
        ))
    ));
}

#[test]
fn editor_artwork_observations_satisfy_structure_but_cannot_render_assets() {
    let all = files();
    let observations = all
        .iter()
        .filter(|(name, _)| name.ends_with(".png") || name.ends_with(".webp"))
        .map(|(name, bytes)| (name.parse().unwrap(), bytes.len() as u64))
        .collect();
    let text: Vec<_> = all
        .into_iter()
        .filter(|(name, _)| name.ends_with(".md") || name.ends_with(".yaml"))
        .collect();
    let corpus = parse(&text);
    let config = config();
    assert!(authored::resolve(&config, &corpus, Default::default()).is_err());
    let resolved =
        authored::resolve_with_asset_metadata(&config, &corpus, observations, Default::default())
            .unwrap();
    assert!(matches!(
        resolved.assets_for(&"site:one".parse().unwrap()),
        Err(authored::AssetError::Uncaptured(_))
    ));
}
