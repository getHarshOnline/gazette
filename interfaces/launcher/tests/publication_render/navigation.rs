//! Structural navigation crosses the actual retained SDK, Theme and CLI boundaries.

use std::{convert::Infallible, path::Path};

use gazette_sdk::html::{self, TextKind};
use serde_json::{Value, json};

use super::{
    Fixture, PAGE, RENDER_ARGUMENTS, acquire, build, bytes, cli, fs, render, source, tempdir,
};

const TEMPLATE: &[u8] = br#"<!doctype html><html><head><title>Navigation</title><script type="application/ld+json">{{ json_ld | json_script }}</script></head><body>{% for trail in breadcrumbs %}<nav data-via="{{ trail.via }}">{% for item in trail.items %}{% if item.route %}<a data-id="{{ item.node.id }}" href="{{ item.route.canonical_url }}">{{ item.node.title }}</a>{% else %}<span data-id="{{ item.node.id }}">{{ item.node.title }}</span>{% endif %}{% endfor %}</nav>{% endfor %}{% for step in journeys %}<section data-journey="{{ step.journey.id }}" data-position="{{ step.position }}" data-total="{{ step.total }}">{% if step.previous %}{% if step.previous.route %}<a rel="prev" href="{{ step.previous.route.canonical_url }}">{{ step.previous.node.title }}</a>{% else %}<span data-unrouted="previous">{{ step.previous.node.title }}</span>{% endif %}{% endif %}{% if step.next %}{% if step.next.route %}<a rel="next" href="{{ step.next.route.canonical_url }}">{{ step.next.node.title }}</a>{% else %}<span data-unrouted="next">{{ step.next.node.title }}</span>{% endif %}{% endif %}</section>{% endfor %}{% for item in related %}<span data-related="{{ item.id }}">{{ item.title }}</span>{% endfor %}{{ body_html | safe }}</body></html>"#;

fn document(id: &str, site: &str, title: &str, route: Option<&str>, structure: &str) -> String {
    let route = route.map_or_else(String::new, |path| format!("route={{path='{path}'}}\n"));
    format!(
        "+++\n[content]\nid='{id}'\nsite='{site}'\ntitle='{title}'\nlifecycle='archived'\n{route}structure={{{structure}}}\n+++\nRetained body for {id}.\n"
    )
}

fn fixture() -> Fixture {
    fixture_with_legacy_overlap(false)
}

fn fixture_with_legacy_overlap(legacy: bool) -> Fixture {
    let mut page = PAGE
        .replace("collections=['collection:home']\n", "")
        .replace(
            "related=['content:other']",
            "related=['content:other','collection:foreign','journey:z']",
        )
        .replace("lifecycle='draft'", "lifecycle='published'")
        + "\n[[site:other.example/foreign/|Foreign <collection>]]\n";
    let branch = document(
        "collection:branch",
        "site:one",
        "Branch & detail",
        Some("/branch/"),
        "kind='collection',parent='collection:home'",
    );
    let foreign = document(
        "collection:foreign",
        "site:other",
        "Foreign <collection>",
        Some("/foreign/"),
        "kind='collection'",
    );
    let other = document(
        "content:other",
        "site:other",
        "Foreign without route",
        None,
        "kind='page',family={kind='documentation'}",
    );
    let last = document(
        "content:last",
        "site:one",
        "Last member",
        Some("/last/"),
        "kind='page',family={kind='documentation'}",
    );
    let prior = document(
        "content:prior",
        "site:one",
        "Prior member",
        Some("/prior/"),
        "kind='page',family={kind='documentation'}",
    );
    let solo = document(
        "content:solo",
        "site:one",
        "Standalone",
        Some("/solo/"),
        "kind='page',family={kind='documentation'}",
    );
    let mut journey = document(
        "journey:a",
        "site:one",
        "Ordered journey",
        Some("/journey/"),
        "kind='journey',collection='collection:branch',members=[{content='content:prior',primary=true},{content='content:one',primary=true},{content='content:last',primary=true}]",
    );
    let mut alternate = document(
        "journey:z",
        "site:other",
        "Alternate journey",
        None,
        "kind='journey',collection='collection:foreign',members=[]",
    );
    let singleton = document(
        "content:singleton",
        "site:one",
        "Singleton member",
        Some("/singleton/"),
        "kind='page',family={kind='documentation'}",
    );
    let singleton_journey = document(
        "journey:singleton",
        "site:one",
        "Singleton journey",
        Some("/singleton-journey/"),
        "kind='journey',collection='collection:branch',members=[{content='content:singleton',primary=true}]",
    );
    if legacy {
        page = page.replacen(
            "+++\n",
            "+++\ncollections=['collection:branch','collection:foreign']\n",
            1,
        );
        journey = journey.replace("primary=true", "primary=false");
        alternate = alternate.replace(
            "members=[]",
            "members=[{content='content:one',primary=false}]",
        );
    }
    Fixture::new(&[
        ("theme/templates/page.html", TEMPLATE),
        ("one/page.md", page.as_bytes()),
        ("one/branch.md", branch.as_bytes()),
        ("one/journey.md", journey.as_bytes()),
        ("one/last.md", last.as_bytes()),
        ("one/prior.md", prior.as_bytes()),
        ("one/singleton.md", singleton.as_bytes()),
        ("one/singleton-journey.md", singleton_journey.as_bytes()),
        ("one/solo.md", solo.as_bytes()),
        ("other/page.md", other.as_bytes()),
        ("other/collection.md", foreign.as_bytes()),
        ("other/journey.md", alternate.as_bytes()),
    ])
}

fn configuration(fixture: &Fixture) -> String {
    fixture.config().replacen(
        "public_base_url=",
        "machine={search='lexical',json_ld=true}\npublic_base_url=",
        1,
    )
}

fn linked_data(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    html::inspect(bytes, html::Limits::default(), |kind, value| {
        match kind {
            TextKind::JsonScript => {
                values.push(serde_json::from_str(value).expect("complete JSON-LD script"))
            }
            TextKind::Script | TextKind::ModuleScript | TextKind::DataScript | TextKind::Style => {
                panic!("navigation must not create executable text")
            }
            _ => (),
        }
        Ok::<_, Infallible>(())
    })
    .expect("real HTML tokenizer validates navigation transport");
    values
}

#[test]
fn all_structural_paths_and_exact_journey_neighbors_share_rendered_and_machine_models() {
    let fixture = fixture();
    let config = configuration(&fixture);
    let inputs = acquire(&config);
    assert_eq!(
        inputs
            .compilation()
            .workspace
            .composition_for(&"site:other".parse().expect("destination site"))
            .and_then(|composition| composition.theme())
            .map(|selection| selection.value().package.as_str()),
        Some("theme:one"),
        "the exact destination Theme binding exists before projection"
    );
    let rendered = render(&inputs).expect("sole local residence with foreign related endpoints");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("site model");
    let page = &model["pages"]["content:one"];
    let trails = page["breadcrumbs"]
        .as_array()
        .expect("all structural paths");
    let paths: Vec<_> = trails.iter().map(|trail| {
        json!({"via":trail["via"],"ids":trail["items"].as_array().expect("ordered endpoints").iter().map(|item| item["node"]["id"].clone()).collect::<Vec<_>>()})
    }).collect();
    assert_eq!(
        paths,
        vec![
            json!({"via":"journey:a","ids":["collection:home","collection:branch","journey:a","content:one"]})
        ]
    );
    assert_eq!(trails[0]["items"][1]["node"]["lifecycle"], "archived");
    let related = page["related"]
        .as_array()
        .expect("explicit related endpoints");
    let foreign = related
        .iter()
        .find(|node| node["id"] == "collection:foreign")
        .expect("foreign reference retained");
    assert_eq!(foreign["site"], "site:other");
    assert_eq!(
        foreign["presentation"]["theme"], "theme:one",
        "navigation carries the destination site's resolved presentation"
    );
    assert_eq!(foreign["route"]["path"], "/foreign/");
    for id in ["content:other", "journey:z"] {
        assert!(
            related
                .iter()
                .find(|node| node["id"] == id)
                .expect("unrouted foreign reference retained")["route"]
                .is_null()
        );
    }
    let memberships = page["journeys"]
        .as_array()
        .expect("sole containing journey");
    assert_eq!(memberships.len(), 1);
    assert_eq!(memberships[0]["journey"]["id"], "journey:a");
    assert_eq!(memberships[0]["position"], 1);
    assert_eq!(memberships[0]["total"], 3);
    assert_eq!(
        page["collections"],
        json!([]),
        "journey residence is not duplicated as a direct collection membership"
    );
    assert_eq!(memberships[0]["previous"]["node"]["id"], "content:prior");
    assert_eq!(
        memberships[0]["previous"]["route"]["canonical_url"],
        "https://one.example/mount/prior/"
    );
    assert_eq!(memberships[0]["next"]["node"]["id"], "content:last");
    assert_eq!(
        memberships[0]["next"]["route"]["canonical_url"],
        "https://one.example/mount/last/"
    );
    let singleton = &model["pages"]["content:singleton"]["journeys"][0];
    assert_eq!(singleton["journey"]["id"], "journey:singleton");
    assert_eq!(singleton["position"], 0);
    assert_eq!(singleton["total"], 1);
    for field in ["previous", "next"] {
        assert_eq!(singleton[field], Value::Null, "no inferred {field}");
    }
    assert!(
        memberships.iter().all(|step| step["primary"] == true),
        "containing journey membership is explicitly primary"
    );
    assert_eq!(
        model["pages"]["content:last"]["journeys"][0]["next"],
        Value::Null
    );
    assert_eq!(
        model["pages"]["content:prior"]["journeys"][0]["previous"],
        Value::Null
    );
    assert_eq!(
        model["pages"]["content:prior"]["journeys"][0]["next"]["node"]["id"],
        "content:one"
    );
    let standalone = &model["pages"]["content:solo"];
    assert_eq!(standalone["breadcrumbs"][0]["via"], "content:solo");
    assert_eq!(
        standalone["breadcrumbs"][0]["items"]
            .as_array()
            .expect("self-only path")
            .len(),
        1
    );
    assert_eq!(standalone["journeys"], json!([]));
    for foreign in ["content:other", "collection:foreign", "journey:z"] {
        assert!(
            model["pages"].get(foreign).is_none(),
            "foreign endpoint is not a local page"
        );
    }

    let html = std::str::from_utf8(bytes(tree, "guide/index.html"))
        .expect("rendered HTML")
        .replace("&#x2F;", "/");
    let mut attributes = Vec::new();
    let mut text = Vec::new();
    html::inspect(
        bytes(tree, "guide/index.html"),
        html::Limits::default(),
        |kind, value| {
            match kind {
                TextKind::Attribute => attributes.push(value.to_owned()),
                TextKind::Text => text.push(value.to_owned()),
                _ => (),
            }
            Ok::<_, Infallible>(())
        },
    )
    .expect("native decoder preserves escaped Theme attributes and text");
    for expected in [
        "journey:a",
        "collection:home",
        "collection:branch",
        "https://one.example/mount/",
        "https://one.example/mount/branch/",
        "https://other.example/foreign/",
        "https://one.example/mount/prior/",
        "https://one.example/mount/last/",
        "prev",
        "next",
    ] {
        assert!(
            attributes.iter().any(|value| value == expected),
            "exact decoded navigation attribute {expected}"
        );
    }
    for expected in [
        "Home",
        "Branch & detail",
        "Foreign <collection>",
        "Prior member",
        "Last member",
    ] {
        assert!(
            text.iter().any(|value| value == expected),
            "exact decoded navigation text {expected}"
        );
    }
    assert!(html.contains("<span data-related=\"journey:z\">Alternate journey</span>"));
    assert!(html.contains("<span data-related=\"content:other\">Foreign without route</span>"));
    assert!(
        !attributes
            .iter()
            .any(|value| value == "https://one.example/mount/foreign/"),
        "foreign routes never acquire the current site's prefix"
    );
    assert!(
        !attributes
            .iter()
            .any(|value| value == "https://other.example/elsewhere/"),
        "no stale or guessed foreign neighbor route"
    );
    assert!(
        !html.contains("Retained body for content:other"),
        "foreign navigation does not copy body content"
    );
    assert!(!html.contains(fixture.directory.path().to_str().expect("native path")));

    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("real search output");
    let linked: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site JSON-LD");
    let parsed = build::parse_publication(&inputs, Default::default())
        .expect("same retained source without native reads");
    let publication = parsed
        .resolve(Default::default())
        .expect("shared compiler model");
    let resolved = publication.authored();
    assert_eq!(
        resolved
            .content()
            .route(&"collection:foreign".parse().expect("foreign identity"))
            .expect("exact foreign route")
            .canonical_url,
        "https://other.example/foreign/"
    );
    let discovery = resolved.discovery().expect("exact source-bound discovery");
    let mut native_linked = std::collections::BTreeMap::new();
    for (id, page) in model["pages"].as_object().expect("all local pages") {
        let projection = resolved
            .project_discovery_page(
                &id.parse().expect("exact model identity"),
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .expect("same emitted page projection");
        let expected_direct = serde_json::to_value(
            projection
                .discovery_json_ld(&discovery)
                .expect("direct source-bound role"),
        )
        .expect("compiler direct JSON-LD");
        let expected_listing = serde_json::to_value(
            projection
                .listing_json_ld(&discovery)
                .expect("eligible aggregate role"),
        )
        .expect("compiler aggregate JSON-LD");
        let public_direct = serde_json::to_value(
            projection
                .discovery_json_ld(&discovery)
                .expect("direct role")
                .public(),
        )
        .expect("public direct JSON-LD");
        let public_listing = serde_json::to_value(
            projection
                .listing_json_ld(&discovery)
                .expect("aggregate role")
                .public(),
        )
        .expect("public aggregate JSON-LD");
        let search = search
            .iter()
            .find(|value| value["content"]["id"] == *id)
            .expect("matching search page");
        assert_eq!(search["content"]["presentation"]["theme"], "theme:one");
        assert_eq!(search["breadcrumbs"], page["breadcrumbs"]);
        assert_eq!(search["journeys"], page["journeys"]);
        let ld = &page["json_ld"];
        assert_eq!(
            ld, &public_listing,
            "site model uses the compiler's aggregate role"
        );
        assert_eq!(expected_listing["gazette:breadcrumbs"]["@type"], "@json");
        assert_eq!(
            expected_listing["gazette:breadcrumbs"]["@value"],
            page["breadcrumbs"]
        );
        assert!(
            ld.as_object()
                .unwrap()
                .keys()
                .all(|key| !key.starts_with("gazette:"))
        );
        assert_eq!(
            linked["@graph"]
                .as_array()
                .expect("graph")
                .iter()
                .find(|value| value["identifier"] == *id)
                .expect("matching linked-data page"),
            ld
        );
        assert_eq!(
            linked_data(bytes(
                tree,
                page["route"]["output"].as_str().expect("output")
            ))
            .as_slice(),
            std::slice::from_ref(&public_direct)
        );
        for field in [
            "@id",
            "@type",
            "identifier",
            "name",
            "creativeWorkStatus",
            "url",
            "text",
            "publisher",
            "gazette:route",
            "gazette:site",
            "gazette:breadcrumbs",
            "breadcrumb",
            "mainEntityOfPage",
            "gazette:collections",
            "gazette:journeys",
            "gazette:editorialRelated",
            "gazette:headings",
            "gazette:attributions",
        ] {
            assert_eq!(
                expected_direct.get(field),
                expected_listing.get(field),
                "all-discoverable scalar and ordered-navigation parity for {id}: {field}"
            );
        }
        if id == "collection:branch" {
            assert_eq!(
                expected_direct["gazette:structure"]["@value"]["parent"],
                "collection:home"
            );
            assert_eq!(
                expected_listing["gazette:structure"]["@value"],
                json!({"kind":"collection"})
            );
        }
        if id == "journey:a" {
            assert_eq!(
                expected_direct["gazette:structure"]["@value"]["members"]
                    .as_array()
                    .expect("exact authored members")
                    .len(),
                3
            );
            assert_eq!(
                expected_direct["gazette:structure"]["@value"]["members"]
                    .as_array()
                    .expect("exact authored members")
                    .iter()
                    .map(|member| member["content"].clone())
                    .collect::<Vec<_>>(),
                [
                    json!("content:prior"),
                    json!("content:one"),
                    json!("content:last")
                ]
            );
            assert_eq!(
                expected_listing["gazette:structure"]["@value"],
                json!({"kind":"journey"})
            );
            assert_eq!(
                expected_direct["mainEntity"], expected_listing["mainEntity"],
                "all eligible journey members keep declared positions"
            );
        }
        native_linked.insert(id.clone(), expected_listing);
    }
    let ld = &page["json_ld"];
    assert_eq!(ld["@type"], "Article");
    assert_eq!(ld["identifier"], "content:one");
    assert!(
        ld.get("breadcrumb").is_none(),
        "Article is not relabeled a WebPage"
    );
    assert_eq!(ld["mainEntityOfPage"]["@type"], "WebPage");
    assert_ne!(
        ld["mainEntityOfPage"]["@id"], ld["@id"],
        "Article and its presentation page retain distinct identities"
    );
    let standard = ld["mainEntityOfPage"]["breadcrumb"]
        .as_array()
        .expect("all standard breadcrumb lists");
    assert_eq!(standard.len(), trails.len());
    for (list, trail) in standard.iter().zip(trails) {
        assert_eq!(list["@type"], "BreadcrumbList");
        let entries = list["itemListElement"].as_array().expect("ordered list");
        let items = trail["items"].as_array().expect("same endpoints");
        assert_eq!(entries.len(), items.len());
        for (index, (entry, item)) in entries.iter().zip(items).enumerate() {
            assert_eq!(entry["@type"], "ListItem");
            assert_eq!(entry["position"], index + 1);
            assert_eq!(entry["name"], item["node"]["title"]);
            // A trail names each step by identity; the step's facts stay on its own page.
            assert_eq!(
                entry["item"].as_object().map(|item| item.len()),
                Some(1),
                "a step is referenced by identity alone"
            );
            if item["route"].is_null() {
                assert!(
                    entry["item"]["@id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("_:")),
                    "no synthesized endpoint URL"
                );
            } else {
                assert_eq!(entry["item"]["@id"], item["route"]["canonical_url"]);
            }
        }
    }
    let native_ld = &native_linked["content:one"];
    assert_eq!(native_ld["gazette:journeys"][0]["gazette:position"], 1);
    assert_eq!(native_ld["gazette:journeys"][0]["gazette:total"], 3);
    assert_eq!(
        native_ld["gazette:journeys"][0]["gazette:previous"]["identifier"],
        "content:prior"
    );
    assert_eq!(
        native_ld["gazette:journeys"][0]["gazette:previous"]["url"],
        "https://one.example/mount/prior/"
    );
    assert_eq!(
        native_ld["gazette:journeys"][0]["gazette:next"]["url"],
        "https://one.example/mount/last/"
    );
    for field in ["gazette:previous", "gazette:next"] {
        assert!(
            native_linked["content:singleton"]["gazette:journeys"][0]
                .get(field)
                .is_none(),
            "singleton has no invented {field}"
        );
    }
    let foreign_related = native_ld["gazette:editorialRelated"]
        .as_array()
        .expect("related JSON-LD");
    assert_eq!(
        foreign_related
            .iter()
            .find(|node| node["identifier"] == "collection:foreign")
            .expect("foreign collection")["gazette:site"]["identifier"],
        "site:other"
    );
    for id in ["content:other", "journey:z"] {
        assert!(
            foreign_related
                .iter()
                .find(|node| node["identifier"] == id)
                .expect("foreign unrouted reference")
                .get("url")
                .is_none(),
            "relatedness never synthesizes {id}'s URL"
        );
    }
    assert_eq!(
        model["pages"]["collection:branch"]["json_ld"]["@type"],
        "CollectionPage"
    );
    assert!(model["pages"]["collection:branch"]["json_ld"]["breadcrumb"].is_array());
    assert_eq!(standalone["json_ld"]["@type"], "WebPage");
    assert!(
        standalone["json_ld"].get("breadcrumb").is_none(),
        "a trail that holds only the page itself says nothing"
    );

    let parent = tempdir().expect("disjoint staging parent");
    let sdk_stage = build::stage_publication(
        &rendered,
        parent.path(),
        source::Limits::default(),
        16 << 10,
    )
    .expect("SDK native stage");
    assert_eq!(
        fs::read(sdk_stage.path().join("guide/index.html")).expect("staged navigation"),
        bytes(tree, "guide/index.html")
    );
    sdk_stage.close().expect("scoped SDK stage cleanup");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI stage receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("same SDK inventory")
    );
    let directory = Path::new(receipt["directory"].as_str().expect("CLI directory"));
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("exact CLI read-back"),
                tree.file(&entry.path).expect("SDK bytes")
            );
        }
    }
    fs::write(
        fixture.directory.path().join("other/collection.md"),
        "changed foreign ancestry",
    )
    .expect("disposable mutation");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "changed template",
    )
    .expect("disposable mutation");
    let repeated = render(&inputs).expect("retained rebuild ignores changed source and template");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        serde_json::to_value(repeated.site().evidence()).expect("rebuild evidence"),
        serde_json::to_value(rendered.site().evidence()).expect("original evidence")
    );
}

#[test]
fn legacy_overlapping_and_cross_site_memberships_fail_with_retained_declarations() {
    let fixture = fixture_with_legacy_overlap(true);
    let inputs = acquire(&configuration(&fixture));
    let parsed =
        build::parse_publication(&inputs, Default::default()).expect("actual retained ASTs");
    let error = parsed
        .resolve(Default::default())
        .expect_err("no partial legacy graph");
    let build::PublicationResolutionError::Authored(gazette_sdk::authored::Error::Structure {
        error,
        declarations,
    }) = error
    else {
        panic!("expected source-bound structural failure");
    };
    let gazette_sdk::publication::content::Error::Invalid(diagnostics) = error else {
        panic!("expected exact independent graph constraints");
    };
    for code in [
        "content_multiple_residence",
        "journey_member_not_primary",
        "content_containment_site",
    ] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "retained legacy constraint {code}: {diagnostics:?}"
        );
    }
    let page_id = "content:one".parse().expect("exact page identity");
    let homes: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code == "content_multiple_residence" && diagnostic.source == page_id
        })
        .map(|diagnostic| {
            diagnostic
                .target
                .as_ref()
                .expect("conflicting residence")
                .as_str()
        })
        .collect();
    assert_eq!(
        homes,
        [
            "collection:branch",
            "collection:foreign",
            "journey:a",
            "journey:z"
        ]
    );
    let origins = declarations
        .get(&page_id)
        .expect("retained declaring source");
    assert_eq!(origins.len(), 1);
    assert!(origins[0].path.as_str().ends_with("one/page.md"));
    assert!(
        origins[0].span.is_some(),
        "metadata range remains bound to source bytes"
    );
    assert_eq!(
        origins[0].source_sha256,
        source::Sha256Digest::of(
            fs::read(fixture.directory.path().join("one/page.md"))
                .expect("exact fixture source")
                .as_slice()
        )
    );
}

#[test]
fn missing_local_route_remains_a_whole_site_failure_instead_of_navigation_fallback() {
    let fixture = fixture();
    let config = configuration(&fixture);
    // A missing foreign route is a retained non-clickable endpoint; the same
    // missing route on a selected local page must not become an invented URL.
    let inputs = build::acquire_publication(
        &config,
        &"site:other".parse().expect("other site"),
        super::toml::Limits::default(),
        source::PreparationLimits {
            sources: 8,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("other site's exact capture");
    let error = render(&inputs).expect_err("unrouted selected pages cannot render");
    assert_eq!(error.code(), "site_route_required");
    let parent = tempdir().expect("preserved stage parent");
    fs::write(parent.path().join("keep"), b"unchanged").expect("sentinel");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[3] = "site:other";
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("site_route_required"));
    assert_eq!(
        fs::read(parent.path().join("keep")).expect("sentinel"),
        b"unchanged"
    );
    assert_eq!(fs::read_dir(parent.path()).expect("no stage").count(), 1);
}
