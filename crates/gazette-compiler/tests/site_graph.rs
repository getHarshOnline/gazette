//! Scoped graph projection through retained TOML/Markdown and the actual resolver.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::{Key, content::Limits};
use serde_json::{Value, json};

#[path = "site_graph/discovery.rs"]
mod discovery;

fn key(value: &str) -> Key {
    value.parse().expect("exact namespaced fixture key")
}

fn configuration() -> String {
    let mut source = String::new();
    for principal in ["publisher", "author", "foreign", "unused"] {
        source.push_str(&format!(
            "[[principals]]\nid='principal:{principal}'\nkind='person'\n"
        ));
    }
    for site in ["home", "foreign", "empty"] {
        source.push_str(&format!(
            "[[sites]]\nid='site:{site}'\npublisher='principal:publisher'\ndeployable=false\n"
        ));
    }
    for project in ["subject", "context", "provenance", "unused"] {
        source.push_str(&format!(
            "[[projects]]\nid='project:{project}'\nlifecycle='done'\n\
             visibility='private'\navailability='unavailable'\n"
        ));
    }
    source.push_str(
        "[[sources]]\nid='source:private'\n\
         location={kind='repository',root='/private/graph-fixture',commit='1111111111111111111111111111111111111111',subdirectory='docs'}\n\
         [[brands]]\nid='brand:one'\nname='Not an identity source'\n\
         [[brands.publications]]\nsite='site:home'\npublic_base_url='https://home.test/mount/'\n\
         authored=[{source='source:private',target='private-home'}]\n\
         [[brands.publications]]\nsite='site:foreign'\npublic_base_url='https://foreign.test/'\n\
         authored=[{source='source:private',target='private-foreign'}]\n\
         [[placements]]\nrole='primary'\nsite='site:home'\nproject='project:provenance'\n\
         render=true\ndiscoverable=false\ndocuments={source='source:private',target='private-native'}\n",
    );
    source
}

fn document(id: &str, site: &str, lifecycle: &str, structure: &str, extra: &str) -> String {
    format!(
        "+++\n{extra}\n[content]\nid='{id}'\nsite='site:{site}'\n\
         title='{id} title'\nlifecycle='{lifecycle}'\n\
         route={{path='/{}/'}}\nstructure={structure}\n+++\n\
         Body deliberately absent from graph: {id}\n",
        id.replace(':', "-")
    )
}

fn documents() -> Vec<(String, String)> {
    let page = "{kind='page',family={kind='article'}}";
    vec![
        (
            "private-home/draft.md".into(),
            document(
                "content:draft",
                "home",
                "draft",
                page,
                "related=['content:foreign']\n\
             projects=[{project='project:subject',kind='about'}]",
            )
            .replace(
                "lifecycle='draft'",
                "lifecycle='draft'\nattributions=[{principal='principal:author',role='author'}]",
            ),
        ),
        (
            "private-home/archive.md".into(),
            document(
                "content:archive",
                "home",
                "archived",
                page,
                "collections=['collection:local']\nrelated=['journey:foreign']",
            ),
        ),
        (
            "private-home/collection.md".into(),
            document(
                "collection:local",
                "home",
                "published",
                "{kind='collection'}",
                "related=['collection:foreign']\nprojects=[{project='project:context',kind='context'}]",
            ),
        ),
        (
            "private-home/journey.md".into(),
            document(
                "journey:local",
                "home",
                "published",
                "{kind='journey',collection='collection:local',members=[{content='content:step',primary=true},{content='content:draft',primary=true}]}",
                "",
            ),
        ),
        (
            "private-native/guide.md".into(),
            document("content:native", "home", "published", page, ""),
        ),
        (
            "private-foreign/collection.md".into(),
            document(
                "collection:foreign",
                "foreign",
                "draft",
                "{kind='collection'}",
                "projects=[{project='project:context',kind='context'}]",
            ),
        ),
        (
            "private-foreign/page.md".into(),
            document(
                "content:foreign",
                "foreign",
                "draft",
                page,
                "related=['collection:local']",
            )
            .replace(
                "title='content:foreign title'",
                "title='FOREIGN SECRET TITLE'",
            )
            .replace(
                "lifecycle='draft'",
                "lifecycle='draft'\nattributions=[{principal='principal:foreign',role='author'}]",
            ),
        ),
        (
            "private-foreign/journey.md".into(),
            document(
                "journey:foreign",
                "foreign",
                "draft",
                "{kind='journey',collection='collection:foreign',members=[{content='content:foreign',primary=true}]}",
                "related=['content:archive']",
            ),
        ),
        (
            "private-foreign/unrelated.md".into(),
            document(
                "content:unrelated",
                "foreign",
                "published",
                page,
                "related=['content:draft']",
            ),
        ),
        (
            "private-home/step.md".into(),
            document("content:step", "home", "archived", page, ""),
        ),
    ]
}

fn corpus(files: &[(String, String)]) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        files.iter().map(|(path, bytes)| Input {
            path: path.parse().expect("literal retained path"),
            bytes: bytes.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real retained corpus parser")
}

fn projection(files: &[(String, String)]) -> Vec<u8> {
    let compiled = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let corpus = corpus(files);
    let resolved = authored::resolve(&compiled, &corpus, Limits::default()).expect("bound graph");
    let graph = resolved
        .project_site_graph(&key("site:home"), Limits::default())
        .expect("site graph");
    serde_json::to_vec(&graph).expect("bounded fixture serialization")
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("identity-keyed map")
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn selected_site_preserves_local_states_and_minimal_foreign_references_without_private_sources() {
    let files = documents();
    let bytes = projection(&files);
    let graph: Value = serde_json::from_slice(&bytes).expect("graph JSON");
    assert_eq!(graph["format"], "gazette-local-site-graph/1");
    assert_eq!(graph["site"], "site:home");
    assert_eq!(graph["publisher"], "principal:publisher");
    assert_eq!(
        keys(&graph["nodes"]),
        [
            "collection:local",
            "content:archive",
            "content:draft",
            "content:native",
            "content:step",
            "journey:local"
        ]
    );
    assert_eq!(graph["nodes"]["content:draft"]["lifecycle"], "draft");
    assert_eq!(graph["nodes"]["content:archive"]["lifecycle"], "archived");
    assert_eq!(
        keys(&graph["references"]),
        ["collection:foreign", "content:foreign", "journey:foreign"]
    );
    for (id, reference) in graph["references"].as_object().expect("foreign references") {
        assert_eq!(reference["id"], id.as_str());
        assert_eq!(reference["site"], "site:foreign");
        let mut fields = keys(reference);
        fields.sort_unstable();
        assert_eq!(fields, ["canonical_url", "id", "site", "title"]);
    }
    assert_eq!(
        graph["references"]["content:foreign"]["canonical_url"],
        "https://foreign.test/content-foreign/"
    );
    let compiled = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Limits::default()).expect("bound graph");
    for id in keys(&graph["nodes"]) {
        assert_eq!(
            graph["routes"][id],
            serde_json::to_value(
                resolved
                    .content()
                    .route(&key(id))
                    .expect("same compiled route")
            )
            .expect("route")
        );
    }
    assert_eq!(
        graph["references"]["content:foreign"]["title"],
        "FOREIGN SECRET TITLE"
    );
    let text = std::str::from_utf8(&bytes).expect("UTF-8 graph");
    for forbidden in [
        "Body deliberately",
        "private-home",
        "private-foreign",
        "private-native",
        "/private/graph-fixture",
        "source:private",
        "principal:foreign",
        "principal:unused",
        "project:unused",
        "content:unrelated",
    ] {
        assert!(
            !text.contains(forbidden),
            "unexpected graph disclosure: {forbidden}"
        );
    }
}

#[test]
fn typed_incident_edges_keep_journey_positions_and_do_not_reverse_editorial_direction() {
    let graph: Value = serde_json::from_slice(&projection(&documents())).expect("graph");
    assert_eq!(
        graph["memberships"],
        json!([
            {"content":"content:archive","collection":"collection:local"}
        ])
    );
    assert_eq!(
        graph["related"],
        json!([
            {"source":"collection:local","target":"collection:foreign"},
            {"source":"content:archive","target":"journey:foreign"},
            {"source":"content:draft","target":"content:foreign"}
        ])
    );
    let compiled = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let files = documents();
    let corpus = corpus(&files);
    let resolved =
        authored::resolve(&compiled, &corpus, Limits::default()).expect("complete directed graph");
    for (source, target) in [
        ("content:foreign", "collection:local"),
        ("journey:foreign", "content:archive"),
    ] {
        assert!(
            resolved
                .content()
                .graph()
                .related()
                .iter()
                .any(|edge| edge.source == key(source) && edge.target == key(target)),
            "incoming declaration remains in its original owning graph"
        );
        assert!(
            !graph["related"]
                .as_array()
                .expect("outgoing site references")
                .iter()
                .any(|edge| edge["source"] == source),
            "site projection does not reverse an incoming relation"
        );
    }
    assert_eq!(
        graph["containers"],
        json!([
            {"child":"journey:local","parent":"collection:local"}
        ])
    );
    assert_eq!(
        graph["journeys"],
        json!([
            {"journey":"journey:local","content":"content:step","position":0,"primary":true},
            {"journey":"journey:local","content":"content:draft","position":1,"primary":true}
        ])
    );
    assert_eq!(
        graph["nodes"]["journey:local"]["structure"]["members"][0]["content"],
        "content:step"
    );
}

#[test]
fn scoped_catalogs_reuse_resolved_context_and_source_provenance_without_identity_inference() {
    let graph: Value = serde_json::from_slice(&projection(&documents())).expect("graph");
    assert_eq!(
        keys(&graph["principals"]),
        ["principal:author", "principal:publisher"]
    );
    assert_eq!(
        keys(&graph["projects"]),
        ["project:context", "project:provenance", "project:subject"]
    );
    for project in graph["projects"]
        .as_object()
        .expect("scoped catalog")
        .values()
    {
        assert_eq!(project["lifecycle"], "done");
        assert_eq!(project["visibility"], "private");
        assert_eq!(project["availability"], "unavailable");
    }
    assert_eq!(
        graph["project_links"]["content:draft"]["project:subject"]["relation"],
        "about"
    );
    assert_eq!(
        graph["project_links"]["content:draft"]["project:context"],
        json!({
            "relation":"mentions", "evidence":[{"source":"collection:local","kind":"context"}]
        })
    );
    assert_eq!(
        graph["declarations"],
        json!([
            {"content":"collection:local","project":"project:context","kind":"context"},
            {"content":"content:draft","project":"project:subject","kind":"about"}
        ])
    );
    assert_eq!(
        graph["provenance"]["content:native"],
        json!({"site":"site:home","project":"project:provenance"})
    );
    assert_eq!(
        graph["provenance"]["content:draft"],
        json!({"site":"site:home"})
    );
    assert_eq!(
        graph["project_links"]["content:native"],
        json!({}),
        "source placement creates no subject/mention"
    );
    assert!(
        graph["nodes"]["content:native"]["attributions"].is_null(),
        "publisher does not become an author"
    );
}

#[test]
fn source_order_is_irrelevant_but_journey_order_remains_meaningful() {
    let files = documents();
    let first = projection(&files);
    let mut reordered = files.clone();
    reordered.reverse();
    assert_eq!(first, projection(&reordered));
    let journey = reordered
        .iter_mut()
        .find(|(path, _)| path == "private-home/journey.md")
        .expect("journey");
    journey.1 = journey.1.replace(
        "members=[{content='content:step',primary=true},{content='content:draft',primary=true}]",
        "members=[{content='content:draft',primary=true},{content='content:step',primary=true}]",
    );
    let changed: Value = serde_json::from_slice(&projection(&reordered)).expect("changed graph");
    assert_eq!(changed["journeys"][0]["content"], "content:draft");
    assert_eq!(changed["journeys"][0]["position"], 0);
    assert_ne!(first, projection(&reordered));
}

#[test]
fn missing_site_and_tighter_projection_limits_return_no_partial_graph() {
    let files = documents();
    let compiled = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let corpus = corpus(&files);
    let resolved =
        authored::resolve(&compiled, &corpus, Limits::default()).expect("complete graph");
    for site in ["site:missing", "principal:publisher", "project:subject"] {
        let error = resolved
            .project_site_graph(&key(site), Limits::default())
            .expect_err("unknown exact site");
        assert_eq!(error.code(), "graph_site");
        assert!(matches!(error, authored::SiteGraphError::Site(id) if id == key(site)));
    }
    for limits in [
        Limits {
            nodes: 1,
            ..Limits::default()
        },
        Limits {
            edges: 0,
            ..Limits::default()
        },
        Limits {
            text_bytes: 1,
            ..Limits::default()
        },
    ] {
        let error = resolved
            .project_site_graph(&key("site:home"), limits)
            .expect_err("projection ceiling cannot truncate to success");
        assert_eq!(error.code(), "graph_limit");
    }
    assert!(
        resolved
            .project_site_graph(&key("site:home"), Limits::default())
            .is_ok(),
        "failed projections leave the resolved owner reusable"
    );
}

#[test]
fn exact_projection_ceiling_includes_foreign_catalogs_edges_and_encoded_bytes() {
    let files = documents();
    let compiled = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Limits::default()).expect("bound graph");
    let graph = resolved
        .project_site_graph(&key("site:home"), Limits::default())
        .expect("graph");
    for node in graph.nodes.values() {
        assert!(
            std::ptr::eq(
                *node,
                resolved
                    .content()
                    .graph()
                    .node(&node.id)
                    .expect("native graph node")
            ),
            "projection borrows its sole node owner"
        );
    }
    let nodes =
        graph.nodes.len() + graph.references.len() + graph.projects.len() + graph.principals.len();
    let links: usize = graph
        .project_links
        .values()
        .map(|links| {
            links
                .values()
                .map(|link| 1 + link.evidence().len())
                .sum::<usize>()
        })
        .sum();
    let edges = graph.memberships.len()
        + graph.related.len()
        + graph.containers.len()
        + graph.journeys.len()
        + graph.declarations.len()
        + graph.provenance.len()
        + graph
            .nodes
            .values()
            .map(|node| node.attributions.len())
            .sum::<usize>()
        + links;
    let text_bytes = serde_json::to_vec(&graph)
        .expect("same serialized graph")
        .len();
    let exact = Limits {
        nodes,
        edges,
        text_bytes,
    };
    assert!(
        resolved
            .project_site_graph(&key("site:home"), exact)
            .is_ok(),
        "inclusive exact budgets"
    );
    for limits in [
        Limits {
            nodes: nodes - 1,
            ..exact
        },
        Limits {
            edges: edges - 1,
            ..exact
        },
        Limits {
            text_bytes: text_bytes - 1,
            ..exact
        },
    ] {
        assert_eq!(
            resolved
                .project_site_graph(&key("site:home"), limits)
                .expect_err("one unit short")
                .code(),
            "graph_limit"
        );
    }
}

#[test]
fn foreign_absent_route_and_empty_site_never_invent_urls_or_local_records() {
    let mut files = documents();
    let foreign = files
        .iter_mut()
        .find(|(path, _)| path == "private-foreign/page.md")
        .expect("foreign source");
    foreign.1 = foreign.1.replace("route={path='/content-foreign/'}\n", "");
    let compiled = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let corpus = corpus(&files);
    let resolved = authored::resolve(&compiled, &corpus, Limits::default())
        .expect("route is optional for structural inspection");
    let graph = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:home"), Limits::default())
            .expect("home graph"),
    )
    .expect("projection");
    assert_eq!(
        graph["references"]["content:foreign"],
        json!({"id":"content:foreign","site":"site:foreign","title":"FOREIGN SECRET TITLE"})
    );
    let empty = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:empty"), Limits::default())
            .expect("configured empty site"),
    )
    .expect("projection");
    assert_eq!(keys(&empty["principals"]), ["principal:publisher"]);
    for map in [
        "nodes",
        "references",
        "routes",
        "projects",
        "project_links",
        "provenance",
    ] {
        assert!(
            empty[map].as_object().expect("map").is_empty(),
            "empty site's {map}"
        );
    }
    for edges in [
        "memberships",
        "related",
        "containers",
        "journeys",
        "declarations",
    ] {
        assert!(
            empty[edges].as_array().expect("edge set").is_empty(),
            "empty site's {edges}"
        );
    }
}
