//! Real retained legal composition carries configured intent, never running-service claims.

use super::*;
use serde_json::json;
use std::path::Path;

const LEGAL: &str = "+++\n[content]\nid='content:owner-terms'\ntitle='Shared owner terms'\nlifecycle='draft'\n+++\n# Exact owner terms\n\nRetained **single-source** legal prose.\n";
const PROCESSING: &str = "processing={hosting={provider='github-pages'},analytics={enabled=false,provider='google-analytics'},subscription={enabled=false,provider='google-apps-script'},discussion={enabled=false,provider='giscus',repository='https://github.com/example/not-running'},webmcp={enabled=false}}\n";

fn fixture() -> Fixture {
    Fixture::new(&[
        ("project/terms.md", LEGAL.as_bytes()),
        ("theme/templates/page.html", b"{{ body_html | safe }}{% if legal_processing is defined %}<script id=\"processing\" type=\"application/json\">{{ legal_processing | json_script }}</script>{% endif %}"),
    ])
}

fn configured(fixture: &Fixture) -> String {
    let binding = |site: &str| {
        format!(
            "legal=[{{source='source:legal',entry='terms.md',source_content='content:owner-terms',id='content:{site}-terms',route='/terms/',family='utility'}}]\n"
        )
    };
    fixture
        .config()
        .replacen(
            "id='site:one'\n",
            "id='site:one'\nroles=[{principal='owner:one',role='data-controller'}]\n",
            1,
        )
        .replacen(
            "id='site:other'\n",
            "id='site:other'\nroles=[{principal='owner:one',role='site-operator'}]\n",
            1,
        )
        .replacen(
            "site='site:one'\npublic_base_url=",
            &format!(
                "site='site:one'\n{PROCESSING}{}public_base_url=",
                binding("one")
            ),
            1,
        )
        .replacen(
            "site='site:other'\npublic_base_url=",
            &format!(
                "site='site:other'\nprocessing={{webmcp={{enabled=false}}}}\n{}public_base_url=",
                binding("other")
            ),
            1,
        )
        + &format!(
            "[[sources]]\nid='source:legal'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='project'}}\n",
            fixture.directory.path().to_str().unwrap(),
            fixture.commit
        )
}

fn html_context(html: &[u8]) -> Value {
    let html = std::str::from_utf8(html).unwrap();
    let (_, remaining) = html
        .split_once("<script id=\"processing\" type=\"application/json\">")
        .expect("native Tera context");
    serde_json::from_str(remaining.split_once("</script>").unwrap().0).unwrap()
}

#[test]
fn retained_legal_processing_matches_theme_model_cli_and_site_scoping() {
    let fixture = fixture();
    let config = configured(&fixture);
    let inputs = acquire(&config);
    fs::write(
        fixture.directory.path().join("project/terms.md"),
        "changed live legal source",
    )
    .unwrap();
    let rendered = render(&inputs).expect("retained legal publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:one-terms"];
    let expected = json!({"hosting":{"provider":"github-pages"},"analytics":false,"subscription":false,"discussion":{"enabled":false},"webmcp":false});
    assert_eq!(page["legal_processing"], expected);
    assert_eq!(page["site"]["roles"][0]["role"], "data-controller");
    assert_eq!(page["site"]["publisher"], "owner:one");
    assert_eq!(html_context(bytes(tree, "terms/index.html")), expected);
    assert!(
        std::str::from_utf8(bytes(tree, "terms/index.html"))
            .unwrap()
            .contains("<strong>single-source</strong>")
    );
    assert!(
        !model["pages"]["content:one"]
            .as_object()
            .unwrap()
            .contains_key("legal_processing")
    );
    assert!(page.get("discussion").is_none());
    let serialized = expected.to_string();
    for absent in [
        "not-running",
        "google-analytics",
        "google-apps-script",
        "repository_id",
        "category_id",
        "giscus",
    ] {
        assert!(!serialized.contains(absent));
    }
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    let other = resolved
        .authored()
        .project_page(
            &"content:other-terms".parse().unwrap(),
            &gazette_sdk::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let other_context = serde_json::to_value(&other).unwrap();
    assert_eq!(other_context["legal_processing"], json!({"webmcp":false}));
    assert_eq!(other_context["site"]["roles"][0]["role"], "site-operator");
    assert!(other.body_html.contains("single-source"));
    // The retained SDK build survives changed live bytes. A new CLI invocation
    // must independently verify the pin, rather than inheriting that capture.
    let refused = cli(&config, &RENDER_ARGUMENTS);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("source_content_mismatch"));
    fs::write(fixture.directory.path().join("project/terms.md"), LEGAL).unwrap();
    let parent = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        fs::read(Path::new(receipt["directory"].as_str().unwrap()).join("terms/index.html"))
            .unwrap(),
        bytes(tree, "terms/index.html")
    );
}

#[test]
fn initialized_mcp_retains_identical_legal_processing_bytes_without_provider_activation() {
    let fixture = fixture();
    let config = configured(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).unwrap();
    let parent = tempdir().unwrap();
    let arguments = json!({"publication":{"recipe_json":json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":config},"sites":[{"site":"site:one","model_output":"model.json"}]}).to_string(),"local":true,"allow_read":true},"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024});
    mcp_carrier::initialized_interact("2025-11-25", |_, send| {
        let response = send(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":arguments}}),
        );
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
        let directory = Path::new(
            response["result"]["structuredContent"]["directory"]
                .as_str()
                .unwrap(),
        );
        assert_eq!(
            fs::read(directory.join("terms/index.html")).unwrap(),
            bytes(rendered.site().output(), "terms/index.html")
        );
    });
}

#[test]
fn enabled_unsupported_processing_refuses_before_missing_native_source_is_read_or_output_created() {
    let fixture = fixture();
    let config = configured(&fixture)
        .replace("subscription={enabled=false", "subscription={enabled=true")
        .replace(
            "subdirectory='project'",
            "subdirectory='unavailable-private-source'",
        );
    let parent = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("workspace_processing_unsupported"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    assert!(acquire_result(&config).is_err());
}

#[test]
fn disabled_processing_conflicts_before_selected_native_discussion_provider_effects() {
    let fixture = fixture();
    let config = configured(&fixture)
        .replacen("[[principals]]", "[composition.plugins]\n'discussion-plugin'={state='enabled'}\n[composition.capabilities]\n'discussion.render/v1'={state='enabled',provider={kind='plugin',package='discussion-plugin',provider='discussion'},requirement='required'}\n[[principals]]", 1)
        .replacen("deployable=false\n", "deployable=false\ndiscussion={capability='discussion.render/v1',repository_id='unread-repo-id',category_id='unread-category-id'}\n", 1)
        .replacen("site='site:one'\nprocessing=", "site='site:one'\nrepository='https://github.com/example/site'\nprocessing=", 1)
        + "[[plugin_packages]]\npackage='discussion-plugin'\nsource='source:legal'\nmanifest='missing-provider.toml'\n";
    let parent = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("workspace_processing_discussion_conflict"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    mcp_carrier::initialized_interact("2025-11-25", |_, send| {
        let arguments = json!({"publication":{"recipe_json":json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":config},"sites":[{"site":"site:one","model_output":"model.json"}]}).to_string(),"local":true,"allow_read":true},"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024});
        let response = send(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":arguments}}),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(
            response
                .to_string()
                .contains("workspace_processing_discussion_conflict"),
            "{response}"
        );
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
        let next = send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
        assert!(next.get("error").is_none(), "{next}");
    });
}

#[test]
fn legal_binding_alias_renders_a_redirect_to_the_composed_page() {
    let fixture = fixture();
    let config = configured(&fixture).replacen(
        "id='content:one-terms',route='/terms/'",
        "id='content:one-terms',route='/terms/',aliases=['/tnc-policy/']",
        1,
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("retained legal publication with an alias");
    let tree = rendered.site().output();
    let redirect = std::str::from_utf8(bytes(tree, "tnc-policy/index.html")).unwrap();
    assert!(
        redirect.contains("url=https:&#x2F;&#x2F;one.example&#x2F;mount&#x2F;terms&#x2F;"),
        "{redirect}"
    );
    assert!(
        std::str::from_utf8(bytes(tree, "terms/index.html"))
            .unwrap()
            .contains("<strong>single-source</strong>")
    );
}
