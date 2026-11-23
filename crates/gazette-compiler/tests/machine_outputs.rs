//! Strict machine declarations consume the actual retained page and subordinate commentary.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use serde_json::{Value, json};

fn config(search: &str) -> String {
    format!(
        "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={{kind='repository',root='/private/machine-input',commit='1111111111111111111111111111111111111111',subdirectory='content'}}\n[[brands]]\nid='brand:one'\nname='Machine fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/mount/'\nauthored=[{{source='source:one',target='one'}}]\nmachine={{search={search},graph={{enabled=true,path='/graph/'}}}}\n"
    )
}

#[test]
fn exact_consumer_search_and_graph_shapes_are_strict_and_selected_fields_are_nonempty_distinct() {
    let search =
        "{language='english',fields=['title','description','keywords','route','body','notes']}";
    let compiled = workspace::compile(&config(search), Default::default())
        .expect("explicit consumer configuration");
    let publication = compiled
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap();
    let selection = publication.machine.search.as_ref().unwrap();
    assert_eq!(selection.profile(), workspace::SearchProfile::English);
    assert_eq!(
        serde_json::to_value(selection.fields()).unwrap(),
        json!(["title", "description", "keywords", "route", "body", "notes"])
    );
    let graph = publication.machine.graph.as_ref().unwrap();
    assert!(graph.enabled);
    assert_eq!(graph.path.output().as_str(), "graph/index.html");
    for search in [
        "{language='english',fields=[]}",
        "{language='english',fields=['notes','notes']}",
    ] {
        let workspace::Error::Invalid(errors) =
            workspace::compile(&config(search), Default::default())
                .expect_err("exact selected fields")
        else {
            panic!("semantic selection diagnostics");
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_search_fields")
        );
    }
    for search in [
        "{language='automatic',fields=['title']}",
        "{language='english',fields=['private-path']}",
        "{language='english',fields=['title'],guess=true}",
        "['english',['title']]",
    ] {
        assert!(
            workspace::compile(&config(search), Default::default()).is_err(),
            "unsupported policy {search}"
        );
    }
    for graph in [
        "{enabled=true,path='../graph/'}",
        "{enabled=true,path='/graph/',template='guess.html'}",
        "[true,'/graph/']",
    ] {
        assert!(
            workspace::compile(
                &config("'lexical'").replace("{enabled=true,path='/graph/'}", graph),
                Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn native_machine_schema_admits_only_the_same_named_nonempty_distinct_field_contract() {
    let schema = schemars::schema_for!(workspace::SearchConfiguration);
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for valid in [
        json!("lexical"),
        json!({"language":"english","fields":["title","description","keywords","route","body","notes"]}),
    ] {
        assert!(validator.is_valid(&valid));
    }
    for invalid in [
        json!({"language":"english","fields":[]}),
        json!({"language":"english","fields":["title","title"]}),
        json!({"language":"english","fields":["private-path"]}),
        json!(true),
        json!(["english", ["title"]]),
    ] {
        assert!(
            !validator.is_valid(&invalid),
            "strict native schema: {invalid}"
        );
    }
    let schema = schemars::schema_for!(workspace::GraphConfiguration);
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(validator.is_valid(&json!({"enabled":true,"path":"/graph/"})));
    assert!(
        !validator.is_valid(&json!({"enabled":true,"path":"/graph/","template":"inferred.html"}))
    );
}

#[test]
fn selected_native_ranking_values_borrow_one_article_and_preserve_complete_safe_metadata() {
    let compilation = workspace::compile(
        &config("{language='english',fields=['description','keywords','route','notes']}"),
        Default::default(),
    )
    .unwrap();
    let article = "+++\n[content]\nid='content:article'\nsite='site:one'\ntitle='Unselected title'\ndescription='Description token'\nkeywords=['keyword token']\nlifecycle='published'\nroute={path='/article/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Unselected title\n\nMain body token.\n";
    let notes = "# Commentary\n\nDistinct **notes token**.\n";
    let files = [
        ("one/article/index.md", article),
        ("one/article/notes.md", notes),
    ];
    let corpus = ParsedCorpus::parse(
        files.iter().map(|(path, source)| Input {
            path: path.parse().unwrap(),
            bytes: source.as_bytes(),
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let page = resolved
        .project_page(
            &"content:article".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let selection = compilation
        .workspace
        .publication_for(&"site:one".parse().unwrap())
        .unwrap()
        .machine
        .search
        .as_ref()
        .unwrap();
    let value = serde_json::to_value(page.search_with(selection)).unwrap();
    assert_eq!(value["indexed"]["description"], "Description token");
    assert_eq!(value["indexed"]["keywords"], json!(["keyword token"]));
    assert_eq!(
        value["indexed"]["route"],
        "https://example.test/mount/article/"
    );
    assert_eq!(
        value["indexed"]["notes"],
        page.data.notes.as_ref().unwrap().body_text
    );
    assert!(value["indexed"].get("title").is_none());
    assert!(value["indexed"].get("body").is_none());
    assert_eq!(
        value["content"],
        serde_json::to_value(&page.data.content).unwrap()
    );
    assert_eq!(value["body"], page.body_text);
    assert_eq!(
        value["notes"]["body_html"],
        page.data.notes.as_ref().unwrap().body_html
    );
    assert_eq!(resolved.content().graph().nodes().len(), 1);
    assert!(
        !serde_json::to_string(&value)
            .unwrap()
            .contains("/private/machine-input")
    );
    let discovery = resolved.discovery().unwrap();
    let safe = serde_json::to_value(
        page.listing(&discovery)
            .unwrap()
            .search_with(selection)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(safe["indexed"], value["indexed"]);
    let mut metadata: Value =
        serde_json::to_value(page.listing(&discovery).unwrap().search().unwrap()).unwrap();
    metadata["indexed"] = value["indexed"].clone();
    assert_eq!(
        safe, metadata,
        "exact existing discovery serializer owns result metadata"
    );
}
