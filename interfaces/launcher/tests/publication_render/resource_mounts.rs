//! Explicit retained Theme resource mounts through native compilers and local staging.

use gazette_sdk::workspace::Key;
use serde_json::{Value, json};
use std::error::Error as _;

use super::{
    Fixture, MANIFEST, TEMPLATE, acquire, acquire_result, build, bytes, cli, fs, render, sites,
    source, tempdir, toml,
};

const TOKENS: &[u8] = b"$ink: #123456;\n$space: 7px;\n";
const PREBUILT: &[u8] = b":root { --mounted-token: 11px; }\n";
const STYLE: &[u8] =
    b"@use 'vendor/tokens';\n.card { color: tokens.$ink; padding: tokens.$space; }\n";
const RESOURCES: &str = "resource_requirements=['style:scss','style:prebuilt']\n\
[[resources]]\nid='style:scss'\ninputs=['main.scss','vendor/_tokens.scss']\nbuild={kind='stylesheet',entry='main.scss'}\n\
[[resources]]\nid='style:prebuilt'\ninputs=['vendor/prebuilt.css']\nbuild={kind='stylesheet',entry='vendor/prebuilt.css'}\n";
const SELECTIONS: &str = "[composition.resources]\n\
'style:scss'={state='enabled',resource={owner={kind='theme',package='theme:one'},resource='style:scss'},output='/assets/world.css',use='site'}\n\
'style:prebuilt'={state='enabled',resource={owner={kind='theme',package='theme:one'},resource='style:prebuilt'},output='/assets/tokens.css',use='site'}\n";
const MOUNTS: &str = "[theme_packages.resource_inputs]\n\
'vendor/_tokens.scss'={input='tokens',entry='palette/_native.scss'}\n\
'vendor/prebuilt.css'={input='tokens',entry='built/native.css'}\n";

struct Mounted {
    publication: Fixture,
    tokens: super::TempDir,
}

impl Mounted {
    fn new(extra: &[(&str, &[u8])]) -> Self {
        let manifest = format!("{MANIFEST}{RESOURCES}");
        let template = format!(
            "{TEMPLATE}{{% for resource in presentation.theme_resources %}}<link rel=\"stylesheet\" href=\"{{{{ resource.url }}}}\">{{% endfor %}}"
        );
        let mut files = vec![
            ("theme/theme.toml", manifest.as_bytes()),
            ("theme/main.scss", STYLE),
            ("theme/templates/page.html", template.as_bytes()),
        ];
        files.extend_from_slice(extra);
        let publication = Fixture::new(&files);
        let tokens = tempdir().expect("explicit separate native token source");
        for (entry, content) in [
            ("palette/_native.scss", TOKENS),
            ("built/native.css", PREBUILT),
            ("palette/_secret.scss", b"$secret: red;\n".as_slice()),
            ("graph.dot", b"digraph G { a -> b; }\n".as_slice()),
        ] {
            let path = tokens.path().join(entry);
            fs::create_dir_all(path.parent().expect("exact entry parent"))
                .expect("test-owned token directories");
            fs::write(path, content).expect("actual retained token bytes");
        }
        Self {
            publication,
            tokens,
        }
    }

    fn config(&self) -> String {
        let mut config = self.publication.config().replacen(
            "theme={package='theme:one'}\n",
            &format!("theme={{package='theme:one'}}\n{SELECTIONS}"),
            1,
        );
        config.push_str(MOUNTS);
        config.push_str(&format!(
            "[design_inputs]\ntokens='source:tokens'\n\
             [[sources]]\nid='source:tokens'\nlocation={{kind='filesystem',root={:?}}}\n",
            self.tokens.path().to_str().expect("explicit native source")
        ));
        config
    }
}

fn key(value: &str) -> Key {
    value.parse().expect("declared key")
}

fn evidence(rendered: &build::PublicationRender<'_>, resource: &str) -> Value {
    serde_json::to_value(
        &rendered.site().evidence().resources[&resource.parse().expect("resource identity")],
    )
    .expect("private native evidence")
}

fn staged(config: &str) -> (super::TempDir, Value) {
    let parent = tempdir().expect("disjoint local stage parent");
    let result = cli(
        config,
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--model-output",
            "model.json",
            "--render-year",
            "2040",
            "--output-parent",
            parent.path().to_str().expect("stage parent"),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt = serde_json::from_slice(&result.stdout).expect("native stage receipt");
    (parent, receipt)
}

fn assert_no_stage(config: &str, code: &str) {
    let parent = tempdir().expect("failure preserves actual output parent");
    fs::write(parent.path().join("user.bin"), b"untouched\0\xff").expect("unrelated user bytes");
    let before = source::inspect(parent.path(), None, Default::default()).expect("before");
    let result = cli(
        config,
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--output-parent",
            parent.path().to_str().expect("stage parent"),
        ],
    );
    assert!(!result.status.success());
    assert!(result.stdout.is_empty(), "no partial stage receipt");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.starts_with(&format!("gazette: {code}")), "{stderr}");
    assert!(!stderr.contains("panicked"));
    let after = source::inspect(parent.path(), None, Default::default()).expect("after");
    assert_eq!(
        before, after,
        "no stage, temporary file or unrelated effect"
    );
}

#[test]
fn mounted_scss_and_prebuilt_css_reach_native_theme_model_and_exact_cli_stage() {
    let fixture = Mounted::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    assert_eq!(
        inputs.sources().len(),
        4,
        "two content trees, Theme and shared tokens"
    );
    let rendered = render(&inputs).expect("real native resource adapters");
    let tree = rendered.site().output();
    let css = std::str::from_utf8(bytes(tree, "assets/world.css")).expect("compiled CSS");
    assert!(css.contains("#123456") && css.contains("7px"), "{css}");
    assert!(!css.contains("@use") && !css.contains("$ink"));
    let prebuilt = std::str::from_utf8(bytes(tree, "assets/tokens.css")).expect("native CSS");
    assert!(prebuilt.contains("--mounted-token") && prebuilt.contains("11px"));
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("public model");
    let resources = &model["pages"]["content:one"]["presentation"]["theme_resources"];
    assert_eq!(
        resources.as_array().expect("ordered Theme resources").len(),
        2
    );
    for (id, output) in [
        ("style:scss", "assets/world.css"),
        ("style:prebuilt", "assets/tokens.css"),
    ] {
        assert_eq!(model["resources"][id]["kind"], "stylesheet");
        assert_eq!(
            model["resources"][id]["url"],
            format!("https://one.example/mount/{output}")
        );
        assert_eq!(
            model["resources"][id]["sha256"],
            json!(source::Sha256Digest::of(bytes(tree, output)))
        );
    }
    let proof = evidence(&rendered, "style:scss");
    assert_eq!(proof["source"], "source:theme");
    assert_eq!(
        proof["input_origins"]["main.scss"]["source"],
        "source:theme"
    );
    assert!(proof["input_origins"]["main.scss"].get("input").is_none());
    let mounted = &proof["input_origins"]["vendor/_tokens.scss"];
    assert_eq!(mounted["input"], "tokens");
    assert_eq!(mounted["source"], "source:tokens");
    assert_eq!(mounted["entry"], "palette/_native.scss");
    assert_eq!(mounted["sha256"], json!(source::Sha256Digest::of(TOKENS)));
    assert_eq!(
        mounted["tree"],
        json!(
            inputs.sources()[&key("source:tokens")]
                .inventory()
                .identity()
        )
    );
    assert_eq!(
        proof["dependencies"]["vendor/_tokens.scss"],
        json!(source::Sha256Digest::of(TOKENS))
    );
    assert_eq!(
        evidence(&rendered, "style:prebuilt")["input_origins"]["vendor/prebuilt.css"]["entry"],
        "built/native.css"
    );
    for private in [
        "input_origins",
        "palette/_native.scss",
        "source:tokens",
        fixture.tokens.path().to_str().expect("root"),
    ] {
        assert!(
            !std::str::from_utf8(bytes(tree, "model.json"))
                .expect("JSON")
                .contains(private)
        );
        assert!(
            !std::str::from_utf8(bytes(tree, "guide/index.html"))
                .expect("HTML")
                .contains(private)
        );
    }
    let (_parent, receipt) = staged(&config);
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("actual stage bytes"),
                bytes(tree, entry.path.as_str())
            );
        }
    }
    assert_eq!(receipt["evidence"]["resources"]["style:scss"], proof);
}

#[test]
fn retained_mounts_ignore_disk_changes_and_fresh_capture_changes_native_input_output_and_cache() {
    let fixture = Mounted::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let first = render(&inputs).expect("retained baseline");
    let before = first.site().output().inventory().identity();
    let first_proof = evidence(&first, "style:scss");
    let set = sites::acquire_set(&config, &["site:one"]);
    let model = "model.json".parse().expect("model");
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let options = sites::options(&["site:one"], &page, &alias, &model);
    let old_plan = build::plan_build_cache(&set, &options, Default::default())
        .expect("retained cache closure");
    let changed = b"$ink: #654321;\n$space: 13px;\n";
    fs::write(fixture.tokens.path().join("palette/_native.scss"), changed)
        .expect("owned source changes after capture");
    let replay = render(&inputs).expect("no native source reopening");
    assert_eq!(replay.site().output().inventory().identity(), before);
    assert_eq!(evidence(&replay, "style:scss"), first_proof);
    assert_eq!(
        build::plan_build_cache(&set, &options, Default::default()).expect("retained cache plan"),
        old_plan
    );
    let fresh = acquire(&config);
    let next = render(&fresh).expect("fresh actual capture");
    let proof = evidence(&next, "style:scss");
    assert_ne!(proof["input"], first_proof["input"]);
    assert_ne!(proof["output_sha256"], first_proof["output_sha256"]);
    assert_eq!(
        proof["input_origins"]["vendor/_tokens.scss"]["sha256"],
        json!(source::Sha256Digest::of(changed))
    );
    let css = std::str::from_utf8(bytes(next.site().output(), "assets/world.css")).expect("CSS");
    assert!(css.contains("#654321") && css.contains("13px"));
    let next_set = sites::acquire_set(&config, &["site:one"]);
    assert_ne!(
        build::plan_build_cache(&next_set, &options, Default::default())
            .expect("fresh cache closure"),
        old_plan
    );
}

#[test]
fn equal_byte_rebinding_changes_private_origins_and_configuration_not_public_resources() {
    let fixture = Mounted::new(&[]);
    let config = fixture.config();
    let rebound = config
        .replace("input='tokens'", "input='palette'")
        .replace("tokens='source:tokens'", "palette='source:tokens-copy'")
        .replace("id='source:tokens'", "id='source:tokens-copy'");
    let first = acquire(&config);
    let second = acquire(&rebound);
    assert_ne!(
        first.compilation().source_sha256,
        second.compilation().source_sha256
    );
    assert!(!second.sources().contains_key(&key("source:tokens")));
    let original = render(&first).expect("original mounted program");
    let revised = render(&second).expect("same retained bytes, explicit different origin");
    assert_eq!(
        original.site().output().inventory().identity(),
        revised.site().output().inventory().identity()
    );
    let original_proof = evidence(&original, "style:scss");
    let revised_proof = evidence(&revised, "style:scss");
    assert_eq!(original_proof["input"], revised_proof["input"]);
    assert_eq!(
        original_proof["dependencies"],
        revised_proof["dependencies"]
    );
    let origin = &revised_proof["input_origins"]["vendor/_tokens.scss"];
    assert_eq!(origin["input"], "palette");
    assert_eq!(origin["source"], "source:tokens-copy");
    assert_ne!(
        original_proof["input_origins"]["vendor/_tokens.scss"]["source_binding_sha256"],
        origin["source_binding_sha256"]
    );
    assert_eq!(
        original_proof["input_origins"]["vendor/_tokens.scss"]["tree"],
        origin["tree"]
    );
}

#[test]
fn ambiguous_missing_and_undeclared_mounts_fail_native_metadata_before_staging() {
    let collision = Mounted::new(&[("theme/vendor/_tokens.scss", TOKENS)]);
    let missing = Mounted::new(&[]);
    let undeclared = Mounted::new(&[]);
    for (config, expected_source, expected_path, expected_reason) in [
        (collision.config(), "source:theme", "vendor/_tokens.scss", "resource input is both package-local and mounted"),
        (missing.config().replace("entry='palette/_native.scss'", "entry='palette/missing.scss'"), "source:tokens", "palette/missing.scss", "Theme resource input must be a retained regular file"),
        (undeclared.config().replace("[theme_packages.resource_inputs]\n", "[theme_packages.resource_inputs]\n'undeclared.scss'={input='tokens',entry='palette/_native.scss'}\n"), "source:theme", "undeclared.scss", "mounted target is not a declared Theme resource input"),
    ] {
        let error = acquire_result(&config).expect_err("SDK metadata gate returns no partial acquisition");
        assert_eq!(error.code(), "package_input");
        let build::CompositionError::Capability(capability) = error else {
            panic!("exact early native metadata gate required")
        };
        let Some(build::MetadataError::Input { source, path, reason }) = capability.source().and_then(|error| error.downcast_ref::<build::MetadataError>()) else {
            panic!("exact native metadata input diagnostic required")
        };
        assert_eq!(source.as_str(), expected_source);
        assert_eq!(path.as_str(), expected_path);
        assert_eq!(*reason, expected_reason);
        assert_no_stage(&config, "package_input");
    }
}

#[test]
fn a_retained_but_undeclared_external_import_is_not_an_ambient_stylesheet_input() {
    let fixture = Mounted::new(&[(
        "theme/main.scss",
        b"@use 'vendor/secret';\n.card { color: secret.$secret; }\n",
    )]);
    let config = fixture.config();
    let inputs = acquire(&config);
    assert!(
        inputs.sources()[&key("source:tokens")]
            .file(&"palette/_secret.scss".parse().expect("exact source entry"))
            .is_some()
    );
    inputs
        .check_metadata(Default::default())
        .expect("declared mount selection remains valid");
    let error = render(&inputs).expect_err("native Grass cannot read an unmounted source sibling");
    assert!(error.code().starts_with("resource_"), "{error}");
    assert_no_stage(&config, "resource_");
}

#[test]
fn unselected_theme_mounts_do_not_capture_unavailable_sources_or_admit_incidental_packages() {
    let fixture = Mounted::new(&[]);
    let config = format!(
        "{}\
        [[sources]]\nid='source:unselected'\nlocation={{kind='filesystem',root={:?}}}\n\
        [[theme_packages]]\npackage='theme:unselected'\nsource='source:tokens'\nmanifest='absent-theme.toml'\n\
        resource_inputs={{'unused.scss'={{input='unused',entry='absent.scss'}}}}\n\
        [design_inputs]\ntokens='source:tokens'\nunused='source:unselected'\n",
        fixture
            .config()
            .replace("[design_inputs]\ntokens='source:tokens'\n", ""),
        fixture
            .tokens
            .path()
            .join("not-created")
            .to_str()
            .expect("unselected explicit root")
    );
    let inputs = acquire(&config);
    assert_eq!(inputs.sources().len(), 4);
    assert!(!inputs.sources().contains_key(&key("source:unselected")));
    let metadata = inputs
        .check_metadata(Default::default())
        .expect("incidental capture is not selected Theme identity");
    assert_eq!(metadata.inspect().theme_packages.len(), 1);
    render(&inputs).expect("unselected mounts neither read nor compiled");
}

#[test]
fn dependency_theme_mounts_share_one_native_capture_across_sites_and_count_against_bounds() {
    let root_manifest =
        format!("{MANIFEST}dependencies=[{{package='theme:foundation'}}]\n{RESOURCES}");
    let fixture = Mounted::new(&[("theme/theme.toml", root_manifest.as_bytes())]);
    let foundation_manifest = "package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\nresource_requirements=['style:foundation']\n[[resources]]\nid='style:foundation'\ninputs=['foundation.scss','vendor/_tokens.scss']\nbuild={kind='stylesheet',entry='foundation.scss'}\n";
    let foundation = Fixture::new(&[
        ("theme/theme.toml", foundation_manifest.as_bytes()),
        (
            "theme/foundation.scss",
            b"@use 'vendor/tokens';\n.foundation { margin: tokens.$space; }\n",
        ),
    ]);
    let config = format!("{}\
        [[sources]]\nid='source:foundation'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n\
        [[theme_packages]]\npackage='theme:foundation'\nsource='source:foundation'\nmanifest='theme.toml'\n\
        resource_inputs={{'vendor/_tokens.scss'={{input='tokens',entry='palette/_native.scss'}}}}\n",
        fixture.config().replace("[composition.resources]\n", "[composition.resources]\n'style:foundation'={state='enabled',resource={owner={kind='theme',package='theme:foundation'},resource='style:foundation'},output='/assets/foundation.css',use='site'}\n"),
        foundation.directory.path().to_str().expect("dependency repository"), foundation.commit);
    let selected = ["site:one", "site:other"];
    let inputs = sites::acquire_set(&config, &selected);
    assert_eq!(
        inputs.sources().len(),
        5,
        "shared tokens captured once, not per package or site"
    );
    let one = inputs.sites()[&key("site:one")]
        .as_ref()
        .expect("first site capture");
    let other = inputs.sites()[&key("site:other")]
        .as_ref()
        .expect("second site capture");
    assert!(std::ptr::eq(
        &one.sources()[&key("source:tokens")],
        &other.sources()[&key("source:tokens")]
    ));
    let model = "model.json".parse().expect("model");
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let rendered = build::render_publications(
        &inputs,
        sites::options(&selected, &page, &alias, &model),
        Default::default(),
    )
    .expect("native shared graph render");
    assert!(rendered.is_complete());
    for site in selected {
        let output = rendered.sites()[&key(site)]
            .as_ref()
            .expect("complete site output");
        let css = std::str::from_utf8(bytes(output.site().output(), "assets/foundation.css"))
            .expect("native dependency CSS");
        assert!(css.contains(".foundation") && css.contains("7px"));
        assert_eq!(
            evidence(output, "style:foundation")["input_origins"]["vendor/_tokens.scss"]["source"],
            "source:tokens"
        );
    }
    let limited = build::PublicationSetAcquisitionLimits {
        sources: source::PreparationLimits {
            sources: 4,
            ..build::PublicationSetAcquisitionLimits::default().sources
        },
        ..Default::default()
    };
    let error = build::acquire_publications(
        &config,
        &selected.map(key),
        toml::Limits::default(),
        limited,
    )
    .expect_err("dependency capture must not bypass union source limit");
    assert_eq!(error.code(), "composition_source_limit");
}

fn mounted_dot_fixture() -> (Mounted, String) {
    let manifest = format!(
        "{MANIFEST}{}\n[[resources]]\nid='image:diagram'\ninputs=['images/main.dot']\nbuild={{kind='dot-image',entry='images/main.dot'}}\n",
        RESOURCES.replace("'style:prebuilt']", "'style:prebuilt','image:diagram']")
    );
    let template = format!(
        "{TEMPLATE}{{% for resource in presentation.theme_resources %}}{{% if resource.id == 'image:diagram' %}}<img src=\"{{{{ resource.url }}}}\" width=\"{{{{ resource.image.output.width }}}}\" height=\"{{{{ resource.image.output.height }}}}\" alt=\"Retained mounted graph\">{{% endif %}}{{% endfor %}}"
    );
    let fixture = Mounted::new(&[
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let config = fixture
        .config()
        .replace(
            "[theme_packages.resource_inputs]\n",
            "[theme_packages.resource_inputs]\n'images/main.dot'={input='tokens',entry='graph.dot'}\n",
        )
        .replace(
            "[composition.resources]\n",
            "[composition.resources]\n'image:diagram'={state='enabled',resource={owner={kind='theme',package='theme:one'},resource='image:diagram'},output='/assets/diagram.png',use='site'}\n",
        );
    (fixture, config)
}

fn dot_config(config: &str, selected: &gazette_sdk::process::PinnedExecutable) -> String {
    format!(
        "dot_renderer={{program={},executable={{bytes={},sha256='{}'}}}}\n{config}",
        serde_json::to_string(&selected.program).expect("literal native executable path"),
        selected.executable.bytes,
        selected.executable.sha256
    )
}

#[test]
fn mounted_dot_requires_configured_provider_and_explicit_execution_permission() {
    let (fixture, config) = mounted_dot_fixture();
    let permitted = build::DiagramGenerationOptions {
        execution: gazette_sdk::process::Options {
            allow_execute: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // A nonexistent provider is deliberately never opened: permission is checked first.
    let unselected = gazette_sdk::process::PinnedExecutable {
        program: fixture
            .tokens
            .path()
            .join("not-created-dot")
            .to_str()
            .expect("path")
            .into(),
        executable: gazette_sdk::process::ExecutablePin {
            bytes: 1,
            sha256: source::Sha256Digest::of(b"x"),
        },
    };
    for (config, options, code) in [
        (config.clone(), permitted, "diagram_unconfigured"),
        (
            dot_config(&config, &unselected),
            Default::default(),
            "diagram_permission",
        ),
    ] {
        let mut inputs = sites::acquire_set(&config, &["site:one", "site:other"]);
        let error = build::acquire_diagrams(&mut inputs, Default::default(), options)
            .expect_err("no implicit native execution");
        assert_eq!(error.code(), code);
        let refusal = serde_json::to_value(error).expect("original effect custody");
        assert_eq!(refusal["completed"], json!([]));
        assert!(refusal["active"].is_null());
        assert!(inputs.diagram_capture().is_none());
        for view in inputs.sites().values() {
            assert!(render(view.as_ref().expect("complete source admission")).is_err());
        }
        assert_no_stage(&config, "resource_input");
    }
}

#[cfg(unix)]
#[test]
#[ignore = "requires explicit real Graphviz provider qualification"]
fn mounted_dot_native_generation_and_mcp_stage_preserve_original_and_virtual_origins() {
    use std::os::unix::fs::PermissionsExt as _;

    let real = std::path::PathBuf::from(
        std::env::var_os("GAZETTE_TEST_DOT").expect("explicit native Graphviz qualification"),
    );
    assert!(real.is_absolute(), "no provider discovery or installation");
    let real = fs::canonicalize(real).expect("selected installed Graphviz");
    fs::read(&real).expect("actual selected executable is readable");
    let provider = tempdir().expect("small owned native forwarding provider");
    let marker = provider.path().join("invocations");
    let program = provider.path().join("dot-forwarder");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf 'invoked\\n' >> {}\nexec {} \"$@\"\n",
            serde_json::to_string(marker.to_str().expect("counter path")).expect("literal path"),
            serde_json::to_string(real.to_str().expect("Graphviz path")).expect("literal path")
        ),
    )
    .expect("forward to the actual native provider, never synthesize PNG");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).expect("owned executable");
    let pin = fs::read(&program).expect("exact forwarding executable bytes");
    let selected = gazette_sdk::process::PinnedExecutable {
        program: program.to_str().expect("absolute provider").into(),
        executable: gazette_sdk::process::ExecutablePin {
            bytes: pin.len() as u64,
            sha256: source::Sha256Digest::of(&pin),
        },
    };
    let (fixture, config) = mounted_dot_fixture();
    let config = dot_config(&config, &selected);
    let mut inputs = sites::acquire_set(&config, &["site:one", "site:other"]);
    let original = inputs.sources()[&key("source:tokens")]
        .file(&"graph.dot".parse().expect("original entry"))
        .expect("retained DOT")
        .to_vec();
    let options = build::DiagramGenerationOptions {
        execution: gazette_sdk::process::Options {
            allow_execute: true,
            ..Default::default()
        },
        diagrams: 1,
        ..Default::default()
    };
    build::acquire_diagrams(&mut inputs, Default::default(), options)
        .expect("one real native job shared across both sites");
    assert_eq!(
        fs::read(&marker).expect("native execution counter"),
        b"invoked\n"
    );
    let capture = serde_json::to_value(inputs.diagram_capture()).expect("private native receipt");
    let records = capture["records"].as_array().expect("native records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["entry"], "graph.dot");
    assert_eq!(records[0]["sources"], json!(["source:tokens"]));
    assert_eq!(
        records[0]["input"],
        json!(
            inputs.sources()[&key("source:tokens")]
                .inventory()
                .identity()
        )
    );
    assert_eq!(records[0]["request"], json!(original));
    assert_eq!(records[0]["diagnostics"], json!([]));
    assert_eq!(records[0]["exit"]["code"], 0);
    let one = inputs.sites()[&key("site:one")]
        .as_ref()
        .expect("selected site");
    let rendered = render(one).expect("retained native PNG and native Sass/CSS");
    let png = bytes(rendered.site().output(), "assets/diagram.png");
    let expected_png = png.to_vec();
    let geometry =
        gazette_sdk::raster::inspect(gazette_sdk::raster::Format::Png, png, Default::default())
            .expect("actual Graphviz output decoded by native raster adapter");
    assert!(geometry.width > 1 && geometry.height > 1);
    assert_eq!(records[0]["response"], json!(png));
    let proof = evidence(&rendered, "image:diagram");
    let origin = &proof["input_origins"]["images/main.dot"];
    assert_eq!(origin["input"], "tokens");
    assert_eq!(origin["source"], "source:tokens");
    assert_eq!(origin["entry"], "graph.dot");
    assert_eq!(origin["tree"], records[0]["input"]);
    assert_eq!(origin["sha256"], json!(source::Sha256Digest::of(&original)));
    assert_ne!(
        proof["input"], origin["tree"],
        "virtual package identity is not the original whole source"
    );
    assert_eq!(
        proof["dependencies"],
        json!({"images/main.dot":source::Sha256Digest::of(&original)})
    );
    let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
        .expect("public projection");
    assert_eq!(model["resources"]["image:diagram"]["kind"], "png-image");
    assert_eq!(
        model["resources"]["image:diagram"]["image"]["output"],
        json!({"width":geometry.width,"height":geometry.height})
    );
    assert_eq!(
        model["resources"]["image:diagram"]["url"],
        "https://one.example/mount/assets/diagram.png"
    );
    let html = std::str::from_utf8(bytes(rendered.site().output(), "guide/index.html"))
        .expect("native Theme HTML")
        .replace("&#x2F;", "/");
    assert!(
        html.contains("src=\"https://one.example/mount/assets/diagram.png\""),
        "{html}"
    );
    assert!(html.contains(&format!(
        "width=\"{}\" height=\"{}\"",
        geometry.width, geometry.height
    )));
    assert!(html.contains("alt=\"Retained mounted graph\""));

    let parent = tempdir().expect("actual initialized MCP stage parent");
    super::mcp_carrier::initialized_interact("2025-11-25", |_, send| {
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("initialized native catalog")
            .iter()
            .find(|tool| tool["name"] == "stage_rendered_site")
            .expect("native staging contract");
        let input_schema =
            jsonschema::validator_for(&tool["inputSchema"]).expect("native input schema");
        let output_schema =
            jsonschema::validator_for(&tool["outputSchema"]).expect("native output schema");
        let arguments = json!({"publication":{
            "recipe_json":json!({"format":"gazette-local-publication-set/1",
                "workspace":{"kind":"supplied","source":config},
                "diagram_generation":{"allow_execute":true,"max_input_bytes":1048576,"timeout_ms":30000,
                    "diagrams":1,"output_bytes":4194304,"diagnostic_bytes":65536},
                "sites":[{"site":"site:one","model_output":"model.json"}]}).to_string(),
            "local":true,"allow_read":true},"create_stage":true,
            "output_parent":parent.path(),"directory_bytes":1024});
        input_schema
            .validate(&arguments)
            .expect("original native recipe contract");
        let response = send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"stage_rendered_site","arguments":arguments}}));
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
        let receipt = &response["result"]["structuredContent"];
        output_schema
            .validate(receipt)
            .expect("original typed native receipt");
        assert_eq!(
            receipt["inventory"],
            json!(rendered.site().output().inventory())
        );
        assert_eq!(receipt["evidence"]["resources"]["image:diagram"], proof);
        let directory = std::path::Path::new(receipt["directory"].as_str().expect("native stage"));
        assert_eq!(
            fs::read(directory.join("assets/diagram.png")).expect("staged native PNG"),
            png
        );
        assert_eq!(
            source::inspect(directory, None, Default::default()).expect("read-back"),
            *rendered.site().output().inventory()
        );
    });
    assert_eq!(
        fs::read(&marker).expect("one generation in each independent native request"),
        b"invoked\ninvoked\n"
    );
    let inventory = rendered.site().output().inventory().identity();
    drop(rendered);
    fs::write(
        fixture.tokens.path().join("graph.dot"),
        b"digraph Changed { x -> y; }\n",
    )
    .expect("change only original disk source");
    fs::remove_file(&program).expect("retire only owned forwarding provider before replay");
    for view in inputs.sites().values() {
        let replay = render(view.as_ref().expect("retained selected site"))
            .expect("no provider or disk reread on replay");
        assert_eq!(
            bytes(replay.site().output(), "assets/diagram.png"),
            expected_png
        );
        assert_eq!(
            evidence(&replay, "image:diagram")["dependencies"],
            proof["dependencies"]
        );
    }
    assert_eq!(
        render(inputs.sites()[&key("site:one")].as_ref().expect("site"))
            .expect("retained replay")
            .site()
            .output()
            .inventory()
            .identity(),
        inventory
    );
    assert_eq!(
        build::acquire_diagrams(&mut inputs, Default::default(), options)
            .expect_err("never rerun an already retained acquisition")
            .code(),
        "diagram_already_captured"
    );
    assert_eq!(
        fs::read(marker).expect("no replay execution"),
        b"invoked\ninvoked\n"
    );
    assert_eq!(
        serde_json::to_value(inputs.diagram_capture()).expect("unchanged original acquisition"),
        capture
    );
}

#[test]
fn dependency_only_mounts_are_discovered_once_with_exact_native_union_budgets() {
    let manifest = format!("{MANIFEST}dependencies=[{{package='theme:dependency'}}]\n{RESOURCES}");
    let fixture = Mounted::new(&[("theme/theme.toml", manifest.as_bytes())]);
    let dependency_manifest = "package='theme:dependency'\nnamespace='dependency'\ntemplates='templates'\n[[resources]]\nid='style:unselected'\ninputs=['vendor/retained.css']\nbuild={kind='stylesheet',entry='vendor/retained.css'}\n";
    let dependency = Fixture::new(&[("theme/theme.toml", dependency_manifest.as_bytes())]);
    let external = tempdir().expect("distinct dependency-only mounted input root");
    let external_bytes = b".dependency-only { margin: 19px; }\n";
    fs::write(external.path().join("declared.css"), external_bytes)
        .expect("actual dependency-only resource bytes");
    let config = format!(
        "{}\
        [[sources]]\nid='source:dependency-theme'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n\
        [[sources]]\nid='source:dependency-input'\nlocation={{kind='filesystem',root={:?}}}\n\
        [[sources]]\nid='source:unrelated'\nlocation={{kind='filesystem',root={:?}}}\n\
        [[theme_packages]]\npackage='theme:dependency'\nsource='source:dependency-theme'\nmanifest='theme.toml'\n\
        resource_inputs={{'vendor/retained.css'={{input='dependency',entry='declared.css'}}}}\n\
        [[theme_packages]]\npackage='theme:unrelated'\nsource='source:unrelated'\nmanifest='missing.toml'\n\
        resource_inputs={{'unused.css'={{input='unused',entry='missing.css'}}}}\n",
        fixture.config().replace(
            "[design_inputs]\n",
            "[design_inputs]\ndependency='source:dependency-input'\nunused='source:unrelated'\n"
        ),
        dependency
            .directory
            .path()
            .to_str()
            .expect("dependency repository"),
        dependency.commit,
        external.path().to_str().expect("dependency input root"),
        external
            .path()
            .join("not-created")
            .to_str()
            .expect("unselected root")
    );
    let selected = ["site:one", "site:other"];
    let inputs = sites::acquire_set(&config, &selected);
    assert_eq!(inputs.sources().len(), 6);
    assert!(!inputs.sources().contains_key(&key("source:unrelated")));
    let captured = &inputs.sources()[&key("source:dependency-input")];
    assert_eq!(
        captured.file(&"declared.css".parse().expect("exact entry")),
        Some(external_bytes.as_slice())
    );
    for site in selected {
        let view = inputs.sites()[&key(site)]
            .as_ref()
            .expect("complete selected site");
        assert!(std::ptr::eq(
            captured,
            &view.sources()[&key("source:dependency-input")]
        ));
        assert!(
            view.compilation()
                .workspace
                .composition_for(&key(site))
                .expect("composition")
                .resources()
                .values()
                .all(|selection| !matches!(selection.value(),
                gazette_sdk::workspace::ResourceSelection::Enabled { resource, .. }
                if matches!(&resource.owner, gazette_sdk::workspace::ResourceOwner::Theme(package)
                    if package.as_str() == "theme:dependency")))
        );
        let metadata = view
            .check_metadata(Default::default())
            .expect("dependency admission from exact retained bytes");
        let inspection =
            serde_json::to_value(metadata.inspect()).expect("native package-origin inspection");
        let origin = &inspection["theme_packages"]["theme:dependency"]["input_origins"]["vendor/retained.css"];
        assert_eq!(origin["input"], "dependency");
        assert_eq!(origin["source"], "source:dependency-input");
        assert_eq!(origin["entry"], "declared.css");
        assert_eq!(
            origin["sha256"],
            json!(source::Sha256Digest::of(external_bytes))
        );
        assert!(
            inspection["theme_packages"]
                .get("theme:unrelated")
                .is_none()
        );
        let output = render(view).expect("dependency admission does not activate its resource");
        assert!(
            !output
                .site()
                .evidence()
                .resources
                .contains_key(&"style:unselected".parse().expect("resource"))
        );
    }
    let entries = inputs
        .sources()
        .values()
        .map(|input| input.inventory().entries().len())
        .sum();
    let bytes = inputs
        .sources()
        .values()
        .map(|input| input.inventory().total_bytes())
        .sum();
    let exact = build::PublicationSetAcquisitionLimits {
        sources: source::PreparationLimits {
            sources: 6,
            entries,
            bytes,
            ..build::PublicationSetAcquisitionLimits::default().sources
        },
        ..Default::default()
    };
    let bounded =
        build::acquire_publications(&config, &selected.map(key), toml::Limits::default(), exact)
            .expect("exact union budget proves dependency inputs are not recaptured per site");
    assert_eq!(bounded.sources().len(), 6);
    assert!(bounded.sites().values().all(Result::is_ok));
    for sources in [
        source::PreparationLimits {
            sources: 5,
            ..exact.sources
        },
        source::PreparationLimits {
            entries: entries - 1,
            ..exact.sources
        },
        source::PreparationLimits {
            bytes: bytes - 1,
            ..exact.sources
        },
    ] {
        match build::acquire_publications(
            &config,
            &selected.map(key),
            toml::Limits::default(),
            build::PublicationSetAcquisitionLimits { sources, ..exact },
        ) {
            Err(error) => assert!(matches!(error, build::PublicationSetError::Acquisition(_))),
            Ok(incomplete) => {
                assert_eq!(
                    incomplete.sites().len(),
                    2,
                    "no silently shortened selection"
                );
                assert!(
                    incomplete.sites().values().all(Result::is_err),
                    "both consumers retain the shared prerequisite failure"
                );
                assert!(!incomplete.source_failures().is_empty());
            }
        }
    }
}

#[test]
fn initialized_mcp_rejects_missing_mount_then_stages_sdk_and_cli_identical_native_resources() {
    let fixture = Mounted::new(&[]);
    let config = fixture.config();
    let bad = config.replace(
        "entry='palette/_native.scss'",
        "entry='palette/absent.scss'",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("SDK native mounted compilation");
    let (_cli_parent, cli_receipt) = staged(&config);
    let parent = tempdir().expect("initialized MCP's explicit local stage parent");
    fs::write(parent.path().join("keep"), b"unrelated user bytes").expect("unrelated file");
    let before = source::inspect(parent.path(), None, Default::default()).expect("before request");
    let token_before = source::inspect(fixture.tokens.path(), None, Default::default())
        .expect("selected source inventory");
    super::mcp_carrier::initialized_interact("2025-11-25", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        assert!(catalog.get("error").is_none(), "{catalog}");
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("initialized catalog")
            .iter()
            .find(|tool| tool["name"] == "stage_rendered_site")
            .expect("existing native staging operation");
        let input_schema = jsonschema::validator_for(&tool["inputSchema"]).expect("input schema");
        let output_schema =
            jsonschema::validator_for(&tool["outputSchema"]).expect("output schema");
        for (id, workspace, failed) in [(2, bad.as_str(), true), (3, config.as_str(), false)] {
            let arguments = json!({
                "publication":{
                    "recipe_json":json!({"format":"gazette-local-publication-set/1","render_year":2040,
                        "workspace":{"kind":"supplied","source":workspace},
                        "sites":[{"site":"site:one","model_output":"model.json"}]}).to_string(),
                    "local":true,"allow_read":true
                },
                "create_stage":true,"output_parent":parent.path(),"directory_bytes":1024
            });
            input_schema
                .validate(&arguments)
                .expect("actual existing native request shape");
            let response = send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"stage_rendered_site","arguments":arguments}}));
            assert!(
                response.get("error").is_none(),
                "healthy initialized transport: {response}"
            );
            assert_eq!(response["result"]["isError"], failed, "{response}");
            let value = &response["result"]["structuredContent"];
            output_schema
                .validate(value)
                .expect("typed original native result");
            let text: Value = serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .expect("readable original result"),
            )
            .expect("JSON result");
            assert_eq!(text, *value);
            if failed {
                assert_eq!(value["code"], "package_input");
                assert_eq!(
                    source::inspect(parent.path(), None, Default::default())
                        .expect("no rejected-request effects"),
                    before
                );
            } else {
                assert_eq!(value["inventory"], cli_receipt["inventory"]);
                assert_eq!(value["evidence"], cli_receipt["evidence"]);
                assert_eq!(value["inputs"], cli_receipt["inputs"]);
                let directory =
                    std::path::Path::new(value["directory"].as_str().expect("real stage"));
                assert_eq!(
                    source::inspect(directory, None, Default::default()).expect("read-back"),
                    *rendered.site().output().inventory()
                );
                for entry in rendered.site().output().inventory().entries() {
                    if matches!(entry.content, source::Content::File { .. }) {
                        assert_eq!(
                            fs::read(directory.join(entry.path.as_str()))
                                .expect("MCP staged native bytes"),
                            bytes(rendered.site().output(), entry.path.as_str())
                        );
                    }
                }
                for resource in ["style:scss", "style:prebuilt"] {
                    assert_eq!(
                        value["evidence"]["resources"][resource],
                        evidence(&rendered, resource)
                    );
                }
                assert_eq!(
                    fs::read_dir(parent.path())
                        .expect("one stage plus user file")
                        .count(),
                    2
                );
            }
            assert_eq!(
                fs::read(parent.path().join("keep")).expect("user data retained"),
                b"unrelated user bytes"
            );
        }
    });
    assert_eq!(
        source::inspect(fixture.tokens.path(), None, Default::default())
            .expect("native mounted inputs unchanged"),
        token_before
    );
}
