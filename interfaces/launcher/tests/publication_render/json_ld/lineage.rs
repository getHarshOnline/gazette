//! Declared lineage crosses the native publication path without ownership inference.

use super::{
    Fixture, PROJECT_DOC, TEMPLATE, Value, acquire, assert_connected, build, bytes, fs, render,
};

fn fixture() -> Fixture {
    let template = String::from_utf8(TEMPLATE.to_vec())
        .expect("template")
        .replace(
            "<h1>",
            "{% for link in projects %}{% if link.project.origin is defined %}<b>{{ link.project.id }}={{ link.project.origin.kind }}{% if link.project.origin.upstream is defined %}:{{ link.project.origin.upstream }}{% endif %}</b>{% endif %}{% endfor %}<h1>",
        );
    let document = PROJECT_DOC.replace(
        "kind='about'}]",
        "kind='about'},{project='project:native',kind='mentions'},{project='project:mirror',kind='mentions'},{project='project:unspecified',kind='mentions'}]",
    );
    let manifest = crate::MANIFEST.replace("graph='graph.html'", "graph='graph.json'");
    Fixture::new(&[
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
        (
            "theme/templates/graph.json",
            b"{{ projects | json_encode }}",
        ),
        ("project/guide.md", document.as_bytes()),
    ])
}

fn configuration(fixture: &Fixture, upstream_visibility: &str) -> String {
    let mut config = fixture
        .project_config(true)
        .replace(
            "id='project:subject'",
            "id='project:subject'\norigin={kind='original'}",
        )
        .replace(
            "id='project:native'\nlifecycle",
            "id='project:native'\norigin={kind='successor-fork',upstream='project:subject'}\nlifecycle",
        )
        .replace(
            "public_base_url=",
            "machine={json_ld=true,search='lexical'}\npublic_base_url=",
        );
    config.push_str(&format!(
        "[[projects]]\nid='project:mirror'\norigin={{kind='convenience-mirror',upstream='project:upstream'}}\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
         [[projects]]\nid='project:upstream'\nname='Do not export this record'\nlifecycle='active'\nvisibility='{upstream_visibility}'\navailability='available'\n\
         [[projects]]\nid='project:unspecified'\nname='Fork of something'\nrepository='https://code.example.test/forks/mirror'\nlifecycle='active'\nvisibility='public'\navailability='available'\n"
    ));
    config
}

fn linked_project<'a>(linked: &'a Value, id: &str) -> &'a Value {
    linked["about"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(linked["mentions"].as_array().into_iter().flatten())
        .find(|node| node["identifier"] == id)
        .expect("same linked project")
}

#[test]
fn declared_origins_remain_native_while_public_projects_keep_identity_without_derivation_claims() {
    let fixture = fixture();
    let config = configuration(&fixture, "public");
    let inputs = acquire(&config);
    let render_graph = || {
        build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: None,
                graph: Some(build::GraphRenderOptions {
                    output: &"graph.json".parse().expect("graph output"),
                    limits: Default::default(),
                }),
                model_output: Some(&"model.json".parse().expect("model")),
                limits: Default::default(),
            },
            Default::default(),
        )
    };
    let rendered = render_graph().expect("connected lineage publication");
    let tree = rendered.site().output();
    let model = assert_connected(&inputs, tree);
    let page = &model["pages"]["doc:native"];
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|value| value["content"]["id"] == "doc:native")
        .expect("same document");
    assert_eq!(record["projects"], page["projects"]);
    let graph: Value = serde_json::from_slice(bytes(tree, "graph.json")).expect("themed graph");
    let html = std::str::from_utf8(bytes(tree, "docs/native/index.html")).expect("HTML");
    let linked = &page["json_ld"];
    let parsed = build::parse_publication(&inputs, Default::default()).expect("retained corpus");
    let resolved = parsed
        .resolve(Default::default())
        .expect("native semantic owner");
    let projection = resolved
        .authored()
        .project_page(
            &"doc:native".parse().unwrap(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete native project declarations");
    let native = serde_json::to_value(projection.json_ld()).expect("native provenance and origins");
    assert_eq!(
        native["gazette:provenance"]["gazette:project"]["identifier"],
        "project:native"
    );
    assert_eq!(
        native["gazette:provenance"]["gazette:site"]["identifier"],
        "site:one"
    );
    super::assert_public(linked);
    for field in ["isBasedOn", "translationOfWork", "citation"] {
        assert!(
            linked.get(field).is_none(),
            "source provenance is not {field}"
        );
    }
    for (id, kind, upstream) in [
        ("project:subject", "original", None),
        ("project:native", "successor-fork", Some("project:subject")),
        (
            "project:mirror",
            "convenience-mirror",
            Some("project:upstream"),
        ),
    ] {
        let project = &page["projects"]
            .as_array()
            .expect("project subjects")
            .iter()
            .find(|value| value["project"]["id"] == id)
            .expect("explicit project")["project"];
        assert_eq!(project["origin"]["kind"], kind);
        assert_eq!(
            project["origin"].get("upstream").and_then(Value::as_str),
            upstream
        );
        assert_eq!(graph[id], *project);
        let node = linked_project(&native, id);
        let public = linked_project(linked, id);
        assert_eq!(public["@type"], "Thing");
        assert_eq!(public["@id"], node["@id"]);
        assert_eq!(public["identifier"], project["id"]);
        for field in [
            "gazette:origin",
            "isBasedOn",
            "translationOfWork",
            "citation",
        ] {
            assert!(public.get(field).is_none(), "project origin is not {field}");
        }
        assert_eq!(node["gazette:origin"]["gazette:kind"], kind);
        if let Some(upstream) = upstream {
            let reference = &node["gazette:origin"]["gazette:upstream"];
            assert_eq!(reference["identifier"], upstream);
            assert!(
                reference["@id"]
                    .as_str()
                    .expect("linked identity")
                    .starts_with("_:")
            );
            assert!(reference.get("name").is_none());
            if upstream == "project:subject" {
                assert_eq!(reference["@id"], linked["about"][0]["@id"]);
            }
        } else {
            assert!(node["gazette:origin"].get("gazette:upstream").is_none());
        }
        let suffix = upstream
            .map(|upstream| format!(":{upstream}"))
            .unwrap_or_default();
        assert!(html.contains(&format!("<b>{id}={kind}{suffix}</b>")));
    }
    assert!(
        graph.get("project:upstream").is_none(),
        "lineage does not export the upstream record"
    );
    assert!(
        graph["project:unspecified"].get("origin").is_none(),
        "name and URL never infer origin"
    );
    assert!(
        linked_project(&native, "project:unspecified")
            .get("gazette:origin")
            .is_none()
    );
    let unspecified = linked_project(linked, "project:unspecified");
    assert_eq!(unspecified["@type"], "Thing");
    assert_eq!(unspecified["name"], "Fork of something");
    for field in [
        "gazette:origin",
        "isBasedOn",
        "translationOfWork",
        "citation",
    ] {
        assert!(
            unspecified.get(field).is_none(),
            "a name or URL does not establish {field}"
        );
    }
    assert!(
        !linked.to_string().contains("project:upstream"),
        "native upstream identity is not a public derivation claim"
    );
    assert!(
        !serde_json::to_string(&model)
            .expect("model text")
            .contains("Do not export this record")
    );
    let native = git2::Repository::open(fixture.directory.path()).expect("disposable repository");
    native
        .remote("origin", "https://code.example.test/another/fork")
        .expect("changed ambient remote");
    fs::write(
        fixture.directory.path().join("project/guide.md"),
        "changed original source",
    )
    .expect("disposable source change");
    let repeat = render_graph().expect("retained repeat with identical selections");
    assert_eq!(
        repeat.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    for path in [
        "site.jsonld",
        "search.json",
        "docs/native/index.html",
        "graph.json",
    ] {
        assert_eq!(bytes(repeat.site().output(), path), bytes(tree, path));
    }
}

#[test]
fn nonpublic_upstream_refuses_only_emitted_lineage_while_private_inspection_retains_it() {
    let fixture = fixture();
    for visibility in ["private", "unlisted"] {
        let config = configuration(&fixture, visibility);
        let inputs = acquire(&config);
        let parsed = build::parse_publication(&inputs, Default::default()).expect("native parser");
        let resolved = parsed
            .resolve(Default::default())
            .expect("valid structural references");
        let authored = resolved.authored();
        let id = "doc:native".parse().expect("content");
        let site = "site:one".parse().expect("site");
        let private = authored
            .project_page(
                &id,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("complete private view");
        let before = serde_json::to_value(private.json_ld()).expect("private JSON-LD");
        assert!(before.to_string().contains("project:upstream"));
        let discovery = authored
            .discovery()
            .expect("unrelated discovery selection survives");
        let error = authored
            .project_discovery_page(
                &id,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .expect_err("typed lineage refusal");
        assert_eq!(error.code(), "publication_project_origin");
        let error = authored
            .project_discovery_graph(&site, Default::default(), &discovery)
            .expect_err("graph refuses the same edge without stripping");
        assert_eq!(error.code(), "publication_project_origin");
        assert!(serde_json::to_value(private.listing(&discovery).expect("eligible page")).is_err());
        assert!(
            serde_json::to_value(private.listing_json_ld(&discovery).expect("eligible page"))
                .is_err()
        );
        assert!(
            serde_json::to_value(private.discovery_view(&discovery).expect("direct view")).is_err()
        );
        assert!(
            serde_json::to_value(private.discovery_json_ld(&discovery).expect("direct view"))
                .is_err()
        );
        assert!(
            serde_json::to_value(
                private
                    .listing_json_ld(&discovery)
                    .expect("eligible public page")
                    .public()
            )
            .is_err(),
            "public serialization retains the same lineage admission boundary"
        );
        assert!(
            serde_json::to_value(
                private
                    .discovery_json_ld(&discovery)
                    .expect("direct public view")
                    .public()
            )
            .is_err(),
            "omitting technical vocabulary cannot bypass lineage admission"
        );
        assert_eq!(
            serde_json::to_value(private.json_ld()).expect("unchanged private view"),
            before
        );
        assert!(
            authored
                .project_site_graph(&site, Default::default())
                .is_ok()
        );
        assert!(
            authored
                .project_discovery_graph(
                    &"site:other".parse().expect("other site"),
                    Default::default(),
                    &discovery,
                )
                .is_ok(),
            "one site's lineage failure does not reject an independent site"
        );
        let error = render(&inputs).expect_err("actual render cannot emit partial lineage");
        assert_eq!(error.code(), "publication_project_origin");
    }
    // Document indexing is not catalog identity admission: hidden upstream docs
    // do not block a separately public lineage reference, nor become exported docs.
    let config = configuration(&fixture, "public")
        .replace("upstream='project:subject'", "upstream='project:upstream'")
        + "[[placements]]\nrole='primary'\nsite='site:one'\nproject='project:upstream'\nrender=false\ndiscoverable=false\n";
    render(&acquire(&config)).expect("public catalog identity with hidden documentation");
}
