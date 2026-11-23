//! A neutral base/world Theme, workspace and bundled page traverse the real application.

#[allow(dead_code)]
#[path = "support/mcp.rs"]
mod mcp_carrier;

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const BASE_MANIFEST: &str = "package='theme:base'\nnamespace='base'\ntemplates='templates'\n";
const WORLD_MANIFEST: &str = "package='theme:world'\nnamespace='world'\ntemplates='templates'\n\
palettes=[{id='palette:neutral',browser_theme_colors={light='#ffffff',dark='#000000'}}]\nrender={page='page.html',alias='alias.html'}\n\
[[dependencies]]\npackage='theme:base'\n";
const BASE_FRAME: &str = "<!doctype html><html><body data-physics=\"base\">\
{% block body %}{% endblock body %}</body></html>";
const WORLD_PAGE: &str = "{% extends 'base/frame.html' %}{% block body %}\
<article data-world=\"neutral\" data-theme=\"{{ presentation.theme }}\" \
data-palette=\"{{ presentation.palette }}\"><h1>{{ content.title }}</h1>\
{{ body_html | safe }}{% for resource in presentation.resources %}\
<script type=\"module\" src=\"{{ resource.url }}\"></script>{% endfor %}</article>\
{% endblock body %}";
const ALIAS: &str = "<a href=\"{{ route.canonical_url }}\">Moved</a>";
const PAGE: &str = "+++\n[content]\nid='content:guide'\nsite='site:one'\n\
title='Neutral guide'\nlifecycle='draft'\n\
route={path='/guide/',aliases=['/previous.html']}\n\
structure={kind='page',family={kind='documentation'}}\n+++\n# Retained body\n";

struct Fixture {
    root: TempDir,
    configuration: String,
}

impl Fixture {
    fn new() -> Self {
        Self::with_world_page(WORLD_PAGE)
    }

    fn with_world_page(world_page: &str) -> Self {
        let root = tempdir().expect("isolated neutral fixture");
        let repository = Repository::init(root.path()).expect("fixture Git repository");
        for (path, bytes) in [
            ("base/theme.toml", BASE_MANIFEST.as_bytes()),
            ("base/templates/frame.html", BASE_FRAME.as_bytes()),
            ("world/theme.toml", WORLD_MANIFEST.as_bytes()),
            ("world/templates/page.html", world_page.as_bytes()),
            ("world/templates/alias.html", ALIAS.as_bytes()),
            ("content/guide/index.md", PAGE.as_bytes()),
            (
                "content/guide/main.ts",
                b"export const enhanced: boolean = true;\n".as_slice(),
            ),
        ] {
            let target = root.path().join(path);
            fs::create_dir_all(target.parent().expect("fixture parent"))
                .expect("fixture directories");
            fs::write(target, bytes).expect("fixture bytes");
        }
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(["base", "world", "content"], IndexAddOption::DEFAULT, None)
            .expect("tracked fixture tree");
        let tree = repository
            .find_tree(index.write_tree().expect("fixture tree id"))
            .expect("fixture tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed fixture identity");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "neutral DesignSystem fixture",
                &tree,
                &[],
            )
            .expect("fixture commit");
        let path = root.path().to_str().expect("UTF-8 fixture path");
        let configuration = format!(
            "[composition]\ntheme={{package='theme:world',palette='palette:neutral'}}\n\
             [[principals]]\nid='owner:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
             [[brands]]\nid='brand:one'\nname='Neutral fixture'\n\
             [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n\
             authored=[{{source='source:content',target='authored'}}]\n\
             [[sources]]\nid='source:base'\nlocation={{kind='repository',root={path:?},commit='{commit}',subdirectory='base'}}\n\
             [[sources]]\nid='source:world'\nlocation={{kind='repository',root={path:?},commit='{commit}',subdirectory='world'}}\n\
             [[sources]]\nid='source:content'\nlocation={{kind='repository',root={path:?},commit='{commit}',subdirectory='content'}}\n\
             [[theme_packages]]\npackage='theme:base'\nsource='source:base'\nmanifest='theme.toml'\n\
             [[theme_packages]]\npackage='theme:world'\nsource='source:world'\nmanifest='theme.toml'\n"
        );
        Self {
            root,
            configuration,
        }
    }
}

#[test]
fn initialized_mcp_survives_a_recursive_theme_macro_and_answers_the_next_request() {
    let fixture = Fixture::with_world_page(
        "{% macro r(n) %}{{ self::r(n=n+1) }}{% endmacro %}{{ self::r(n=0) }}",
    );
    let healthy = Fixture::new();
    let before = [
        source::inspect(fixture.root.path(), None, source::Limits::default())
            .expect("recursive input baseline"),
        source::inspect(healthy.root.path(), None, source::Limits::default())
            .expect("healthy input baseline"),
    ];
    let arguments = json!({
        "source":fixture.configuration,
        "site":"site:one",
        "allow_read":true
    });
    let healthy_arguments = json!({
        "source":healthy.configuration,
        "site":"site:one",
        "allow_read":true
    });
    mcp_carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let failed = send(request(
            1,
            "tools/call",
            json!({"name":"compile_templates","arguments":arguments}),
        ));
        assert!(failed.get("error").is_none(), "{failed}");
        assert_eq!(failed["result"]["isError"], true, "{failed}");
        let message = failed["result"]["content"][0]["text"]
            .as_str()
            .expect("readable diagnostic");
        assert!(
            message.contains("recursive template macro `r`"),
            "{message}"
        );
        assert!(message.contains("world/page.html"), "{message}");
        assert!(message.contains("admission:"), "{message}");
        assert!(
            !message.contains("at line"),
            "no fabricated AST position: {message}"
        );
        assert!(
            !message.contains("column"),
            "no fabricated AST position: {message}"
        );

        // Compile real healthy captured templates on this same initialized
        // process, before a catalog response can substitute for recovery.
        let succeeded = send(request(
            2,
            "tools/call",
            json!({"name":"compile_templates","arguments":healthy_arguments}),
        ));
        assert!(succeeded.get("error").is_none(), "{succeeded}");
        assert_eq!(succeeded["result"]["isError"], false, "{succeeded}");

        let catalog = send(request(3, "tools/list", json!({})));
        assert!(
            catalog["result"]["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()),
            "{catalog}"
        );
        let compiled = result(
            &succeeded,
            descriptor(&catalog, "compile_templates"),
            &healthy_arguments,
        );
        assert_eq!(
            compiled["tool"],
            "tera@1.20.1/gazette-captured-templates-v8"
        );
        assert_eq!(compiled["limits"]["execution_depth"], 64);
    });
    let after = [
        source::inspect(fixture.root.path(), None, source::Limits::default())
            .expect("recursive input unchanged"),
        source::inspect(healthy.root.path(), None, source::Limits::default())
            .expect("healthy input unchanged"),
    ];
    assert_eq!(after, before);
}

fn run(configuration: &str, arguments: &[&str]) -> Output {
    let mut arguments = arguments.to_vec();
    if arguments.first() == Some(&"check")
        && !matches!(arguments.get(1), Some(&"build" | &"workspace"))
        && !arguments.contains(&"--workspace-root")
    {
        arguments.push("--stdin");
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette executable");
    child
        .stdin
        .take()
        .expect("Gazette stdin")
        .write_all(configuration.as_bytes())
        .expect("workspace bytes");
    child.wait_with_output().expect("Gazette completion")
}

fn file<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("expected output")
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn descriptor<'a>(catalog: &'a Value, name: &str) -> &'a Value {
    catalog["result"]["tools"]
        .as_array()
        .expect("live tool catalog")
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("missing live tool {name}"))
}

fn result(response: &Value, tool: &Value, arguments: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    jsonschema::validator_for(&tool["inputSchema"])
        .expect("discovered input schema")
        .validate(arguments)
        .expect("request matches live discovery");
    let value = &response["result"]["structuredContent"];
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("discovered output schema")
        .validate(value)
        .expect("response matches live discovery");
    assert_eq!(
        serde_json::from_str::<Value>(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("readable tool result")
        )
        .expect("tool result JSON"),
        *value
    );
    value.clone()
}

#[test]
fn neutral_base_world_workspace_and_page_package_are_ready_through_sdk_and_cli() {
    let fixture = Fixture::new();
    let before = source::inspect(fixture.root.path(), None, source::Limits::default())
        .expect("original fixture inventory");
    let site = "site:one".parse().expect("site identity");
    let inputs = build::acquire_publication(
        &fixture.configuration,
        &site,
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 3,
            entries: 64,
            bytes: 64 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("exact base, world and content capture");
    let parsed = build::parse_publication(&inputs, Default::default()).expect("retained content");
    let resolved = parsed.resolve(Default::default()).expect("shared model");
    let inspection = serde_json::to_value(resolved.inspect()).expect("private inspection");
    assert_eq!(
        inspection["page_packages"]["content:guide"]["root"],
        "guide"
    );
    assert_eq!(
        inspection["page_packages"]["content:guide"]["companions"][0]["relative"],
        "main.ts"
    );

    let rendered = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect("complete retained Theme render");
    let output = rendered.site().output();
    let html = std::str::from_utf8(file(output, "guide/index.html"))
        .expect("HTML")
        .replace("&#x2F;", "/");
    assert!(html.contains("data-physics=\"base\""), "{html}");
    assert!(html.contains("data-world=\"neutral\""), "{html}");
    assert!(html.contains("theme:world"), "{html}");
    assert!(html.contains("palette:neutral"), "{html}");
    assert!(html.contains("guide/main.js"), "{html}");
    let module = std::str::from_utf8(file(output, "guide/main.js")).expect("browser module");
    assert!(module.contains("export"), "{module}");
    assert!(module.contains("enhanced"), "{module}");
    assert!(!module.contains(": boolean"), "{module}");
    let model: Value = serde_json::from_slice(file(output, "model.json")).expect("shared model");
    assert_eq!(
        model["pages"]["content:guide"]["presentation"]["theme"],
        "theme:world"
    );
    assert_eq!(
        model["pages"]["content:guide"]["presentation"]["resources"][0]["url"],
        "https://example.test/guide/main.js"
    );

    let inspected = run(
        &fixture.configuration,
        &["check", "--publication", "site:one"],
    );
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&inspected.stdout).expect("CLI inspection"),
        inspection
    );

    let parent = tempdir().expect("retained output parent");
    let staged = run(
        &fixture.configuration,
        &[
            "render-site",
            "--render-year",
            "2040",
            "--local",
            "--site",
            "site:one",
            "--model-output",
            "model.json",
            "--output-parent",
            parent.path().to_str().expect("output parent"),
        ],
    );
    assert!(
        staged.status.success(),
        "{}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let receipt: Value = serde_json::from_slice(&staged.stdout).expect("retention receipt");
    let directory = Path::new(receipt["directory"].as_str().expect("retained directory"));
    for path in [
        "guide/index.html",
        "guide/main.js",
        "model.json",
        "previous.html",
    ] {
        assert_eq!(
            fs::read(directory.join(path)).expect("retained bytes"),
            file(output, path)
        );
    }
    assert_eq!(
        source::inspect(fixture.root.path(), None, source::Limits::default())
            .expect("unchanged fixture source"),
        before
    );
    assert_eq!(
        model["pages"]["content:guide"]["content"]["id"],
        json!("content:guide")
    );
}

#[test]
fn neutral_theme_consumer_traverses_live_mcp_discovery_build_review_and_page_package() {
    let fixture = Fixture::new();
    let before = source::inspect(fixture.root.path(), None, source::Limits::default())
        .expect("original fixture inventory");
    let source_arguments =
        json!({"source":fixture.configuration,"site":"site:one","allow_read":true});

    mcp_carrier::interact(|send| {
        let catalog = send(request(1, "tools/list", json!({})));
        let effects = [
            (
                "inspect_composition",
                "workspace",
                "inspect",
                "read",
                json!(["filesystem-read"]),
            ),
            (
                "check_composition",
                "workspace",
                "verify",
                "read",
                json!(["filesystem-read"]),
            ),
            (
                "compile_templates",
                "workspace",
                "verify",
                "read",
                json!(["filesystem-read"]),
            ),
            (
                "inspect_publication",
                "build",
                "inspect",
                "read",
                json!(["filesystem-read"]),
            ),
            (
                "review_publication",
                "build",
                "inspect",
                "read",
                json!(["filesystem-read"]),
            ),
            (
                "authoring_start",
                "authoring",
                "act",
                "local",
                json!(["memory-state"]),
            ),
            (
                "authoring_page_package",
                "authoring",
                "inspect",
                "read",
                json!([]),
            ),
            (
                "authoring_close_session",
                "authoring",
                "act",
                "local",
                json!(["memory-state"]),
            ),
        ];
        for (name, capability, intent, effect, potential) in effects {
            let tool = descriptor(&catalog, name);
            assert_eq!(tool["_meta"]["io.rtepl.gazette/capability"], capability);
            assert_eq!(tool["_meta"]["io.rtepl.gazette/intent"], intent);
            assert_eq!(tool["_meta"]["io.rtepl.gazette/effect"], effect);
            assert_eq!(
                tool["_meta"]["io.rtepl.gazette/potentialEffects"], potential,
                "{name} effect facets"
            );
        }
        assert_eq!(
            descriptor(&catalog, "check_composition")["_meta"]["io.rtepl.gazette/relations"],
            json!([
                {"kind":"inspect","operation":"inspect_composition"},
                {"kind":"verify","operation":"compile_templates"}
            ])
        );
        assert_eq!(
            descriptor(&catalog, "authoring_start")["_meta"]["io.rtepl.gazette/terminalResultOwner"],
            "authoring_close_session"
        );

        for (id, name) in [
            (2, "inspect_composition"),
            (3, "check_composition"),
            (4, "compile_templates"),
            (5, "inspect_publication"),
            (6, "review_publication"),
        ] {
            let response = send(request(
                id,
                "tools/call",
                json!({"name":name,"arguments":source_arguments.clone()}),
            ));
            let value = result(&response, descriptor(&catalog, name), &source_arguments);
            match name {
                "inspect_composition" => {
                    assert_eq!(value["site"], "site:one");
                    assert_eq!(value["sources"].as_object().expect("sources").len(), 2);
                }
                "check_composition" => {
                    assert_eq!(value["acquisition"]["site"], "site:one");
                    assert_eq!(
                        value["theme_order"].as_array().expect("Theme order"),
                        &[json!("theme:base"), json!("theme:world")]
                    );
                    assert_eq!(
                        value["theme_packages"]
                            .as_object()
                            .expect("Theme packages")
                            .len(),
                        2
                    );
                }
                "compile_templates" => {
                    assert_eq!(value["packages"].as_array().expect("packages").len(), 2);
                    assert_eq!(value["theme"], "theme:world");
                    assert_eq!(value["packages"][0]["namespace"], "base");
                    assert_eq!(value["packages"][1]["namespace"], "world");
                    assert_eq!(value["packages"][0]["mounted_files"][0], "base/frame.html");
                }
                "inspect_publication" => {
                    assert_eq!(value["status"], "resolved-unadmitted");
                    assert_eq!(value["page_packages"]["content:guide"]["root"], "guide");
                }
                "review_publication" => {
                    assert_eq!(value["request"]["site"], "site:one");
                    assert!(
                        value["sha256"]
                            .as_str()
                            .is_some_and(|digest| !digest.is_empty())
                    );
                }
                _ => unreachable!(),
            }
        }

        let authoring_arguments = json!({
            "workspace":{"id":"workspace","path":"workspace.toml","text":fixture.configuration},
            "documents":[{"id":"guide","path":"authored/guide/index.md","text":PAGE}],
            "page_inputs":[{
                "id":"guide-module",
                "path":"authored/guide/main.ts",
                "bytes_base64":STANDARD.encode(b"export const enhanced: boolean = true;\n")
            }],
            "package_baselines":[
                {"kind":"theme","package":"theme:base","id":"base","path":"base/theme.toml","text":BASE_MANIFEST},
                {"kind":"theme","package":"theme:world","id":"world","path":"world/theme.toml","text":WORLD_MANIFEST}
            ]
        });
        let started = send(request(
            7,
            "tools/call",
            json!({"name":"authoring_start","arguments":authoring_arguments}),
        ));
        let started = result(
            &started,
            descriptor(&catalog, "authoring_start"),
            &authoring_arguments,
        );
        let handle = started["session"].as_str().expect("opaque session");
        assert_eq!(started["generation"], 1);

        let package_arguments = json!({"session":handle,"generation":1,"document":"guide"});
        let package = send(request(
            8,
            "tools/call",
            json!({"name":"authoring_page_package","arguments":package_arguments}),
        ));
        let package = result(
            &package,
            descriptor(&catalog, "authoring_page_package"),
            &package_arguments,
        );
        assert_eq!(package["value"]["content"], "content:guide");
        assert_eq!(
            package["value"]["package"]["document"],
            "authored/guide/index.md"
        );
        assert_eq!(package["value"]["package"]["root"], "guide");
        assert_eq!(
            package["value"]["package"]["companions"][0]["path"],
            "authored/guide/main.ts"
        );
        assert_eq!(
            package["value"]["package"]["companions"][0]["relative"],
            "main.ts"
        );
        assert_eq!(package["value"]["role"]["kind"], "document");

        let close_arguments = json!({"session":handle,"generation":1});
        let closed = send(request(
            9,
            "tools/call",
            json!({"name":"authoring_close_session","arguments":close_arguments}),
        ));
        let closed = result(
            &closed,
            descriptor(&catalog, "authoring_close_session"),
            &close_arguments,
        );
        assert_eq!(closed["changed"], true);
    });

    assert_eq!(
        source::inspect(fixture.root.path(), None, source::Limits::default())
            .expect("unchanged fixture source"),
        before
    );
}
