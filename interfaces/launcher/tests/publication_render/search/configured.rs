//! Actual configured fields cross retained capture, native projection and the browser provider.

use super::*;
use std::process::Command;

const ARTICLE: &str = "+++\n[content]\nid='content:article'\nsite='site:one'\ntitle='unselectedtitle'\ndescription='descriptiontoken'\nkeywords=['keywordtoken']\nlifecycle='published'\nroute={path='/article/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# unselectedtitle\n\nunselectedbody.\n";
const NOTES: &str = "# Commentary\n\nDistinct **commentarytoken**.\n";
const TEMPLATE: &[u8] = b"<main>{{ body_html | safe }}</main><aside>{{ notes.body_html | safe }}</aside><span>{{ presentation.search.profile }}</span>{% for field in presentation.search.fields %}<i>{{ field }}</i>{% endfor %}";

fn fixture() -> Fixture {
    Fixture::new(&[
        ("one/article/package/index.md", ARTICLE.as_bytes()),
        ("one/article/package/notes.md", NOTES.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
        ("theme/templates/graph.html", b"{{ __tera_context | safe }}"),
    ])
}

fn config(fixture: &Fixture, fields: &str, graph: bool) -> String {
    fixture.config().replace("subdirectory='one'", "subdirectory='one/article'")
        .replacen("public_base_url=", &format!("machine={{search={{language='english',fields={fields}}},graph={{enabled={graph},path='/graph/'}}}}\npublic_base_url="), 1)
}

#[test]
fn configured_search_fields_control_actual_provider_hits_while_graph_reuses_the_same_model() {
    let fixture = fixture();
    let config = config(&fixture, "['description','keywords','route','notes']", true);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("configured native machine outputs");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let documents: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    assert_eq!(
        documents.len(),
        1,
        "one article, not a separate commentary catalogue"
    );
    let page = &model["pages"]["content:article"];
    let record = &documents[0];
    assert_eq!(record["content"], page["content"]);
    assert_eq!(record["route"], page["route"]);
    assert_eq!(record["body"], page["body_text"]);
    assert_eq!(record["notes"], page["notes"]);
    assert_eq!(record["indexed"]["notes"], page["notes"]["body_text"]);
    assert_eq!(record["indexed"]["description"], "descriptiontoken");
    assert_eq!(
        record["indexed"]["keywords"],
        serde_json::json!(["keywordtoken"])
    );
    assert!(record["indexed"].get("title").is_none());
    assert!(record["indexed"].get("body").is_none());
    assert_eq!(page["presentation"]["search"]["profile"], "english");
    assert_eq!(
        page["presentation"]["search"]["fields"],
        serde_json::json!(["description", "keywords", "route", "notes"])
    );
    let html = std::str::from_utf8(bytes(tree, "article/index.html")).unwrap();
    assert!(
        html.contains(
            "<span>english</span><i>description</i><i>keywords</i><i>route</i><i>notes</i>"
        )
    );
    let graph: Value = serde_json::from_slice(bytes(tree, "graph/index.html"))
        .expect("actual retained Theme graph context");
    assert_eq!(
        graph, model["graph"],
        "one native discovery graph enters Theme and machine model"
    );
    assert_eq!(graph["nodes"].as_object().unwrap().len(), 1);
    assert_eq!(
        rendered
            .site()
            .evidence()
            .graph
            .as_ref()
            .unwrap()
            .output
            .as_str(),
        "graph/index.html"
    );
    for value in [&model, record, &graph] {
        assert!(
            !value
                .to_string()
                .contains(fixture.directory.path().to_str().unwrap())
        );
    }
    let directory = tempdir().unwrap();
    let corpus = directory.path().join("search.json");
    fs::write(&corpus, bytes(tree, "search.json")).unwrap();
    // The real browser subpath owns tokenization/ranking; no fixture evaluator replaces it.
    let script = r#"import {readFileSync} from 'node:fs';
import {createIndex} from './sdk/js/search/index.mjs';
const records=JSON.parse(readFileSync(process.argv[1],'utf8'));
const index=createIndex(records,{profile:'english',maxDocuments:20,maxBytes:1048576,maxQueryBytes:1024,maxResults:20});
const hits={};
for(const [field,query] of [['description','descriptiontoken'],['keywords','keywordtoken'],['notes','commentarytoken'],['route',records[0].route.url]]) hits[field]=index.search(query,{limit:1,fields:{[field]:1}}).map(hit=>hit.record.content.id);
for(const query of ['unselectedtitle','unselectedbody']) hits[query]=index.search(query,{limit:1}).length;
console.log(JSON.stringify({size:index.size,hits}));"#;
    let output = Command::new(crate::native::node())
        .args(["--input-type=module", "-e", script])
        .arg(&corpus)
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .expect("actual selected Node provider");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(proof["size"], 1);
    for field in ["description", "keywords", "route", "notes"] {
        assert_eq!(proof["hits"][field], serde_json::json!(["content:article"]));
    }
    assert_eq!(proof["hits"]["unselectedtitle"], 0);
    assert_eq!(proof["hits"]["unselectedbody"], 0);
}

#[test]
fn configured_disabled_graph_never_creates_an_output_and_scope_changes_do_not_delete_notes() {
    let fixture = fixture();
    let inputs = acquire(&config(&fixture, "['title']", false));
    let rendered = render(&inputs).expect("title-only native selection");
    let tree = rendered.site().output();
    assert!(tree.file(&"graph/index.html".parse().unwrap()).is_none());
    assert!(rendered.site().evidence().graph.is_none());
    let records: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    assert_eq!(
        records[0]["indexed"],
        serde_json::json!({"title":"unselectedtitle"})
    );
    assert!(
        records[0]["notes"]["body_html"]
            .as_str()
            .unwrap()
            .contains("commentarytoken")
    );
    assert!(!bytes(tree, "article/index.html").is_empty());
}

#[test]
fn graph_request_cannot_override_configured_destination_or_disabled_intent_and_never_stages_a_subset()
 {
    let fixture = fixture();
    for (config, output) in [
        (config(&fixture, "['title']", true), "other-graph.html"),
        (config(&fixture, "['title']", false), "graph/index.html"),
    ] {
        let parent = tempdir().unwrap();
        fs::write(parent.path().join("canary"), b"keep").unwrap();
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend([
            "--graph-output",
            output,
            "--output-parent",
            parent.path().to_str().unwrap(),
        ]);
        let result = cli(&config, &arguments);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("site_graph_selection"), "{diagnostic}");
        assert_eq!(fs::read(parent.path().join("canary")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
    }
}

#[test]
fn exact_keyword_discovery_uses_emitted_metadata_without_widening_ranking_fields() {
    let article = ARTICLE.replace(
        "keywords=['keywordtoken']",
        "keywords=['keywordtoken','AI / modèles + mémoire']",
    );
    let fixture = Fixture::new(&[
        ("one/article/package/index.md", article.as_bytes()),
        ("one/article/package/notes.md", NOTES.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
    ]);
    let inputs = acquire(&config(&fixture, "['title']", false));
    let rendered = render(&inputs).expect("title-only native search selection");
    let tree = rendered.site().output();
    let directory = tempdir().unwrap();
    let corpus = directory.path().join("search.json");
    fs::write(&corpus, bytes(tree, "search.json")).unwrap();
    let script = r#"import {readFileSync} from 'node:fs';
import {createIndex} from './sdk/js/search/index.mjs';
const records=JSON.parse(readFileSync(process.argv[1],'utf8'));
const index=createIndex(records,{profile:'english',maxDocuments:20,maxBytes:1048576,maxQueryBytes:1024,maxResults:20});
const keyword='AI / modèles + mémoire';
const url=new URL('https://example.test/search/');url.searchParams.set('keyword',keyword);
const selected=index.browse('',{limit:20,filters:{keyword:[url.searchParams.get('keyword')]}});
console.log(JSON.stringify({
  selected:selected.hits.map(hit=>hit.record),total:selected.total,facets:selected.facets.keyword,
  folded:index.browse('',{limit:20,filters:{keyword:[keyword.toLowerCase()]}}).total,
  ranked:index.search('keywordtoken',{limit:20}).length,
  narrowed:index.browse('absent',{limit:20,filters:{keyword:[keyword]}}).total
}));"#;
    let output = Command::new(crate::native::node())
        .args(["--input-type=module", "-e", script])
        .arg(&corpus)
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .expect("actual SDK over native emitted corpus");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: Value = serde_json::from_slice(&output.stdout).unwrap();
    let records: Value = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    assert_eq!(
        proof["selected"], records,
        "original complete native records"
    );
    assert_eq!(proof["total"], 1);
    assert_eq!(
        proof["folded"], 0,
        "no case-based keyword identity inference"
    );
    assert_eq!(proof["ranked"], 0, "unselected keywords remain unranked");
    assert_eq!(proof["narrowed"], 0, "free text is a separate restriction");
    assert_eq!(
        proof["facets"],
        serde_json::json!([
            {"value":"AI / modèles + mémoire","count":1,"label":"AI / modèles + mémoire"},
            {"value":"keywordtoken","count":1,"label":"keywordtoken"}
        ])
    );
}
