//! Mixed retained resources reach one local publication model and checked output tree.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const THEME_MANIFEST: &str = r#"package='theme:one'
namespace='fixture'
templates='templates'
browser_theme_colors={light='#ffffff',dark='#000000'}
render={page='page.html',alias='alias.html'}
resource_requirements=['module:script','style:palette']
"#;
const PLUGIN_MANIFEST: &str = r#"package='publication-resources'
[resources.'module:script']
inputs=['main.ts']
build={kind='browser-module',entry='main.ts'}
[resources.'style:palette']
inputs=['main.scss','_tokens.scss']
build={kind='stylesheet',entry='main.scss'}
"#;
const SCRIPT: &str = "export const answer: number = 42;\n";
const STYLE: &str =
    "@use 'tokens';\n.card { color: tokens.$ink; .label { padding: tokens.$space; } }\n";
const TOKENS: &str = "$ink: #123456;\n$space: 7px;\n";
const PAGE: &str = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Mixed resources'\nlifecycle='published'\nroute={path='/',aliases=['/old.html']}\nstructure={kind='page',family={kind='article'}}\n+++\n# Local publication\n";
const TEMPLATE: &str = r#"{{ body_html | safe }}{% for resource in presentation.theme_resources %}{% if resource.kind == "stylesheet" %}<link rel="stylesheet" href="{{ resource.url }}">{% elif resource.kind == "browser-module" %}<script type="module" src="{{ resource.url }}"></script>{% endif %}{% endfor %}"#;

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(overrides: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("isolated native source");
        let repository = Repository::init(root.path()).expect("fixture Git");
        for (path, bytes) in [
            ("theme/theme.toml", THEME_MANIFEST.as_bytes()),
            ("theme/plugin.toml", PLUGIN_MANIFEST.as_bytes()),
            ("theme/templates/page.html", TEMPLATE.as_bytes()),
            ("theme/templates/alias.html", b"Moved".as_slice()),
            ("theme/main.ts", SCRIPT.as_bytes()),
            ("theme/main.scss", STYLE.as_bytes()),
            ("theme/_tokens.scss", TOKENS.as_bytes()),
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
            .expect("exact source set");
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
                "styles fixture",
                &tree,
                &[],
            )
            .expect("isolated commit");
        Self { root, commit }
    }

    fn config(&self) -> String {
        // The stylesheet route deliberately has no .css suffix. This proves
        // local typed consumption, not a deployed server's MIME configuration.
        let mut config = "[composition]\ntheme={package='theme:one'}\n[composition.plugins]\n'publication-resources'={state='enabled'}\n[composition.resources]\n'module:script'={state='enabled',resource={owner={kind='plugin',package='publication-resources'},resource='module:script'},output='/assets/script.js',use='site'}\n'style:palette'={state='enabled',resource={owner={kind='plugin',package='publication-resources'},resource='style:palette'},output='/assets/palette',use='site'}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\nauthored=[{source='source:content',target='authored'}]\n".to_owned();
        for name in ["theme", "content"] {
            config.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{name}'}}\n", self.root.path().to_str().expect("root"), self.commit));
        }
        config.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n[[plugin_packages]]\npackage='publication-resources'\nsource='source:theme'\nmanifest='plugin.toml'\n");
        config
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    acquire_result(config).expect("complete selected capture")
}

fn acquire_result(config: &str) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_publication(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 2,
            entries: 64,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
}

fn render(
    inputs: &build::AcquiredInputs,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
}

fn bytes<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("expected output")
}

fn cli(config: &str, parent: &Path) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "render-site",
            "--render-year",
            "2040",
            "--local",
            "--site",
            "site:one",
            "--model-output",
            "model.json",
            "--output-parent",
            parent.to_str().expect("output parent"),
        ])
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
        .expect("configuration transport");
    process.wait_with_output().expect("CLI completion")
}

fn assert_failure(config: &str, code: &str) {
    let actual = match acquire_result(config) {
        Ok(inputs) => render(&inputs)
            .expect_err("no partial successful publication")
            .code(),
        Err(error) => error.code(),
    };
    assert!(
        actual.starts_with(code),
        "expected {code}, received {actual}"
    );
    let parent = tempdir().expect("explicit output parent");
    fs::write(parent.path().join("unrelated.txt"), b"preserve").expect("unrelated bytes");
    let before = source::inspect(parent.path(), None, source::Limits::default()).expect("before");
    let output = cli(config, parent.path());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no success receipt");
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with(&format!("gazette: {code}")),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("panicked"),
        "a rejected resource must return a diagnostic, not a compiler panic"
    );
    let after = source::inspect(parent.path(), None, source::Limits::default()).expect("after");
    assert_eq!(
        after.identity(),
        before.identity(),
        "no stage or unrelated mutation"
    );
}

#[test]
fn optional_site_resources_reach_the_theme_without_becoming_requirements() {
    let manifest = THEME_MANIFEST.replace(
        "resource_requirements=['module:script','style:palette']",
        "resource_requirements=[]",
    );
    let fixture = Fixture::new(&[("theme/theme.toml", manifest.as_bytes())]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let built = render(&inputs).expect("optional site resources");
    let tree = built.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let presentation = &model["pages"]["content:one"]["presentation"];
    assert_eq!(presentation["theme_resources"][0]["id"], "module:script");
    assert_eq!(presentation["theme_resources"][1]["id"], "style:palette");
    assert_eq!(
        presentation["browser_modules"]["preloads"][0]["url"],
        "https://one.example/mount/assets/script.js"
    );
    let html = String::from_utf8(bytes(tree, "index.html").to_vec()).unwrap();
    assert_eq!(html.matches("<script type=\"module\"").count(), 1);
    let dependency = config.replacen("use='site'", "use='dependency'", 1);
    let inputs = acquire(&dependency);
    let built = render(&inputs).expect("unused dependency is not a site contribution");
    let tree = built.site().output();
    assert!(tree.file(&"assets/script.js".parse().unwrap()).is_none());
    assert!(
        !String::from_utf8(bytes(tree, "index.html").to_vec())
            .unwrap()
            .contains("<script")
    );
}

#[test]
fn mixed_resources_share_kind_and_url_through_theme_cli_and_retained_model() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    assert_eq!(inputs.sources().len(), 2);
    assert!(!fixture.root.path().join("workspace").exists());
    let built = render(&inputs).expect("mixed native compilers");
    let tree = built.site().output();
    let css = bytes(tree, "assets/palette");
    let css_text = std::str::from_utf8(css).expect("CSS");
    let compact: String = css_text.chars().filter(|c| !c.is_whitespace()).collect();
    let compact = compact.replace(";}", "}");
    assert!(compact.contains(".card{color:#123456}"), "{css_text}");
    assert!(compact.contains(".card.label{padding:7px}"), "{css_text}");
    assert!(!css_text.contains("@use"));
    assert!(!css_text.contains("$ink"));
    assert_ne!(css, STYLE.as_bytes(), "not an uncompiled copy");
    let javascript = bytes(tree, "assets/script.js");
    assert!(
        !std::str::from_utf8(javascript)
            .expect("JavaScript")
            .contains(": number")
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("local model");
    for (index, key, kind, output) in [
        (1, "style:palette", "stylesheet", "assets/palette"),
        (0, "module:script", "browser-module", "assets/script.js"),
    ] {
        let payload = bytes(tree, output);
        let expected = json!({"kind":kind,"url":format!("https://one.example/mount/{output}"),"output":output,"sha256":source::Sha256Digest::of(payload),"bytes":payload.len()});
        assert_eq!(model["resources"][key], expected);
        assert_eq!(
            model["pages"]["content:one"]["presentation"]["theme_resources"][index],
            json!({"id":key, "kind":kind,"url":format!("https://one.example/mount/{output}"),"output":output,"sha256":source::Sha256Digest::of(payload),"bytes":payload.len()})
        );
    }
    let html = std::str::from_utf8(bytes(tree, "index.html"))
        .expect("HTML")
        .replace("&#x2F;", "/");
    assert!(
        html.contains(
            "<link rel=\"stylesheet\" href=\"https://one.example/mount/assets/palette\">"
        )
    );
    assert!(html.contains(
        "<script type=\"module\" src=\"https://one.example/mount/assets/script.js\"></script>"
    ));
    assert_eq!(html.matches("<link ").count(), 1);
    assert_eq!(html.matches("<script ").count(), 1);
    let public = std::str::from_utf8(bytes(tree, "model.json")).expect("public JSON");
    for private in [
        fixture.root.path().to_str().expect("root"),
        "main.scss",
        "_tokens.scss",
        "theme.toml",
        "plugin.toml",
        "source:theme",
    ] {
        assert!(
            !public.contains(private),
            "private compilation facts: {private}"
        );
    }

    let parent = tempdir().expect("local retained output");
    fs::write(parent.path().join("unrelated.txt"), b"preserve").expect("unrelated bytes");
    for _ in 0..2 {
        let output = cli(&config, parent.path());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
        let retained = Path::new(receipt["directory"].as_str().expect("retained directory"));
        assert_eq!(
            source::inspect(retained, None, source::Limits::default())
                .expect("read-back")
                .identity(),
            tree.inventory().identity()
        );
        for entry in tree.inventory().entries() {
            if matches!(entry.content, source::Content::File { .. }) {
                assert_eq!(
                    fs::read(retained.join(entry.path.as_str())).expect("retained bytes"),
                    tree.file(&entry.path).expect("SDK bytes")
                );
            }
        }
        let evidence = &receipt["evidence"]["resources"]["style:palette"];
        assert_eq!(evidence["source"], "source:theme");
        assert_eq!(evidence["resource"]["owner"]["kind"], "plugin");
        assert_eq!(
            evidence["resource"]["owner"]["package"],
            "publication-resources"
        );
        assert_eq!(evidence["resource"]["resource"], "style:palette");
        assert_eq!(evidence["use"], "site");
        assert_eq!(
            evidence["recipe"],
            json!({"kind":"stylesheet","entry":"main.scss"})
        );
        assert_eq!(
            evidence["manifest_sha256"],
            json!(source::Sha256Digest::of(PLUGIN_MANIFEST.as_bytes()))
        );
        assert_eq!(
            evidence["output_sha256"],
            json!(source::Sha256Digest::of(css))
        );
        assert_eq!(
            evidence["dependencies"],
            json!({"main.scss":source::Sha256Digest::of(STYLE.as_bytes()),"_tokens.scss":source::Sha256Digest::of(TOKENS.as_bytes())})
        );
        assert!(
            !evidence["tool"]
                .as_str()
                .expect("compiler identity")
                .is_empty()
        );
        assert_eq!(
            receipt["evidence"]["resources"]["module:script"]["dependencies"],
            json!({"main.ts":source::Sha256Digest::of(SCRIPT.as_bytes())})
        );
    }
    assert_eq!(
        fs::read(parent.path().join("unrelated.txt")).expect("unrelated retained"),
        b"preserve"
    );
    fs::write(
        fixture.root.path().join("theme/_tokens.scss"),
        b"invalid after capture",
    )
    .expect("isolated source change");
    fs::write(
        fixture.root.path().join("theme/main.scss"),
        b"@use 'outside';",
    )
    .expect("isolated source change");
    fs::write(
        fixture.root.path().join("theme/main.ts"),
        b"invalid after capture",
    )
    .expect("isolated source change");
    assert_eq!(
        render(&inputs)
            .expect("immutable captured inputs")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    let before = source::inspect(parent.path(), None, source::Limits::default())
        .expect("before stale acquisition");
    let output = cli(&config, parent.path());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source_content_mismatch"));
    assert_eq!(
        source::inspect(parent.path(), None, source::Limits::default())
            .expect("after stale acquisition")
            .identity(),
        before.identity()
    );
}

#[test]
fn invalid_missing_or_escaping_selected_styles_fail_before_staging() {
    for style in [
        ".card { color: $missing; }",
        "@use 'missing';",
        "@use '../content/outside';",
        "@use '/outside';",
        "@import 'https://example.invalid/external.css';",
        ".card { background: url('https://example.invalid/image.png'); }",
    ] {
        let fixture = Fixture::new(&[("theme/main.scss", style.as_bytes())]);
        assert_failure(&fixture.config(), "resource_");
    }
    // The entry is valid: these errors occur only while reading its actual
    // imported partial, exercising the resource compiler's transitive-read boundary.
    let manifest = PLUGIN_MANIFEST.replace(
        "inputs=['main.scss','_tokens.scss']",
        "inputs=['main.scss','_tokens.scss','_palette.scss']",
    );
    for tokens in [
        b"@use 'palette.scss?revision=1' as palette;\n$ink: palette.$ink;\n$space: palette.$space;\n".as_slice(),
        b".broken {".as_slice(),
        b"\xff".as_slice(),
    ] {
        let fixture = Fixture::new(&[
            ("theme/plugin.toml", manifest.as_bytes()),
            ("theme/_tokens.scss", tokens),
            ("theme/_palette.scss", TOKENS.as_bytes()),
        ]);
        assert_failure(&fixture.config(), "resource_");
    }
}

#[test]
fn captured_but_undeclared_style_dependency_is_not_an_implicit_resource_input() {
    let manifest = PLUGIN_MANIFEST.replace(
        "inputs=['main.scss','_tokens.scss']",
        "inputs=['main.scss']",
    );
    let fixture = Fixture::new(&[("theme/plugin.toml", manifest.as_bytes())]);
    assert_failure(&fixture.config(), "resource_");
}

#[test]
fn selected_style_without_recipe_or_package_export_is_not_silently_omitted() {
    let manifest = PLUGIN_MANIFEST.replace("build={kind='stylesheet',entry='main.scss'}\n", "");
    let fixture = Fixture::new(&[("theme/plugin.toml", manifest.as_bytes())]);
    assert_failure(&fixture.config(), "toml_invalid");
    let fixture = Fixture::new(&[]);
    assert_failure(
        &fixture
            .config()
            .replace("resource='style:palette'", "resource='style:missing'"),
        "package_selection",
    );
}

#[test]
fn stylesheet_and_other_outputs_share_one_collision_owner() {
    let fixture = Fixture::new(&[]);
    for output in ["/index.html", "/old.html", "/model.json"] {
        assert_failure(
            &fixture
                .config()
                .replace("output='/assets/palette'", &format!("output='{output}'")),
            "source_inventory",
        );
    }
    assert_failure(
        &fixture
            .config()
            .replace("output='/assets/palette'", "output='/assets/script.js'"),
        "resource_input",
    );
    let fixture = Fixture::new(&[("content/download.txt", b"exact static bytes")]);
    let config = fixture
        .config()
        .replace(
            "target='authored'",
            "target='authored',static_assets={route_prefix='/assets/'}",
        )
        .replace("output='/assets/palette'", "output='/assets/download.txt'");
    assert_failure(&config, "source_inventory");
}

#[test]
fn stylesheet_urls_bind_selected_assets_through_theme_model_and_cli() {
    use base64::Engine as _;
    let image = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==").expect("PNG");
    let style = ".card { background-image: url('https://one.example/mount/images/pixel.png'); }";
    let manifest = format!(
        "{PLUGIN_MANIFEST}[resources.'image:pixel']\ninputs=['pixel.png']\nbuild={{kind='raster-image',entry='pixel.png',input_format='png',resize={{operation='scale',width=1,height=1}},encoding={{format='png'}},apply_orientation=true}}\n"
    );
    let template = format!(
        "{TEMPLATE}{{% for reference, target in presentation.theme_resources[1].references %}}<i>{{{{ reference }}}}:{{{{ target }}}}</i>{{% endfor %}}"
    );
    let fixture = Fixture::new(&[
        ("theme/plugin.toml", manifest.as_bytes()),
        ("theme/main.scss", style.as_bytes()),
        ("theme/pixel.png", &image),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = fixture.config().replace(
        "[[principals]]",
        "'image:pixel'={state='enabled',resource={owner={kind='plugin',package='publication-resources'},resource='image:pixel'},output='/images/pixel.png',use='site'}\n[[principals]]",
    );
    let inputs = acquire(&config);
    let built = render(&inputs).expect("selected retained CSS resource");
    let tree = built.site().output();
    assert!(!bytes(tree, "images/pixel.png").is_empty());
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    let references = json!({"https://one.example/mount/images/pixel.png":"images/pixel.png"});
    assert_eq!(
        model["resources"]["style:palette"]["references"],
        references
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["theme_resources"][1]["references"],
        references
    );
    let html = std::str::from_utf8(bytes(tree, "index.html"))
        .expect("HTML")
        .replace("&#x2F;", "/");
    assert!(html.contains("https://one.example/mount/images/pixel.png:images/pixel.png"));
    assert!(
        std::str::from_utf8(bytes(tree, "assets/palette"))
            .expect("CSS")
            .contains("https://one.example/mount/images/pixel.png")
    );
    let parent = tempdir().expect("local stage");
    let output = cli(&config, parent.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
    assert_eq!(
        source::inspect(
            Path::new(receipt["directory"].as_str().expect("stage")),
            None,
            source::Limits::default()
        )
        .expect("read-back")
        .identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        fs::read(fixture.root.path().join("theme/main.scss")).expect("unchanged source"),
        style.as_bytes()
    );
    fs::write(
        fixture.root.path().join("theme/pixel.png"),
        b"changed after capture",
    )
    .expect("fixture mutation");
    assert_eq!(
        render(&inputs)
            .expect("retained rebuild")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
}

#[test]
fn missing_stylesheet_targets_fail_before_any_output_is_staged() {
    let fixture = Fixture::new(&[(
        "theme/main.scss",
        b".card { background: url('../images/missing.png'); }",
    )]);
    assert_failure(&fixture.config(), "publication_resource_binding");
}
