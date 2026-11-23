//! Page-owned resources use retained source, native compilers and one site inventory.

#[path = "publication_page_resources/diagrams.rs"]
mod diagrams;
#[path = "publication_page_resources/fonts.rs"]
mod fonts;
#[path = "publication_page_resources/images.rs"]
mod images;
#[path = "publication_page_resources/modules.rs"]
mod modules;

use gazette_test_support as native;

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use gazette_sdk::{
    build,
    preview::{self, PreviewReservation, PreviewSession},
    source, toml,
    workspace::Key,
};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const RESOURCES: &str = "[[resources]]\ninputs=['main.scss','_tokens.scss']\nbuild={kind='stylesheet',entry='main.scss'}\noutput='/assets/z-first.css'\n[[resources]]\ninputs=['main.ts','dep.ts']\nbuild={kind='browser-module',entry='main.ts'}\noutput='/assets/a-second.js'\n";
const MAIN: &str = "import { answer } from './dep.ts';\nexport const value: number = answer;\n";
const DEPENDENCY: &str = "export const answer: number = 42;\n";
const STYLE: &str = "@use 'tokens';\n.card { color: tokens.$ink; .child { padding: 7px; } }\n";
const TOKENS: &str = "$ink: #123456;\n";
const THEME_MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\nresource_requirements=['module:shared']\n";
const THEME_MANIFEST_WITHOUT_RESOURCES: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n";
const PLUGIN_MANIFEST: &str = "package='page-resources'\n[resources.'module:shared']\ninputs=['shared.ts']\nbuild={kind='browser-module',entry='shared.ts'}\n";
const SITE_RESOURCE_SELECTION: &str = "'module:shared'={state='enabled',resource={owner={kind='plugin',package='page-resources'},resource='module:shared'},output='/assets/shared.js',use='site'}";
const TEMPLATE: &str = r#"{{ body_html | safe }}{% for resource in presentation.theme_resources %}<script data-shared="yes" type="module" src="{{ resource.url }}"></script>{% endfor %}{% for resource in presentation.resources %}{% if resource.kind == "stylesheet" %}<link data-local="yes" rel="stylesheet" href="{{ resource.url }}">{% elif resource.kind == "browser-module" %}<script data-local="yes" type="module" src="{{ resource.url }}"></script>{% endif %}{% endfor %}"#;

fn page(id: &str, route: &str, resources: &str) -> String {
    let aliases = if id == "one" {
        ",aliases=['/old.html']"
    } else {
        ""
    };
    format!(
        "+++\n{resources}[content]\nid='content:{id}'\nsite='site:one'\ntitle='Page {id}'\nlifecycle='draft'\nroute={{path='{route}'{aliases}}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Page {id}\n"
    )
}

struct Fixture {
    root: TempDir,
    commit: Oid,
    native: bool,
}

impl Fixture {
    fn new(native: bool, overrides: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("isolated native fixture");
        let repository = Repository::init(root.path()).expect("fixture Git");
        let first = page("one", "/one/", RESOURCES);
        let second = page("two", "/two/", "");
        for (path, bytes) in [
            ("theme/theme.toml", THEME_MANIFEST.as_bytes()),
            ("theme/plugin.toml", PLUGIN_MANIFEST.as_bytes()),
            ("theme/templates/page.html", TEMPLATE.as_bytes()),
            ("theme/templates/alias.html", TEMPLATE.as_bytes()),
            (
                "theme/shared.ts",
                b"export const shared: boolean = true;\n".as_slice(),
            ),
            ("content/one.md", first.as_bytes()),
            ("content/two.md", second.as_bytes()),
            ("content/main.ts", MAIN.as_bytes()),
            ("content/dep.ts", DEPENDENCY.as_bytes()),
            ("content/main.scss", STYLE.as_bytes()),
            ("content/_tokens.scss", TOKENS.as_bytes()),
        ]
        .into_iter()
        .chain(overrides.iter().copied())
        {
            // Same fixture can put the owner and companions in project-native docs.
            let path = if native && path.starts_with("content/") && path != "content/two.md" {
                path.replacen("content/", "project/", 1)
            } else {
                path.to_owned()
            };
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("fixture directories");
            fs::write(path, bytes).expect("fixture source");
        }
        let mut index = repository.index().expect("index");
        index
            .add_all(
                ["theme", "content", "project", "other"],
                IndexAddOption::DEFAULT,
                None,
            )
            .expect("fixture files");
        let tree = repository
            .find_tree(index.write_tree().expect("tree id"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed identity");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "page resources fixture",
                &tree,
                &[],
            )
            .expect("isolated fixture commit");
        Self {
            root,
            commit,
            native,
        }
    }

    fn configuration(&self) -> String {
        let mut config = format!(
            "[composition]\ntheme={{package='theme:one'}}\n[composition.plugins]\n'page-resources'={{state='enabled'}}\n[composition.resources]\n{SITE_RESOURCE_SELECTION}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\nauthored=[{{source='source:content',target='authored'}}]\n"
        );
        for name in ["theme", "content"]
            .into_iter()
            .chain(self.native.then_some("project"))
        {
            config.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{name}'}}\n", self.root.path().to_str().expect("root"), self.commit));
        }
        config.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n[[plugin_packages]]\npackage='page-resources'\nsource='source:theme'\nmanifest='plugin.toml'\n");
        if self.native {
            config.push_str("[[projects]]\nid='project:native'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[placements]]\nrole='primary'\nsite='site:one'\nproject='project:native'\nrender=true\ndiscoverable=true\ndocuments={source='source:project',target='native-docs'}\n");
        }
        config
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_publication(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 5,
            entries: 96,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("complete retained acquisition")
}

fn render(
    inputs: &build::AcquiredInputs,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    render_with_limits(inputs, Default::default())
}

fn render_with_limits(
    inputs: &build::AcquiredInputs,
    limits: build::PublicationRenderLimits,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: source::Limits::default(),
        },
        limits,
    )
}

fn bytes<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("expected output")
}

fn cli(config: &str, parent: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--model-output",
            "model.json",
            "--render-year",
            "2040",
            "--output-parent",
            parent.to_str().expect("parent"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("config transport");
    child.wait_with_output().expect("CLI completion")
}

fn failure(config: &str, expected: &str) {
    let error = render(&acquire(config)).expect_err("no partial publication");
    assert_eq!(error.code(), expected, "{error}");
    let parent = tempdir().expect("output parent");
    fs::write(parent.path().join("unrelated.txt"), b"preserve").expect("sentinel");
    let before = source::inspect(parent.path(), None, source::Limits::default()).expect("before");
    let output = cli(config, parent.path());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no successful receipt");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.starts_with(&format!("gazette: {expected}")),
        "{diagnostic}"
    );
    assert!(
        !diagnostic.contains("panicked"),
        "provider errors stay typed"
    );
    assert_eq!(
        before.identity(),
        source::inspect(parent.path(), None, source::Limits::default())
            .expect("after")
            .identity(),
        "no stage or unrelated mutation"
    );
}

#[test]
fn page_resources_compile_in_order_with_same_cli_model_aliases_and_exact_native_source() {
    for native in [false, true] {
        let fixture = Fixture::new(native, &[]);
        let config = fixture.configuration();
        let inputs = acquire(&config);
        let built = render(&inputs).expect("local page resources");
        let tree = built.site().output();
        let retained_identity = tree.inventory().identity();
        let disclosure = built
            .site()
            .probe_disclosure(
                &std::collections::BTreeMap::from([(
                    "rule:compiled-value"
                        .parse()
                        .expect("private rule identity"),
                    "value".to_owned(),
                )]),
                Default::default(),
            )
            .expect("real SWC-generated JavaScript reaches the native disclosure parser");
        let compiled_path = "assets/a-second.js".parse().expect("compiled output path");
        assert_eq!(
            built.site().media_types().get(&compiled_path),
            Some(&gazette_sdk::render::MediaType::JavaScript),
            "actual compiled browser-module declaration, not a suffix guess"
        );
        let coverage = disclosure
            .files()
            .iter()
            .find(|file| file.path == compiled_path)
            .expect("compiled module decoder coverage");
        assert_eq!(
            coverage.decoding,
            build::DisclosureDecoding::JavascriptSyntax {
                profile: gazette_sdk::javascript::PROFILE,
                goal: gazette_sdk::javascript::Goal::Module,
            }
        );
        assert_eq!(
            coverage.sha256,
            source::Sha256Digest::of(bytes(tree, "assets/a-second.js"))
        );
        assert!(
            disclosure.findings().iter().any(|finding| {
                finding.path == compiled_path
                    && finding.rule.as_str() == "rule:compiled-value"
                    && finding.representation
                        == build::DisclosureRepresentation::JavascriptIdentifier
            }),
            "the compiled exported value is decoded as an actual JavaScript identifier"
        );
        assert_eq!(
            disclosure.output().inventory().identity(),
            retained_identity
        );
        assert_eq!(tree.inventory().identity(), retained_identity);
        let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("site model");
        assert_eq!(
            model["browser_modules"]["modules"]["module:shared"]["url"],
            "https://one.example/mount/assets/shared.js",
            "the selected browser resource is represented once in the shared module graph"
        );
        let resources = model["pages"]["content:one"]["presentation"]["resources"]
            .as_array()
            .expect("ordered local resources");
        assert_eq!(resources.len(), 2);
        for (index, kind, path) in [
            (0, "stylesheet", "assets/z-first.css"),
            (1, "browser-module", "assets/a-second.js"),
        ] {
            let payload = bytes(tree, path);
            assert_eq!(
                resources[index],
                json!({"kind":kind,"url":format!("https://one.example/mount/{path}"),"output":path,"sha256":source::Sha256Digest::of(payload),"bytes":payload.len()})
            );
        }
        assert_eq!(
            model["pages"]["content:two"]["presentation"]["resources"],
            json!([])
        );
        let shared = &model["resources"]["module:shared"];
        assert_eq!(shared["output"], "assets/shared.js");
        for id in ["content:one", "content:two"] {
            assert_eq!(
                model["pages"][id]["presentation"]["theme_resources"][0],
                json!({"id":"module:shared","kind":shared["kind"],"url":shared["url"],"output":shared["output"],"sha256":shared["sha256"],"bytes":shared["bytes"]})
            );
        }
        let css = std::str::from_utf8(bytes(tree, "assets/z-first.css")).expect("CSS");
        let compact: String = css.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(compact.contains(".card{color:#123456"), "{css}");
        assert!(compact.contains(".card.child{padding:7px"), "{css}");
        assert!(!css.contains("@use"));
        let js = std::str::from_utf8(bytes(tree, "assets/a-second.js")).expect("JavaScript");
        assert!(js.contains("42") && js.contains("export"), "{js}");
        assert!(!js.contains(": number") && !js.contains("./dep.ts"));
        // Execute only this trusted test module: Node is a validator, not a
        // Gazette provider or fallback for missing retained imports.
        let mut node = Command::new(native::node())
            .args(["--input-type=module", "--eval", "let code=''; for await(const part of process.stdin) code+=part; const m=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64')); if(m.value!==42) throw Error('export mismatch'); process.stdout.write(String(m.value));"])
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().expect("native Node fixture validator");
        node.stdin
            .take()
            .expect("Node stdin")
            .write_all(js.as_bytes())
            .expect("emitted bytes");
        let evaluated = node.wait_with_output().expect("native evaluation");
        assert!(
            evaluated.status.success(),
            "{}",
            String::from_utf8_lossy(&evaluated.stderr)
        );
        assert_eq!(evaluated.stdout, b"42");
        for path in ["one/index.html", "old.html"] {
            let html = std::str::from_utf8(bytes(tree, path))
                .expect("HTML")
                .replace("&#x2F;", "/");
            assert!(
                html.find("assets/z-first.css").expect("first stylesheet")
                    < html.find("assets/a-second.js").expect("second module")
            );
            assert_eq!(html.matches("data-local=\"yes\"").count(), 2);
            assert!(html.contains("assets/shared.js"));
        }
        let other = std::str::from_utf8(bytes(tree, "two/index.html")).expect("second page");
        assert!(!other.contains("data-local=\"yes\""));
        let public = std::str::from_utf8(bytes(tree, "model.json")).expect("model text");
        for private in [
            fixture.root.path().to_str().expect("root"),
            "main.ts",
            "dep.ts",
            "main.scss",
            "_tokens.scss",
        ] {
            assert!(
                !public.contains(private),
                "private resource coordinate {private}"
            );
        }
        let evidence = serde_json::to_value(built.site().evidence()).expect("private evidence");
        let local = evidence["page_resources"]["content:one"]
            .as_array()
            .expect("ordered private evidence");
        assert_eq!(local.len(), 2);
        for (index, entry) in [(0, "main.scss"), (1, "main.ts")] {
            assert_eq!(
                local[index]["source"],
                if native {
                    "source:project"
                } else {
                    "source:content"
                }
            );
            assert_eq!(local[index]["recipe"]["entry"], entry);
            assert!(local[index]["dependencies"].get(entry).is_some());
            assert_eq!(local[index]["output_sha256"], resources[index]["sha256"]);
            assert!(
                !local[index]["tool"]
                    .as_str()
                    .expect("native tool")
                    .is_empty()
            );
        }
        let parent = tempdir().expect("retained output parent");
        let output = cli(&config, parent.path());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
        let directory = Path::new(receipt["directory"].as_str().expect("retained directory"));
        assert_eq!(
            source::inspect(directory, None, source::Limits::default())
                .expect("readback")
                .identity(),
            tree.inventory().identity()
        );
        for entry in tree.inventory().entries() {
            if matches!(entry.content, source::Content::File { .. }) {
                assert_eq!(
                    fs::read(directory.join(entry.path.as_str())).expect("retained bytes"),
                    tree.file(&entry.path).expect("SDK output")
                );
            }
        }
        let folder = if native { "project" } else { "content" };
        fs::write(
            fixture.root.path().join(folder).join("dep.ts"),
            "this is not TypeScript",
        )
        .expect("later native mutation");
        let again = render(&inputs).expect("retained compilation, no native reread");
        assert_eq!(
            again.site().output().inventory().identity(),
            tree.inventory().identity()
        );
        assert_eq!(
            serde_json::to_value(again.site().evidence()).expect("repeat evidence"),
            evidence
        );
    }
}

#[test]
fn reserved_preview_urls_reach_compiled_page_and_shared_resources_over_http() {
    let fixture = Fixture::new(false, &[]);
    let config = fixture.configuration();
    let site: Key = "site:one".parse().expect("configured site");
    let inputs = build::acquire_publications(
        &config,
        std::slice::from_ref(&site),
        Default::default(),
        Default::default(),
    )
    .expect("exact retained resource and Theme acquisition");
    let reservation = preview::reserve(
        &inputs,
        BTreeMap::from([(
            site.clone(),
            "127.0.0.1:0".parse().expect("loopback binding"),
        )]),
        preview::Options {
            allow_listeners: true,
            ..Default::default()
        },
    )
    .expect("hold the actual listener before compiling resource URLs");
    let address = reservation.addresses()[&site];
    let base = reservation.urls()[&site].as_str().to_owned();
    assert_eq!(base, format!("http://{address}/mount/"));
    let rendered = preview::render(
        &inputs,
        &reservation,
        build::PublicationSetRenderOptions {
            derive_selection: false,
            preview_link_bases: None,
            selection: None,
            preview_urls: None,
            review_responses: None,
            sites: BTreeMap::from([(
                site.clone(),
                build::SiteRenderOptions {
                    render_year: None,
                    graph: None,
                    model_output: Some(&"model.json".parse().expect("model output")),
                    limits: Default::default(),
                },
            )]),
        },
        Default::default(),
    )
    .expect("real shared TypeScript and page TypeScript/Sass compilation");
    let publication = rendered.sites()[&site].as_ref().expect("complete site");
    let output = publication.site().output();
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).expect("shared model");
    let html = std::str::from_utf8(bytes(output, "one/index.html"))
        .expect("rendered page")
        .replace("&#x2F;", "/");
    let resources = &model["pages"]["content:one"]["presentation"]["resources"];
    let shared = &model["resources"]["module:shared"];
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["theme_resources"][0],
        json!({"id":"module:shared","kind":shared["kind"],"url":shared["url"],"output":shared["output"],"sha256":shared["sha256"],"bytes":shared["bytes"]}),
        "page and site share the same compiled resource projection"
    );
    let mut session = preview::attach(reservation, &rendered).expect("attach original socket");
    assert_eq!(session.addresses()[&site], address);
    for (resource, path, attribute, mime) in [
        (
            &resources[0],
            "assets/z-first.css",
            "href",
            "text/css; charset=utf-8",
        ),
        (
            &resources[1],
            "assets/a-second.js",
            "src",
            "text/javascript; charset=utf-8",
        ),
        (
            shared,
            "assets/shared.js",
            "src",
            "text/javascript; charset=utf-8",
        ),
    ] {
        let expected = bytes(output, path);
        assert!(!expected.is_empty(), "actual compiler output");
        assert_eq!(resource["url"], format!("{base}{path}"));
        assert_eq!(
            resource["sha256"],
            json!(source::Sha256Digest::of(expected))
        );
        assert!(
            html.contains(&format!("{attribute}=\"{base}{path}\"")),
            "{html}"
        );
        let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .expect("native loopback client");
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("bounded read");
        client
            .write_all(
                format!(
                    "GET /mount/{path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .expect("actual resource request");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match session.poll().expect("native preview polling") {
                preview::Poll::Idle => {
                    assert!(Instant::now() < deadline, "bounded connection acceptance");
                    std::thread::yield_now();
                }
                preview::Poll::Responded => break,
                preview::Poll::Disconnected => panic!("compiled resource response failed"),
            }
        }
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .expect("complete native response");
        let split = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .expect("HTTP header boundary");
        let headers = std::str::from_utf8(&response[..split]).expect("ASCII response headers");
        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"), "{headers}");
        assert!(
            headers
                .lines()
                .any(|line| line == format!("Content-Type: {mime}")),
            "{headers}"
        );
        assert!(
            headers
                .lines()
                .any(|line| line == format!("Content-Length: {}", expected.len()))
        );
        assert_eq!(
            &response[split + 4..],
            expected,
            "exact retained compiled resource bytes"
        );
    }
    session.close();
    assert_eq!(
        fixture.configuration(),
        config,
        "preview never rewrites workspace choices"
    );
    for (path, expected) in [
        ("content/main.ts", MAIN),
        ("content/dep.ts", DEPENDENCY),
        ("content/main.scss", STYLE),
        ("content/_tokens.scss", TOKENS),
        ("theme/shared.ts", "export const shared: boolean = true;\n"),
        ("theme/templates/page.html", TEMPLATE),
    ] {
        assert_eq!(
            fs::read(fixture.root.path().join(path)).expect("original native source"),
            expected.as_bytes()
        );
    }
}

#[test]
fn invalid_page_resource_declarations_fail_before_any_staging() {
    for declaration in [
        "resources = [[['main.ts','dep.ts'], {kind='browser-module',entry='main.ts'}, '/assets/article.js']]\n".to_owned(),
        RESOURCES.replace("output='/assets/a-second.js'", "output='/assets/article/'"),
        RESOURCES.replace(
            "inputs=['main.ts','dep.ts']",
            "inputs=['main.ts','main.ts','dep.ts']",
        ),
        RESOURCES.replace("inputs=['main.ts','dep.ts']", "inputs=['dep.ts']"),
        RESOURCES.replace("inputs=['main.ts','dep.ts']", "inputs=[]"),
        RESOURCES.replace("output='/assets/a-second.js'", "output='/assets/z-first.css'"),
        RESOURCES.replace("inputs=['main.ts','dep.ts']", "inputs=['main.ts','../dep.ts']"),
        RESOURCES.replace("output='/assets/a-second.js'", "output='/assets/a-second.js'\nunknown=true"),
    ] {
        let page = page("one", "/one/", &declaration);
        let fixture = Fixture::new(false, &[("content/one.md", page.as_bytes())]);
        failure(&fixture.configuration(), "publication_authored");
    }
}

#[test]
fn missing_undeclared_or_escaping_imports_do_not_borrow_other_captured_sources() {
    for entry in [
        "import { x } from './missing.ts'; export { x };",
        "import { x } from './undeclared.ts'; export { x };",
        "import { x } from '../theme/shared.ts'; export { x };",
    ] {
        let fixture = Fixture::new(
            false,
            &[
                ("content/main.ts", entry.as_bytes()),
                ("content/undeclared.ts", b"export const x = 1;"),
            ],
        );
        failure(&fixture.configuration(), "resource_input");
    }
    let declaration = RESOURCES.replace(
        "inputs=['main.ts','dep.ts']",
        "inputs=['main.ts','dep.ts','absent.ts']",
    );
    let page = page("one", "/one/", &declaration);
    let fixture = Fixture::new(false, &[("content/one.md", page.as_bytes())]);
    failure(&fixture.configuration(), "resource_input");
}

#[test]
fn page_resource_outputs_share_page_alias_and_model_collision_ownership() {
    for output in [
        "/one/index.html",
        "/old.html",
        "/model.json",
        "/assets/shared.js",
    ] {
        let declaration = RESOURCES.replace(
            "output='/assets/a-second.js'",
            &format!("output='{output}'"),
        );
        let page = page("one", "/one/", &declaration);
        let fixture = Fixture::new(false, &[("content/one.md", page.as_bytes())]);
        failure(&fixture.configuration(), "source_inventory");
    }
}

#[test]
fn page_resources_respect_shared_metadata_and_program_output_limits() {
    let fixture = Fixture::new(
        false,
        &[(
            "theme/theme.toml",
            THEME_MANIFEST_WITHOUT_RESOURCES.as_bytes(),
        )],
    );
    let configuration = fixture.configuration();
    let page_only = configuration.replace(
        SITE_RESOURCE_SELECTION,
        "'module:shared'={state='disabled'}",
    );
    assert_ne!(
        page_only, configuration,
        "exercise page compilation without an earlier site-resource limit"
    );
    let inputs = acquire(&page_only);
    for metadata in [true, false] {
        let mut limits = build::PublicationRenderLimits::default();
        if metadata {
            limits.output.resource_metadata_bytes = 1;
        } else {
            limits.output.resources.output_bytes = 1;
        }
        let error = render_with_limits(&inputs, limits).expect_err("bounded whole publication");
        assert_eq!(error.code(), "resource_limit", "{error}");
    }
    render(&inputs).expect("failed bounded attempts did not damage retained inputs");
}

#[test]
fn bare_theme_render_cannot_hide_declared_page_resources() {
    let fixture = Fixture::new(false, &[]);
    let inputs = acquire(&fixture.configuration());
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained corpus");
    let resolved = parsed
        .resolve(Default::default())
        .expect("source-bound graph");
    let metadata = inputs
        .check_metadata(Default::default())
        .expect("presentation metadata");
    let theme =
        build::compile_templates(&metadata, Default::default()).expect("actual Theme compiler");
    let error = theme
        .render_page(
            resolved.authored(),
            &"content:one".parse().expect("page"),
            &"page.html".parse().expect("template"),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("a template-only caller has not compiled declared resources");
    assert_eq!(error.code(), "page_uncompiled_resources");
    render(&inputs).expect("same inputs work through the complete compilation path");
}

#[test]
fn foreign_site_resource_failure_does_not_compile_into_selected_site() {
    let foreign = page("foreign", "/foreign/", "[[resources]]\ninputs=['bad.ts']\nbuild={kind='browser-module',entry='bad.ts'}\noutput='/assets/bad.js'\n")
        .replace("site='site:one'", "site='site:other'");
    let fixture = Fixture::new(
        false,
        &[
            ("other/foreign.md", foreign.as_bytes()),
            ("other/bad.ts", b"import './missing.ts';"),
        ],
    );
    let mut config = fixture.configuration();
    config.push_str(&format!("[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:other'\nname='Other'\n[[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.example/'\nauthored=[{{source='source:other',target='other'}}]\n[[sources]]\nid='source:other'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='other'}}\n", fixture.root.path().to_str().expect("root"), fixture.commit));
    let inputs = acquire(&config);
    let publication = render(&inputs).expect("foreign compiler failure is not selected");
    assert!(
        publication
            .site()
            .output()
            .file(&"assets/bad.js".parse().expect("path"))
            .is_none()
    );
    let model: Value =
        serde_json::from_slice(bytes(publication.site().output(), "model.json")).expect("model");
    assert!(model["pages"].get("content:foreign").is_none());
    let other = build::acquire_publication(
        &config,
        &"site:other".parse().expect("site"),
        Default::default(),
        source::PreparationLimits {
            sources: 5,
            entries: 96,
            bytes: 1024 * 1024,
            per_source: Default::default(),
        },
    )
    .expect("same retained source set");
    let error = render(&other).expect_err("selecting the bad source exposes its import failure");
    assert_eq!(error.code(), "resource_input", "{error}");
    let parent = tempdir().expect("CLI staging root");
    let output = cli(&config, parent.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unused_opaque_inputs_and_notebooks_remain_uncompiled_obligations() {
    const NOTEBOOK: &[u8] = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#;
    for (path, content) in [
        ("unused.bin", b"opaque".as_slice()),
        ("analysis.ipynb", NOTEBOOK),
    ] {
        for declared in [false, true] {
            let declaration = if declared {
                RESOURCES.replace(
                    "inputs=['main.ts','dep.ts']",
                    &format!("inputs=['main.ts','dep.ts','{path}']"),
                )
            } else {
                RESOURCES.to_owned()
            };
            let page = page("one", "/one/", &declaration);
            let input_path = format!("content/{path}");
            let fixture = Fixture::new(
                false,
                &[("content/one.md", page.as_bytes()), (&input_path, content)],
            );
            failure(&fixture.configuration(), "site_uncompiled_asset");
        }
    }
}
