//! Actual pinned Node acquisition followed by pure retained publication projections.
//! Shared native-test support selects and qualifies the suite's Node runtime;
//! this fixture pins that executable's bytes for the math acquisition boundary.

use std::{collections::BTreeMap, path::Path, sync::OnceLock};

use gazette_sdk::{process, workspace::Key};
use serde_json::json;

use super::{Fixture, PAGE, Value, build, bytes, fs, render, source, tempdir};

#[path = "math/notebooks.rs"]
mod notebooks;

fn executable() -> &'static (String, process::ExecutablePin) {
    static NODE: OnceLock<(String, process::ExecutablePin)> = OnceLock::new();
    NODE.get_or_init(|| {
        let path = crate::native::node();
        let bytes = fs::read(path).expect("complete native executable bytes");
        (
            path.to_str().expect("UTF-8 executable path").to_owned(),
            process::ExecutablePin {
                bytes: u64::try_from(bytes.len()).expect("bounded executable length"),
                sha256: source::Sha256Digest::of(&bytes),
            },
        )
    })
}

fn command(directory: &Path) -> process::Command {
    let (program, pin) = executable();
    process::Command {
        program: program.clone(),
        executable: Some(*pin),
        arguments: Vec::new(),
        directory: fs::canonicalize(directory)
            .expect("explicit native working directory")
            .to_str()
            .expect("working directory path")
            .to_owned(),
        environment: BTreeMap::new(),
    }
}

fn config(fixture: &Fixture, selected: Option<&process::Command>) -> String {
    let workspace = fixture
        .config()
        .replace(
            "authored=[{source='source:one',target='one'}]",
            "authored=[{source='source:one',target='one',markdown_profile='publication-math'}]",
        )
        .replace(
            "public_base_url=",
            "machine={search='lexical'}\npublic_base_url=",
        );
    let Some(command) = selected else {
        return workspace;
    };
    let pin = command.executable.expect("explicit fixture executable pin");
    format!(
        "math_typesetter={{program={},executable={{bytes={},sha256='{}'}},arguments={},directory={},environment={{}},source='source:math',entrypoint='render.mjs'}}\n[[sources]]\nid='source:math'\nlocation={{kind='repository',root={},commit='{}',subdirectory='math'}}\n{workspace}",
        serde_json::to_string(&command.program).expect("TOML-compatible string"),
        pin.bytes,
        pin.sha256,
        serde_json::to_string(&command.arguments).expect("TOML-compatible string array"),
        serde_json::to_string(&command.directory).expect("TOML-compatible string"),
        serde_json::to_string(
            fixture
                .directory
                .path()
                .to_str()
                .expect("UTF-8 fixture repository"),
        )
        .expect("TOML-compatible repository root"),
        fixture.commit,
    )
}

fn fixture() -> Fixture {
    let page = PAGE.replace(
        "**Retained** body\n",
        "Before $x$ and `$literal$`.\n\n<!-- more -->\n\n```math\ny^2\n```\n",
    );
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        (
            "math/render.mjs",
            include_bytes!("../../../../adapters/math/render.mjs"),
        ),
        (
            "math/vendor/katex-0.18.4/katex.mjs",
            include_bytes!("../../../../adapters/math/vendor/katex-0.18.4/katex.mjs"),
        ),
    ])
}

fn permitted() -> build::MathGenerationOptions {
    build::MathGenerationOptions {
        execution: process::Options {
            allow_execute: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn real_typesetting_reaches_theme_model_and_search_then_replays_without_native_cwd() {
    let fixture = fixture();
    let runtime_directory = tempdir().expect("disposable generator working directory");
    let command = command(runtime_directory.path());
    let mut inputs = super::sites::acquire_set(&config(&fixture, Some(&command)), &["site:one"]);
    build::acquire_math(&mut inputs, Default::default(), permitted())
        .expect("actual pinned Node and vendored Gazette KaTeX acquisition");
    let capture = inputs.math_capture().expect("one retained shared capture");
    assert_eq!(
        capture.workspace_sha256(),
        inputs.compilation().source_sha256
    );
    let resource = &inputs.sources()[&"source:math".parse::<Key>().expect("source identity")];
    assert_eq!(capture.resource_tree(), resource.inventory().identity());
    assert_eq!(capture.resource_entrypoint().as_str(), "render.mjs");
    let calls: Vec<_> = capture.invocations().collect();
    assert_eq!(calls.len(), 1);
    let (request, response, digest, exit) = calls[0];
    assert_eq!(exit.code, Some(0));
    assert_eq!(
        digest,
        source::Sha256Digest::of(&serde_json::to_vec(&command).expect("configured command"))
    );
    assert_eq!(
        serde_json::from_str::<Value>(request).expect("exact retained generator request"),
        json!({"formulas":[{"tex":"x","display":false},{"tex":"y^2\n","display":true}]})
    );
    let generated: Value = serde_json::from_str(response).expect("exact complete response");
    let fragments = generated["mathml"]
        .as_array()
        .expect("ordered native results");
    assert_eq!(fragments.len(), 2);
    let inline = fragments[0].as_str().expect("native inline MathML");
    let display = fragments[1].as_str().expect("native display MathML");
    assert!(inline.starts_with("<math xmlns=\"http://www.w3.org/1998/Math/MathML\">"));
    assert!(display.contains("display=\"block\""));
    assert!(display.contains("<annotation encoding=\"application/x-tex\">y^2\n</annotation>"));

    let site = &inputs.sites()[&"site:one".parse::<Key>().expect("site identity")];
    let site = site.as_ref().expect("complete selected site acquisition");
    let rendered = render(site).expect("retained math reaches the production publication path");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let page = &model["pages"]["content:one"];
    let body_html = page["body_html"].as_str().expect("shared rendered body");
    assert!(body_html.contains(inline));
    assert!(body_html.contains(display));
    assert!(body_html.contains("<code>$literal$</code>"));
    assert!(!body_html.contains("language-math"));
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme HTML");
    assert!(html.contains(body_html));
    let body_text = page["body_text"].as_str().expect("shared native text");
    assert!(body_text.contains("y^2"));
    assert!(!body_text.contains("<math"));
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("search output");
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == "content:one")
        .expect("same document in search");
    assert_eq!(record["body"], body_text);

    // Remove only the disposable process cwd: the real generator and interpreter stay untouched.
    // A second native invocation could no longer start with this exact configured command.
    runtime_directory
        .close()
        .expect("retire disposable native working directory");
    assert!(!Path::new(&command.directory).exists());
    let replay = render(site).expect("pure rerender uses retained MathML, not a native process");
    let replay_tree = replay.site().output();
    assert_eq!(
        tree.inventory().identity(),
        replay_tree.inventory().identity()
    );
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(tree.file(&entry.path), replay_tree.file(&entry.path));
        }
    }
    assert_eq!(
        inputs
            .math_capture()
            .expect("retained capture")
            .invocations()
            .count(),
        1
    );
}

#[test]
fn selected_math_without_typesetter_or_execution_permission_is_not_silently_rendered() {
    let fixture = fixture();
    let mut unconfigured = super::sites::acquire_set(&config(&fixture, None), &["site:one"]);
    let error = build::acquire_math(&mut unconfigured, Default::default(), permitted())
        .expect_err("selected math requires the configured typesetter");
    assert_eq!(error.code(), "math_unconfigured");
    assert_eq!(error.completed().count(), 0);
    assert!(error.active_request().is_none());
    assert!(unconfigured.math_capture().is_none());
    let site = &unconfigured.sites()[&"site:one".parse::<Key>().expect("site identity")];
    assert!(render(site.as_ref().expect("captured site")).is_err());

    let directory = tempdir().expect("disposable denied command cwd");
    let selected = command(directory.path());
    let mut denied = super::sites::acquire_set(&config(&fixture, Some(&selected)), &["site:one"]);
    let error = build::acquire_math(&mut denied, Default::default(), Default::default())
        .expect_err("configuration is not execution permission");
    assert_eq!(error.code(), "process_permission");
    assert_eq!(error.completed().count(), 0);
    assert!(error.active_request().is_none());
    assert!(denied.math_capture().is_none());
    let site = &denied.sites()[&"site:one".parse::<Key>().expect("site identity")];
    assert!(render(site.as_ref().expect("captured site")).is_err());
}

#[test]
fn workspace_math_uses_defaults_but_requires_each_calls_execution_permission() {
    let fixture = fixture();
    let directory = tempdir().unwrap();
    let command = command(directory.path());
    let workspace = format!(
        "{}\n[build]\nsites=['site:one','site:other']\nlocal_drafts='include'\nmath=true\n",
        config(&fixture, Some(&command))
    );
    let request = json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":workspace}});
    let denied = super::cli(&request.to_string(), &["render-sites", "--local"]);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("process_permission"));
    let allowed = super::cli(
        &request.to_string(),
        &["render-sites", "--local", "--allow-execute"],
    );
    assert!(allowed.status.success(), "{allowed:?}");
    let report: Value = serde_json::from_slice(&allowed.stdout).unwrap();
    assert!(report["math"].is_object());
    crate::mcp_carrier::initialized_interact_with_call_timeout(
        "2026-07-28",
        std::time::Duration::from_secs(60),
        |_, send| {
            for (id, permission) in [(1, false), (2, true)] {
                let response = send(
                    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                        "name":"render_sites","arguments":{
                            "workspace":{"kind":"supplied","source":workspace},
                            "local":true,"allow_read":true,"allow_execute":permission
                        }
                    }}),
                );
                assert!(response.get("error").is_none(), "{response}");
                assert_eq!(response["result"]["isError"], !permission, "{response}");
                if permission {
                    assert!(response["result"]["structuredContent"]["math"].is_object());
                } else {
                    assert_eq!(
                        response["result"]["structuredContent"]["code"],
                        "process_permission"
                    );
                }
            }
        },
    );
    let single = super::cli(
        &workspace,
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--allow-execute",
        ],
    );
    assert!(single.status.success(), "{single:?}");
    for extra in [vec!["--allow-execute"], vec!["--allow-execute", "--watch"]] {
        let mut preview = super::sites::preview::Process::with_arguments(&request, &extra);
        assert_eq!(preview.event()["event"], "ready");
        preview.close();
    }
}
