//! Navigation is a borrowed structural projection, not a folder or visibility policy.

use super::*;

fn trail_ids(trail: &authored::BreadcrumbTrail<'_>) -> Vec<Key> {
    trail
        .items
        .iter()
        .map(|link| link.node.id.clone())
        .collect()
}

#[test]
fn one_editorial_residence_preserves_the_complete_collection_and_journey_path() {
    let files = fixtures();
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("graph");
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("all paths");
    let trails = &page.data.breadcrumbs;
    assert_eq!(
        trails
            .iter()
            .map(|trail| trail.via.clone())
            .collect::<Vec<_>>(),
        [key("journey:z")]
    );
    assert_eq!(
        trail_ids(&trails[0]),
        [
            key("collection:root"),
            key("collection:child"),
            key("journey:z"),
            key("article:a")
        ]
    );
    assert_eq!(
        resolved
            .content()
            .graph()
            .editorial_parent(&key("article:a"))
            .map(|node| &node.id),
        Some(&key("journey:z"))
    );
    assert_eq!(
        resolved
            .content()
            .graph()
            .editorial_ancestry(&key("article:a"))
            .expect("editorial ancestry")
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        [
            key("article:a"),
            key("journey:z"),
            key("collection:child"),
            key("collection:root")
        ]
    );
    for trail in trails {
        for link in &trail.items {
            assert!(
                link.route.is_none(),
                "no guessed route for an unrouted object"
            );
            assert!(std::ptr::eq(
                link.node,
                resolved
                    .content()
                    .graph()
                    .node(&link.node.id)
                    .expect("same node")
            ));
            assert_eq!(
                link.node.title, "Same title",
                "no Home or slug-derived label"
            );
        }
    }
    for (id, expected) in [
        ("collection:root", vec![key("collection:root")]),
        (
            "collection:child",
            vec![key("collection:root"), key("collection:child")],
        ),
        (
            "journey:z",
            vec![
                key("collection:root"),
                key("collection:child"),
                key("journey:z"),
            ],
        ),
        ("article:empty", vec![key("article:empty")]),
    ] {
        let view = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("container or unrelated page");
        assert_eq!(view.data.breadcrumbs.len(), 1);
        assert_eq!(trail_ids(&view.data.breadcrumbs[0]), expected);
        assert_eq!(view.data.breadcrumbs[0].via, &key(id));
    }
    let repeat = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("repeat");
    assert_eq!(
        serde_json::to_value(&page.data.breadcrumbs).expect("paths"),
        serde_json::to_value(&repeat.data.breadcrumbs).expect("same paths")
    );
}

#[test]
fn journey_neighbors_follow_declared_sequence_without_filtering_or_primary_fallback() {
    let mut files = fixtures();
    files[2].1 = files[2].1.replace(
        "{content='article:a',primary=true}]",
        "{content='article:a',primary=true},{content='article:next',primary=true}]",
    );
    files.push((
        "next.md",
        doc(
            "article:next",
            "site:a",
            "published",
            "",
            "kind='page'\nfamily={kind='article'}",
            "Next",
        ),
    ));
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).expect("graph");
    for (id, position, previous, next, primary) in [
        ("docs:local", 0, None, Some("article:a"), true),
        (
            "article:a",
            1,
            Some("docs:local"),
            Some("article:next"),
            true,
        ),
        ("article:next", 2, Some("article:a"), None, true),
    ] {
        let view = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("complete sequence");
        let membership = view
            .data
            .journeys
            .iter()
            .find(|value| value.journey.id == key("journey:z"))
            .expect("explicit journey");
        assert_eq!(
            (membership.position, membership.total, membership.primary),
            (position, 3, primary)
        );
        assert_eq!(
            membership
                .members
                .iter()
                .map(|link| link.node.id.clone())
                .collect::<Vec<_>>(),
            [key("docs:local"), key("article:a"), key("article:next")]
        );
        assert_eq!(membership.members[position].node.id, key(id));
        assert_eq!(
            membership
                .previous
                .as_ref()
                .map(|link| link.node.id.clone()),
            previous.map(key)
        );
        assert_eq!(
            membership.next.as_ref().map(|link| link.node.id.clone()),
            next.map(key)
        );
        assert!(membership.route.is_none());
        for link in membership
            .previous
            .iter()
            .chain(membership.next.iter())
            .chain(membership.members.iter())
        {
            assert!(
                link.route.is_none(),
                "unrouted neighbor is retained rather than skipped"
            );
            assert!(std::ptr::eq(
                link.node,
                resolved
                    .content()
                    .graph()
                    .node(&link.node.id)
                    .expect("shared endpoint")
            ));
        }
        assert_eq!(
            resolved
                .content()
                .graph()
                .primary_journey(&key(id))
                .is_some(),
            primary
        );
    }
    let middle = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("middle");
    let journey = &middle.data.journeys[0];
    assert_eq!(
        journey.previous.as_ref().expect("previous").node.lifecycle,
        ContentLifecycle::Archived
    );
    assert_eq!(
        journey.previous.as_ref().expect("previous").node.site,
        key("site:a")
    );
    assert_eq!(middle.data.content.lifecycle, ContentLifecycle::Draft);
    let single = resolved
        .project_page(
            &key("article:single"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("separate singleton");
    let singleton = &single.data.journeys[0];
    assert_eq!(
        (singleton.position, singleton.total, singleton.primary),
        (0, 1, true)
    );
    assert!(singleton.previous.is_none() && singleton.next.is_none());
    assert_eq!(singleton.members.len(), 1);
    assert_eq!(singleton.members[0].node.id, key("article:single"));
}

#[test]
fn navigation_and_related_links_borrow_exact_home_and_foreign_canonical_routes() {
    let compilation = workspace::compile("[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.example/mount/'\n[[brands.publications]]\nsite='site:b'\npublic_base_url='https://b.example/other/'\n", Default::default()).expect("two exact sites");
    let files = vec![
        ("collection.md", doc("collection:one", "site:a", "published", "", "kind='collection'", "Collection").replace("[content.structure]", "route={path='/guides/'}\n[content.structure]")),
        ("journey.md", doc("journey:one", "site:a", "archived", "", "kind='journey'\ncollection='collection:one'\nmembers=[{content='article:a',primary=true},{content='article:next',primary=true}]", "Journey").replace("[content.structure]", "route={path='/learning/',aliases=['/old/']}\n[content.structure]")),
        ("a.md", doc("article:a", "site:a", "draft", "related=['article:b']\n", "kind='page'\nfamily={kind='article'}", "A").replace("[content.structure]", "route={path='/one/'}\n[content.structure]")),
        ("b.md", doc("article:b", "site:b", "archived", "", "kind='page'\nfamily={kind='article'}", "B").replace("[content.structure]", "route={path='/two/'}\n[content.structure]")),
        ("next.md", doc("article:next", "site:a", "archived", "", "kind='page'\nfamily={kind='article'}", "Next").replace("[content.structure]", "route={path='/next/'}\n[content.structure]")),
    ];
    let corpus = parse(&files, Default::default());
    let resolved =
        authored::resolve(&compilation, &corpus, Default::default()).expect("bound routes");
    let view = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("cross-site traversal");
    let journey = &view.data.journeys[0];
    assert!(journey.primary);
    assert!(std::ptr::eq(
        journey.route.expect("journey route"),
        resolved
            .content()
            .route(&key("journey:one"))
            .expect("shared route")
    ));
    let next = journey.next.as_ref().expect("next exact member");
    assert_eq!(
        journey
            .members
            .iter()
            .map(|member| member.route.unwrap().url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://a.example/mount/one/",
            "https://a.example/mount/next/"
        ]
    );
    assert!(std::ptr::eq(
        next.route.expect("next route"),
        resolved
            .content()
            .route(&key("article:next"))
            .expect("shared next route")
    ));
    let navigation = serde_json::to_value(&view.data.breadcrumbs).expect("serialized navigation");
    assert_eq!(
        navigation[0]["items"][1]["route"]["canonical_url"],
        "https://a.example/mount/learning/"
    );
    assert_eq!(
        navigation[0]["items"][2]["route"]["canonical_url"],
        "https://a.example/mount/one/"
    );
    assert_eq!(
        serde_json::to_value(next).expect("next")["route"]["canonical_url"],
        "https://a.example/mount/next/"
    );
    let foreign = &view.data.related[0];
    assert_eq!(foreign.node.site, key("site:b"));
    assert!(std::ptr::eq(
        foreign.node,
        resolved
            .content()
            .graph()
            .node(&key("article:b"))
            .expect("shared foreign node")
    ));
    assert_eq!(
        foreign
            .resolved_route
            .expect("resolved foreign endpoint")
            .url,
        "https://b.example/other/two/"
    );
    for (id, field, expected) in [
        (
            "collection:one",
            "children",
            vec!["https://a.example/mount/learning/"],
        ),
        (
            "journey:one",
            "steps",
            vec![
                "https://a.example/mount/one/",
                "https://a.example/mount/next/",
            ],
        ),
    ] {
        let page = resolved
            .project_page(
                &key(id),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("routed container");
        let value = serde_json::to_value(&page).expect("page transport");
        let destinations: Vec<_> = value[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["resolved_route"]["url"].as_str().unwrap())
            .collect();
        assert_eq!(destinations, expected);
        assert!(
            value[field][0]["route"]["path"]
                .as_str()
                .unwrap()
                .starts_with('/'),
            "authored path stays authored"
        );
    }
    let foreign_page = resolved
        .project_page(
            &foreign.node.id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("foreign page at its own site");
    assert!(std::ptr::eq(
        foreign_page.data.route.expect("foreign route"),
        resolved
            .content()
            .route(&key("article:b"))
            .expect("shared foreign route")
    ));
    assert_eq!(
        serde_json::to_value(foreign_page.data.route.expect("resolved related route"))
            .expect("related")["canonical_url"],
        "https://b.example/other/two/"
    );
}

#[test]
fn complete_journey_members_obey_projection_occurrence_limits() {
    let mut files = vec![
        (
            "collection.md",
            doc(
                "collection:one",
                "site:a",
                "draft",
                "",
                "kind='collection'",
                "Collection",
            ),
        ),
        (
            "journey.md",
            doc(
                "journey:one",
                "site:a",
                "draft",
                "",
                "kind='journey'\ncollection='collection:one'\nmembers=[{content='article:a',primary=true},{content='article:b',primary=true},{content='article:c',primary=true},{content='article:d',primary=true},{content='article:e',primary=true}]",
                "Sequence",
            ),
        ),
    ];
    for (path, id) in [
        ("a.md", "a"),
        ("b.md", "b"),
        ("c.md", "c"),
        ("d.md", "d"),
        ("e.md", "e"),
    ] {
        files.push((
            path,
            doc(
                &format!("article:{id}"),
                "site:a",
                "draft",
                "",
                "kind='page'\nfamily={kind='article'}",
                "Entry",
            ),
        ));
    }
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(
        &compilation,
        &corpus,
        content::Limits {
            edges: 8,
            ..Default::default()
        },
    )
    .expect("declared graph fits");
    let error = resolved
        .project_page(
            &key("article:c"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("complete member expansion is bounded, never truncated");
    assert_eq!(error.code(), "authored_projection_limit");
}

#[test]
fn deep_editorial_path_obeys_projection_occurrence_and_encoded_text_limits() {
    let paths = [
        "c0.md", "c1.md", "c2.md", "c3.md", "c4.md", "c5.md", "c6.md", "c7.md",
    ];
    let mut files = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let parent = if index == 0 {
            String::new()
        } else {
            format!("\nparent='collection:c{}'", index - 1)
        };
        files.push((
            path,
            doc(
                &format!("collection:c{index}"),
                "site:a",
                "draft",
                "",
                &format!("kind='collection'{parent}"),
                "Body",
            ),
        ));
    }
    files.push((
        "page.md",
        doc(
            "article:a",
            "site:a",
            "draft",
            "collections=['collection:c7']\n",
            "kind='page'\nfamily={kind='article'}",
            "Body",
        ),
    ));
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let ample = authored::resolve(&compilation, &corpus, Default::default()).expect("graph");
    assert_eq!(
        ample
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("ample bounds")
            .data
            .breadcrumbs
            .len(),
        1
    );
    for limits in [
        content::Limits {
            edges: 8,
            ..Default::default()
        },
        content::Limits {
            text_bytes: 1024,
            ..Default::default()
        },
    ] {
        let resolved = authored::resolve(&compilation, &corpus, limits)
            .expect("small source graph fits; expansion does not");
        let error = resolved
            .project_page(
                &key("article:a"),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("no partial navigation on amplification");
        assert_eq!(error.code(), "authored_projection_limit", "{error}");
    }
}

#[test]
fn collection_context_expansion_is_bounded_without_partial_siblings() {
    let mut files = vec![(
        "root.md",
        doc(
            "collection:root",
            "site:a",
            "draft",
            "",
            "kind='collection'",
            "Root",
        ),
    )];
    for (path, id) in [
        ("a.md", "article:a"),
        ("b.md", "article:b"),
        ("c.md", "article:c"),
        ("d.md", "article:d"),
        ("e.md", "article:e"),
    ] {
        files.push((
            path,
            doc(
                id,
                "site:a",
                "draft",
                "collections=['collection:root']\n",
                "kind='page'\nfamily={kind='article'}",
                "Question",
            ),
        ));
    }
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(
        &compilation,
        &corpus,
        content::Limits {
            edges: 5,
            ..Default::default()
        },
    )
    .expect("five declared memberships fit");
    let error = resolved
        .project_page(
            &key("article:c"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("sibling expansion plus parent exceeds projection budget");
    assert_eq!(error.code(), "authored_projection_limit");
}

fn balanced_documentation_events(events: &[serde_json::Value]) {
    let mut stack = Vec::new();
    for event in events {
        match event["kind"].as_str().unwrap() {
            "group-open" => stack.push(("group", serde_json::Value::Null)),
            "page-open" => stack.push(("page", event["has_children"].clone())),
            "group-close" => assert_eq!(stack.pop().unwrap().0, "group"),
            "page-close" => {
                let (kind, children) = stack.pop().unwrap();
                assert_eq!(kind, "page");
                assert_eq!(event["has_children"], children);
            }
            other => panic!("unexpected event {other}"),
        }
    }
    assert!(stack.is_empty());
}

#[test]
fn documentation_without_groups_uses_collection_member_order() {
    let mut files = fixtures();
    files[0].1 = files[0].1.replace(
        "kind='collection'",
        "kind='collection'\npurpose='documentation'\nmember_order=['journey:a']",
    );
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("collection:root"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        ids(&page.data.children),
        [key("journey:a"), key("collection:child")]
    );
    let outline = serde_json::to_value(page.data.documentation_navigation.unwrap()).unwrap();
    let events = outline["events"].as_array().unwrap();
    balanced_documentation_events(events);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["kind"] == "page-open")
            .map(|event| event["destination"]["node"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "journey:a",
            "article:single",
            "collection:child",
            "journey:z",
            "docs:local",
            "article:a"
        ]
    );
}

#[test]
fn product_documentation_outline_preserves_groups_order_nested_containers_and_current_state() {
    let mut files = fixtures();
    files[0].1 = files[0].1.replace("kind='collection'", "kind='collection'\npurpose='documentation'\nmember_order=['collection:child']\nnavigation=[{group='Start here',pages=[{content='journey:a'}]},{group='Understand',pages=[{content='collection:child'}]}]");
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let value = serde_json::to_value(&page).unwrap();
    let outline = &value["documentation_navigation"];
    assert_eq!(outline["root"]["node"]["id"], "collection:root");
    assert_eq!(outline["current"], false);
    assert!(
        outline.get("items").is_none(),
        "one resolved representation"
    );
    let events = outline["events"].as_array().unwrap();
    balanced_documentation_events(events);
    assert_eq!(events[0]["kind"], "group-open");
    assert_eq!(events[0]["title"], "Start here");
    let pages: Vec<_> = events
        .iter()
        .filter(|event| event["kind"] == "page-open")
        .collect();
    assert_eq!(
        pages
            .iter()
            .map(|event| event["destination"]["node"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "journey:a",
            "article:single",
            "collection:child",
            "journey:z",
            "docs:local",
            "article:a"
        ]
    );
    assert_eq!(pages[2]["ancestor"], true);
    assert_eq!(pages[3]["ancestor"], true);
    assert_eq!(pages[2]["has_children"], true);
    assert_eq!(pages[5]["current"], true);
    assert_eq!(pages[5]["has_children"], false);
    assert_eq!(
        page.data.content.structure,
        resolved
            .content()
            .graph()
            .node(&key("article:a"))
            .unwrap()
            .structure
    );
    let root = resolved
        .project_page(
            &key("collection:root"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(root.data.documentation_navigation.as_ref().unwrap().current);
    let unrelated = resolved
        .project_page(
            &key("article:empty"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(unrelated.data.documentation_navigation.is_none());

    files[1].1 = files[1].1.replace(
        "parent='collection:root'",
        "parent='collection:root'\nnavigation=[{group='Local',pages=[{content='journey:z'}]}]",
    );
    let corpus = parse(&files, Default::default());
    let local = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let page = local
        .project_page(
            &key("article:a"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let outline = serde_json::to_value(page.data.documentation_navigation.unwrap()).unwrap();
    assert_eq!(outline["root"]["node"]["id"], "collection:child");
    assert_eq!(outline["events"][0]["title"], "Local");
    balanced_documentation_events(outline["events"].as_array().unwrap());
}

#[test]
fn documentation_traversal_keeps_deep_containment_without_recursive_template_data() {
    let mut owned = vec![(
        "manual.md".to_owned(),
        doc(
            "collection:manual",
            "site:a",
            "published",
            "",
            "kind='collection'\npurpose='documentation'\nnavigation=[{group='Read',pages=[{content='collection:level0'}]}]",
            "Manual",
        ),
    )];
    for index in 0..128 {
        let id = format!("collection:level{index}");
        let parent = if index == 0 {
            "collection:manual".to_owned()
        } else {
            format!("collection:level{}", index - 1)
        };
        owned.push((
            format!("level{index}.md"),
            doc(
                &id,
                "site:a",
                "published",
                "",
                &format!("kind='collection'\nparent='{parent}'"),
                "Chapter",
            ),
        ));
    }
    owned.push((
        "leaf.md".to_owned(),
        doc(
            "content:leaf",
            "site:a",
            "published",
            "collections=['collection:level127']\n",
            "kind='page'\nfamily={kind='documentation'}",
            "Use the tool",
        ),
    ));
    let files: Vec<_> = owned
        .iter()
        .map(|(path, source)| (path.as_str(), source.clone()))
        .collect();
    let corpus = parse(&files, Default::default());
    let compilation = compilation();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &key("content:leaf"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let outline = serde_json::to_value(page.data.documentation_navigation.unwrap()).unwrap();
    let events = outline["events"].as_array().unwrap();
    balanced_documentation_events(events);
    assert_eq!(events.len(), 260);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["kind"] == "page-open")
            .count(),
        129
    );
    let leaf = events
        .iter()
        .find(|event| event["current"] == true)
        .unwrap();
    assert_eq!(leaf["destination"]["node"]["id"], "content:leaf");
    assert_eq!(leaf["has_children"], false);
}
