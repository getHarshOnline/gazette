//! Exact provider-runtime absence is semantic status, never a missing resource shortcut.

use super::*;
use serde_json::json;

const TEMPLATE: &str = "{{ body_html | safe }}{% for name, capability in presentation.capabilities %}<p data-capability=\"{{ name }}\">{{ capability.state }}:{{ capability.reason }}</p>{% endfor %}{% for resource in presentation.theme_resources %}<script type=\"module\" src=\"{{ resource.url }}\"></script>{% endfor %}";

fn configuration(fixture: &Fixture) -> String {
    fixture
        .configuration("optional")
        .replace(RESOURCE_SELECTION, "")
}

#[test]
fn unsupported_plugin_reaches_theme_model_receipt_and_replay_without_a_resource() {
    let fixture = Fixture::new(&[
        (
            "theme/theme.toml",
            THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
        ),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
    ]);
    let config = configuration(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs, "model.json").expect("explicit provider-runtime absence");
    let tree = rendered.site().output();
    let unavailable = json!({"state":"unavailable","reason":"unsupported"});
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    assert_eq!(model["capabilities"]["publication.search/v1"], unavailable);
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["capabilities"]["publication.search/v1"],
        unavailable
    );
    assert_eq!(model["resources"], json!({}));
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["theme_resources"],
        json!([])
    );
    let html = std::str::from_utf8(bytes(tree, "index.html")).expect("Theme output");
    assert!(html.contains("unavailable:unsupported"));
    assert!(!html.contains("<script"));
    assert!(
        tree.file(&"assets/search.js".parse().expect("unselected path"))
            .is_none()
    );

    let evidence = json!(rendered.site().evidence());
    let capability = &evidence["capabilities"]["publication.search/v1"];
    assert_eq!(capability["state"], "unavailable");
    assert_eq!(capability["reason"], "unsupported");
    assert_eq!(capability["provider"]["kind"], "plugin");
    assert_eq!(
        capability["provider"]["package"],
        "publication-capabilities"
    );
    assert_eq!(capability["provider"]["provider"], "search");
    assert_eq!(capability["requirement"], "optional");
    assert_eq!(capability["selection"]["scope"], "workspace");
    assert_eq!(evidence["resources"], json!({}));

    let parent = tempdir().expect("fresh stage parent");
    let mut arguments = ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(output.status.success(), "{output:?}");
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("actual CLI receipt");
    assert_eq!(
        receipt["evidence"]["capabilities"],
        evidence["capabilities"]
    );
    assert_eq!(receipt["evidence"]["resources"], json!({}));
    let retained = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    assert_eq!(
        source::inspect(retained, None, Default::default())
            .expect("independent native inventory")
            .identity(),
        tree.inventory().identity()
    );
    fs::write(
        fixture.root.path().join("theme/main.ts"),
        b"broken live source after capture",
    )
    .expect("disposable live edit");
    assert_eq!(
        render(&inputs, "model.json")
            .expect("retained inputs, no reread")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
}

#[test]
fn unsupported_plugin_requires_exact_optional_permission_and_provider_contract() {
    let fixture = Fixture::new(&[(
        "theme/theme.toml",
        THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
    )]);
    let required = fixture
        .configuration("required")
        .replace(RESOURCE_SELECTION, "");
    assert_failure(&required, "model.json", "publication_uncompiled_capability");
    let unpermitted = configuration(&fixture).replace(",allow_absence='unsupported'", "");
    assert_failure(
        &unpermitted,
        "model.json",
        "publication_uncompiled_capability",
    );

    let missing_capability = PLUGIN_MANIFEST.replace(
        "implements=['publication.search/v1']",
        "implements=['commerce.inventory/v1']",
    );
    let fixture = Fixture::new(&[
        (
            "theme/theme.toml",
            THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
        ),
        ("theme/plugin.toml", missing_capability.as_bytes()),
    ]);
    assert_failure(&configuration(&fixture), "model.json", "package_selection");

    let fixture = Fixture::new(&[
        (
            "theme/theme.toml",
            THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
        ),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
    ]);
    let inputs = acquire(&configuration(&fixture));
    let mut limits = build::PublicationRenderLimits::default();
    limits.output.resource_metadata_bytes = 0;
    let error = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: Default::default(),
        },
        limits,
    )
    .expect_err("unavailable capability evidence is still bounded");
    assert_eq!(error.code(), "resource_limit");
}

#[test]
fn unavailable_capability_never_claims_an_independent_static_output() {
    let fixture = Fixture::new(&[
        (
            "theme/theme.toml",
            THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
        ),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
        (
            "content/download.txt",
            b"authored static bytes remain independently owned",
        ),
    ]);
    let config = configuration(&fixture).replace(
        "authored=[{source='source:content',target='authored'}]",
        "authored=[{source='source:content',target='authored',static_assets={route_prefix='/assets/'}}]",
    );
    let inputs = acquire(&config);
    let rendered =
        render(&inputs, "model.json").expect("unavailable capability owns no resource route");
    assert_eq!(
        bytes(rendered.site().output(), "assets/download.txt"),
        b"authored static bytes remain independently owned"
    );
    assert!(
        rendered
            .site()
            .output()
            .file(&"assets/search.js".parse().expect("unclaimed route"))
            .is_none()
    );
    let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
        .expect("shared model");
    assert_eq!(model["resources"], json!({}));
}
