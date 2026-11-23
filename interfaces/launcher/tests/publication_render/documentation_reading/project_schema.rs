//! A project's exact provider reading page enriches the shared model without changing navigation.

use super::*;
use serde_json::json;

const DOCUMENT: &str = "https://docs.example/v2/library/guide";
const SCHEMA_TEMPLATE: &[u8] = b"<script type=\"application/ld+json\">{{ json_ld | json_script }}</script><main>{{ body_html | safe }}</main>";

fn fixture(project: &str, destination: &str) -> (Fixture, String) {
    let page = PAGE
        .replacen(
            "+++\n",
            &format!("+++\nprojects=[{{project={project:?},kind='about'}}]\n"),
            1,
        )
        .replace("lifecycle='draft'", "lifecycle='published'");
    let (fixture, mut config) = fixture_config_with(&[
        ("one/page.md", page.as_bytes()),
        ("theme/templates/page.html", SCHEMA_TEMPLATE),
    ]);
    config = config.replace(
        "machine={search='lexical',",
        "machine={json_ld=true,search='lexical',",
    );
    let resources = format!("resources={{code_documentation={destination:?}}}\n");
    if project == "project:provider" {
        config = config.replace(
            "id='project:provider'\n",
            &format!("id='project:provider'\n{resources}"),
        );
    } else {
        config.push_str(&format!("\n[[projects]]\nid={project:?}\n{resources}lifecycle='active'\nvisibility='public'\navailability='available'\n"));
    }
    (fixture, config)
}

fn with_catalog(config: &str) -> build::AcquiredInputs {
    let documentation = build::acquire_documentation(
        config,
        &"site:one".parse().unwrap(),
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .unwrap();
    let assembly = build::assemble_documentation(&documentation, Default::default()).unwrap();
    let mut inputs = acquire(config);
    inputs
        .install_documentation_catalog(assembly.catalog_capture().unwrap())
        .unwrap();
    inputs
}

fn model(tree: &source::CapturedSource) -> Value {
    serde_json::from_slice(bytes(tree, "model.json")).unwrap()
}

fn expected() -> Value {
    json!({"@id":DOCUMENT,"@type":"WebPage","url":DOCUMENT,"name":"Library guide"})
}

#[test]
fn known_provider_subject_is_shared_by_html_model_site_graph_cli_and_initialized_mcp() {
    let (_fixture, config) = fixture("project:provider", DOCUMENT);
    let inputs = with_catalog(&config);
    let rendered = render(&inputs).unwrap();
    let tree = rendered.site().output();
    let model = model(tree);
    let page = &model["pages"]["content:one"];
    assert_eq!(
        page["json_ld"]["about"][0]["subjectOf"],
        json!([expected()])
    );
    assert_eq!(
        page["projects"][0]["code_documentation"],
        json!({"public_url":DOCUMENT,"title":"Library guide"})
    );
    let schema = gazette_sdk::schema::contract(gazette_sdk::schema::Contract::TemplateContext);
    let project_schema = json!({
        "$ref":"#/$defs/ProjectView", "$defs":schema.as_value()["$defs"]
    });
    let validator = jsonschema::validator_for(&project_schema).unwrap();
    validator.validate(&page["projects"][0]).unwrap();
    let mut malformed = page["projects"][0].clone();
    malformed["code_documentation"]["title"] = json!(42);
    assert!(validator.validate(&malformed).is_err());
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).unwrap();
    let script = html
        .split_once("<script type=\"application/ld+json\">")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    assert_eq!(
        serde_json::from_str::<Value>(script).unwrap(),
        page["json_ld"]
    );
    let graph: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).unwrap();
    let work = graph["@graph"]
        .as_array()
        .unwrap()
        .iter()
        .find(|work| work["identifier"] == "content:one")
        .unwrap();
    assert_eq!(work["about"][0]["subjectOf"], json!([expected()]));

    let parent = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).unwrap()
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    assert_eq!(
        fs::read(directory.join("guide/index.html")).unwrap(),
        bytes(tree, "guide/index.html")
    );

    let recipe = json!({
        "format":"gazette-local-publication-set/1",
        "workspace":{"kind":"supplied","source":config},
        "sites":[{"site":"site:one","model_output":"model.json"}],
        "render_year":2040
    });
    crate::mcp_carrier::initialized_interact_with_call_timeout(
        "2026-07-28",
        std::time::Duration::from_secs(30),
        |_, send| {
            let response = send(
                json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                    "name":"render_sites","arguments":{"recipe_json":recipe.to_string(),"local":true,"allow_read":true}
                }}),
            );
            assert!(response.get("error").is_none(), "{response}");
            assert_eq!(response["result"]["isError"], false, "{response}");
            assert_eq!(
                response["result"]["structuredContent"]["sites"]["site:one"]["inventory"],
                serde_json::to_value(tree.inventory()).unwrap()
            );
        },
    );
}

#[test]
fn unknown_external_cross_project_and_absent_catalog_keep_untyped_references() {
    for (project, destination, captured) in [
        (
            "project:other",
            "https://docs.example/v2/library/missing",
            true,
        ),
        ("project:other", "https://external.example/manual", true),
        ("project:other", DOCUMENT, true),
        ("project:provider", DOCUMENT, false),
    ] {
        let (_fixture, config) = fixture(project, destination);
        let inputs = if captured {
            with_catalog(&config)
        } else {
            acquire(&config)
        };
        let rendered = render(&inputs).unwrap();
        let model = model(rendered.site().output());
        let page = &model["pages"]["content:one"];
        assert_eq!(
            page["json_ld"]["about"][0]["subjectOf"],
            json!([{"@id":destination}])
        );
        assert!(page["projects"][0].get("code_documentation").is_none());
    }
    let (_fixture, config) = fixture("project:provider", "https://external.example/manual");
    let error = render(&with_catalog(&config)).unwrap_err();
    assert!(format!("{error:?}").contains("project_documentation_conflict"));
}

#[test]
fn provider_subject_identity_stays_public_while_its_resource_action_uses_preview() {
    let (_fixture, config) = fixture("project:provider", DOCUMENT);
    let config = config.replacen(
        "public_base_url='https://one.example/mount/'",
        "local_port=41235\npublic_base_url='https://one.example/mount/'",
        1,
    );
    let recipe = json!({
        "format":"gazette-local-publication-set/1",
        "workspace":{"kind":"supplied","source":config},
        "sites":[{"site":"site:one","model_output":"model.json"}],
        "render_year":2040,"addressing":"local",
        "preview_link_bases":{"https://docs.example/v2/":"http://127.0.0.1:41234/v2/"}
    });
    build::LocalPublicationRecipe::parse_json(&serde_json::to_vec(&recipe).unwrap(), 1 << 20, 1)
        .unwrap()
        .with_rendered(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            |set| {
                assert!(set.is_complete());
                let rendered = set
                    .sites()
                    .get(&"site:one".parse().unwrap())
                    .unwrap()
                    .as_ref()
                    .unwrap();
                let model = model(rendered.site().output());
                let page = &model["pages"]["content:one"];
                assert_eq!(
                    page["json_ld"]["about"][0]["subjectOf"],
                    json!([expected()])
                );
                let action = page["projects"][0]["resources"]["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|action| action["kind"] == "code-documentation")
                    .unwrap();
                assert_eq!(action["url"], "http://127.0.0.1:41234/v2/library/guide");
            },
        )
        .unwrap();
}
