//! Capability status and independently selected site resources share one retained build.

#[path = "publication_capabilities/absence.rs"]
mod absence;

use gazette_test_support as native;

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

const THEME_MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\nresource_requirements=['module:search']\n";
const THEME_MANIFEST_WITHOUT_RESOURCES: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n";
const PLUGIN_MANIFEST: &str = "package='publication-capabilities'\n[resources.'module:search']\ninputs=['main.ts','dep.ts']\nbuild={kind='browser-module',entry='main.ts'}\n[providers.search]\nimplements=['publication.search/v1']\n";
const MAIN: &str = "import { answer } from './dep.ts';\nconst value: number = answer;\ndocument.documentElement.dataset.answer = String(value);\nexport { value };\n";
const DEPENDENCY: &str = "export const answer: number = 42;\n";
const PAGE: &str = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Local capability'\nlifecycle='published'\nroute={path='/',aliases=['/old.html']}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained content\n";
const TEMPLATE: &str = "{{ body_html | safe }}{% for resource in presentation.theme_resources %}<script type=\"module\" src=\"{{ resource.url }}\"></script>{% endfor %}";
const RESOURCE_SELECTION: &str = "[composition.resources]\n'module:search'={state='enabled',resource={owner={kind='plugin',package='publication-capabilities'},resource='module:search'},output='/assets/search.js',use='site'}\n";
const ARGUMENTS: [&str; 6] = [
    "render-site",
    "--local",
    "--site",
    "site:one",
    "--model-output",
    "model.json",
];

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(overrides: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("isolated native repository");
        let repository = Repository::init(root.path()).expect("fixture Git");
        for (path, bytes) in [
            ("theme/theme.toml", THEME_MANIFEST.as_bytes()),
            ("theme/plugin.toml", PLUGIN_MANIFEST.as_bytes()),
            ("theme/templates/page.html", TEMPLATE.as_bytes()),
            (
                "theme/templates/alias.html",
                b"<a href=\"{{ route.canonical_url }}\">Moved</a>".as_slice(),
            ),
            ("theme/main.ts", MAIN.as_bytes()),
            ("theme/dep.ts", DEPENDENCY.as_bytes()),
            ("content/page.md", PAGE.as_bytes()),
        ]
        .into_iter()
        .chain(overrides.iter().copied())
        {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("fixture directories");
            fs::write(path, bytes).expect("fixture bytes");
        }
        let mut index = repository.index().expect("index");
        index
            .add_all(["theme", "content"], IndexAddOption::DEFAULT, None)
            .expect("exact fixture source set");
        let tree = repository
            .find_tree(index.write_tree().expect("tree identity"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "capability fixture",
                &tree,
                &[],
            )
            .expect("isolated commit");
        Self { root, commit }
    }

    fn configuration(&self, requirement: &str) -> String {
        let permission = if requirement == "optional" {
            ",allow_absence='unsupported'"
        } else {
            ""
        };
        let mut config = format!(
            "[composition]\ntheme={{package='theme:one'}}\n[composition.plugins]\n'publication-capabilities'={{state='enabled'}}\n[composition.capabilities]\n'publication.search/v1'={{state='enabled',provider={{kind='plugin',package='publication-capabilities',provider='search'}},requirement='{requirement}'{permission}}}\n{RESOURCE_SELECTION}[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\nauthored=[{{source='source:content',target='authored'}}]\n"
        );
        for name in ["theme", "content"] {
            config.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{name}'}}\n", self.root.path().to_str().expect("root"), self.commit));
        }
        config.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n[[plugin_packages]]\npackage='publication-capabilities'\nsource='source:theme'\nmanifest='plugin.toml'\n");
        config
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_publication(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 3,
            entries: 64,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("complete selected capture")
}

fn render<'a>(
    inputs: &'a build::AcquiredInputs,
    model: &str,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&model.parse().expect("model output")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
}

fn bytes<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("expected output")
}

fn cli(config: &str, arguments: &[&str]) -> Output {
    let mut arguments = arguments.to_vec();
    if arguments.first() == Some(&"render-site") && !arguments.contains(&"--render-year") {
        arguments.extend(["--render-year", "2040"]);
    }
    let mut process = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual Gazette CLI");
    process
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("workspace transport");
    process.wait_with_output().expect("CLI completion")
}

fn execute_trusted_fixture_module(program: &[u8]) {
    // Node validates only this test's trusted emitted fixture over stdin. It is
    // not Gazette's compiler/runtime and supplies no file or network fallback.
    const VALIDATOR: &str = r#"
let code = '';
process.stdin.setEncoding('utf8');
for await (const chunk of process.stdin) code += chunk;
globalThis.document = { documentElement: { dataset: {} } };
const module = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
if (module.value !== 42) throw new Error('retained ESM export mismatch');
if (document.documentElement.dataset.answer !== '42') throw new Error('retained ESM effect mismatch');
process.stdout.write(JSON.stringify({ value: module.value, answer: document.documentElement.dataset.answer }));
"#;
    let mut process = Command::new(native::node())
        .args(["--input-type=module", "--eval", VALIDATOR])
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node.js is a required native validator for this trusted fixture test");
    process
        .stdin
        .take()
        .expect("validator stdin")
        .write_all(program)
        .expect("exact retained module transport");
    let output = process
        .wait_with_output()
        .expect("native ESM validator completion");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("executed module result"),
        serde_json::json!({"value":42,"answer":"42"})
    );
}

fn assert_failure(config: &str, model: &str, expected: &str) {
    let site = "site:one".parse().expect("site");
    let acquired = build::acquire_publication(
        config,
        &site,
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 3,
            entries: 64,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    );
    let code = match acquired {
        Ok(inputs) => render(&inputs, model)
            .expect_err("selected capability must not disappear")
            .code(),
        Err(error) => error.code(),
    };
    assert!(code.starts_with(expected), "wrong SDK failure: {code}");
    let parent = tempdir().expect("explicit output parent");
    fs::create_dir(parent.path().join("unrelated")).expect("unrelated directory");
    fs::write(
        parent.path().join("unrelated/keep.txt"),
        b"keep exact bytes",
    )
    .expect("unrelated bytes");
    let before =
        source::inspect(parent.path(), None, source::Limits::default()).expect("before failure");
    let mut arguments = ARGUMENTS.to_vec();
    arguments[5] = model;
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(config, &arguments);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "no success receipt before complete output"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.starts_with(&format!("gazette: {expected}")),
        "wrong CLI failure: {error}"
    );
    assert_eq!(
        source::inspect(parent.path(), None, source::Limits::default())
            .expect("after failure")
            .identity(),
        before.identity(),
        "no stage or changed unrelated bytes"
    );
}

#[test]
fn selected_site_resource_reaches_cli_theme_and_model_while_capability_is_unavailable() {
    let fixture = Fixture::new(&[]);
    let config = fixture.configuration("optional");
    let inputs = acquire(&config);
    assert_eq!(
        inputs
            .sources()
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["source:content", "source:theme"]
    );
    assert!(!fixture.root.path().join("workspace").exists());
    let rendered = render(&inputs, "model.json").expect("real browser-module compilation");
    let tree = rendered.site().output();
    let javascript = bytes(tree, "assets/search.js");
    let javascript_text = std::str::from_utf8(javascript).expect("JavaScript output");
    assert_ne!(javascript, MAIN.as_bytes(), "not copied TypeScript");
    assert!(
        javascript_text.contains("42"),
        "relative dependency is actually consumed"
    );
    assert!(javascript_text.contains("dataset.answer"));
    assert!(
        !javascript_text.contains(": number"),
        "TypeScript types are erased"
    );
    assert!(
        !javascript_text.contains("./dep.ts"),
        "no unserved TypeScript runtime dependency"
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("same local model");
    let capability = serde_json::json!({"state":"unavailable","reason":"unsupported"});
    let resource = serde_json::json!({"kind":"browser-module","url":"https://one.example/mount/assets/search.js","output":"assets/search.js","sha256":source::Sha256Digest::of(javascript),"bytes":javascript.len()});
    assert_eq!(model["capabilities"]["publication.search/v1"], capability);
    assert_eq!(model["resources"]["module:search"], resource);
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["capabilities"]["publication.search/v1"],
        capability
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["theme_resources"][0],
        serde_json::json!({"id":"module:search", "kind":"browser-module","url":"https://one.example/mount/assets/search.js","output":"assets/search.js","sha256":source::Sha256Digest::of(javascript),"bytes":javascript.len()})
    );
    let html = std::str::from_utf8(bytes(tree, "index.html")).expect("HTML");
    assert!(html.replace("&#x2F;", "/").contains(
        "<script type=\"module\" src=\"https://one.example/mount/assets/search.js\"></script>"
    ));
    let public_model = std::str::from_utf8(bytes(tree, "model.json")).expect("JSON");
    for private in [
        fixture.root.path().to_str().expect("root"),
        "main.ts",
        "dep.ts",
        "theme.toml",
        "plugin.toml",
        "source:theme",
    ] {
        assert!(
            !public_model.contains(private),
            "private compilation evidence leaked: {private}"
        );
    }
    assert_eq!(
        render(&inputs, "model.json")
            .expect("repeat compilation")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    let parent = tempdir().expect("explicit retained output");
    fs::write(parent.path().join("unrelated.txt"), b"preserve").expect("unrelated bytes");
    let mut arguments = ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    let retained = std::path::Path::new(receipt["directory"].as_str().expect("retained directory"));
    assert_eq!(
        source::inspect(retained, None, source::Limits::default())
            .expect("independent read-back")
            .identity(),
        tree.inventory().identity()
    );
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("actual output bytes"),
                tree.file(&entry.path).expect("SDK output bytes")
            );
        }
    }
    execute_trusted_fixture_module(
        &fs::read(retained.join("assets/search.js")).expect("actual CLI-retained module"),
    );
    let capability_evidence = receipt["evidence"]["capabilities"]["publication.search/v1"]
        .as_object()
        .expect("private capability evidence");
    for field in ["provider", "selection", "requirement", "state", "reason"] {
        assert!(
            capability_evidence.contains_key(field),
            "missing consumed capability evidence {field}"
        );
    }
    assert_eq!(capability_evidence["provider"]["kind"], "plugin");
    assert_eq!(
        capability_evidence["provider"]["package"],
        "publication-capabilities"
    );
    assert_eq!(capability_evidence["provider"]["provider"], "search");
    assert_eq!(capability_evidence["selection"]["scope"], "workspace");
    assert_eq!(capability_evidence["requirement"], "optional");
    assert_eq!(capability_evidence["state"], "unavailable");
    assert_eq!(capability_evidence["reason"], "unsupported");
    let evidence = receipt["evidence"]["resources"]["module:search"]
        .as_object()
        .expect("private resource evidence");
    assert_eq!(evidence["source"], "source:theme");
    assert_eq!(evidence["resource"]["owner"]["kind"], "plugin");
    assert_eq!(
        evidence["resource"]["owner"]["package"],
        "publication-capabilities"
    );
    assert_eq!(evidence["resource"]["resource"], "module:search");
    assert_eq!(evidence["selection"]["scope"], "workspace");
    assert_eq!(evidence["use"], "site");
    assert_eq!(
        evidence["manifest_sha256"],
        serde_json::to_value(source::Sha256Digest::of(PLUGIN_MANIFEST.as_bytes()))
            .expect("manifest digest")
    );
    assert_eq!(evidence["output_sha256"], resource["sha256"]);
    assert_eq!(
        evidence["dependencies"],
        serde_json::json!({
            "main.ts":source::Sha256Digest::of(MAIN.as_bytes()),
            "dep.ts":source::Sha256Digest::of(DEPENDENCY.as_bytes())
        })
    );
    assert_eq!(evidence["recipe"]["kind"], "browser-module");
    assert_eq!(evidence["recipe"]["entry"], "main.ts");
    assert_eq!(
        fs::read(parent.path().join("unrelated.txt")).expect("preserved unrelated bytes"),
        b"preserve"
    );
    fs::write(
        fixture.root.path().join("theme/dep.ts"),
        b"export const answer: number = 99;\n",
    )
    .expect("isolated change after capture");
    fs::write(
        fixture.root.path().join("theme/main.ts"),
        b"invalid after capture",
    )
    .expect("isolated change after capture");
    assert_eq!(
        render(&inputs, "model.json")
            .expect("retained compile inputs")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    let output = cli(&config, &ARGUMENTS);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source_content_mismatch"));
}

#[test]
fn invalid_selected_resources_fail_independently_of_optional_capability_status() {
    for source in [
        "export const value: number = ;\n",
        "import { answer } from './missing.ts'; export { answer };\n",
        "import { answer } from '../content/page.md'; export { answer };\n",
        "import { answer } from '../../outside.ts'; export { answer };\n",
        "import { answer } from 'https://example.invalid/module.ts'; export { answer };\n",
    ] {
        let fixture = Fixture::new(&[("theme/main.ts", source.as_bytes())]);
        assert_failure(
            &fixture.configuration("optional"),
            "model.json",
            "resource_",
        );
    }
}

#[test]
fn invalid_recipe_fails_but_capability_does_not_infer_an_unselected_resource() {
    let manifest = PLUGIN_MANIFEST.replace("build={kind='browser-module',entry='main.ts'}\n", "");
    let fixture = Fixture::new(&[("theme/plugin.toml", manifest.as_bytes())]);
    assert_failure(
        &fixture.configuration("optional"),
        "model.json",
        "toml_invalid",
    );
    let fixture = Fixture::new(&[(
        "theme/theme.toml",
        THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
    )]);
    let config = fixture
        .configuration("optional")
        .replace(RESOURCE_SELECTION, "");
    let inputs = acquire(&config);
    let rendered =
        render(&inputs, "model.json").expect("provider status does not invent a package resource");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    assert_eq!(model["resources"], serde_json::json!({}));
    assert!(
        tree.file(&"assets/search.js".parse().expect("path"))
            .is_none()
    );
}

#[test]
fn generated_resources_share_page_alias_model_and_parent_collision_checks() {
    let fixture = Fixture::new(&[]);
    for (output, model) in [
        ("/index.html", "model.json"),
        ("/old.html", "model.json"),
        ("/model.json", "model.json"),
        ("/assets/search.js", "assets"),
    ] {
        let config = fixture
            .configuration("optional")
            .replace("output='/assets/search.js'", &format!("output='{output}'"));
        assert_failure(&config, model, "source_inventory");
    }
}

#[test]
fn site_resource_override_and_capability_disable_remain_independent() {
    let fixture = Fixture::new(&[]);
    let config = fixture.configuration("optional").replace("[[brands]]", "[sites.composition.resources]\n'module:search'={state='enabled',resource={owner={kind='plugin',package='publication-capabilities'},resource='module:search'},output='/site/search.js',use='site'}\n[[brands]]");
    let inputs = acquire(&config);
    let rendered = render(&inputs, "model.json").expect("site-owned selection atom");
    assert!(
        rendered
            .site()
            .output()
            .file(&"assets/search.js".parse().expect("inherited output"))
            .is_none()
    );
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).expect("model");
    assert_eq!(
        model["resources"]["module:search"]["url"],
        "https://one.example/mount/site/search.js"
    );
    let fixture = Fixture::new(&[]);
    let config = fixture.configuration("optional").replace(
        "[[brands]]",
        "[sites.composition.capabilities]\n'publication.search/v1'={state='disabled'}\n[[brands]]",
    );
    let inputs = acquire(&config);
    let rendered =
        render(&inputs, "model.json").expect("explicit disable suppresses inherited compilation");
    let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
        .expect("disabled model");
    assert_eq!(model["capabilities"], serde_json::json!({}));
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["capabilities"],
        serde_json::json!({})
    );
    assert!(model["resources"].get("module:search").is_some());
    assert!(
        std::str::from_utf8(bytes(rendered.site().output(), "index.html"))
            .expect("HTML")
            .contains("<script")
    );
    let output = cli(&config, &ARGUMENTS);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn one_provider_serves_two_capabilities_while_one_resource_compiles_once() {
    let manifest = PLUGIN_MANIFEST.replace(
        "implements=['publication.search/v1']",
        "implements=['publication.search/v1','commerce.catalog/v1']",
    );
    let fixture = Fixture::new(&[("theme/plugin.toml", manifest.as_bytes())]);
    let config = fixture.configuration("optional").replace("[composition.resources]", "'commerce.catalog/v1'={state='enabled',provider={kind='plugin',package='publication-capabilities',provider='search'},requirement='optional',allow_absence='unsupported'}\n[composition.resources]");
    let inputs = acquire(&config);
    let rendered = render(&inputs, "model.json").expect("one program supplies two contracts");
    let tree = rendered.site().output();
    assert_eq!(
        tree.inventory()
            .entries()
            .iter()
            .filter(|entry| entry.path.as_str() == "assets/search.js")
            .count(),
        1
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared resources");
    assert_eq!(
        model["capabilities"]
            .as_object()
            .expect("two capabilities")
            .len(),
        2
    );
    assert_eq!(
        model["resources"].as_object().expect("one resource").len(),
        1
    );
    assert_eq!(
        model["capabilities"]["publication.search/v1"]["state"],
        "unavailable"
    );
    assert_eq!(
        model["capabilities"]["commerce.catalog/v1"]["state"],
        "unavailable"
    );
    let output = cli(&config, &ARGUMENTS);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("actual CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let evidence = &receipt["evidence"]["capabilities"];
    for field in ["provider", "state", "reason"] {
        assert_eq!(
            evidence["publication.search/v1"][field],
            evidence["commerce.catalog/v1"][field]
        );
    }
    assert_eq!(
        evidence["publication.search/v1"]["selection"]["field"]["capability"],
        "publication.search/v1"
    );
    assert_eq!(
        evidence["commerce.catalog/v1"]["selection"]["field"]["capability"],
        "commerce.catalog/v1"
    );
    assert_eq!(
        receipt["evidence"]["resources"]
            .as_object()
            .expect("one independent resource")
            .len(),
        1
    );
}

#[test]
fn identical_provider_names_and_distinct_resource_owners_do_not_share_programs() {
    let alternate_manifest = "package='alternate-capabilities'\n[resources.'module:alternate']\ninputs=['main.ts','dep.ts']\nbuild={kind='browser-module',entry='main.ts'}\n[providers.search]\nimplements=['commerce.inventory/v1']\n";
    let alternate_dependency = "export const answer: number = 84;\n";
    let fixture = Fixture::new(&[
        ("theme/alternate/plugin.toml", alternate_manifest.as_bytes()),
        ("theme/alternate/main.ts", MAIN.as_bytes()),
        ("theme/alternate/dep.ts", alternate_dependency.as_bytes()),
    ]);
    let config = format!("{}[[sources]]\nid='source:alternate'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme/alternate'}}\n[[plugin_packages]]\npackage='alternate-capabilities'\nsource='source:alternate'\nmanifest='plugin.toml'\n", fixture.configuration("optional")
        .replace("[composition.plugins]", "[composition.plugins]\n'alternate-capabilities'={state='enabled'}")
        .replace("[composition.resources]", "'commerce.inventory/v1'={state='enabled',provider={kind='plugin',package='alternate-capabilities',provider='search'},requirement='optional',allow_absence='unsupported'}\n[composition.resources]")
        .replace("[[principals]]", "'module:alternate'={state='enabled',resource={owner={kind='plugin',package='alternate-capabilities'},resource='module:alternate'},output='/assets/alternate.js',use='site'}\n[[principals]]"), fixture.root.path().to_str().expect("root"), fixture.commit);
    let inputs = acquire(&config);
    assert_eq!(inputs.sources().len(), 3);
    let rendered = render(&inputs, "model.json").expect("exact source plus provider identity");
    let tree = rendered.site().output();
    assert!(
        std::str::from_utf8(bytes(tree, "assets/search.js"))
            .expect("Theme JS")
            .contains("42")
    );
    assert!(
        std::str::from_utf8(bytes(tree, "assets/alternate.js"))
            .expect("workspace JS")
            .contains("84")
    );
    assert_ne!(
        bytes(tree, "assets/search.js"),
        bytes(tree, "assets/alternate.js")
    );
    let output = cli(&config, &ARGUMENTS);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI evidence");
    let capabilities = &receipt["evidence"]["capabilities"];
    assert_eq!(
        capabilities["publication.search/v1"]["provider"]["provider"],
        "search"
    );
    assert_eq!(
        capabilities["commerce.inventory/v1"]["provider"]["provider"],
        "search"
    );
    assert_eq!(
        capabilities["publication.search/v1"]["provider"]["package"],
        "publication-capabilities"
    );
    assert_eq!(
        capabilities["commerce.inventory/v1"]["provider"]["package"],
        "alternate-capabilities"
    );
    let evidence = &receipt["evidence"]["resources"];
    assert_eq!(evidence["module:search"]["source"], "source:theme");
    assert_eq!(evidence["module:alternate"]["source"], "source:alternate");
    assert_ne!(
        evidence["module:search"]["input"],
        evidence["module:alternate"]["input"]
    );
    assert_eq!(
        evidence["module:alternate"]["dependencies"]["dep.ts"],
        serde_json::to_value(source::Sha256Digest::of(alternate_dependency.as_bytes()))
            .expect("alternate exact input")
    );
    assert_failure(
        &config.replace(
            "output='/assets/alternate.js'",
            "output='/assets/search.js'",
        ),
        "model.json",
        "resource_input",
    );
}

#[test]
fn compiled_resource_cannot_overwrite_configured_static_output() {
    let fixture = Fixture::new(&[("content/download.txt", b"retained authored download")]);
    let config = fixture.configuration("optional")
        .replace("authored=[{source='source:content',target='authored'}]", "authored=[{source='source:content',target='authored',static_assets={route_prefix='/assets/'}}]")
        .replace("output='/assets/search.js'", "output='/assets/download.txt'");
    assert_failure(&config, "model.json", "source_inventory");
}

#[test]
fn declared_unused_input_changes_private_identity_not_program_or_public_output() {
    let manifest = PLUGIN_MANIFEST.replace(
        "inputs=['main.ts','dep.ts']",
        "inputs=['main.ts','dep.ts','unused.ts']",
    );
    let before = Fixture::new(&[
        ("theme/plugin.toml", manifest.as_bytes()),
        ("theme/unused.ts", b"export const unused: number = 1;\n"),
    ]);
    let after = Fixture::new(&[
        ("theme/plugin.toml", manifest.as_bytes()),
        ("theme/unused.ts", b"export const unused: number = 2;\n"),
    ]);
    let before_config = before.configuration("optional");
    let after_config = after.configuration("optional");
    let before_inputs = acquire(&before_config);
    let after_inputs = acquire(&after_config);
    let original = render(&before_inputs, "model.json").expect("original declared closure");
    let changed = render(&after_inputs, "model.json").expect("changed unused declared input");
    assert_eq!(
        bytes(original.site().output(), "assets/search.js"),
        bytes(changed.site().output(), "assets/search.js")
    );
    assert_eq!(
        original.site().output().inventory().identity(),
        changed.site().output().inventory().identity()
    );
    let original_evidence =
        serde_json::to_value(original.site().evidence()).expect("original private evidence");
    let changed_evidence =
        serde_json::to_value(changed.site().evidence()).expect("changed private evidence");
    let original = &original_evidence["resources"]["module:search"];
    let changed = &changed_evidence["resources"]["module:search"];
    assert_ne!(original["input"], changed["input"]);
    assert_eq!(original["dependencies"], changed["dependencies"]);
    assert!(original["dependencies"].get("unused.ts").is_none());
    assert_eq!(original["manifest_sha256"], changed["manifest_sha256"]);
    assert_eq!(original["output_sha256"], changed["output_sha256"]);
}

#[test]
fn compiler_and_complete_output_limits_bound_actual_generated_resources() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.configuration("optional"));
    let baseline = render(&inputs, "model.json").expect("baseline compiled publication");
    let program_bytes = bytes(baseline.site().output(), "assets/search.js").len();
    let mut resources = build::PublicationRenderLimits::default();
    resources.output.resources.output_bytes = program_bytes - 1;
    let error = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: source::Limits::default(),
        },
        resources,
    )
    .expect_err("actual generated program exceeds compiler ceiling");
    assert_eq!(error.code(), "resource_limit");
    for limits in [
        source::Limits {
            file_bytes: program_bytes as u64 - 1,
            ..source::Limits::default()
        },
        source::Limits {
            total_bytes: baseline.site().output().inventory().total_bytes() - 1,
            ..source::Limits::default()
        },
        source::Limits {
            entries: baseline.site().output().inventory().entries().len() - 1,
            ..source::Limits::default()
        },
    ] {
        let error = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                graph: None,
                model_output: Some(&"model.json".parse().expect("model")),
                limits,
            },
            Default::default(),
        )
        .expect_err("complete output includes generated resource bytes and directories");
        assert!(
            matches!(error, build::PublicationRenderError::Site(_)),
            "{error}"
        );
    }
}

#[test]
fn capability_metadata_budget_counts_each_provider_binding_independently() {
    let manifest = PLUGIN_MANIFEST.replace(
        "implements=['publication.search/v1']",
        "implements=['publication.search/v1','commerce.selector/v1']",
    );
    let fixture = Fixture::new(&[("theme/plugin.toml", manifest.as_bytes())]);
    let config = fixture.configuration("optional");
    let one = acquire(&config);
    let two = acquire(&config.replace("[composition.resources]", "'commerce.selector/v1'={state='enabled',provider={kind='plugin',package='publication-capabilities',provider='search'},requirement='optional',allow_absence='unsupported'}\n[composition.resources]"));
    let render_at = |inputs: &build::AcquiredInputs, budget| {
        build::render_publication(
            inputs,
            build::SiteRenderOptions {
                render_year: None,
                graph: None,
                model_output: Some(&"model.json".parse().expect("model")),
                limits: source::Limits::default(),
            },
            build::PublicationRenderLimits {
                output: build::PublicationOutputLimits {
                    resource_metadata_bytes: budget,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .map(|_| ())
    };
    let mut lower = 0;
    let mut upper = build::PublicationOutputLimits::default().resource_metadata_bytes;
    render_at(&one, upper).expect("one capability under generous default");
    render_at(&two, upper).expect("two provider bindings supported under generous default");
    // The equal-length alias sorts after the original, so the first capability
    // still fits: rejection must account for additional facts, not a longer ID.
    // Find the native acceptance boundary without buffering or duplicating its
    // evidence serialization; each observation runs the actual compiler pipeline.
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        match render_at(&one, middle) {
            Ok(()) => upper = middle,
            Err(error) => {
                assert_eq!(
                    error.code(),
                    "resource_limit",
                    "only the selected metadata ceiling may fail: {error}"
                );
                lower = middle + 1;
            }
        }
    }
    assert!(upper > 0, "real capability facts have nonzero cost");
    render_at(&one, upper).expect("exact single-capability metadata boundary");
    assert_eq!(
        render_at(&one, upper - 1)
            .expect_err("one byte under boundary")
            .code(),
        "resource_limit"
    );
    assert_eq!(
        render_at(&two, upper)
            .expect_err("one resource does not make a second capability binding free")
            .code(),
        "resource_limit"
    );
}
