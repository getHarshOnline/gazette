//! Emitted listings preserve the original graph while suppressing generated endpoints.

use super::*;

fn mixed_documents() -> Vec<(String, String)> {
    let mut files = documents();
    files[3].1 = files[3]
        .1
        .replace(
            "{content='content:draft',primary=true}",
            "{content='content:native',primary=true},{content='content:draft',primary=true}",
        )
        .replace(
            "collection='collection:local'",
            "collection='collection:hidden'",
        );
    files.push((
        "private-native/hidden.md".into(),
        document(
            "collection:hidden",
            "home",
            "published",
            "{kind='collection',parent='collection:local'}",
            "projects=[{project='project:subject',kind='context'}]",
        ),
    ));
    files
}

#[test]
fn discovery_omits_nested_hidden_ids_and_whole_mixed_assertions_without_changing_inspection() {
    let config =
        workspace::compile(&configuration(), Default::default()).expect("native workspace");
    let files = mixed_documents();
    let corpus = corpus(&files);
    let resolved =
        authored::resolve(&config, &corpus, Limits::default()).expect("original resolved graph");
    let before = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:home"), Limits::default())
            .expect("private projection"),
    )
    .expect("private JSON");
    let discovery = resolved
        .discovery()
        .expect("complete source-bound eligibility");
    let view = resolved
        .project_discovery_graph(&key("site:home"), Limits::default(), &discovery)
        .expect("emitted graph");
    let value = serde_json::to_value(&view).expect("listing JSON");
    let encoded = serde_json::to_string(&value).expect("listing bytes");
    assert_eq!(value["format"], "gazette-discovery-site-graph/1");
    for hidden in ["content:native", "collection:hidden"] {
        assert!(
            !encoded.contains(hidden),
            "suppressed endpoint leaked: {hidden}"
        );
    }
    assert!(
        value["nodes"]["journey:local"]["structure"]
            .get("members")
            .is_none()
    );
    let steps: Vec<_> = value["journeys"]
        .as_array()
        .expect("journey listing")
        .iter()
        .filter(|step| step["journey"] == "journey:local")
        .map(|step| step["position"].as_u64().expect("original position"))
        .collect();
    assert_eq!(steps, [0, 2], "no rewritten sequence or bridged position");
    assert!(
        value["project_links"]["content:draft"]
            .get("project:subject")
            .is_none(),
        "mixed evidence assertion must be omitted whole"
    );
    assert!(
        value["projects"].get("project:subject").is_some(),
        "independent visible about declaration retains project identity"
    );
    assert!(
        value["declarations"]
            .as_array()
            .expect("declarations")
            .iter()
            .any(|declaration| declaration["content"] == "content:draft"
                && declaration["project"] == "project:subject")
    );
    assert_eq!(
        before["nodes"]["journey:local"]["structure"]["members"]
            .as_array()
            .expect("canonical members")
            .len(),
        3
    );
    assert_eq!(
        before["project_links"]["content:draft"]["project:subject"]["evidence"]
            .as_array()
            .expect("complete mixed evidence")
            .len(),
        2
    );
    assert_eq!(
        serde_json::to_value(
            resolved
                .project_site_graph(&key("site:home"), Limits::default())
                .expect("private projection after discovery")
        )
        .expect("private JSON"),
        before
    );
}

#[test]
fn discovery_binding_and_complete_output_limits_are_not_bypassed() {
    let config = workspace::compile(&configuration(), Default::default()).expect("workspace");
    let files = documents();
    let corpus = corpus(&files);
    let resolved = authored::resolve(&config, &corpus, Limits::default()).expect("first graph");
    let other =
        authored::resolve(&config, &corpus, Limits::default()).expect("distinct equal graph");
    let discovery = resolved.discovery().expect("bound discovery");
    assert!(matches!(
        other.project_discovery_graph(&key("site:home"), Limits::default(), &discovery),
        Err(authored::SiteGraphError::Discovery)
    ));
    let value = resolved
        .project_discovery_graph(&key("site:home"), Limits::default(), &discovery)
        .expect("bounded output");
    let exact = serde_json::to_vec(&value)
        .expect("exact encoded size")
        .len();
    resolved
        .project_discovery_graph(
            &key("site:home"),
            Limits {
                text_bytes: exact,
                ..Limits::default()
            },
            &discovery,
        )
        .expect("exact complete output fits");
    assert!(matches!(
        resolved.project_discovery_graph(
            &key("site:home"),
            Limits {
                text_bytes: exact - 1,
                ..Limits::default()
            },
            &discovery
        ),
        Err(authored::SiteGraphError::Limit("encoded bytes"))
    ));
    assert!(matches!(
        resolved.project_discovery_graph(
            &key("site:home"),
            Limits {
                nodes: 0,
                ..Limits::default()
            },
            &discovery
        ),
        Err(authored::SiteGraphError::Limit("nodes"))
    ));
}

#[test]
fn suppressed_foreign_endpoint_does_not_return_through_incident_relationships() {
    let config = format!(
        "{}\n[[placements]]\nrole='mirror'\nsite='site:foreign'\nproject='project:provenance'\nrender=true\ndiscoverable=false\ndocuments={{source='source:private',target='hidden-foreign'}}\n",
        configuration()
    );
    let compiled =
        workspace::compile(&config, Default::default()).expect("two-site source bindings");
    let mut files = documents();
    files[6].0 = "hidden-foreign/page.md".into();
    let corpus = corpus(&files);
    let resolved =
        authored::resolve(&compiled, &corpus, Limits::default()).expect("cross-site graph");
    let discovery = resolved.discovery().expect("all source bindings resolved");
    let graph = resolved
        .project_discovery_graph(&key("site:home"), Limits::default(), &discovery)
        .expect("local discovery with eligible foreign references");
    let value = serde_json::to_value(&graph).expect("listing JSON");
    assert!(
        !serde_json::to_string(&value)
            .expect("complete listing")
            .contains("content:foreign")
    );
    assert!(
        !serde_json::to_string(&value)
            .expect("complete listing")
            .contains("FOREIGN SECRET TITLE"),
        "a suppressed foreign endpoint must not disclose its display title"
    );
    assert!(
        value["references"].get("collection:foreign").is_some(),
        "independent eligible related collection remains"
    );
    assert!(
        value["related"]
            .as_array()
            .expect("incident references")
            .iter()
            .any(|edge| edge["source"] == "content:archive" && edge["target"] == "journey:foreign")
    );
}

#[test]
fn suppressed_canonical_endpoint_cannot_reenter_discovery_references() {
    let config = format!(
        "{}\n[[placements]]\nrole='mirror'\nsite='site:foreign'\nproject='project:provenance'\nrender=true\ndiscoverable=false\ndocuments={{source='source:private',target='hidden-foreign'}}\n",
        configuration()
    );
    let compiled = workspace::compile(&config, Default::default()).expect("workspace");
    let mut files = documents();
    files[6].0 = "hidden-foreign/page.md".into();
    files[0].1 = files[0].1.replace(
        "route={path='/content-draft/'}",
        "route={path='/content-draft/',canonical='content:foreign'}",
    );
    let corpus = corpus(&files);
    let resolved =
        authored::resolve(&compiled, &corpus, Limits::default()).expect("canonical origin");
    let discovery = resolved.discovery().expect("source eligibility");
    assert!(!discovery.contains(&key("content:foreign")));
    let value = serde_json::to_value(
        resolved
            .project_discovery_graph(&key("site:home"), Limits::default(), &discovery)
            .expect("discovery graph"),
    )
    .expect("JSON");
    assert!(value["references"].get("content:foreign").is_none());
    assert!(
        !serde_json::to_string(&value)
            .expect("complete JSON")
            .contains("FOREIGN SECRET TITLE")
    );
    assert!(
        value["nodes"].get("content:draft").is_some(),
        "local content remains available"
    );
    let private = serde_json::to_value(
        resolved
            .project_site_graph(&key("site:home"), Limits::default())
            .expect("private inspection"),
    )
    .expect("private JSON");
    assert_eq!(
        private["references"]["content:foreign"]["title"],
        "FOREIGN SECRET TITLE"
    );
}
