//! Retained article companions cross the real neutral Theme, CLI and MCP paths.

use super::*;
use serde_json::json;

const ARTICLE: &str = "+++\ntemplate='fixture/article.html'\n[content]\nid='content:article'\nsite='site:one'\ntitle='Retained article'\nlifecycle='published'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Shared\n\nContinuous main contribution.\n";
const NOTES: &str = "# Shared\n\nSupplementary **detail**. [local](#shared)\n\n[[site:one.example/guide/#shared|Main argument]] [[site:one.example/guide/#gazette-notes-shared|Commentary]]\n";
const ARTICLE_TEMPLATE: &[u8] = b"<article data-template=\"article\"><main id=\"article-main\">{{ body_html | safe }}</main>{% if notes is defined %}<aside id=\"article-notes\">{{ notes.body_html | safe }}</aside>{% endif %}</article>";

fn fixture(notes: &str) -> Fixture {
    Fixture::new(&[
        ("one/article/package/index.md", ARTICLE.as_bytes()),
        ("one/article/package/notes.md", notes.as_bytes()),
        ("theme/templates/article.html", ARTICLE_TEMPLATE),
    ])
}

fn config(fixture: &Fixture) -> String {
    fixture
        .config()
        .replace("subdirectory='one'", "subdirectory='one/article'")
        .replace(
            "public_base_url=",
            "machine={search='lexical',json_ld=true}\npublic_base_url=",
        )
}

fn publication(config: &str) -> Value {
    json!({
        "recipe_json": json!({
            "format":"gazette-local-publication-set/1",
            "render_year":2040,
            "workspace":{"kind":"supplied","source":config},
            "sites":[{"site":"site:one","model_output":"model.json"}]
        }).to_string(),
        "local":true, "allow_read":true
    })
}

#[test]
fn retained_article_notes_reach_selected_theme_cli_and_one_public_search_record() {
    let fixture = fixture(NOTES);
    let config = config(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("real retained article package");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("public model");
    let pages = model["pages"].as_object().expect("one owning page");
    assert_eq!(pages.len(), 1, "notes do not invent a content identity");
    let article = &pages["content:article"];
    let notes = &article["notes"];
    let main = article["body_html"]
        .as_str()
        .expect("continuous main fragment");
    let supplementary = notes["body_html"]
        .as_str()
        .expect("distinct notes fragment");
    assert!(main.contains("Continuous main contribution."));
    assert!(main.contains("id=\"shared\""));
    assert!(!main.contains("Supplementary"));
    assert!(!main.contains("gazette-notes-"));
    assert!(supplementary.contains("<h2 id=\"gazette-notes-shared\""));
    assert!(supplementary.contains("Supplementary <strong>detail</strong>"));
    assert!(supplementary.contains("href=\"#gazette-notes-shared\""));
    assert!(supplementary.contains("href=\"https://one.example/mount/guide/#shared\""));
    assert!(
        supplementary.contains("href=\"https://one.example/mount/guide/#gazette-notes-shared\"")
    );
    assert_eq!(notes["links"][1]["content"], "content:article");
    assert_eq!(notes["links"][2]["content"], "content:article");
    assert!(!supplementary.contains("Continuous main"));
    assert_eq!(notes["headings"][0]["level"], 2);
    assert_eq!(notes["headings"][0]["fragment"], "#gazette-notes-shared");
    assert_eq!(
        notes["source_sha256"],
        json!(source::Sha256Digest::of(NOTES.as_bytes()))
    );
    for field in ["source", "id", "route"] {
        assert!(notes.get(field).is_none(), "no companion {field}");
    }
    assert!(article.get("source").is_none());
    let html =
        std::str::from_utf8(bytes(tree, "guide/index.html")).expect("selected neutral Theme");
    assert!(html.contains("data-template=\"article\""));
    assert!(html.contains(&format!("<main id=\"article-main\">{main}</main>")));
    assert!(html.contains(&format!(
        "<aside id=\"article-notes\">{supplementary}</aside>"
    )));
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("public search");
    assert_eq!(search.len(), 1, "notes are not another search document");
    assert_eq!(search[0]["content"]["id"], "content:article");
    assert_eq!(search[0]["notes"], *notes);
    assert_eq!(search[0]["body"], article["body_text"]);
    assert!(
        !search[0]["body"]
            .as_str()
            .expect("primary search body")
            .contains("Supplementary")
    );
    for public in [
        bytes(tree, "model.json"),
        bytes(tree, "search.json"),
        html.as_bytes(),
    ] {
        let text = std::str::from_utf8(public).expect("public UTF-8");
        for private in [
            fixture.directory.path().to_str().expect("private root"),
            "one/article/package/index.md",
            "one/article/package/notes.md",
            "one/package/index.md",
            "one/package/notes.md",
            "one/index.md",
            "one/notes.md",
        ] {
            assert!(!text.contains(private), "private coordinate {private}");
        }
    }
    let evidence = serde_json::to_value(rendered.site().evidence()).expect("operator evidence");
    assert_eq!(
        evidence["pages"]["content:article"]["template"],
        "fixture/article.html"
    );
    let parent = tempdir().expect("fresh CLI stage");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("retained inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("actual stage"));
    for path in ["guide/index.html", "model.json", "search.json"] {
        assert_eq!(
            fs::read(directory.join(path)).expect("staged bytes"),
            bytes(tree, path)
        );
    }
    // Rendering consumes the acquired index, companion and Theme, not changed
    // native checkout bytes or an independently invented presentation DTO.
    fs::write(
        fixture
            .directory
            .path()
            .join("one/article/package/notes.md"),
        b"changed after capture",
    )
    .expect("disposable native change");
    fs::write(
        fixture
            .directory
            .path()
            .join("theme/templates/article.html"),
        b"changed Theme",
    )
    .expect("disposable Theme change");
    let repeated = render(&inputs).expect("retained article and notes ASTs");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
}

#[test]
fn initialized_mcp_recovers_from_missing_note_anchor_and_stages_the_real_article() {
    let good = fixture(NOTES);
    let bad = fixture("# Shared\n\n[[site:one.example/guide/#missing|Broken crossing]]\n");
    let parent = tempdir().expect("MCP output parent");
    fs::write(parent.path().join("keep"), b"unrelated").expect("unrelated output");
    let args = |config: &str| {
        json!({
            "publication":publication(config), "create_stage":true,
            "output_parent":parent.path(), "directory_bytes":1024
        })
    };
    mcp_carrier::initialized_interact("2025-11-25", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        assert!(catalog.get("error").is_none(), "{catalog}");
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("initialized catalog")
            .iter()
            .find(|tool| tool["name"] == "stage_rendered_site")
            .expect("native staging tool");
        let input_schema =
            jsonschema::validator_for(&tool["inputSchema"]).expect("native input schema");
        let output_schema =
            jsonschema::validator_for(&tool["outputSchema"]).expect("native output schema");
        let bad_args = args(&config(&bad));
        input_schema
            .validate(&bad_args)
            .expect("actual invalid-note input");
        let failure = send(
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":bad_args}}),
        );
        assert!(
            failure.get("error").is_none(),
            "application diagnostic, not carrier failure: {failure}"
        );
        assert_eq!(failure["result"]["isError"], true);
        let diagnostic = &failure["result"]["structuredContent"];
        output_schema
            .validate(diagnostic)
            .expect("typed diagnostic");
        assert_eq!(
            diagnostic["code"], "markdown_link_resolution",
            "{diagnostic}"
        );
        assert!(
            diagnostic["message"]
                .as_str()
                .expect("native anchor diagnostic")
                .contains("fragment is absent from the target document anchor index")
        );
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no partial stage")
                .count(),
            1
        );
        assert_eq!(
            fs::read(parent.path().join("keep")).expect("unrelated file"),
            b"unrelated"
        );
        let good_config = config(&good);
        let good_args = args(&good_config);
        input_schema
            .validate(&good_args)
            .expect("actual retained package input");
        let response = send(
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":good_args}}),
        );
        assert!(
            response.get("error").is_none(),
            "initialized session still healthy: {response}"
        );
        assert_eq!(response["result"]["isError"], false, "{response}");
        let receipt = &response["result"]["structuredContent"];
        output_schema
            .validate(receipt)
            .expect("native staged evidence");
        let text: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("text receipt"),
        )
        .expect("JSON receipt");
        assert_eq!(text, *receipt);
        let inputs = acquire(&good_config);
        let rendered = render(&inputs).expect("same retained producer");
        assert_eq!(
            receipt["inventory"],
            serde_json::to_value(rendered.site().output().inventory()).expect("same inventory")
        );
        let directory = std::path::Path::new(receipt["directory"].as_str().expect("MCP stage"));
        for path in ["guide/index.html", "model.json", "search.json"] {
            assert_eq!(
                fs::read(directory.join(path)).expect("MCP bytes"),
                bytes(rendered.site().output(), path)
            );
        }
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("one successful stage")
                .count(),
            2
        );
    });
}
