//! Native Graphviz qualification through the configured acquisition and publication path.
//! Requires explicit `GAZETTE_TEST_DOT`; never discovers, installs or skips.
//! Retained compilation is proved separately from generator hermeticity or cross-host pixels.

use super::*;
use gazette_sdk::{process, raster};
use std::{path::PathBuf, sync::OnceLock};

const DOT: &str = "digraph Publication { node [shape=box]; source [label=\"Retained source\"]; output [label=\"Publication\"]; source -> output; }\n";
const RESOURCE: &str = "[[resources]]\ninputs=['diagram.dot']\nbuild={kind='dot-image',entry='diagram.dot'}\noutput='/assets/diagram.png'\n";
const TEMPLATE: &str = r#"{{ body_html | safe }}{% for resource in presentation.resources %}<img src="{{ resource.url }}" width="{{ resource.image.output.width }}" height="{{ resource.image.output.height }}" data-sha256="{{ resource.sha256 }}" alt="Retained source flows into publication">{% endfor %}"#;

fn executable() -> &'static process::PinnedExecutable {
    static DOT: OnceLock<process::PinnedExecutable> = OnceLock::new();
    DOT.get_or_init(|| {
        let selected = PathBuf::from(
            std::env::var_os("GAZETTE_TEST_DOT")
                .expect("native diagram qualification requires explicit GAZETTE_TEST_DOT"),
        );
        assert!(
            selected.is_absolute(),
            "explicit native provider path must be absolute"
        );
        let path = fs::canonicalize(selected).expect("exact installed Graphviz executable");
        let bytes = fs::read(&path).expect("independent native executable pin");
        process::PinnedExecutable {
            program: path.to_str().expect("native executable path").to_owned(),
            executable: process::ExecutablePin {
                bytes: bytes.len() as u64,
                sha256: source::Sha256Digest::of(&bytes),
            },
        }
    })
}

#[cfg(unix)]
fn canary_provider(root: &Path) -> (process::PinnedExecutable, PathBuf) {
    use std::os::unix::fs::PermissionsExt as _;

    let marker = root.join("provider-invoked");
    let program = root.join("dot-canary");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf invoked > {}\nexit 91\n",
            serde_json::to_string(marker.to_str().expect("marker path"))
                .expect("literal shell path")
        ),
    )
    .expect("canary provider bytes");
    let mut permissions = fs::metadata(&program)
        .expect("canary metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&program, permissions).expect("canary executable permission");
    let program = fs::canonicalize(program).expect("canonical canary path");
    let bytes = fs::read(&program).expect("canary provider pin");
    (
        process::PinnedExecutable {
            program: program.to_str().expect("canary path").to_owned(),
            executable: process::ExecutablePin {
                bytes: bytes.len() as u64,
                sha256: source::Sha256Digest::of(&bytes),
            },
        },
        marker,
    )
}

#[cfg(unix)]
fn forwarding_provider(root: &Path, revision: &str) -> process::PinnedExecutable {
    use std::os::unix::fs::PermissionsExt as _;

    let program = root.join("dot-forwarder");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\n# {revision}\nexec {} \"$@\"\n",
            serde_json::to_string(&executable().program).expect("literal Graphviz path")
        ),
    )
    .expect("forwarding provider bytes");
    let mut permissions = fs::metadata(&program)
        .expect("forwarder metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&program, permissions).expect("forwarder executable permission");
    let program = fs::canonicalize(program).expect("canonical forwarder path");
    let bytes = fs::read(&program).expect("forwarder provider pin");
    process::PinnedExecutable {
        program: program.to_str().expect("forwarder path").to_owned(),
        executable: process::ExecutablePin {
            bytes: bytes.len() as u64,
            sha256: source::Sha256Digest::of(&bytes),
        },
    }
}

fn diagram_fixture(overrides: &[(&str, &[u8])]) -> Fixture {
    let mut fixture = Fixture::new(false, overrides);
    let repository = Repository::open(fixture.root.path()).expect("owned fixture repository");
    let parent = repository
        .find_commit(fixture.commit)
        .expect("initial fixture pin");
    let mut index = repository.index().expect("fixture index");
    index
        .read_tree(&parent.tree().expect("committed fixture tree"))
        .expect("start from exact committed fixture");
    for path in [
        "content/main.ts",
        "content/dep.ts",
        "content/main.scss",
        "content/_tokens.scss",
    ] {
        fs::remove_file(fixture.root.path().join(path)).expect("remove only unused fixture input");
        index
            .remove_path(Path::new(path))
            .expect("remove fixture input from exact next tree");
    }
    let tree = repository
        .find_tree(index.write_tree().expect("isolated diagram tree"))
        .expect("tree");
    let signature = Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
        .expect("fixed fixture signature");
    fixture.commit = repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "select diagram-only authored inputs",
            &tree,
            &[&parent],
        )
        .expect("pin complete diagram fixture");
    fixture
}

fn fixture(dot: &str) -> Fixture {
    let first = page("one", "/one/", RESOURCE);
    diagram_fixture(&[
        ("content/one.md", first.as_bytes()),
        ("content/diagram.dot", dot.as_bytes()),
        ("theme/templates/page.html", TEMPLATE.as_bytes()),
        ("theme/templates/alias.html", TEMPLATE.as_bytes()),
    ])
}

fn config(fixture: &Fixture, selected: Option<&process::PinnedExecutable>) -> String {
    let base = fixture.configuration();
    let Some(command) = selected else {
        return base;
    };
    format!(
        "dot_renderer={{program={},executable={{bytes={},sha256='{}'}}}}\n{base}",
        serde_json::to_string(&command.program).expect("TOML-compatible program"),
        command.executable.bytes,
        command.executable.sha256,
    )
}

fn acquire_set(config: &str) -> build::AcquiredPublicationSet {
    build::acquire_publications(
        config,
        &["site:one".parse().expect("site")],
        Default::default(),
        Default::default(),
    )
    .expect("exact retained source acquisition")
}

fn site(inputs: &build::AcquiredPublicationSet) -> &build::AcquiredInputs {
    inputs.sites()[&"site:one".parse::<Key>().expect("site")]
        .as_ref()
        .expect("complete configured site")
}

fn permitted() -> build::DiagramGenerationOptions {
    build::DiagramGenerationOptions {
        execution: process::Options {
            allow_execute: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn request(config: &str, allow_execute: bool) -> Value {
    json!({
        "format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":config},
        "diagram_generation":{
            "allow_execute":allow_execute,"max_input_bytes":1048576,"timeout_ms":30000,
            "diagrams":16,"output_bytes":4194304,"diagnostic_bytes":65536
        },
        "sites":[{"site":"site:one",
            "model_output":"model.json"}]
    })
}

fn cli_request(request: &Value) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["render-sites", "--local"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual direct Gazette interface");
    child
        .stdin
        .take()
        .expect("private recipe input")
        .write_all(request.to_string().as_bytes())
        .expect("complete recipe transport");
    child
        .wait_with_output()
        .expect("native publication outcome")
}

#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn native_dot_png_reaches_theme_shared_model_cli_and_retained_replay() {
    let fixture = fixture(DOT);
    let selected = executable().clone();
    let config = config(&fixture, Some(&selected));
    let mut inputs = acquire_set(&config);
    assert!(
        render(site(&inputs)).is_err(),
        "no renderer-side native fallback"
    );
    build::acquire_diagrams(&mut inputs, Default::default(), permitted())
        .expect("actual selected Graphviz acquisition");
    let capture = serde_json::to_value(inputs.diagram_capture().expect("retained capture"))
        .expect("private native acquisition evidence");
    assert_eq!(
        capture["workspace_sha256"],
        json!(inputs.compilation().source_sha256)
    );
    let records = capture["records"]
        .as_array()
        .expect("exact acquired diagram records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["request"], json!(DOT.as_bytes()));
    assert_eq!(records[0]["diagnostics"], json!([]));
    assert_eq!(records[0]["exit"]["code"], 0);
    let rendered = render(site(&inputs)).expect("retained image enters publication");
    let tree = rendered.site().output();
    let png = bytes(tree, "assets/diagram.png");
    let pixels = raster::inspect(raster::Format::Png, png, Default::default())
        .expect("actual generated PNG pixels");
    assert!(pixels.width > 1 && pixels.height > 1);
    assert_eq!(records[0]["response"], json!(png));
    assert_eq!(records[0]["output_sha256"], json!(pixels.sha256));
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let resources = &model["pages"]["content:one"]["presentation"]["resources"];
    assert_eq!(resources.as_array().expect("ordered resources").len(), 1);
    assert_eq!(resources[0]["kind"], "png-image");
    assert_eq!(resources[0]["sha256"], json!(pixels.sha256));
    assert_eq!(
        resources[0]["image"]["output"],
        json!({"width":pixels.width,"height":pixels.height})
    );
    assert_eq!(
        resources[0]["url"],
        "https://one.example/mount/assets/diagram.png"
    );
    assert_eq!(
        model["pages"]["content:two"]["presentation"]["resources"],
        json!([])
    );
    for path in ["one/index.html", "old.html"] {
        let html = std::str::from_utf8(bytes(tree, path))
            .expect("Theme output")
            .replace("&#x2F;", "/");
        assert!(
            html.contains("src=\"https://one.example/mount/assets/diagram.png\""),
            "{html}"
        );
        assert!(html.contains(&format!(
            "width=\"{}\" height=\"{}\"",
            pixels.width, pixels.height
        )));
        assert!(html.contains("alt=\"Retained source flows into publication\""));
    }
    let native = cli_request(&request(&config, true));
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let report: Value = serde_json::from_slice(&native.stdout).expect("CLI set receipt");
    assert_eq!(report["status"], "local_set_rendered");
    assert_eq!(report["diagrams"], capture);
    assert_eq!(
        report["sites"]["site:one"]["inventory"],
        json!(tree.inventory())
    );
    let identity = tree.inventory().identity();
    let original_files: Vec<_> = tree
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            tree.file(&entry.path)
                .map(|bytes| (entry.path.clone(), bytes.to_vec()))
        })
        .collect();
    drop(rendered);
    fixture
        .root
        .close()
        .expect("retire only owned source repository");
    let replay = render(site(&inputs)).expect("retained render without source or provider cwd");
    assert_eq!(replay.site().output().inventory().identity(), identity);
    for (path, original) in original_files {
        assert_eq!(
            replay.site().output().file(&path),
            Some(original.as_slice())
        );
    }
    assert_eq!(
        serde_json::to_value(inputs.diagram_capture()).expect("unchanged capture"),
        capture
    );
}

#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn selected_theme_dot_resource_reaches_shared_and_page_projections() {
    let manifest = super::PLUGIN_MANIFEST
        .replace("inputs=['shared.ts']", "inputs=['diagram.dot']")
        .replace(
            "build={kind='browser-module',entry='shared.ts'}",
            "build={kind='dot-image',entry='diagram.dot'}",
        );
    let template = TEMPLATE.replace(
        "for resource in presentation.resources",
        "for resource in presentation.theme_resources",
    );
    let first = page("one", "/one/", "");
    let fixture = diagram_fixture(&[
        ("theme/plugin.toml", manifest.as_bytes()),
        ("theme/diagram.dot", DOT.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
        ("theme/templates/alias.html", template.as_bytes()),
        ("content/one.md", first.as_bytes()),
    ]);
    let config = config(&fixture, Some(executable()))
        .replace("output='/assets/shared.js'", "output='/assets/shared.png'");
    let mut inputs = acquire_set(&config);
    assert!(
        render(site(&inputs)).is_err(),
        "required capability has no native fallback"
    );
    build::acquire_diagrams(&mut inputs, Default::default(), permitted())
        .expect("selected Theme provider acquires its retained DOT");
    let capture = serde_json::to_value(inputs.diagram_capture()).expect("private capture");
    let records = capture["records"]
        .as_array()
        .expect("capability capture records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["sources"], json!(["source:theme"]));
    assert_eq!(records[0]["entry"], "diagram.dot");
    assert_eq!(records[0]["request"], json!(DOT.as_bytes()));
    let rendered = render(site(&inputs)).expect("retained capability renders");
    let output = rendered.site().output();
    let png = bytes(output, "assets/shared.png");
    let pixels = raster::inspect(raster::Format::Png, png, Default::default())
        .expect("native capability PNG is decoded");
    assert_eq!(records[0]["response"], json!(png));
    assert_eq!(records[0]["output_sha256"], json!(pixels.sha256));
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).expect("shared model");
    let shared = &model["resources"]["module:shared"];
    assert_eq!(shared["kind"], "png-image");
    assert_eq!(shared["sha256"], json!(pixels.sha256));
    assert_eq!(
        shared["image"]["output"],
        json!({"width":pixels.width,"height":pixels.height})
    );
    assert_eq!(shared["url"], "https://one.example/mount/assets/shared.png");
    for content in ["content:one", "content:two"] {
        let presentation = &model["pages"][content]["presentation"];
        let theme = presentation["theme_resources"]
            .as_array()
            .expect("Theme resource projection");
        assert_eq!(theme.len(), 1);
        assert_eq!(theme[0]["id"], "module:shared");
        for field in ["kind", "url", "output", "sha256", "bytes", "image"] {
            assert_eq!(theme[0][field], shared[field], "shared {field} projection");
        }
        assert_eq!(presentation["capabilities"], json!({}));
        assert_eq!(presentation["resources"], json!([]));
    }
    for path in ["one/index.html", "two/index.html", "old.html"] {
        let html = std::str::from_utf8(bytes(output, path))
            .expect("Theme image output")
            .replace("&#x2F;", "/");
        assert!(
            html.contains("src=\"https://one.example/mount/assets/shared.png\""),
            "{html}"
        );
    }
}

#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn required_diagram_without_provider_permission_or_capture_never_degrades() {
    let fixture = fixture(DOT);
    let mut inputs = acquire_set(&config(&fixture, None));
    let error = build::acquire_diagrams(&mut inputs, Default::default(), permitted())
        .expect_err("configured diagram needs provider");
    assert_eq!(error.code(), "diagram_unconfigured");
    assert!(inputs.diagram_capture().is_none());
    assert!(render(site(&inputs)).is_err());
    let config = config(&fixture, Some(executable()));
    let mut denied = acquire_set(&config);
    let error = build::acquire_diagrams(&mut denied, Default::default(), Default::default())
        .expect_err("configuration does not permit execution");
    assert_eq!(error.code(), "diagram_permission");
    assert!(denied.diagram_capture().is_none());
    let native = cli_request(&request(&config, false));
    assert!(!native.status.success());
    assert!(native.stdout.is_empty());
    assert!(String::from_utf8_lossy(&native.stderr).contains("diagram_permission"));
}

#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn unsupported_or_invalid_dot_refuses_before_native_candidate_not_partial_success() {
    for (dot, expected) in [
        (
            "digraph { node [image=\"absent-owned-image.png\"]; one; }",
            "diagram_profile",
        ),
        ("digraph { one ->", "diagram_profile"),
    ] {
        let fixture = fixture(dot);
        let config = config(&fixture, Some(executable()));
        let mut inputs = acquire_set(&config);
        let error = build::acquire_diagrams(&mut inputs, Default::default(), permitted())
            .expect_err("native warning or syntax failure is not a complete diagram");
        assert_eq!(error.code(), expected, "{error}");
        assert!(inputs.diagram_capture().is_none());
        assert!(render(site(&inputs)).is_err());
        let native = cli_request(&request(&config, true));
        assert!(!native.status.success());
        assert!(native.stdout.is_empty());
        assert!(String::from_utf8_lossy(&native.stderr).contains(expected));
    }
}

#[cfg(unix)]
#[test]
fn external_resource_dot_is_rejected_before_the_provider_can_run() {
    let provider_root = tempdir().expect("isolated provider canary");
    let external = provider_root.path().join("readable.png");
    fs::write(&external, b"readable external bytes").expect("readable external canary");
    let (provider, marker) = canary_provider(provider_root.path());
    let path = external.to_str().expect("external path");
    for dot in [
        format!("digraph {{ node [image={:?}]; node; }}", path),
        format!("digraph {{ graph [imagepath={:?}]; node; }}", path),
        format!("digraph {{ node [shapefile={:?}]; node; }}", path),
        format!("digraph {{ node [label=<<IMG SRC={:?}/>>]; }}", path),
    ] {
        let fixture = fixture(&dot);
        let config = config(&fixture, Some(&provider));
        let mut inputs = acquire_set(&config);
        let error = build::acquire_diagrams(&mut inputs, Default::default(), permitted())
            .expect_err("external-resource DOT is outside the admitted profile");
        assert_eq!(error.code(), "diagram_profile", "{dot}");
        assert!(!marker.exists(), "provider must not run for {dot}");
        assert!(inputs.diagram_capture().is_none());
    }
}

#[cfg(unix)]
#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn changed_provider_bytes_change_retained_diagram_dependency_evidence() {
    let fixture = fixture(DOT);
    let provider_root = tempdir().expect("isolated forwarding provider");
    let first = forwarding_provider(provider_root.path(), "provider-one");
    let mut first_inputs = acquire_set(&config(&fixture, Some(&first)));
    build::acquire_diagrams(&mut first_inputs, Default::default(), permitted())
        .expect("first provider capture");
    let first_capture =
        serde_json::to_value(first_inputs.diagram_capture()).expect("first retained capture");

    let second = forwarding_provider(provider_root.path(), "provider-two");
    assert_ne!(first.executable.sha256, second.executable.sha256);
    let mut second_inputs = acquire_set(&config(&fixture, Some(&second)));
    build::acquire_diagrams(&mut second_inputs, Default::default(), permitted())
        .expect("second provider capture");
    let second_capture =
        serde_json::to_value(second_inputs.diagram_capture()).expect("second retained capture");

    assert_ne!(
        first_capture["records"][0]["command_sha256"],
        second_capture["records"][0]["command_sha256"]
    );
    assert_eq!(
        first_capture["records"][0]["request"],
        second_capture["records"][0]["request"]
    );
    assert_eq!(
        first_capture["records"][0]["response"],
        second_capture["records"][0]["response"]
    );
}

#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn stale_executable_and_input_output_pixel_bounds_refuse_without_capture() {
    let fixture = fixture(DOT);
    for (case, expected) in [
        ("pin", "process_executable"),
        ("input", "diagram_limit"),
        ("output", "process_output_limit"),
        ("pixels", "diagram_raster"),
        ("count", "diagram_limit"),
    ] {
        let mut selected = executable().clone();
        let mut options = permitted();
        match case {
            "pin" => {
                selected.executable.sha256 = source::Sha256Digest::of(b"not selected executable")
            }
            "input" => options.execution.max_input_bytes = 1,
            "output" => options.output_bytes = 1,
            "pixels" => options.raster.width = 1,
            "count" => options.diagrams = 0,
            _ => unreachable!(),
        }
        let mut inputs = acquire_set(&config(&fixture, Some(&selected)));
        let error = build::acquire_diagrams(&mut inputs, Default::default(), options)
            .expect_err("bounded acquisition must refuse");
        assert_eq!(error.code(), expected, "{case}: {error}");
        assert!(inputs.diagram_capture().is_none(), "{case}");
        assert!(render(site(&inputs)).is_err(), "{case}");
    }
}
