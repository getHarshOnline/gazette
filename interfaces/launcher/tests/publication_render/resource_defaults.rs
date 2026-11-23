//! Theme-owned defaults through admission, review and native CLI staging.

use super::{
    Fixture, MANIFEST, PAGE, RENDER_ARGUMENTS, acquire, acquire_result, build, bytes, cli, render,
    tempdir,
};
use serde_json::Value;

#[test]
fn theme_resource_defaults_render_without_workspace_copies_and_keep_overrides() {
    let manifest = format!(
        "{MANIFEST}resource_requirements=['style:base','style:world']\n\
[[dependencies]]\npackage='theme:base'\n\
[[resources]]\nid='style:world'\ninputs=['world.css']\nbuild={{kind='stylesheet',entry='world.css'}}\noutput='/assets/world.css'\n"
    );
    let base = "package='theme:base'\nnamespace='base'\ntemplates='templates'\n\
[[resources]]\nid='style:base'\ninputs=['base.css']\nbuild={kind='stylesheet',entry='base.css'}\noutput='/assets/base.css'\n";
    let fixture = Fixture::new(&[
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/base.toml", base.as_bytes()),
        ("theme/world.css", b".world { color: red; }"),
        ("theme/base.css", b".base { color: blue; }"),
    ]);
    let config = format!(
        "{}[[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='base.toml'\n",
        fixture.config()
    );
    let inputs = acquire(&config);
    let metadata = inputs
        .check_metadata(Default::default())
        .expect("inherited defaults admitted");
    let effective = serde_json::to_value(metadata.inspect()).unwrap();
    assert_eq!(
        effective["composition"]["resources"]["style:base"]["origin"]["scope"]["theme"],
        "theme:base"
    );
    let parsed = build::parse_publication(&inputs, Default::default()).unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    let review = build::review(&resolved, Default::default()).unwrap();
    let composition = review
        .request()
        .surfaces()
        .iter()
        .find(|surface| matches!(surface.target(), build::ReviewTarget::Composition))
        .unwrap();
    assert_eq!(
        composition.value(),
        &effective["composition"],
        "review and rendering share admitted defaults"
    );
    let rendered = render(&inputs).expect("native default resources");
    assert!(
        std::str::from_utf8(bytes(rendered.site().output(), "assets/base.css"))
            .unwrap()
            .contains("blue")
    );
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    assert_eq!(
        model["resources"]["style:world"]["url"],
        "https://one.example/mount/assets/world.css"
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["theme_resources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let stage = tempdir().unwrap();
    let result = cli(
        &config,
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--output-parent",
            stage.path().to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(
        std::path::Path::new(receipt["directory"].as_str().unwrap())
            .join("assets/base.css")
            .is_file()
    );

    let override_config = config.replacen("theme={package='theme:one'}\n", "theme={package='theme:one'}\n[composition.resources]\n'style:world'={state='enabled',resource={owner={kind='theme',package='theme:one'},resource='style:world'},output='/custom.css',use='site'}\n", 1);
    let overridden = acquire(&override_config);
    let rendered = render(&overridden).expect("workspace keeps its exact override");
    assert!(
        rendered
            .site()
            .output()
            .file(&"custom.css".parse().unwrap())
            .is_some()
    );
    assert!(
        rendered
            .site()
            .output()
            .file(&"assets/world.css".parse().unwrap())
            .is_none()
    );
    let disabled = config.replacen(
        "theme={package='theme:one'}\n",
        "theme={package='theme:one'}\n[composition.resources]\n'style:world'={state='disabled'}\n",
        1,
    );
    assert!(
        acquire_result(&disabled).is_err(),
        "disabled required resource cannot recover its Theme default"
    );
}

#[test]
fn theme_copy_reaches_review_pages_components_and_cli_with_local_overrides() {
    let copy = include_str!("../../../../crates/gazette-compiler/tests/fixtures/site_chrome.toml")
        .replace("composition.chrome", "chrome");
    let base = format!(
        "package='theme:base'\nnamespace='base'\ntemplates='templates'\n{copy}\n[chrome.regions]\n'base:note'='From Base'\n[chrome.shell.search]\ntrigger_label='Find'\ntitle='Find pages'\nfield_label='Query'\nloading_message='Loading'\nempty_message='No matches'\nunavailable_message='Unavailable'\n"
    );
    let manifest = format!(
        "{MANIFEST}[[dependencies]]\npackage='theme:base'\n[chrome]\nfooter_label='World footer'\n[components.copy]\ntemplate='copy.html'\n"
    );
    let page = PAGE.replace("**Retained** body\n", ":::copy\nRetained body.\n:::\n");
    let fixture = Fixture::new(&[
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/base.toml", base.as_bytes()),
        ("one/page.md", page.as_bytes()),
        ("theme/templates/page.html", b"<nav>{{ presentation.chrome.navigation_label }}</nav><footer>{{ presentation.chrome.footer_label }}</footer>{{ body_html | safe }}"),
        ("theme/templates/copy.html", b"<aside>{{ presentation.chrome.regions['base:note'] }} {{ component.body.html | safe }}</aside>"),
    ]);
    let config = format!("{}[[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='base.toml'\n", fixture.config())
        .replacen("[[principals]]", "[composition.chrome]\nnavigation_label='Workspace navigation'\n[composition.chrome.shell.search]\nenabled=false\n[[principals]]", 1)
        .replacen("[[sites]]\nid='site:other'", "[sites.composition.chrome]\nfooter_label='Site footer'\n[[sites]]\nid='site:other'", 1);
    let inputs = acquire(&config);
    let metadata = inputs.check_metadata(Default::default()).unwrap();
    let inspected = serde_json::to_value(metadata.inspect()).unwrap();
    let composition = &inspected["composition"];
    assert_eq!(
        composition["chrome"]["origins"]["/skip_to_content_label"]["scope"]["theme"],
        "theme:base"
    );
    let parsed = build::parse_publication(&inputs, Default::default()).unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    let review = build::review(&resolved, Default::default()).unwrap();
    assert_eq!(
        review
            .request()
            .surfaces()
            .iter()
            .find(|surface| matches!(surface.target(), build::ReviewTarget::Composition))
            .unwrap()
            .value(),
        composition
    );
    let rendered = render(&inputs).expect("Theme copy completes partial workspace copy");
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
    let chrome = &model["pages"]["content:one"]["presentation"]["chrome"];
    assert_eq!(chrome["skip_to_content_label"], "Skip to content");
    assert_eq!(chrome["navigation_label"], "Workspace navigation");
    assert_eq!(chrome["footer_label"], "Site footer");
    assert!(
        chrome["shell"].get("search").is_none(),
        "disabled search stays absent despite Theme labels"
    );
    let html = std::str::from_utf8(bytes(rendered.site().output(), "guide/index.html")).unwrap();
    assert!(html.contains("<nav>Workspace navigation</nav>"));
    assert!(html.contains("<footer>Site footer</footer>"));
    assert!(
        html.contains("<aside>From Base "),
        "components use the same admitted region defaults"
    );

    let stage = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", stage.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    assert_eq!(
        std::fs::read(directory.join("guide/index.html")).unwrap(),
        bytes(rendered.site().output(), "guide/index.html")
    );

    let enabled = config.replace("enabled=false", "enabled=true");
    let enabled_inputs = acquire(&enabled);
    let enabled_render = render(&enabled_inputs).expect("copy can describe unavailable search");
    assert!(
        enabled_render
            .site()
            .output()
            .file(&"search.json".parse().unwrap())
            .is_none(),
        "search copy does not enable an undeclared search corpus"
    );
}
