//! Retained island assembly evidence; browser mounting is qualified separately.

use super::*;
use serde_json::json;

#[path = "islands/browser.rs"]
mod browser;

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='page.html'}\n[components.counter]\ntemplate='components/counter.html'\narguments={type='object',required=['label'],additionalProperties=false,properties={label={type='string'}}}\ninteraction={module='module:counter',export='mountCounter',mount='load'}\n[components.unused]\ntemplate='components/counter.html'\ninteraction={module='module:unused',export='mountUnused',mount='load'}\n";
const PLUGIN: &str = "package='fixture-plugin'\n[resources.'module:counter']\ninputs=['counter.ts','increment.ts']\nbuild={kind='browser-module',entry='counter.ts'}\n[resources.'module:unused']\ninputs=['unused.ts']\nbuild={kind='browser-module',entry='unused.ts'}\n";
const MODULE: &str = "import { step } from './increment.ts'; let count: number = 0; export function mountCounter({element, props}) { count += step; element.textContent = props.label + ':' + count; }";
const SELECTION: &str = "'module:counter'={state='enabled',resource={owner={kind='plugin',package='fixture-plugin'},resource='module:counter'},output='/assets/counter.js',use='dependency'}\n'module:unused'={state='enabled',resource={owner={kind='plugin',package='fixture-plugin'},resource='module:unused'},output='/assets/unused.js',use='dependency'}\n";
const TEMPLATE: &str = "{% if summary is defined %}<section class=\"excerpt\">{{ summary.html | safe }}</section>{% endif %}<main>{{ body_html | safe }}</main>{% if presentation.interaction is defined %}<script type=\"module\" src=\"{{ presentation.interaction.url }}\"></script>{% endif %}";
const COMPONENT: &str = "<aside class=\"static-counter\">{{ component.body.html | safe }}</aside>";
const DANGEROUS: &str = "</script><img src=x onerror=alert(1)> & \"quoted\" 🦀";

fn notes_fixture() -> Fixture {
    let article = "+++\n[content]\nid='content:notes'\nsite='site:one'\ntitle='Notes enhancement'\nlifecycle='published'\nroute={path='/notes/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Main argument\n\nContinuous main meaning.\n";
    let template = format!(
        "{TEMPLATE}{{% if notes is defined %}}<aside id=\"article-notes\">{{{{ notes.body_html | safe }}}}</aside>{{% endif %}}"
    );
    Fixture::new(&[
        ("one/notes/index.md", article.as_bytes()),
        (
            "one/notes/notes.md",
            b"# Supplement\n\n:::counter {label='notes'}\nSupplementary fallback.\n:::\n",
        ),
        ("theme/theme.toml", MANIFEST.as_bytes()),
        ("theme/plugin.toml", PLUGIN.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
        (
            "theme/templates/components/counter.html",
            COMPONENT.as_bytes(),
        ),
        ("theme/counter.ts", MODULE.as_bytes()),
        ("theme/increment.ts", b"export const step: number = 1;"),
        ("theme/unused.ts", b"export function mountUnused() {}"),
    ])
}

#[test]
fn notes_only_island_uses_the_article_bootstrap_and_retained_resource_closure() {
    let fixture = notes_fixture();
    let inputs = acquire(&configured(&fixture, SELECTION));
    let rendered = render(&inputs).expect("notes-only interaction must be assembled");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:notes"];
    assert_eq!(page["components"], json!([]));
    assert_eq!(page["notes"]["components"].as_array().unwrap().len(), 1);
    assert!(page["notes"].get("id").is_none());
    assert!(
        !page["body_html"]
            .as_str()
            .unwrap()
            .contains("Supplementary")
    );
    let resource = &page["presentation"]["interaction"];
    let path = resource["output"]
        .as_str()
        .expect("article-owned bootstrap");
    let bootstrap = std::str::from_utf8(bytes(tree, path)).unwrap();
    assert!(bootstrap.contains("mountCounter"));
    assert!(bootstrap.contains("assets/counter.js"));
    assert!(!bootstrap.contains("mountUnused"));
    assert!(tree.file(&"assets/counter.js".parse().unwrap()).is_some());
    assert!(tree.file(&"assets/unused.js".parse().unwrap()).is_none());
    let html = std::str::from_utf8(bytes(tree, "notes/index.html")).unwrap();
    assert!(html.contains("data-gazette-state=\"pending\""));
    assert!(html.contains("id=\"article-notes\""));
    assert!(html.contains(&resource["url"].as_str().unwrap().replace('/', "&#x2F;")));
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    assert_eq!(evidence["islands"]["content:notes"]["resource"], *resource);
    assert_eq!(evidence["islands"].as_object().unwrap().len(), 1);
    fixture
        .directory
        .close()
        .expect("remove exact owned input tree");
    let replay = render(&inputs).expect("no ambient notes or module reads");
    assert_eq!(
        tree.inventory().identity(),
        replay.site().output().inventory().identity()
    );
}

#[test]
fn notes_only_island_refuses_absent_or_disabled_modules_without_staging() {
    let fixture = notes_fixture();
    for selection in ["", "'module:counter'={state='disabled'}\n"] {
        let config = configured(&fixture, selection);
        let inputs = acquire(&config);
        assert_eq!(
            render(&inputs)
                .expect_err("no notes module fallback")
                .code(),
            "resource_input"
        );
        let destination = tempdir().unwrap();
        fs::write(destination.path().join("keep"), b"untouched").unwrap();
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend(["--output-parent", destination.path().to_str().unwrap()]);
        let output = cli(&config, &arguments);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("resource_input"));
        assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 1);
        assert_eq!(
            fs::read(destination.path().join("keep")).unwrap(),
            b"untouched"
        );
    }
}

#[test]
fn bare_theme_render_cannot_skip_notes_only_interaction_compilation() {
    let fixture = notes_fixture();
    let inputs = acquire(&configured(&fixture, SELECTION));
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained notes corpus");
    let resolved = parsed.resolve(Default::default()).expect("shared meaning");
    let metadata = inputs
        .check_metadata(Default::default())
        .expect("Theme metadata");
    let theme = build::compile_templates(&metadata, Default::default()).expect("native templates");
    let error = theme
        .render_page(
            resolved.authored(),
            &"content:notes".parse().unwrap(),
            &"fixture/page.html".parse().unwrap(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("bare caller must not leave a notes-only island pending");
    assert_eq!(error.code(), "page_uncompiled_interaction");
}

fn fixture() -> Fixture {
    let body = format!(
        "# Interactive example\n\n::::counter {{label={}}}\nOuter **meaning**.\n\n:::counter {{label='nested'}}\nNested fallback.\n:::\n\n<!-- more -->\n\nOuter remainder.\n::::\n\n:::counter {{label='sibling'}}\nSibling fallback.\n:::\n",
        serde_json::to_string(DANGEROUS).expect("literal TOML-compatible string")
    );
    let page = PAGE.replace("**Retained** body\n", &body);
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/theme.toml", MANIFEST.as_bytes()),
        ("theme/plugin.toml", PLUGIN.as_bytes()),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
        (
            "theme/templates/components/counter.html",
            COMPONENT.as_bytes(),
        ),
        ("theme/counter.ts", MODULE.as_bytes()),
        ("theme/increment.ts", b"export const step: number = 1;"),
        (
            "theme/unused.ts",
            b"export function mountUnused() { throw new Error('unused module'); }",
        ),
    ])
}

fn configured(fixture: &Fixture, selection: &str) -> String {
    let mut config = fixture.config().replacen(
        "[[principals]]",
        &format!("[composition.plugins]\n'fixture-plugin'={{state='enabled'}}\n[composition.resources]\n{selection}[[principals]]"),
        1,
    );
    config.push_str(
        "[[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n",
    );
    config
}

#[test]
fn nested_islands_share_retained_modules_safe_props_and_page_scoped_bootstrap() {
    let fixture = fixture();
    let config = configured(&fixture, SELECTION);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete retained interaction assembly");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let page = &model["pages"]["content:one"];
    let components = page["components"]
        .as_array()
        .expect("ordered authored occurrences");
    assert_eq!(components.len(), 3);
    assert_eq!(components[0]["arguments"]["label"], DANGEROUS);
    for component in components {
        assert_eq!(component["name"], "counter");
        assert_eq!(
            component["interaction"],
            json!({"module":"module:counter","export":"mountCounter","mount":"load"})
        );
    }
    let body = page["body_html"].as_str().expect("HTML projection");
    assert_eq!(
        body.matches("data-gazette-component=\"counter\"").count(),
        3
    );
    assert_eq!(body.matches("data-gazette-state=\"pending\"").count(), 3);
    assert_eq!(body.matches("data-gazette-fallback").count(), 3);
    assert_eq!(body.matches("data-gazette-mount").count(), 3);
    assert!(body.contains("&lt;/script&gt;&lt;img src=x onerror=alert(1)&gt; &amp;"));
    assert!(body.contains("&quot;label&quot;"));
    assert!(!body.contains("<img src=x"));
    assert!(!body.contains("<script"));
    for fallback in [
        "Outer <strong>meaning</strong>.",
        "Nested fallback.",
        "Sibling fallback.",
    ] {
        assert!(body.contains(fallback), "static fallback lost {fallback}");
    }
    let excerpt = page["summary"]["html"].as_str().expect("static excerpt");
    assert_eq!(excerpt.matches("class=\"static-counter\"").count(), 2);
    assert!(!excerpt.contains("data-gazette-"));
    assert!(!excerpt.contains("Outer remainder"));
    assert!(
        page["body_text"]
            .as_str()
            .unwrap()
            .contains("Outer meaning.")
    );
    assert!(!page["body_text"].as_str().unwrap().contains("mountCounter"));

    let resource = &page["presentation"]["interaction"];
    let path = resource["output"].as_str().expect("bootstrap output");
    let url = resource["url"].as_str().expect("bootstrap URL");
    let module_url = "https://one.example/mount/assets/counter.js";
    assert_eq!(resource["kind"], "browser-module");
    assert_eq!(
        resource["references"],
        json!({(module_url):"assets/counter.js"})
    );
    assert_eq!(
        resource["sha256"],
        source::Sha256Digest::of(bytes(tree, path)).to_string()
    );
    let bootstrap = std::str::from_utf8(bytes(tree, path)).expect("compiled bootstrap");
    assert!(bootstrap.contains(module_url));
    assert!(bootstrap.contains("mountCounter"));
    assert!(!bootstrap.contains("mountUnused"));
    assert!(!bootstrap.contains("assets/unused.js"));
    let module =
        std::str::from_utf8(bytes(tree, "assets/counter.js")).expect("compiled native module");
    assert!(module.contains("mountCounter"));
    assert!(
        !module.contains("./increment.ts"),
        "local dependency is bundled by the existing JS owner"
    );
    assert!(tree.file(&"assets/unused.js".parse().unwrap()).is_none());
    assert_eq!(
        tree.inventory()
            .entries()
            .iter()
            .filter(|entry| entry.path.as_str() == "assets/counter.js")
            .count(),
        1
    );
    let evidence =
        serde_json::to_value(rendered.site().evidence()).expect("private build evidence");
    assert_eq!(evidence["resources"].as_object().unwrap().len(), 1);
    assert_eq!(
        evidence["resources"]["module:counter"]["dependencies"]["counter.ts"],
        source::Sha256Digest::of(MODULE.as_bytes()).to_string()
    );
    assert_eq!(
        evidence["resources"]["module:counter"]["dependencies"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(evidence["islands"].as_object().unwrap().len(), 1);
    assert_eq!(evidence["islands"]["content:one"]["resource"], *resource);
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme output");
    let script = html
        .split_once("<script type=\"module\" src=\"")
        .expect("module script element")
        .1;
    let (src, rest) = script.split_once('"').expect("closed script source");
    assert_eq!(src.replace("&#x2F;", "/"), url);
    assert!(rest.starts_with("></script>"));
    let home = &model["pages"]["collection:home"];
    assert!(home["presentation"].get("interaction").is_none());
    assert!(
        !std::str::from_utf8(bytes(tree, "index.html"))
            .unwrap()
            .contains("<script")
    );

    let destination = tempdir().expect("fresh CLI stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", destination.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("native CLI receipt");
    assert_eq!(receipt["inventory"], json!(tree.inventory()));
    let stage = std::path::Path::new(receipt["directory"].as_str().unwrap());
    for entry in tree.inventory().entries() {
        if let Some(expected) = tree.file(&entry.path) {
            assert_eq!(
                fs::read(stage.join(entry.path.as_str())).expect("exact staged file"),
                expected
            );
        }
    }
    fixture
        .directory
        .close()
        .expect("remove owned native sources after capture");
    let replay = render(&inputs).expect("retained interaction rebuild without native sources");
    assert_eq!(
        replay.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(bytes(replay.site().output(), path), bytes(tree, path));
    assert_eq!(
        bytes(replay.site().output(), "model.json"),
        bytes(tree, "model.json")
    );
}

#[test]
fn absent_or_disabled_interaction_module_refuses_without_partial_staging() {
    let fixture = fixture();
    for selection in ["", "'module:counter'={state='disabled'}\n"] {
        let config = configured(&fixture, selection);
        let inputs = acquire(&config);
        assert_eq!(
            render(&inputs).expect_err("no module fallback").code(),
            "resource_input"
        );
        let destination = tempdir().expect("preserved parent");
        fs::write(destination.path().join("keep"), b"untouched").unwrap();
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend(["--output-parent", destination.path().to_str().unwrap()]);
        let output = cli(&config, &arguments);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("resource_input"));
        assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 1);
        assert_eq!(
            fs::read(destination.path().join("keep")).unwrap(),
            b"untouched"
        );
    }
}

#[test]
fn structured_landing_retains_enhancement_closure_and_static_fallback() {
    let source = r#"format: gazette-landing/v1
content:
  id: content:landing
  site: site:one
  title: A composed entrance
  lifecycle: published
  route: {path: /entrance/}
  structure: {kind: page, family: {kind: utility}}
sections:
  - component: counter
    arguments: {label: entrance}
    heading: {level: 1, text: An entrance}
    body: Complete static meaning.
    sections:
      - component: counter
        arguments: {label: continuation}
        body: A nested continuation.
"#;
    let fixture = Fixture::new(&[
        ("one/entrance.landing.yaml", source.as_bytes()),
        ("theme/theme.toml", MANIFEST.as_bytes()),
        ("theme/plugin.toml", PLUGIN.as_bytes()),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
        (
            "theme/templates/components/counter.html",
            COMPONENT.as_bytes(),
        ),
        ("theme/counter.ts", MODULE.as_bytes()),
        ("theme/increment.ts", b"export const step: number = 1;"),
        ("theme/unused.ts", b"export function mountUnused() {}"),
    ]);
    let inputs = acquire(&configured(&fixture, SELECTION));
    let rendered = render(&inputs).expect("native landing enhancement closure");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:landing"];
    assert_eq!(page["components"].as_array().unwrap().len(), 2);
    let html = std::str::from_utf8(bytes(tree, "entrance/index.html")).unwrap();
    assert_eq!(html.matches("data-gazette-fallback").count(), 2);
    assert!(html.contains("Complete static meaning."));
    assert!(html.contains("A nested continuation."));
    let bootstrap = std::str::from_utf8(bytes(
        tree,
        page["presentation"]["interaction"]["output"]
            .as_str()
            .unwrap(),
    ))
    .unwrap();
    assert!(bootstrap.contains("mountCounter"));
    assert!(tree.file(&"assets/counter.js".parse().unwrap()).is_some());
    assert!(tree.file(&"assets/unused.js".parse().unwrap()).is_none());
    fixture.directory.close().unwrap();
    assert_eq!(
        tree.inventory().identity(),
        render(&inputs)
            .unwrap()
            .site()
            .output()
            .inventory()
            .identity()
    );
}
