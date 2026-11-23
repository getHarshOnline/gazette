//! Directory associations validate addresses after the selected route rules run.
use super::{declaration, key, page};
use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::publication::content::{BrowsingRelation, Error, Graph};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn authored_browsing_uses_the_completed_index_route_without_creating_a_parent() {
    let workspace=super::WORKSPACE.replace("home='content:home'\n","")
        .replace("[[brands]]", "[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/never-opened'}\n[[brands]]")
        + "authored=[{source='source:one',target='one'}]\n";
    let config = workspace::compile(&workspace, Default::default()).unwrap();
    let index = "+++\n[content]\nid='content:blog'\ntitle='Blog'\nlifecycle='draft'\nslug='blog'\nstructure={kind='page',family={kind='index'}}\n+++\n# Blog\n";
    let mentions = "+++\nbrowsing=['content:blog']\n[content]\nid='content:mentions'\ntitle='Mentions'\nlifecycle='draft'\nslug='mentions'\nstructure={kind='page',family={kind='index'}}\n+++\n# Mentions\n";
    let corpus = ParsedCorpus::parse(
        [("one/blog.md", index), ("one/mentions.md", mentions)]
            .into_iter()
            .map(|(path, body)| Input {
                path: path.parse().unwrap(),
                bytes: body.as_bytes(),
            }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    resolved.validate_source_bindings().unwrap();
    let id = key("content:mentions");
    let locations =
        serde_json::to_value(resolved.project_browsing_locations(&id, None).unwrap()).unwrap();
    assert_eq!(locations[0]["url"], "https://one.test/mount/blog/");
    assert_eq!(locations[0]["via"], "content:mentions");
    assert!(resolved.content().graph().editorial_parent(&id).is_none());
    assert_eq!(
        resolved.content().route(&id).unwrap().url,
        "https://one.test/mount/mentions/"
    );
}

#[test]
fn completion_still_rejects_unrouted_and_canonical_browsing_targets() {
    for canonical in [false, true] {
        let mut index = page("content:index", "index");
        index["structure"]["family"]["kind"] = json!("index");
        let mut original = page("content:original", "original");
        original["route"] = json!({"path":"/original/"});
        if canonical {
            index["route"] = json!({"path":"/index/","canonical":"content:original"});
        }
        let mut input = declaration(
            vec![page("content:mentions", "mentions"), index, original],
            json!([]),
        );
        input.browsing.push(BrowsingRelation {
            source: key("content:mentions"),
            index: key("content:index"),
        });
        let result = Graph::resolve_with_routes(input, Default::default(), |_| Ok(BTreeMap::new()));
        assert!(
            matches!(result,Err(Error::Invalid(ref rows)) if rows.iter().any(|row|row.code=="content_browsing_route"))
        );
    }
}

#[test]
fn invalid_browsing_structure_refuses_before_route_completion() {
    let mut input = declaration(
        vec![
            page("content:mentions", "mentions"),
            page("content:not-index", "wrong-kind"),
        ],
        json!([]),
    );
    input.browsing.push(BrowsingRelation {
        source: key("content:mentions"),
        index: key("content:not-index"),
    });
    let result = Graph::resolve_with_routes(input, Default::default(), |_| {
        panic!("invalid browsing must not reach route completion")
    });
    assert!(
        matches!(result,Err(Error::Invalid(ref rows)) if rows.iter().any(|row|row.code=="content_browsing_index"))
    );
}
