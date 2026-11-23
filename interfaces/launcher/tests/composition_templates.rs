//! Captured composition templates compile without rereading or publishing sources.

#[path = "composition_templates/mcp.rs"]
mod mcp;
#[path = "support/mcp.rs"]
mod mcp_carrier;

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{authored, build, corpus, render, source, toml, workspace};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const MANIFEST: &str = "package='theme:base'\nnamespace='base'\ntemplates='presentation/templates'\nrender={page='page.html'}\npalettes=[{id='palette:light',browser_theme_colors={light='#ffffff',dark='#000000'}}]\n";
const PLUGIN_MANIFEST: &str = "package='search-plugin'\n[resources.'module:search']\ninputs=['search.js']\nbuild={kind='browser-module',entry='search.js'}\n[providers.search]\nimplements=['publication.search/v1']\n";
const BASE: &[u8] = b"<main>{% block body %}base{% endblock body %}</main>";
const PAGE: &[u8] = b"{% extends 'base/base.html' %}{% block body %}{{ super() }}:{% include 'base/parts/title.html' %}{% endblock body %}";
const TITLE: &[u8] = b"<h1>{{ title }}</h1>";

const CONTENT_TEMPLATE: &str = concat!(
    "<main data-id=\"{{ content.id }}\" data-state=\"{{ content.lifecycle }}\">",
    "<h1>{{ content.title }}</h1>{{ body_html | safe }}",
    "{% for step in steps %}<span>{{ step.id }}:{{ step.lifecycle }}</span>{% endfor %}",
    "{% for child in children %}<i>{{ child.id }}</i>{% endfor %}",
    "{% for link in projects %}<b>{{ link.project.id }}:{{ link.relationship.relation }}:",
    "{{ link.project.visibility }}:{{ link.project.availability }}</b>{% endfor %}</main>"
);

fn authored_files() -> Vec<(&'static str, String)> {
    let node = |id, lifecycle, relations, structure, body| {
        format!(
            "+++\n{relations}\n[content]\nid='{id}'\nsite='site:one'\ntitle='Shared <title>'\nlifecycle='{lifecycle}'\n[content.structure]\n{structure}\n+++\n{body}\n"
        )
    };
    vec![
        (
            "collection.md",
            node(
                "collection:one",
                "published",
                "projects=[{project='project:one',kind='context'}]",
                "kind='collection'",
                "# Collection body",
            ),
        ),
        (
            "journey.md",
            node(
                "journey:one",
                "published",
                "",
                "kind='journey'\ncollection='collection:one'\nmembers=[{content='docs:one',primary=true},{content='article:one',primary=true}]",
                "# Journey body",
            ),
        ),
        (
            "article.md",
            node(
                "article:one",
                "draft",
                "related=['docs:one']\nprojects=[{project='project:one',kind='about'}]",
                "kind='page'\nfamily={kind='article'}",
                "**Article** body",
            ),
        ),
        (
            "docs.md",
            node(
                "docs:one",
                "archived",
                "",
                "kind='page'\nfamily={kind='documentation'}",
                "`Code` documentation",
            ),
        ),
    ]
}

fn authored_config(fixture: &Fixture) -> String {
    format!(
        "{}\n[[projects]]\nid='project:one'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n",
        fixture.config()
    )
}

fn corpus<'a>(files: &'a [(&str, String)]) -> corpus::ParsedCorpus<'a> {
    corpus::ParsedCorpus::parse(
        files.iter().map(|(path, text)| corpus::Input {
            path: path.parse().expect("logical path"),
            bytes: text.as_bytes(),
        }),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("retained corpus")
}

#[test]
fn real_theme_consumes_the_compilers_brand_bound_route_without_rejoining_it() {
    let fixture = Fixture::new(&[(
        "presentation/templates/route.html",
        b"<link rel=\"canonical\" href=\"{{ route.canonical_url }}\"><p>{{ content.id }}</p>",
    )]);
    let config = format!(
        "{}\n[[brands]]\nid='brand:one'\nname='Not a domain'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://EXAMPLE.test:443/mount/'\n",
        authored_config(&fixture)
    );
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("metadata");
    let compiled =
        build::compile_templates(&metadata, render::Limits::default()).expect("templates");
    let mut files = authored_files();
    files[2].1 = files[2].1.replace(
        "[content.structure]",
        "route={path='/caf%C3%A9/',aliases=['/old.html']}\n[content.structure]",
    );
    let parsed = corpus(&files);
    let resolved = authored::resolve(acquired.compilation(), &parsed, Default::default())
        .expect("resolved input");
    let id = "article:one".parse().expect("identity");
    let page = compiled
        .render_page(
            &resolved,
            &id,
            &"base/route.html".parse().expect("template"),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("connected rendering");
    let route = resolved.content().route(&id).expect("compiler route");
    assert_eq!(route.canonical_url, "https://example.test/mount/caf%C3%A9/");
    assert_eq!(
        page.model()["route"],
        serde_json::to_value(route).expect("typed model")
    );
    assert_eq!(
        page.html(),
        "<link rel=\"canonical\" href=\"https:&#x2F;&#x2F;example.test&#x2F;mount&#x2F;caf%C3%A9&#x2F;\"><p>article:one</p>"
    );
    // Decode this fixture's only HTML entity, without weakening template escaping.
    let attribute = page
        .html()
        .split("href=\"")
        .nth(1)
        .expect("href")
        .split('"')
        .next()
        .expect("attribute");
    assert_eq!(attribute.replace("&#x2F;", "/"), route.canonical_url);
    assert_eq!(route.output.as_str(), "café/index.html");
    assert_eq!(route.aliases[0].output.as_str(), "old.html");
    assert_eq!(page.model()["content"]["lifecycle"], "draft");
    assert!(page.model().get("source").is_none());
}

#[test]
fn real_selected_theme_renders_all_authored_kinds_from_one_model() {
    let fixture = Fixture::new(&[(
        "presentation/templates/content.html",
        CONTENT_TEMPLATE.as_bytes(),
    )]);
    let before = fixture.snapshot();
    let config = authored_config(&fixture);
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("metadata");
    let compiled = build::compile_templates(&metadata, render::Limits::default()).expect("compile");
    let files = authored_files();
    let parsed = corpus(&files);
    let resolved = authored::resolve(acquired.compilation(), &parsed, Default::default())
        .expect("shared model");
    let entry = "base/content.html".parse().expect("template");
    for id in ["collection:one", "journey:one", "article:one", "docs:one"] {
        let page = compiled
            .render_page(
                &resolved,
                &id.parse().expect("id"),
                &entry,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("connected rendering");
        assert_eq!(page.model()["content"]["id"], id);
        assert!(page.html().contains(&format!("data-id=\"{id}\"")));
        assert!(page.html().contains("Shared &lt;title&gt;"));
        assert!(
            page.html()
                .contains(page.model()["body_html"].as_str().expect("body"))
        );
        assert_eq!(
            page.evidence().model_sha256,
            source::Sha256Digest::of(&serde_json::to_vec(page.model()).expect("model bytes"))
        );
        assert_eq!(
            page.evidence().html_sha256,
            source::Sha256Digest::of(page.html().as_bytes())
        );
        assert_eq!(
            page.model()["site"],
            json!({
                "id":"site:one",
                "publisher":"owner:one",
                "publisher_principal":{"id":"owner:one","kind":"person"},
                "roles":[],
                "presentation":{
                    "theme":"theme:base",
                    "palette":"palette:light"
                }
            })
        );
        assert_eq!(
            page.model()["presentation"],
            json!({
                "surface":{"kind":"page"},
                "theme":"theme:base",
                "palette":"palette:light",
                "browser_theme_colors":{"light":"#ffffff","dark":"#000000"},
                "capabilities":{},
                "resources":[],
                "theme_resources":[]
            })
        );
        for absent in [
            "source",
            "source_location",
            "configuration",
            "authors",
            "canonical_url",
            "rights",
        ] {
            assert!(
                page.model().get(absent).is_none(),
                "private or invented field {absent}"
            );
        }
        assert!(
            !serde_json::to_string(page.model())
                .expect("json")
                .contains(fixture.root.path().to_str().expect("root"))
        );
    }
    let journey = compiled
        .render_page(
            &resolved,
            &"journey:one".parse().expect("id"),
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("journey");
    assert_eq!(journey.model()["steps"][0]["id"], "docs:one");
    assert_eq!(journey.model()["steps"][1]["id"], "article:one");
    assert!(
        journey
            .html()
            .contains("<span>docs:one:archived</span><span>article:one:draft</span>")
    );
    let article = compiled
        .render_page(
            &resolved,
            &"article:one".parse().expect("id"),
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("article");
    assert_eq!(
        article.model()["projects"][0]["relationship"]["relation"],
        "about"
    );
    assert_eq!(
        article.model()["projects"][0]["relationship"]["evidence"]
            .as_array()
            .expect("all origins")
            .len(),
        2
    );
    assert!(
        article
            .html()
            .contains("project:one:about:private:unavailable")
    );
    let docs = compiled
        .render_page(
            &resolved,
            &"docs:one".parse().expect("id"),
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("docs");
    assert!(
        docs.html()
            .contains("project:one:mentions:private:unavailable")
    );
    let collection = compiled
        .render_page(
            &resolved,
            &"collection:one".parse().expect("id"),
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("collection");
    assert_eq!(
        collection.model()["project_declarations"][0]["declaration"]["kind"],
        "context"
    );
    assert_eq!(collection.model()["projects"], json!([]));
    assert_eq!(fixture.snapshot(), before);
    fs::write(
        fixture
            .root
            .path()
            .join("selected/presentation/templates/content.html"),
        "{% if %}",
    )
    .expect("native edit");
    let mut reversed = authored_files();
    reversed.reverse();
    let reparsed = corpus(&reversed);
    let reordered = authored::resolve(acquired.compilation(), &reparsed, Default::default())
        .expect("reordered");
    let again = compiled
        .render_page(
            &reordered,
            &"article:one".parse().expect("id"),
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("retained templates");
    assert_eq!(again.html(), article.html());
    assert_eq!(again.model(), article.model());
}

#[test]
fn authored_render_checks_configuration_and_home_before_body_or_template() {
    let fixture = Fixture::new(&[]);
    let config = authored_config(&fixture);
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("metadata");
    let compiled = build::compile_templates(&metadata, Default::default()).expect("compile");
    let mut files = authored_files();
    files[2].1.push_str("<script>unsafe()</script>\n");
    let parsed = corpus(&files);
    let different = workspace::compile(&format!("{config}\n# changed bytes"), Default::default())
        .expect("different config");
    let resolved = authored::resolve(&different, &parsed, Default::default()).expect("structure");
    let id = "article:one".parse().expect("id");
    let missing = "missing.html".parse().expect("entry");
    assert!(matches!(
        compiled.render_page(
            &resolved,
            &id,
            &missing,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak
        ),
        Err(build::PageRenderError::Configuration { .. })
    ));
    files[2].1 = files[2].1.replace("site='site:one'", "site='site:other'");
    // The render-home refusal needs a valid graph: related links may cross sites,
    // but moving the article cannot leave it contained in the local journey.
    files[1].1 = files[1]
        .1
        .replace(",{content='article:one',primary=true}", "");
    let parsed = corpus(&files);
    let resolved = authored::resolve(acquired.compilation(), &parsed, Default::default())
        .expect("other home valid");
    assert!(matches!(
        compiled.render_page(
            &resolved,
            &id,
            &missing,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak
        ),
        Err(build::PageRenderError::Site { .. })
    ));
    assert_eq!(
        compiled
            .render_page(
                &resolved,
                &"article:absent".parse().expect("id"),
                &missing,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("exact lookup")
            .code(),
        "authored_content_reference"
    );
}

#[test]
fn configured_listing_still_requires_source_bound_discovery_for_standalone_rendering() {
    let fixture = Fixture::new(&[(
        "presentation/templates/content.html",
        CONTENT_TEMPLATE.as_bytes(),
    )]);
    let config = format!(
        "{}\n[[brands]]\nid='brand:one'\nname='Listing publication'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n\
         listings=[{{content='collection:one',select={{kind='all-pages'}},order={{by='published',direction='ascending',undated='last-title-then-id'}},pagination={{page_size=10,next_pages='page/{{number}}/'}}}}]\n",
        authored_config(&fixture)
    );
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("metadata");
    let compiled = build::compile_templates(&metadata, Default::default()).expect("templates");
    let files = authored_files();
    let parsed = corpus(&files);
    let resolved = authored::resolve(acquired.compilation(), &parsed, Default::default())
        .expect("standalone structure");
    let id = "article:one".parse().unwrap();
    assert!(matches!(
        compiled.render_page(
            &resolved,
            &id,
            &"base/content.html".parse().unwrap(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        ),
        Err(build::PageRenderError::Projection(authored::ProjectionError::Source(
            authored::SourceBindingError::Missing(missing)
        ))) if missing == id
    ));
}

#[test]
fn authored_render_failure_and_exact_context_bounds_never_yield_partial_pages() {
    let fixture = Fixture::new(&[(
        "presentation/templates/content.html",
        CONTENT_TEMPLATE.as_bytes(),
    )]);
    let acquired = acquire(&authored_config(&fixture));
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("metadata");
    let compiled = build::compile_templates(&metadata, Default::default()).expect("compile");
    let files = authored_files();
    let parsed = corpus(&files);
    let resolved =
        authored::resolve(acquired.compilation(), &parsed, Default::default()).expect("structure");
    let id = "article:one".parse().expect("id");
    let entry = "base/content.html".parse().expect("entry");
    let page = compiled
        .render_page(
            &resolved,
            &id,
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("size reference");
    let size = serde_json::to_vec(page.model()).expect("bytes").len();
    for maximum in [0, size - 1, size] {
        let bounded = build::compile_templates(
            &metadata,
            render::Limits {
                context_bytes: maximum,
                ..Default::default()
            },
        )
        .expect("compile");
        let result = bounded.render_page(
            &resolved,
            &id,
            &entry,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        );
        if maximum == size {
            assert_eq!(result.expect("exact bound").html(), page.html());
        } else {
            assert_eq!(result.expect_err("context ceiling").code(), "render_limit");
        }
    }
    assert_eq!(
        compiled
            .render_page(
                &resolved,
                &id,
                &"missing.html".parse().expect("entry"),
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("missing template")
            .code(),
        "render_template"
    );
    let mut raw = authored_files();
    raw[2].1.push_str("<script>unsafe()</script>\n");
    let raw_corpus = corpus(&raw);
    let raw_resolved = authored::resolve(acquired.compilation(), &raw_corpus, Default::default())
        .expect("syntax not trust");
    assert_eq!(
        compiled
            .render_page(
                &raw_resolved,
                &id,
                &entry,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("raw html refused")
            .code(),
        "markdown_raw_html_unavailable"
    );
    assert_eq!(
        compiled
            .render_page(
                &resolved,
                &id,
                &entry,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak
            )
            .expect("reuse after error")
            .html(),
        page.html()
    );
}

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(extra: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("native fixture");
        let repository = Repository::init(root.path()).expect("fixture repository");
        let files = [
            ("meta/theme.toml", MANIFEST.as_bytes()),
            ("meta/plugin.toml", PLUGIN_MANIFEST.as_bytes()),
            ("presentation/templates/base.html", BASE),
            ("presentation/templates/page.html", PAGE),
            ("presentation/templates/parts/title.html", TITLE),
            (
                "search.js",
                b"{% if %} throw new Error('never execute me');".as_slice(),
            ),
            (
                "presentation/templates-neighbor/invalid.html",
                b"{% if %}".as_slice(),
            ),
            ("unselected.bin", b"\xff\xfe".as_slice()),
        ];
        for (path, bytes) in files.iter().chain(extra) {
            let path = root.path().join("selected").join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("directories");
            fs::write(path, bytes).expect("fixture payload");
        }
        let mut index = repository.index().expect("index");
        index
            .add_all(["selected"], IndexAddOption::DEFAULT, None)
            .expect("tracked source");
        let tree = repository
            .find_tree(index.write_tree().expect("write tree"))
            .expect("tree");
        let signature = Signature::new(
            "Gazette fixture",
            "fixture@example.invalid",
            &git2::Time::new(0, 0),
        )
        .expect("signature");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "template fixture",
                &tree,
                &[],
            )
            .expect("commit");
        Self { root, commit }
    }

    fn config(&self) -> String {
        format!(
            "[composition]\ntheme={{package='theme:base',palette='palette:light'}}\n[composition.plugins]\n'search-plugin'={{state='enabled'}}\n[composition.capabilities]\n'publication.search/v1'={{state='enabled',provider={{kind='plugin',package='search-plugin',provider='search'}},requirement='optional'}}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:theme'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='selected'}}\n[[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='meta/theme.toml'\n[[plugin_packages]]\npackage='search-plugin'\nsource='source:theme'\nmanifest='meta/plugin.toml'\n",
            self.root.path().to_str().expect("path"),
            self.commit
        )
    }

    fn snapshot(&self) -> source::Inventory {
        source::inspect(self.root.path(), None, source::Limits::default())
            .expect("whole native fixture including Git")
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_composition(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 2,
            entries: 64,
            bytes: 64 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("retained composition")
}

fn run(config: &str, flags: &[&str]) -> Output {
    let cwd = tempdir().expect("empty unrelated cwd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--stdin"])
        .args(flags)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI");
    let written = child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes());
    let output = child.wait_with_output().expect("completion");
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(!output.status.success());
    }
    assert!(
        fs::read_dir(cwd.path()).expect("cwd").next().is_none(),
        "compilation creates no artifacts, store or deployment"
    );
    output
}

fn check(config: &str) -> Output {
    run(
        config,
        &["--acquire-composition", "site:one", "--compile-templates"],
    )
}

fn rejected<T, E>(result: Result<T, E>) -> E {
    match result {
        Err(error) => error,
        Ok(_) => panic!("invalid input returned successful compiled templates"),
    }
}

#[test]
fn nested_templates_compile_and_render_with_sdk_cli_evidence_parity_without_effects() {
    let fixture = Fixture::new(&[]);
    let before = fixture.snapshot();
    let config = fixture.config();
    let acquired = acquire(&config);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("metadata");
    let compiled = build::compile_templates(&metadata, render::Limits::default())
        .expect("selected templates only");
    let evidence = compiled.evidence();
    assert_eq!(evidence.tool, "tera@1.20.1/gazette-captured-templates-v8");
    assert_eq!(evidence.limits, render::Limits::default());
    let source_key = "source:theme".parse().expect("source identity");
    assert_eq!(
        evidence.configuration_sha256,
        acquired.compilation().source_sha256
    );
    assert_eq!(evidence.site, "site:one".parse().expect("site identity"));
    assert_eq!(
        evidence.theme,
        "theme:base".parse().expect("theme identity")
    );
    assert_eq!(
        evidence.palette,
        Some("palette:light".parse().expect("palette identity"))
    );
    assert_eq!(evidence.packages.len(), 1);
    let package = &evidence.packages[0];
    assert_eq!(package.package, evidence.theme);
    assert_eq!(package.namespace.as_str(), "base");
    assert_eq!(package.source, source_key);
    assert_eq!(
        package.source_tree,
        acquired.sources()[&source_key].inventory().identity()
    );
    assert_eq!(
        package.manifest,
        "meta/theme.toml".parse().expect("manifest path")
    );
    assert_eq!(
        package.manifest_sha256,
        metadata
            .inspect()
            .theme_packages
            .values()
            .next()
            .expect("selected Theme package")
            .manifest_sha256
    );
    assert_eq!(
        package.templates,
        "presentation/templates"
            .parse()
            .expect("template directory")
    );
    let native_templates = source::inspect(
        &fixture.root.path().join("selected/presentation/templates"),
        None,
        source::Limits::default(),
    )
    .expect("independent selected subtree identity");
    assert_ne!(
        evidence.template_tree,
        native_templates.identity(),
        "the virtual namespace includes the explicit Theme package mount"
    );
    assert_eq!(
        package
            .mounted_files
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["base/base.html", "base/page.html", "base/parts/title.html"]
    );
    assert_ne!(evidence.template_tree, package.source_tree);
    assert_eq!(
        serde_json::to_value(&package.source_location).expect("source location"),
        serde_json::to_value(
            &acquired
                .compilation()
                .workspace
                .source(&source_key)
                .expect("configured source")
                .location
        )
        .expect("configuration location")
    );
    let entry = "base/page.html".parse().expect("entry");
    assert_eq!(
        compiled
            .render(&entry, &json!({"title":"नमस्ते <tag>"}))
            .expect("inheritance and nested include"),
        "<main>base:<h1>नमस्ते &lt;tag&gt;</h1></main>".as_bytes()
    );
    for excluded in [
        "presentation/templates/page.html",
        "search.js",
        "meta/theme.toml",
        "meta/plugin.toml",
        "presentation/templates-neighbor/invalid.html",
        "unselected.bin",
    ] {
        assert!(
            compiled
                .render(&excluded.parse().expect("valid logical path"), &json!({}))
                .is_err(),
            "outside subtree was compiled: {excluded}"
        );
    }
    let expected = serde_json::to_value(compiled.evidence()).expect("typed evidence");
    let output = check(&config);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI evidence"),
        expected
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn retained_templates_and_evidence_survive_live_changes_without_rereading() {
    let fixture = Fixture::new(&[]);
    let acquired = acquire(&fixture.config());
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("metadata");
    let before =
        build::compile_templates(&metadata, render::Limits::default()).expect("initial compile");
    let expected = serde_json::to_value(before.evidence()).expect("evidence");
    fs::write(
        fixture
            .root
            .path()
            .join("selected/presentation/templates/page.html"),
        b"{% if %}",
    )
    .expect("owner changes live template");
    fs::write(
        fixture.root.path().join("selected/meta/theme.toml"),
        b"not valid metadata",
    )
    .expect("owner changes live metadata");
    let changed = fixture.snapshot();
    let after =
        build::compile_templates(&metadata, render::Limits::default()).expect("retained compile");
    assert_eq!(
        serde_json::to_value(after.evidence()).expect("evidence"),
        expected
    );
    assert_eq!(
        after
            .render(
                &"base/page.html".parse().expect("entry"),
                &json!({"title":"retained"})
            )
            .expect("retained render"),
        b"<main>base:<h1>retained</h1></main>"
    );
    assert_eq!(fixture.snapshot(), changed);
    assert!(
        !check(&fixture.config()).status.success(),
        "fresh acquisition must not ignore changed native bytes"
    );
}

#[test]
fn every_selected_regular_file_is_parsed_even_without_html_suffix() {
    for path in [
        "presentation/templates/unused.html",
        "presentation/templates/unused.txt",
        "presentation/templates/unused",
    ] {
        let fixture = Fixture::new(&[(path, b"{% if %}")]);
        let before = fixture.snapshot();
        let acquired = acquire(&fixture.config());
        let metadata = acquired
            .check_metadata(toml::Limits::default())
            .expect("metadata does not parse templates");
        let error = rejected(build::compile_templates(
            &metadata,
            render::Limits::default(),
        ));
        assert_eq!(error.code(), "render_template");
        let output = check(&fixture.config());
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "no partial successful evidence");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("template 'base/unused"), "{diagnostic}");
        assert!(diagnostic.contains("line 1, column"), "{diagnostic}");
        assert!(diagnostic.contains("expected"), "{diagnostic}");
        assert_eq!(fixture.snapshot(), before);
    }
}

#[test]
fn selected_invalid_utf8_and_invalid_metadata_paths_fail_without_fallback() {
    let fixture = Fixture::new(&[("presentation/templates/invalid.txt", b"\xff")]);
    let acquired = acquire(&fixture.config());
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("metadata accepts retained file");
    let error = rejected(build::compile_templates(
        &metadata,
        render::Limits::default(),
    ));
    assert_eq!(error.code(), "render_input");
    assert!(error.to_string().contains("UTF-8"), "{error}");
    for path in [
        "../templates",
        "/templates",
        "presentation//templates",
        "presentation/./templates",
    ] {
        let manifest = MANIFEST.replace("presentation/templates", path);
        let fixture = Fixture::new(&[("meta/theme.toml", manifest.as_bytes())]);
        let output = check(&fixture.config());
        assert!(
            !output.status.success(),
            "invalid manifest path accepted: {path}"
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn template_and_render_limits_fail_without_partial_results() {
    let fixture = Fixture::new(&[]);
    let acquired = acquire(&fixture.config());
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("metadata");
    let defaults = render::Limits::default();
    let exact = render::Limits {
        templates: source::Limits {
            entries: 5,
            depth: 3,
            file_bytes: PAGE.len() as u64,
            total_bytes: (BASE.len() + PAGE.len() + TITLE.len()) as u64,
        },
        ..defaults
    };
    let bounded = build::compile_templates(&metadata, exact)
        .expect("limits count only three templates and their relative parts directory");
    assert_eq!(bounded.evidence().limits, exact);
    assert_eq!(
        serde_json::to_value(bounded.evidence()).expect("bounded compilation evidence")["limits"],
        serde_json::to_value(exact).expect("exact requested limits")
    );
    assert_eq!(
        bounded
            .render(
                &"base/page.html".parse().expect("entry"),
                &json!({"title":"exact"})
            )
            .expect("exact resource boundary"),
        b"<main>base:<h1>exact</h1></main>"
    );
    for limits in [
        source::Limits {
            entries: 0,
            ..defaults.templates
        },
        source::Limits {
            depth: 1,
            ..defaults.templates
        },
        source::Limits {
            file_bytes: 1,
            ..defaults.templates
        },
        source::Limits {
            total_bytes: 1,
            ..defaults.templates
        },
    ] {
        assert!(
            build::compile_templates(
                &metadata,
                render::Limits {
                    templates: limits,
                    ..defaults
                }
            )
            .is_err(),
            "template resource ceiling must apply to selected subtree"
        );
    }
    let entry = "base/page.html".parse().expect("entry");
    for (limits, label) in [
        (
            render::Limits {
                context_bytes: 1,
                ..defaults
            },
            "context bytes",
        ),
        (
            render::Limits {
                output_bytes: 1,
                ..defaults
            },
            "output bytes",
        ),
    ] {
        let compiled = build::compile_templates(&metadata, limits)
            .expect("compile independent of render input");
        assert!(
            matches!(compiled.render(&entry, &json!({"title":"bounded"})), Err(render::Error::Limit(actual)) if actual == label)
        );
    }
    let compiled = build::compile_templates(&metadata, defaults).expect("compile");
    assert!(matches!(
        compiled.render(&entry, &json!([])),
        Err(render::Error::ContextShape)
    ));
}

#[test]
fn cli_template_mode_requires_composition_and_rejects_other_inspection_modes() {
    let fixture = Fixture::new(&[]);
    let before = fixture.snapshot();
    let config = fixture.config();
    for flags in [
        vec!["--compile-templates"],
        vec!["--acquire-site", "site:one", "--compile-templates"],
        vec![
            "--acquire-composition",
            "site:one",
            "--compile-templates",
            "--check-composition",
        ],
        vec![
            "--acquire-composition",
            "site:one",
            "--compile-templates",
            "--parse-documents",
        ],
        vec![
            "--acquire-composition",
            "site:one",
            "--compile-templates",
            "--origin",
            "sites",
        ],
        vec![
            "--acquire-site",
            "site:one",
            "--parse-documents",
            "--compile-templates",
        ],
        vec!["--compile-templates", "--composition-origin", "{}"],
        vec!["--compile-templates", "--origin", "{}"],
    ] {
        let output = run(&config, &flags);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{flags:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    for budget in [
        "--max-corpus-files",
        "--max-corpus-bytes",
        "--max-corpus-nodes",
        "--max-corpus-metadata-nodes",
    ] {
        let output = run(
            &config,
            &[
                "--acquire-composition",
                "site:one",
                "--compile-templates",
                budget,
                "1",
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(2),
            "{budget}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    assert_eq!(fixture.snapshot(), before);
}
