//! Actual retained-source generation, Theme discovery and deterministic Markdown materialization.

use super::*;
use serde_json::json;

const ARTICLE: &str = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Machine-accessible article'\ndescription='Read the admitted contribution.'\nlifecycle='draft'\nroute={path='/guide/',aliases=['/old.html']}\nstructure={kind='page',family={kind='article'}}\n+++\n# Shared\n\nMain **contribution**[^n]. [[site:one.example/#home-body|Home]]\n\n[^n]: Main footnote.\n\n[unused]: https://unused.example/private\n";
const NOTES: &str =
    "# Shared\n\nSidebar commentary[^n]. [local](#shared)\n\n[^n]: Notes footnote.\n";
const TEMPLATE: &[u8] = b"<article>{{ body_html | safe }}</article>{% if presentation.markdown_alternate is defined %}<a rel=\"alternate\" type=\"{{ presentation.markdown_alternate.media_type }}\" href=\"{{ presentation.markdown_alternate.url }}\">Markdown</a>{% endif %}";

fn configured(fixture: &Fixture) -> String {
    fixture.config().replace(
        "site='site:one'\npublic_base_url=",
        &format!(
            "site='site:one'\nmachine={{{}}}\npublic_base_url=",
            curated_llms("content:one", &[("Start here", &["content:one"])])
        ),
    )
}

#[test]
fn actual_native_bundle_materializes_resolved_markdown_notes_and_curated_pairs() {
    let uncurated = PAGE
        .replace("id='content:one'", "id='content:uncurated'")
        .replace("path='/guide/'", "path='/uncurated/'")
        .replace("aliases=['/old.html']", "aliases=['/uncurated-old.html']");
    let fixture = Fixture::new(&[
        ("one/page.md", uncurated.as_bytes()),
        ("one/article/index.md", ARTICLE.as_bytes()),
        ("one/article/notes.md", NOTES.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
    ]);
    let config = configured(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("actual native Markdown alternates");
    let tree = rendered.site().output();
    let alternate = std::str::from_utf8(bytes(tree, "guide/index.md")).expect("native Markdown");
    for retained in [
        "Main",
        "contribution",
        "Main footnote",
        "Sidebar commentary",
        "Notes footnote",
        "gazette-notes-shared",
        "Article notes",
        "https://one.example/mount/#home-body",
    ] {
        assert!(
            alternate.contains(retained),
            "missing admitted {retained}: {alternate}"
        );
    }
    for private in [
        "+++",
        "[content]",
        "unused.example",
        "one/article",
        "source:one",
        "Identity:",
        "<section",
        "<h1",
    ] {
        assert!(
            !alternate.contains(private),
            "leaked {private}: {alternate}"
        );
    }
    let html = std::str::from_utf8(bytes(tree, "guide/index.html"))
        .expect("HTML")
        .replace("&#x2F;", "/");
    assert!(html.contains("https://one.example/mount/guide/index.md"));
    assert!(html.contains("text/markdown"));
    let alias = std::str::from_utf8(bytes(tree, "old.html")).expect("alias");
    assert!(!alias.contains("rel=\"alternate\""));
    let llms = std::str::from_utf8(bytes(tree, "llms.txt")).expect("curated entrypoints");
    assert!(llms.contains("guide/index.md"));
    assert!(llms.contains("[HTML](https://one.example/mount/guide/)"));
    assert!(
        !llms.contains("[Home]"),
        "uncurated home is not another sitemap entry"
    );
    let repeated = render(&inputs).expect("repeat exact retained input");
    assert_eq!(
        tree.inventory().identity(),
        repeated.site().output().inventory().identity()
    );
    let parent = tempdir().expect("CLI output parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage root"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI staging evidence");
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    for output in ["guide/index.md", "index.md", "llms.txt"] {
        assert_eq!(
            fs::read(directory.join(output)).expect("actual staged bytes"),
            bytes(tree, output)
        );
    }
}

#[test]
fn initialized_mcp_refuses_invalid_curation_then_materializes_the_valid_plan() {
    let fixture = Fixture::new(&[]);
    let good = configured(&fixture);
    let bad = good.replace(
        "description_from=\"content:one\"",
        "description_from=\"content:absent\"",
    );
    assert_ne!(
        bad, good,
        "exact invalid reference rather than a no-op fixture"
    );
    let parent = tempdir().expect("unchanged staging parent");
    let arguments = |config: &str| {
        json!({
            "publication": {
                "recipe_json": json!({"format":"gazette-local-publication-set/1",
                    "workspace":{"kind":"supplied","source":config},
                    "sites":[{"site":"site:one","model_output":"model.json"}]}).to_string(),
                "local":true,"allow_read":true},
            "create_stage":true,"output_parent":parent.path(),"directory_bytes":1024
        })
    };
    mcp_carrier::initialized_interact("2025-11-25", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("native discovery")
            .iter()
            .find(|tool| tool["name"] == "stage_rendered_site")
            .expect("staging tool");
        let input = jsonschema::validator_for(&tool["inputSchema"]).expect("input schema");
        let output = jsonschema::validator_for(&tool["outputSchema"]).expect("output schema");
        for (id, config, failed) in [(2, &bad, true), (3, &good, false)] {
            let arguments = arguments(config);
            input.validate(&arguments).expect("native input contract");
            let response = send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"stage_rendered_site","arguments":arguments}}));
            assert!(response.get("error").is_none(), "{response}");
            assert_eq!(response["result"]["isError"], failed, "{response}");
            let receipt = &response["result"]["structuredContent"];
            output.validate(receipt).expect("native output contract");
            if failed {
                assert_eq!(
                    fs::read_dir(parent.path())
                        .expect("no partial stage")
                        .count(),
                    0
                );
            } else {
                // Keep the owning capture alive while comparing exact emitted bytes.
                let inputs = acquire(config);
                let rendered = render(&inputs).expect("same native producer");
                let directory =
                    std::path::Path::new(receipt["directory"].as_str().expect("MCP stage"));
                for path in ["guide/index.md", "index.md", "llms.txt"] {
                    assert_eq!(
                        fs::read(directory.join(path)).expect("MCP materialization"),
                        bytes(rendered.site().output(), path)
                    );
                }
            }
        }
    });
}

#[test]
fn theme_machine_entry_receives_complete_built_pages_and_authored_purposes() {
    let manifest = MANIFEST.replace("graph='graph.html'", "graph='graph.html',llms='llms.txt'");
    let fixture = Fixture::new(&[
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/templates/llms.txt", b"# {{ llms.title | markdown_text }}\n> {{ llms.description | markdown_text }}\n{% for page in llms.pages %}- [{{ page.title | markdown_text }}]({{ page.url | markdown_destination }}): {{ page.description | markdown_text }}\n{% endfor %}"),
    ]);
    let machine = curated_llms("content:one", &[]).replace(
        "llms={enabled=true",
        "llms={coverage='all-pages',enabled=true",
    );
    let config = fixture.config().replace(
        "site='site:one'\npublic_base_url=",
        &format!("site='site:one'\nmachine={{{machine}}}\npublic_base_url="),
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).unwrap();
    let text = std::str::from_utf8(bytes(rendered.site().output(), "llms.txt")).unwrap();
    assert_eq!(
        text.lines().filter(|line| line.starts_with("- [")).count(),
        2
    );
    assert!(text.contains("- [Home](https://one.example/mount/index.md): The explicit fixture collection and its retained articles."), "{text}");
    assert!(text.contains("One \\<escaped\\>"));
    assert!(text.contains("https://one.example/mount/guide/index.md"));
    assert!(text.contains("The retained local article."), "{text}");
    for excluded in ["old.html", "elsewhere", "source:one", "Retained body"] {
        assert!(!text.contains(excluded), "unexpected {excluded}: {text}");
    }
}
