//! Discovery filters generated recommendations without rewriting authored page meaning.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::{Key, content::ContentKind};
use serde_json::json;

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn compilation() -> workspace::Compilation {
    compilation_with_base("")
}

fn compilation_with_base(base: &str) -> workspace::Compilation {
    workspace::compile(
        "[composition]\ntheme={package='theme:one',palette='palette:plain'}\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='One'\n\
         [[brands.publications]]\nsite='site:one'\nauthored=[{source='source:shared',target='authored'}]\n\
         [[projects]]\nid='project:native'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         [[sources]]\nid='source:shared'\nlocation={kind='repository',root='/private/native',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
         [[theme_packages]]\npackage='theme:one'\nsource='source:shared'\nmanifest='theme.toml'\n\
         [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:native'\nrender=true\ndiscoverable=false\ndocuments={source='source:shared',target='unlisted'}\n".replace("site='site:one'\nauthored=", &format!("site='site:one'\n{base}authored=")).as_str(),
        Default::default(),
    ).expect("source-bound workspace")
}

fn document(id: &str, edges: &str, structure: &str) -> String {
    format!(
        "+++\n{edges}[content]\nid='{id}'\nsite='site:one'\ntitle='{id}'\nlifecycle='published'\n[content.structure]\n{structure}\n+++\n# Body\n\nExcerpt.\n\n<!-- more -->\n\n[Authored link](https://example.test/unlisted/)\n"
    )
}

fn files() -> Vec<(&'static str, String)> {
    vec![
        (
            "authored/contained.md",
            document(
                "page:contained",
                "collections=['collection:hidden']\n",
                "kind='page'\nfamily={kind='article'}",
            ),
        ),
        (
            "unlisted/root.md",
            document(
                "collection:hidden",
                "projects=[{project='project:native',kind='context'}]\n",
                "kind='collection'",
            ),
        ),
        (
            "authored/child.md",
            document(
                "collection:child",
                "",
                "kind='collection'\nparent='collection:hidden'",
            ),
        ),
        (
            "authored/journey.md",
            document(
                "journey:mixed",
                "",
                "kind='journey'\ncollection='collection:child'\nmembers=[{content='page:mixed-a',primary=true},{content='page:hidden',primary=true},{content='page:mixed-b',primary=true}]",
            ),
        ),
        (
            "authored/clean.md",
            document(
                "journey:clean",
                "",
                "kind='journey'\ncollection='collection:child'\nmembers=[{content='page:a',primary=true},{content='page:b',primary=true}]",
            ),
        ),
        (
            "authored/a.md",
            document(
                "page:a",
                "related=['page:hidden','page:b']\n",
                "kind='page'\nfamily={kind='article'}",
            ),
        ),
        (
            "authored/b.md",
            document("page:b", "", "kind='page'\nfamily={kind='article'}"),
        ),
        (
            "unlisted/page.md",
            document(
                "page:hidden",
                "",
                "kind='page'\nfamily={kind='documentation'}",
            ),
        ),
        (
            "authored/mixed-a.md",
            document("page:mixed-a", "", "kind='page'\nfamily={kind='article'}"),
        ),
        (
            "authored/mixed-b.md",
            document("page:mixed-b", "", "kind='page'\nfamily={kind='article'}"),
        ),
    ]
}

fn parse<'a>(files: &'a [(&str, String)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().expect("logical source path"),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real retained Markdown")
}

#[test]
fn generated_navigation_omits_whole_ineligible_journeys_and_ancestry() {
    let files = files();
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("whole graph");
    let discovery = resolved.discovery().expect("all source bindings");
    let private = resolved
        .project_page(
            &key("page:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete inspection");
    let page = resolved
        .project_discovery_page(
            &key("page:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("discovery page");
    assert_eq!(private.data.journeys.len(), 1);
    assert_eq!(page.data.journeys.len(), 1);
    assert_eq!(page.data.journeys[0].journey.id, key("journey:clean"));
    assert_eq!(page.data.journeys[0].position, 0);
    assert_eq!(page.data.journeys[0].total, 2);
    assert_eq!(
        page.data.journeys[0]
            .next
            .as_ref()
            .expect("exact next")
            .node
            .id,
        key("page:b")
    );
    assert_eq!(
        page.data.breadcrumbs.len(),
        0,
        "hidden ancestor removes the complete editorial trail"
    );
    let mixed_private = resolved
        .project_page(
            &key("page:mixed-a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete mixed journey");
    let mixed = resolved
        .project_discovery_page(
            &key("page:mixed-a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("filtered mixed journey");
    assert_eq!(mixed_private.data.journeys.len(), 1);
    assert!(
        mixed.data.journeys.is_empty(),
        "a hidden sibling omits the whole containing journey"
    );
    assert!(mixed.data.breadcrumbs.is_empty());
    assert_eq!(
        page.data
            .related
            .iter()
            .map(|view| view.node.id.clone())
            .collect::<Vec<_>>(),
        [key("page:b")]
    );
    assert_eq!(page.body_html, private.body_html);
    assert!(
        page.body_html.contains("https://example.test/unlisted/"),
        "authored links are not a discovery widget"
    );
    let direct = serde_json::to_value(page.discovery_view(&discovery).expect("same source"))
        .expect("direct model");
    assert_eq!(
        direct["links"],
        serde_json::to_value(&page.data.links).expect("compiler-proved links"),
        "the discovery-filtered page context preserves the links promised by its schema"
    );
    assert!(
        direct["links"]
            .as_array()
            .is_some_and(|links| !links.is_empty()),
        "the fixture exercises a real retained body link"
    );
    assert_eq!(
        direct["journeys"][0]["journey"]["structure"],
        json!({"kind":"journey"}),
        "safe widget does not expand ordered member declarations"
    );
    let listing = serde_json::to_value(page.listing(&discovery).expect("eligible listing"))
        .expect("listing model");
    assert!(
        listing["projects"].as_array().expect("projects").is_empty(),
        "hidden context evidence omits the complete relation"
    );
    assert!(!listing.to_string().contains("collection:hidden"));
    assert_eq!(listing["body_text"], page.body_text);
    let linked = serde_json::to_value(
        private
            .listing_json_ld(&discovery)
            .expect("eligible private projection"),
    )
    .expect("safe aggregate JSON-LD");
    assert!(
        linked.get("mentions").is_none(),
        "standard relations cannot bypass full evidence eligibility"
    );
    assert!(
        linked["gazette:projectRelations"]
            .as_array()
            .expect("technical relations")
            .is_empty()
    );
    let direct_linked = serde_json::to_value(
        private
            .discovery_json_ld(&discovery)
            .expect("direct projection"),
    )
    .expect("direct authored relationships");
    assert_eq!(
        direct_linked["mentions"][0]["identifier"], "project:native",
        "discovery does not rewrite the direct page's canonical project relationship"
    );
    assert!(!linked.to_string().contains("page:hidden"));
    assert!(!linked.to_string().contains("collection:hidden"));
    let trails = &linked["gazette:breadcrumbs"]["@value"];
    assert_eq!(trails.as_array().expect("typed safe trails").len(), 0);
    assert!(!trails.to_string().contains("page:hidden"));
    assert_eq!(linked["mainEntityOfPage"]["breadcrumb"], json!([]));
}

#[test]
fn direct_container_keeps_authored_steps_but_listing_never_expands_them() {
    let files = files();
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("whole graph");
    let discovery = resolved.discovery().expect("source selection");
    let journey = resolved
        .project_discovery_page(
            &key("journey:mixed"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("direct journey");
    assert_eq!(journey.data.steps.len(), 3);
    assert!(matches!(
        journey.data.content.structure,
        ContentKind::Journey { .. }
    ));
    let direct = serde_json::to_value(journey.discovery_view(&discovery).expect("direct view"))
        .expect("direct JSON");
    assert_eq!(direct["steps"][1]["id"], "page:hidden");
    assert_eq!(
        direct["content"]["structure"]["members"]
            .as_array()
            .expect("authored sequence")
            .len(),
        3
    );
    let view = journey
        .listing(&discovery)
        .expect("listed journey identity");
    let listing = serde_json::to_value(&view).expect("listing JSON");
    assert_eq!(listing["content"]["structure"], json!({"kind":"journey"}));
    assert!(listing.get("steps").is_none());
    assert_eq!(listing["body_html"], journey.body_html);
    assert_eq!(
        listing["summary"]["text"],
        journey.summary.as_ref().expect("native excerpt").text
    );
    assert!(!listing.to_string().contains("page:hidden"));
    let metadata = serde_json::to_value(view.data()).expect("feed metadata only");
    assert!(metadata.get("body_text").is_none());
    assert!(metadata.get("summary").is_none());
    assert_eq!(metadata["content"], listing["content"]);
    let search = serde_json::to_value(view.search().expect("eligible search"))
        .expect("same metadata search");
    assert_eq!(search["body"], journey.body_text);
    assert_eq!(
        search["summary"],
        journey.summary.as_ref().expect("plain excerpt").text
    );
    assert!(search.get("body_html").is_none());
    let direct_search = serde_json::to_value(
        journey
            .discovery_view(&discovery)
            .expect("direct page")
            .search()
            .expect("eligible search from direct page"),
    )
    .expect("safe search from direct page");
    assert_eq!(
        search, direct_search,
        "entry via direct page cannot re-export its raw authored steps"
    );
    let child = resolved
        .project_discovery_page(
            &key("collection:child"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("direct collection");
    assert_eq!(child.data.children.len(), 2);
    assert!(child.data.breadcrumbs.is_empty());
    let hidden = resolved
        .project_discovery_page(
            &key("page:hidden"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("unlisted but renderable");
    assert!(hidden.listing(&discovery).is_none());
    assert!(hidden.discovery_view(&discovery).is_some());
    assert!(
        hidden
            .discovery_view(&discovery)
            .expect("renderable direct page")
            .search()
            .is_none()
    );
    assert!(hidden.data.breadcrumbs.is_empty());
    let contained = resolved
        .project_discovery_page(
            &key("page:contained"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("page with omitted sole parent");
    assert!(
        contained.data.breadcrumbs.is_empty(),
        "do not replace a removed parent trail with an invented standalone trail"
    );
}

#[test]
fn independently_resolved_equal_graph_cannot_supply_discovery() {
    let files = files();
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("owner");
    let other = authored::resolve(&compilation, &corpus, Default::default())
        .expect("equal but different owner");
    let wrong = other.discovery().expect("other source selection");
    assert!(matches!(
        resolved.project_discovery_page(
            &key("page:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &wrong
        ),
        Err(authored::ProjectionError::Binding { .. })
    ));
    let page = resolved
        .project_page(
            &key("page:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("private projection");
    assert!(page.discovery_view(&wrong).is_none());
    assert!(page.listing(&wrong).is_none());
}

fn validator<T: schemars::JsonSchema>() -> jsonschema::Validator {
    let schema = serde_json::to_value(schemars::schema_for!(T)).expect("native transport schema");
    jsonschema::validator_for(&schema).expect("closed native schema compiles")
}

fn validates(validator: &jsonschema::Validator, value: &serde_json::Value) {
    assert!(
        validator.is_valid(value),
        "native transport must match its schema: {:?}",
        validator.iter_errors(value).collect::<Vec<_>>()
    );
}

#[test]
fn native_page_and_discovery_navigation_schemas_match_actual_projected_transports() {
    let mut files = files();
    files[1].0 = "authored/root.md";
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default())
        .expect("complete retained graph");
    let discovery = resolved.discovery().expect("exact source selection");
    let pages = validator::<authored::PageProjection<'_>>();
    let link_schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<authored::NavigationLink<'_>>();
    let links = jsonschema::validator_for(link_schema.as_value()).unwrap();
    let memberships = validator::<authored::JourneyMembership<'_>>();
    let nodes = validator::<authored::DiscoveryNode<'_>>();
    for id in [
        "page:a",
        "page:b",
        "collection:hidden",
        "collection:child",
        "journey:clean",
        "journey:mixed",
    ] {
        let private = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("direct complete native projection");
        validates(
            &pages,
            &serde_json::to_value(&private).expect("native model"),
        );
        let page = resolved
            .project_discovery_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .expect("discovery-safe generated navigation");
        let direct = serde_json::to_value(page.discovery_view(&discovery).expect("bound view"))
            .expect("actual public transport");
        validates(&pages, &direct);
        assert_eq!(direct["content"]["presentation"]["theme"], "theme:one");
        for trail in direct["breadcrumbs"].as_array().expect("breadcrumbs") {
            for link in trail["items"].as_array().expect("trail endpoints") {
                validates(&links, link);
                validates(&nodes, &link["node"]);
                assert_eq!(link["node"]["presentation"]["palette"], "palette:plain");
            }
        }
        for membership in direct["journeys"].as_array().expect("journeys") {
            validates(&memberships, membership);
            let members = membership["members"].as_array().expect("complete sequence");
            assert_eq!(members.len() as u64, membership["total"].as_u64().unwrap());
            assert_eq!(
                members[membership["position"].as_u64().unwrap() as usize]["node"]["id"],
                id
            );
            for member in members {
                validates(&links, member);
                validates(&nodes, &member["node"]);
            }
            assert_eq!(
                membership["journey"]["structure"],
                json!({"kind":"journey"})
            );
            for name in ["previous", "next"] {
                if !membership[name].is_null() {
                    validates(&links, &membership[name]);
                    validates(&nodes, &membership[name]["node"]);
                }
            }
        }
        for name in ["collections", "children", "related"] {
            for node in direct[name].as_array().expect("generated endpoints") {
                validates(&nodes, node);
                assert_eq!(node["presentation"]["theme"], "theme:one");
            }
        }
        let listing = serde_json::to_value(page.listing(&discovery).expect("eligible identity"))
            .expect("actual aggregate transport");
        validates(&nodes, &listing["content"]);
        assert!(listing.get("steps").is_none());
        assert!(
            !listing.to_string().contains("page:hidden"),
            "schema support does not change hidden-member suppression"
        );
        if id == "page:a" {
            assert!(direct["content"].get("listing").is_none());
            assert!(direct["projects"][0].get("works").is_none());
            assert!(direct["projects"][0].get("code_documentation").is_none());
            let mut explicit = direct.clone();
            explicit["content"]["listing"] = json!(false);
            explicit["projects"][0]["works"] = json!([]);
            validates(&pages, &explicit);
            explicit["content"]["listing"] = json!("false");
            assert!(!pages.is_valid(&explicit));
            explicit["content"]["listing"] = json!(false);
            explicit["projects"][0]["works"] = json!({});
            assert!(!pages.is_valid(&explicit));
            assert_eq!(direct["journeys"][0]["next"]["node"]["id"], "page:b");
            assert_eq!(
                direct["breadcrumbs"][0]["items"].as_array().unwrap().len(),
                4
            );
        } else if id == "page:b" {
            assert_eq!(direct["journeys"][0]["previous"]["node"]["id"], "page:a");
        }
    }
}

#[test]
fn discovery_schema_is_shallow_and_closed_without_relaxing_content_admission() {
    let files = files();
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("owner");
    let discovery = resolved.discovery().expect("source selection");
    let native = resolved
        .content()
        .graph()
        .node(&key("journey:mixed"))
        .expect("journey");
    let safe = serde_json::to_value(discovery.node(native).expect("eligible node"))
        .expect("real discovery node");
    let nodes = validator::<authored::DiscoveryNode<'_>>();
    validates(&nodes, &safe);
    assert_eq!(safe["structure"], json!({"kind":"journey"}));
    assert!(!safe.to_string().contains("page:hidden"));
    for (field, value) in [
        (
            "structure",
            json!({"kind":"journey","members":[{"content":"page:hidden","primary":true}]}),
        ),
        (
            "structure",
            json!({"kind":"collection","parent":"collection:hidden"}),
        ),
        ("structure", json!({"kind":"unknown"})),
        (
            "structure",
            json!({"kind":"page","family":{"kind":"unknown"}}),
        ),
        ("presentation", json!({"theme":17,"palette":"not-a-key"})),
        ("private_path", json!("/private/native")),
    ] {
        let mut malformed = safe.clone();
        malformed[field] = value;
        assert!(!nodes.is_valid(&malformed), "refuse malformed {field}");
    }
    let native_schema = validator::<gazette_core::publication::content::ContentNode>();
    validates(
        &native_schema,
        &serde_json::to_value(native).expect("complete native node"),
    );
    assert!(!native_schema.is_valid(&safe));
    assert!(
        serde_json::from_value::<gazette_core::publication::content::ContentNode>(safe).is_err()
    );
}

#[test]
fn discovery_children_keep_compiled_destinations_without_hidden_ancestry_or_members() {
    let files: Vec<_> = files()
        .into_iter()
        .map(|(path, source)| {
            let route = format!("route={{path='/{}'}}\n[content.structure]", path);
            (path, source.replace("[content.structure]", &route))
        })
        .collect();
    let corpus = parse(&files);
    let compilation = compilation_with_base("public_base_url='https://example.test/mount/'\n");
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("routed corpus");
    let discovery = resolved.discovery().expect("bound visibility");
    let page = resolved
        .project_discovery_page(
            &key("collection:child"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("child page");
    let direct = serde_json::to_value(page.discovery_view(&discovery).unwrap()).unwrap();
    let listing = serde_json::to_value(page.listing(&discovery).unwrap()).unwrap();
    for model in [&direct, &listing] {
        let children = model["children"].as_array().unwrap();
        assert_eq!(
            children.len(),
            2,
            "eligible containers remain shallow destinations even when their full membership is not discoverable"
        );
        assert_eq!(children[0]["id"], "journey:clean");
        assert_eq!(
            children[0]["resolved_route"]["url"],
            "https://example.test/mount/authored/clean.md"
        );
        assert_eq!(children[0]["route"]["path"], "/authored/clean.md");
        assert_eq!(children[0]["structure"], json!({"kind":"journey"}));
        assert!(!model.to_string().contains("page:hidden"));
    }
    let schema = schemars::schema_for!(authored::PageProjection<'_>);
    jsonschema::validator_for(&serde_json::to_value(schema).unwrap())
        .unwrap()
        .validate(&direct)
        .expect("routed discovery transport matches public schema");
}

#[test]
fn collection_reading_context_keeps_direct_siblings_and_safe_routes_without_a_sequence() {
    let mut files = files();
    for (path, id) in [
        ("authored/reader.md", "page:reader"),
        ("authored/sibling.md", "page:sibling"),
        ("unlisted/sibling.md", "page:private-sibling"),
    ] {
        files.push((
            path,
            document(
                id,
                "collections=['collection:child']\n",
                "kind='page'\nfamily={kind='article'}",
            ),
        ));
    }
    for (_, source) in &mut files {
        if source.contains("id='collection:child'") {
            *source = source.replace("kind='collection'", "kind='collection'\nmember_order=['page:sibling','page:private-sibling','page:reader']");
        }
    }
    let files: Vec<_> = files
        .into_iter()
        .map(|(path, source)| {
            (
                path,
                source.replace(
                    "[content.structure]",
                    &format!("route={{path='/{}'}}\n[content.structure]", path),
                ),
            )
        })
        .collect();
    let corpus = parse(&files);
    let compilation = compilation_with_base("public_base_url='https://example.test/mount/'\n");
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    for filtered in [false, true] {
        let page = if filtered {
            resolved.project_discovery_page(
                &key("page:reader"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
        } else {
            resolved.project_page(
                &key("page:reader"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
        }
        .unwrap();
        let context = &page.data.collection_navigation[0];
        assert_eq!(context.collection.node.id, key("collection:child"));
        assert_eq!(
            context
                .members
                .iter()
                .any(|item| item.node.id == key("page:private-sibling")),
            !filtered
        );
        assert!(
            !context
                .members
                .iter()
                .any(|item| item.node.id == key("page:a")),
            "journey descendants are not direct collection siblings"
        );
        assert!(page.data.journeys.is_empty());
        let value = serde_json::to_value(page.discovery_view(&discovery).unwrap()).unwrap();
        let context = &value["collection_navigation"][0];
        assert_eq!(
            context["collection"]["node"]["structure"],
            json!({"kind":"collection"})
        );
        assert_eq!(
            context["collection"]["route"]["url"],
            "https://example.test/mount/authored/child.md"
        );
        let ids: Vec<_> = context["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["node"]["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "page:sibling",
                "page:reader",
                "journey:clean",
                "journey:mixed"
            ]
        );
        assert!(!context.to_string().contains("collection:hidden"));
        assert!(!context.to_string().contains("page:hidden"));
        assert!(!context.to_string().contains("page:private-sibling"));
        let schema =
            serde_json::to_value(schemars::schema_for!(authored::PageProjection<'_>)).unwrap();
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&value)
            .unwrap();
    }
    let journey_page = resolved
        .project_page(
            &key("page:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        journey_page.data.collection_navigation.is_empty(),
        "journey membership does not create a second residence"
    );
}

#[test]
fn documentation_outline_filters_destinations_and_does_not_duplicate_trees_into_search() {
    let files = vec![
        (
            "authored/manual.md",
            document(
                "collection:manual",
                "",
                "kind='collection'\npurpose='documentation'\nnavigation=[{group='Start',pages=[{content='page:visible'},{content='collection:empty'}]},{group='Unlisted',pages=[{content='page:hidden'}]}]",
            ),
        ),
        (
            "authored/visible.md",
            document(
                "page:visible",
                "collections=['collection:manual']\n",
                "kind='page'\nfamily={kind='documentation'}",
            ),
        ),
        (
            "authored/empty.md",
            document(
                "collection:empty",
                "",
                "kind='collection'\nparent='collection:manual'",
            ),
        ),
        (
            "unlisted/empty-child.md",
            document(
                "page:empty-child",
                "collections=['collection:empty']\n",
                "kind='page'\nfamily={kind='documentation'}",
            ),
        ),
        (
            "unlisted/hidden.md",
            document(
                "page:hidden",
                "collections=['collection:manual']\n",
                "kind='page'\nfamily={kind='documentation'}",
            ),
        ),
    ];
    let files: Vec<_> = files
        .into_iter()
        .map(|(path, source)| {
            (
                path,
                source.replace(
                    "[content.structure]",
                    &format!("route={{path='/{path}'}}\n[content.structure]"),
                ),
            )
        })
        .collect();
    let corpus = parse(&files);
    let compilation = compilation_with_base("public_base_url='https://example.test/mount/'\n");
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let discovery = resolved.discovery().unwrap();
    let private = resolved
        .project_page(
            &key("page:visible"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let direct = private.discovery_view(&discovery).unwrap();
    let value = serde_json::to_value(&direct).unwrap();
    let navigation = &value["documentation_navigation"];
    assert_eq!(
        navigation["root"]["route"]["url"],
        "https://example.test/mount/authored/manual.md"
    );
    assert_eq!(navigation["current"], false);
    assert_eq!(navigation["events"].as_array().unwrap().len(), 6);
    assert_eq!(navigation["events"][0]["title"], "Start");
    assert_eq!(navigation["events"][1]["current"], true);
    assert_eq!(
        navigation["events"][1]["destination"]["route"]["url"],
        "https://example.test/mount/authored/visible.md"
    );
    assert_eq!(
        navigation["events"][3]["destination"]["node"]["id"],
        "collection:empty"
    );
    assert_eq!(navigation["events"][3]["has_children"], false);
    assert_eq!(
        navigation["events"][4],
        json!({"kind":"page-close","has_children":false})
    );
    assert_eq!(navigation["events"][5], json!({"kind":"group-close"}));
    assert!(!navigation.to_string().contains("empty-child"));
    let schema = serde_json::to_value(schemars::schema_for!(authored::PageProjection<'_>)).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&value)
        .unwrap();
    assert!(
        !serde_json::to_string(navigation)
            .unwrap()
            .contains("page:hidden")
    );
    assert!(
        serde_json::to_value(direct.search().unwrap())
            .unwrap()
            .get("documentation_navigation")
            .is_none()
    );
    let filtered = resolved
        .project_discovery_page(
            &key("page:visible"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(filtered.discovery_view(&discovery).unwrap()).unwrap()["documentation_navigation"],
        *navigation
    );
}
