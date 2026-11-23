//! Derived suggestions use native meaning without changing the authored graph.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::Key;
use serde_json::Value;

fn configuration() -> workspace::Compilation {
    workspace::compile("[[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:a'\npublisher='owner:one'\ndeployable=false\n\
        [[sites]]\nid='site:b'\npublisher='owner:one'\ndeployable=false\n\
        [[sources]]\nid='source:content'\nlocation={kind='repository',root='/private/not-read',commit='1111111111111111111111111111111111111111',subdirectory='content'}\n\
        [[brands]]\nid='brand:a'\nname='A'\n[[brands.publications]]\nsite='site:a'\npublic_base_url='https://a.test/mount/'\nauthored=[{source='source:content',target='a'}]\n\
        [[brands]]\nid='brand:b'\nname='B'\n[[brands.publications]]\nsite='site:b'\npublic_base_url='https://b.test/elsewhere/'\nauthored=[{source='source:content',target='b'}]\n\
        [[projects]]\nid='project:topic'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
        [[projects]]\nid='project:hidden'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n\
        [[placements]]\nsite='site:a'\nproject='project:hidden'\nrole='primary'\nrender=true\ndiscoverable=false\ndocuments={source='source:content',target='hidden'}\n", Default::default()).unwrap()
}

fn page(id: &str, title: &str, extra: &str, metadata: &str) -> (String, String) {
    let site = if id.starts_with("foreign") { "b" } else { "a" };
    let path = if id.starts_with("hidden") {
        "hidden"
    } else {
        site
    };
    (
        format!("{path}/{id}.md"),
        format!(
            "+++\n{extra}[content]\nid='page:{id}'\nsite='site:{site}'\ntitle='{title}'\nlifecycle='published'\nattributions=[{{principal='owner:one',role='author'}}]\n{metadata}\nroute={{path='/{id}/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# {title}\n\nPublic body.\n"
        ),
    )
}

fn container(id: &str, extra: &str, structure: &str) -> (String, String) {
    (
        format!("a/{id}.md"),
        format!(
            "+++\n{extra}[content]\nid='{id}'\nsite='site:a'\ntitle='{id}'\nlifecycle='published'\nroute={{path='/{}/'}}\nstructure={{{structure}}}\n+++\n# Container\n",
            id.replace(':', "-")
        ),
    )
}

fn projections(files: &[(String, String)], source: &str) -> (Value, Value, Value) {
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let configuration = configuration();
    let resolved = authored::resolve(&configuration, &corpus, Default::default()).unwrap();
    let graph_before = serde_json::to_value(resolved.content().graph().related()).unwrap();
    let id: Key = format!("page:{source}").parse().unwrap();
    let discovery = resolved.discovery().unwrap();
    let page = resolved
        .project_discovery_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .unwrap();
    let direct = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let value = serde_json::to_value(&page.data).unwrap();
    assert_eq!(
        value["recommendations"],
        serde_json::to_value(&direct.data.recommendations).unwrap()
    );
    let listed = serde_json::to_value(page.listing(&discovery).unwrap()).unwrap();
    assert_eq!(listed["recommendations"], value["recommendations"]);
    assert_eq!(
        graph_before,
        serde_json::to_value(resolved.content().graph().related()).unwrap(),
        "derived scoring must never add or rewrite authored graph edges"
    );
    (value, graph_before, listed)
}

fn ids(value: &Value) -> Vec<&str> {
    value["recommendations"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect()
}

#[test]
fn directed_edges_rank_cross_site_known_routes_without_graph_mutation() {
    let files = vec![
        page("source", "Saturn", "related=['page:foreign-target']\n", ""),
        page("foreign-target", "Molluscs", "", ""),
        page("incoming", "Chromium", "related=['page:source']\n", ""),
        page("unrelated", "Cypress", "", ""),
    ];
    let (value, graph, _) = projections(&files, "source");
    assert_eq!(value["recommendations"]["algorithm"], "gazette-related/5");
    assert_eq!(ids(&value), ["page:foreign-target", "page:incoming"]);
    let items = &value["recommendations"]["items"];
    assert_eq!(items[0]["score"], 120);
    assert_eq!(items[1]["score"], 100);
    assert_eq!(items[0]["reasons"], serde_json::json!(["outgoing-related"]));
    assert_eq!(items[1]["reasons"], serde_json::json!(["incoming-related"]));
    assert_eq!(
        items[0]["route"]["url"],
        "https://b.test/elsewhere/foreign-target/"
    );
    assert!(
        items[0].get("structure").is_none(),
        "recommendations are shallow, not foreign semantic inventories"
    );
    assert_eq!(value["related"][0]["id"], "page:foreign-target");
    assert_eq!(graph.as_array().unwrap().len(), 2);
}

#[test]
fn graph_and_exact_keywords_admit_without_author_or_site_points() {
    let files = vec![
        page(
            "source",
            "Queue service latency",
            "related=['page:topical']\n",
            "description='Bounded scheduling evidence'\nkeywords=['GPU Scheduling']",
        ),
        page(
            "keywords",
            "A different headline",
            "",
            "keywords=['gpu scheduling']",
        ),
        page(
            "topical",
            "Queue service latency",
            "",
            "description='Bounded scheduling evidence'",
        ),
        page("other", "Cypress photosynthesis", "", ""),
        page("foreign-other", "Mollusc taxonomy", "", ""),
        page("sixth", "Chromium chemistry", "", ""),
    ];
    let (value, _, _) = projections(&files, "source");
    let selected = ids(&value);
    assert!(selected.contains(&"page:keywords"));
    assert!(selected.contains(&"page:topical"));
    assert_eq!(
        selected.len(),
        2,
        "shared author and publication cannot fill unrelated suggestions"
    );
    let topical = value["recommendations"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "page:topical")
        .unwrap();
    assert_eq!(
        topical["reasons"],
        serde_json::json!(["outgoing-related", "topical-metadata"])
    );
}

#[test]
fn stable_ties_are_identity_ordered_and_top_five_never_depend_on_input_order() {
    let mut files = vec![page("source", "Unique", "", "keywords=['Exact topic']")];
    for id in ["g", "b", "f", "a", "e", "d", "c"] {
        files.push(page(id, id, "", "keywords=['Exact topic']"));
    }
    let (before, _, _) = projections(&files, "source");
    files.reverse();
    let (after, _, _) = projections(&files, "source");
    assert_eq!(before["recommendations"], after["recommendations"]);
    assert_eq!(
        ids(&before),
        ["page:a", "page:b", "page:c", "page:d", "page:e"]
    );
}

#[test]
fn self_hidden_utility_unrouted_and_unmatched_pages_do_not_become_suggestions() {
    let mut utility = page("utility", "Aardvark", "", "keywords=['Topic']");
    utility.1 = utility
        .1
        .replace("kind='article'", "kind='utility'")
        .replace("keywords=['Topic']", "");
    let mut unrouted = page("unrouted", "Aardvark", "", "keywords=['Topic']");
    unrouted.1 = unrouted.1.replace("route={path='/unrouted/'}\n", "");
    let files = vec![
        page(
            "source",
            "Saturn",
            "related=['page:hidden-one','page:utility','page:unrouted']\n",
            "keywords=['Topic']",
        ),
        page("hidden-one", "Aardvark", "", "keywords=['Topic']"),
        utility,
        unrouted,
        page("none", "Molluscs", "", ""),
    ];
    let (value, _, _) = projections(&files, "source");
    assert!(ids(&value).is_empty());
    assert_eq!(
        value["related"].as_array().unwrap().len(),
        2,
        "authored links retain direct utility and unrouted meaning; hidden target alone is discovery-filtered"
    );
}

#[test]
fn exact_structural_and_project_meaning_supply_separate_evidence_categories() {
    let files = vec![
        page(
            "source",
            "Saturn",
            "projects=[{project='project:topic',kind='about'}]\n",
            "",
        ),
        page(
            "journey-peer",
            "Cypress",
            "projects=[{project='project:topic',kind='mentions'}]\n",
            "",
        ),
        page(
            "collection-peer",
            "Molluscs",
            "collections=['collection:one']\n",
            "",
        ),
        page(
            "project-peer",
            "Chromium",
            "projects=[{project='project:topic',kind='about'}]\n",
            "",
        ),
        container("collection:one", "", "kind='collection'"),
        container(
            "journey:one",
            "",
            "kind='journey',collection='collection:one',members=[{content='page:source',primary=true},{content='page:journey-peer',primary=true}]",
        ),
    ];
    let (value, _, _) = projections(&files, "source");
    let items = value["recommendations"]["items"].as_array().unwrap();
    let peer = items
        .iter()
        .find(|item| item["id"] == "page:journey-peer")
        .unwrap();
    assert_eq!(peer["score"], 165);
    assert_eq!(
        peer["reasons"],
        serde_json::json!(["shared-journey", "shared-collection", "shared-project"])
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item["id"] == "page:collection-peer")
            .unwrap()["score"],
        50
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item["id"] == "page:project-peer")
            .unwrap()["score"],
        60
    );
}

#[test]
fn hidden_structural_evidence_cannot_influence_visible_suggestions() {
    let mut hidden_collection = container(
        "collection:secret",
        "projects=[{project='project:topic',kind='context'}]\n",
        "kind='collection'",
    );
    hidden_collection.0 = "hidden/collection.md".into();
    let files = vec![
        page(
            "source",
            "Saturn",
            "collections=['collection:secret']\n",
            "",
        ),
        page("peer", "Cypress", "collections=['collection:secret']\n", ""),
        hidden_collection,
    ];
    let (value, _, _) = projections(&files, "source");
    assert!(
        ids(&value).is_empty(),
        "a hidden context must not leak through either collection or inherited project evidence"
    );

    let files = vec![
        page("source", "Saturn", "", ""),
        page("peer", "Cypress", "", ""),
        page("hidden-member", "Molluscs", "", ""),
        container("collection:one", "", "kind='collection'"),
        container(
            "journey:mixed",
            "",
            "kind='journey',collection='collection:one',members=[{content='page:source',primary=true},{content='page:peer',primary=true},{content='page:hidden-member',primary=true}]",
        ),
    ];
    let (value, _, _) = projections(&files, "source");
    assert!(
        ids(&value).is_empty(),
        "a partially eligible journey cannot supply ranking evidence or a shortened implicit collection context"
    );
}

#[test]
fn unicode_keyword_normalization_uses_declared_meaning_and_preserves_display_bytes() {
    let files = vec![
        page("source", "Saturn", "", "keywords=['ＧＰＵ  Scheduling']"),
        page("peer", "Cypress", "", "keywords=['gpu scheduling']"),
    ];
    let (value, _, _) = projections(&files, "source");
    assert_eq!(ids(&value), ["page:peer"]);
    assert_eq!(value["recommendations"]["items"][0]["score"], 24);
    assert_eq!(value["recommendations"]["items"][0]["title"], "Cypress");
    assert_eq!(value["content"]["keywords"][0], "ＧＰＵ  Scheduling");
}

#[test]
fn recommendations_share_existing_complete_navigation_occurrence_budget() {
    let mut files = vec![page("source", "Saturn", "", "keywords=['Exact topic']")];
    for id in ["a", "b", "c", "d", "e"] {
        files.push(page(id, id, "", "keywords=['Exact topic']"));
    }
    for (_, source) in &mut files {
        *source = source.replace("attributions=[{principal='owner:one',role='author'}]\n", "");
    }
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let configuration = configuration();
    let resolved = authored::resolve(
        &configuration,
        &corpus,
        gazette_core::publication::content::Limits {
            edges: 5,
            ..Default::default()
        },
    )
    .unwrap();
    let error = resolved
        .project_page(
            &"page:source".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            authored::ProjectionError::Limit("navigation endpoint occurrences")
        ),
        "one self breadcrumb plus five recommendation endpoints must fail a five-occurrence budget, not return a successful partial result: {error}"
    );
}

#[test]
fn native_projection_schema_exposes_shallow_recommendations_but_authored_schema_does_not() {
    let schema = serde_json::to_value(schemars::schema_for!(authored::PageProjection<'_>)).unwrap();
    assert!(schema["properties"].get("recommendations").is_some());
    let recommendations = &schema["$defs"]["Recommendations"];
    assert!(recommendations["properties"].get("algorithm").is_some());
    let item = &schema["$defs"]["Recommendation"];
    assert!(item["properties"].get("route").is_some());
    assert!(item["properties"].get("score").is_some());
    assert!(item["properties"].get("body_html").is_none());
    assert!(item["properties"].get("related").is_none());
    let authored = serde_json::to_value(authored::declaration_schema()).unwrap();
    assert!(authored["properties"].get("recommendations").is_none());
}

#[test]
fn selected_graph_rebuilds_a_previously_observed_recommendation_index() {
    use gazette_core::publication::content::Disposition;
    let files = [
        page("source", "Saturn", "", "keywords=['Exact topic']"),
        page("peer", "Cypress", "", "keywords=['Exact topic']"),
    ];
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, text)| Input {
            path: path.parse().unwrap(),
            bytes: text.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let configuration = configuration();
    let resolved = authored::resolve(&configuration, &corpus, Default::default()).unwrap();
    let source: Key = "page:source".parse().unwrap();
    let before = resolved
        .project_page(
            &source,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_eq!(
        before.data.recommendations.items.len(),
        1,
        "prime the private-inspection cache before candidate selection"
    );
    drop(before);
    let mut selection = resolved.non_draft_selection().unwrap();
    selection
        .content
        .insert("page:peer".parse().unwrap(), Disposition::Exclude);
    let selected = resolved.into_selected(&selection).unwrap();
    let after = selected
        .project_page(
            &source,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        after.data.recommendations.items.is_empty(),
        "a removed candidate cannot survive in a cache or change document-frequency scoring after the phase boundary"
    );
}

#[test]
fn contextual_description_words_cannot_manufacture_an_unanchored_topic() {
    let files = vec![
        page(
            "source",
            "Rehearsal and evidence",
            "",
            "description='Finding useful questions in generated scenarios while keeping real-world claims accountable to real evidence.'",
        ),
        page(
            "inward",
            "The inward-world inquiry",
            "",
            "description='Changing encounters and the movement of inward sound open questions about scale, attention and devotion.'",
        ),
        page(
            "incident",
            "Who carries the incident?",
            "",
            "description='An alert reaches several people; the response depends on keeping its evidence and next commitment together.'",
        ),
        page("neutral", "Chromium chemistry", "", ""),
        page("other", "Mollusc taxonomy", "", ""),
        page("botany", "Iris botany", "", ""),
        page("geology", "Basalt geology", "", ""),
        page(
            "latency",
            "Queue service latency",
            "",
            "description='Bounded scheduling evidence'",
        ),
        page(
            "service",
            "Queue service latency",
            "",
            "description='Bounded scheduling evidence'",
        ),
    ];
    let (unanchored, _, _) = projections(&files, "source");
    assert!(
        ids(&unanchored).is_empty(),
        "generic contextual descriptions cannot establish an authored relationship or exact shared keyword phrase"
    );
    let (anchored, _, _) = projections(&files, "latency");
    assert!(
        ids(&anchored).is_empty(),
        "even three matching title anchors cannot admit a metadata-only candidate"
    );
}

#[test]
fn english_function_words_do_not_turn_one_content_anchor_into_a_crossing() {
    // These public metadata pairs exposed false suggestions in the native corpus:
    // what/the + interpretation, and and + identity. Neither is a common topic.
    let files = vec![
        page(
            "source",
            "What the witness asks for",
            "",
            "language='en'\ndescription=\"The Bhāgavata continues beyond Markandeya's cosmic encounter into absorption and devotion, changing the question the spectacle seemed to answer.\"\nkeywords=['Markandeya devotion','Bhagavata Purana 12.10','maya and knowledge','Shiva in the heart','textual interpretation']",
        ),
        page(
            "foreign-line-item",
            "What does the line item mean?",
            "",
            "language='en'\ndescription='Turning a document into useful domain information requires units, context, relationships and correction, not merely accurate extraction.'\nkeywords=['document interpretation','domain modelling','quantity units','human correction','data provenance','AI document workflows']",
        ),
        page(
            "holding",
            "Holding an idea without becoming it",
            "",
            "language='en'\ndescription='Detachment makes room to revise an argument without surrendering judgement or accepting every criticism.'\nkeywords=['detachment','identity and ideas','criticism','business collaboration','commitment']",
        ),
        page(
            "foreign-ticket",
            "Unknown effects and interrupted work",
            "",
            "language='en'\ndescription=\"A ticket may exist even when its reply never arrives; recovery connects the original request to the provider's state.\"\nkeywords=['request identity','idempotent retries','external effects','read-back reconciliation']",
        ),
        page("neutral", "Chromium chemistry", "", "language='en'"),
        page("other", "Mollusc taxonomy", "", "language='en'"),
    ];
    for source in ["source", "holding"] {
        let (value, _, _) = projections(&files, source);
        assert!(
            ids(&value).is_empty(),
            "English function words cannot manufacture topical eligibility: {source}"
        );
    }
}

#[test]
fn english_regional_tags_keep_genuine_content_anchors_across_sites() {
    let files = vec![
        page(
            "source",
            "Queue service latency",
            "related=['page:foreign-service']\n",
            "language='en-GB'\ndescription='Bounded scheduling evidence'",
        ),
        page(
            "foreign-service",
            "Queue service latency",
            "",
            "language='en'\ndescription='Bounded scheduling evidence'",
        ),
        page("neutral", "Chromium chemistry", "", "language='en'"),
        page("other", "Mollusc taxonomy", "", "language='en'"),
    ];
    let (value, _, _) = projections(&files, "source");
    assert_eq!(ids(&value), ["page:foreign-service"]);
    assert_eq!(
        value["recommendations"]["items"][0]["reasons"],
        serde_json::json!(["outgoing-related", "topical-metadata"])
    );
}

#[test]
fn missing_or_nonenglish_prose_language_never_selects_english_filtering() {
    for language in ["", "language='fr'\n"] {
        let files = vec![
            page(
                "source",
                "What the witness asks",
                "related=['page:foreign-peer']\n",
                language,
            ),
            page("foreign-peer", "What the line means", "", language),
            page("neutral", "Chromium chemistry", "", language),
            page("other", "Mollusc taxonomy", "", language),
            page("botany", "Iris botany", "", language),
            page("geology", "Basalt geology", "", language),
        ];
        let (value, _, _) = projections(&files, "source");
        assert_eq!(ids(&value), ["page:foreign-peer"]);
        assert_eq!(
            value["recommendations"]["items"][0]["reasons"],
            serde_json::json!(["outgoing-related", "topical-metadata"])
        );
    }
}

#[test]
fn english_filtering_never_weakens_exact_phrases_or_editorial_edges() {
    let files = vec![
        page(
            "source",
            "Saturn",
            "related=['page:foreign-related']\n",
            "language='en'\nkeywords=['What and when']",
        ),
        page("foreign-related", "Cypress", "", "language='en'"),
        page(
            "foreign-keyword",
            "Molluscs",
            "",
            "language='en'\nkeywords=['What and when']",
        ),
    ];
    let (value, _, _) = projections(&files, "source");
    assert_eq!(
        ids(&value),
        ["page:foreign-related", "page:foreign-keyword"]
    );
    assert_eq!(value["recommendations"]["items"][0]["score"], 120);
    assert_eq!(value["recommendations"]["items"][1]["score"], 24);
    assert_eq!(
        value["recommendations"]["items"][1]["reasons"],
        serde_json::json!(["shared-keywords"])
    );
}

#[test]
fn topical_metadata_orders_graph_candidates_but_cannot_admit_its_own_match() {
    let files = vec![
        page(
            "source",
            "Queue service latency",
            "related=['page:a-general','page:z-relevant']\n",
            "language='en'\ndescription='Bounded scheduling evidence'",
        ),
        page("a-general", "Mollusc taxonomy", "", "language='en'"),
        page(
            "z-relevant",
            "Queue service latency",
            "",
            "language='en'\ndescription='Bounded scheduling evidence'",
        ),
        page(
            "foreign-lexical-only",
            "Queue service latency",
            "",
            "language='en'\ndescription='Bounded scheduling evidence'",
        ),
        page("neutral", "Chromium chemistry", "", "language='en'"),
        page("other", "Iris botany", "", "language='en'"),
    ];
    let (value, _, _) = projections(&files, "source");
    assert_eq!(ids(&value), ["page:z-relevant", "page:a-general"]);
    let items = &value["recommendations"]["items"];
    assert!(items[0]["score"].as_u64().unwrap() > 120);
    assert_eq!(
        items[0]["reasons"],
        serde_json::json!(["outgoing-related", "topical-metadata"])
    );
    assert_eq!(items[1]["score"], 120);
    assert_eq!(items[1]["reasons"], serde_json::json!(["outgoing-related"]));
}

#[test]
fn authored_body_links_restore_discovery_without_fabricating_related_edges() {
    let mut source = page("source", "Front desk", "", "");
    source
        .1
        .push_str("\nRead [the target](https://a.test/mount/target/).\n");
    let mut incoming = page("incoming", "Operating context", "", "");
    incoming
        .1
        .push_str("\nSee [the source](https://a.test/mount/source/).\n");
    let files = vec![
        source,
        page("target", "A deliberate change", "", ""),
        incoming,
        page("unrelated", "Unconnected work", "", ""),
    ];
    let (value, graph, _) = projections(&files, "source");
    assert_eq!(ids(&value), ["page:target", "page:incoming"]);
    assert_eq!(
        value["recommendations"]["items"][0]["reasons"],
        serde_json::json!(["outgoing-reference"])
    );
    assert_eq!(
        value["recommendations"]["items"][1]["reasons"],
        serde_json::json!(["incoming-reference"])
    );
    assert!(value["related"].as_array().unwrap().is_empty());
    assert!(graph.as_array().unwrap().is_empty());
}
