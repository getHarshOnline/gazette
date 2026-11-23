//! Real retained publication, producer media and native stage custody through the tool owner.

use super::*;
use serde_json::json;
use std::{collections::BTreeMap, fs};

struct Fixture {
    root: tempfile::TempDir,
    parent: tempfile::TempDir,
    config: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture source");
        let parent = tempfile::tempdir().expect("private output parent");
        for (path, body) in [
            (
                "theme/theme.toml",
                "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n",
            ),
            (
                "theme/templates/page.html",
                "<p>Secret&#32;Value</p>{{ body_html | safe }}",
            ),
            (
                "content/home.md",
                "+++\n[content]\nid='content:home'\nsite='site:one'\ntitle='Secret Value'\nlifecycle='draft'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Kept content\n",
            ),
        ] {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            fs::write(path, body).expect("fixture bytes");
        }
        for index in 0..24 {
            fs::write(
                root.path().join(format!("content/asset-{index}.txt")),
                b"exact native fixture asset",
            )
            .expect("bounded actual asset set");
        }
        let repo = git2::Repository::init(root.path()).expect("fixture Git");
        let mut index = repo.index().expect("index");
        index
            .add_all(["theme", "content"], git2::IndexAddOption::DEFAULT, None)
            .expect("fixture tracking");
        let tree = repo
            .find_tree(index.write_tree().expect("tree"))
            .expect("object");
        let sig =
            git2::Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("signature");
        let commit = repo
            .commit(Some("HEAD"), &sig, &sig, "fixture", &tree, &[])
            .expect("fixture commit");
        let config = format!(
            "[composition]\ntheme={{package='theme:one'}}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Fixture'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.invalid/'\nauthored=[{{source='source:content',target='content',static_assets={{route_prefix='/media/'}}}}]\n[[sources]]\nid='source:theme'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='theme'}}\n[[sources]]\nid='source:content'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='content'}}\n[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
            root.path().to_str().expect("path"),
            root.path().to_str().expect("path")
        );
        drop(tree);
        drop(repo);
        Self {
            root,
            parent,
            config,
        }
    }
    fn args(&self, route: Route) -> Value {
        let recipe = json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":self.config},"sites":[{"site":"site:one","model_output":"model.data"}]});
        let mut args = json!({"publication":{"recipe_json":recipe.to_string(),"local":true,"allow_read":true}});
        match route {
            Route::Stage=>{args["create_stage"]=json!(true);args["output_parent"]=json!(self.parent.path());args["directory_bytes"]=json!(1024);},
            Route::Probe=>args["rules_json"]=json!(json!({"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:secret","literal":"Secret Value"}]}).to_string()),
        }
        args
    }
    fn captured(&self) -> build::AcquiredInputs {
        build::acquire_publication(
            &self.config,
            &"site:one".parse().expect("site"),
            Default::default(),
            source::PreparationLimits {
                sources: 8,
                entries: 256,
                bytes: 1 << 20,
                per_source: Default::default(),
            },
        )
        .expect("independent native acquisition")
    }
}

fn run(route: Route, args: Value, maximum: usize) -> Result<CallToolResult, ErrorData> {
    route
        .decode(args.as_object().expect("args").clone())?
        .execute(maximum)
}
fn output(route: Route, response: CallToolResult, failed: bool) -> Value {
    assert_eq!(response.is_error, Some(failed));
    let result = response.structured_content.expect("native result");
    let tool = json!(route.descriptor());
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("schema")
        .validate(&result)
        .expect("actual output schema");
    result
}
fn render(inputs: &build::AcquiredInputs) -> build::PublicationRender<'_> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_cli::calendar::resolve(None).expect("MCP invocation year")),
            model_output: Some(&"model.data".parse().expect("model")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("native render")
}

#[test]
fn shared_producer_stages_exact_bytes_and_retains_original_render_evidence() {
    let fixture = Fixture::new();
    let inputs = fixture.captured();
    let rendered = render(&inputs);
    let before = source::inspect(fixture.root.path(), None, Default::default())
        .expect("source snapshot")
        .identity();
    let report = output(
        Route::Stage,
        run(Route::Stage, fixture.args(Route::Stage), 1 << 20).expect("real tool"),
        false,
    );
    assert_eq!(report["evidence"], json!(rendered.site().evidence()));
    assert_eq!(
        report["inventory"],
        json!(rendered.site().output().inventory())
    );
    let path = Path::new(report["directory"].as_str().expect("retained path"));
    let captured = source::capture_pinned(
        path,
        rendered.site().output().inventory().identity(),
        Default::default(),
    )
    .expect("independent native readback");
    for entry in captured.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                captured.file(&entry.path),
                rendered.site().output().file(&entry.path)
            );
        }
    }
    assert_eq!(
        source::inspect(fixture.root.path(), None, Default::default())
            .expect("source unchanged")
            .identity(),
        before
    );
}

#[test]
fn producer_probe_uses_original_media_map_and_preserves_partial_evidence() {
    let fixture = Fixture::new();
    let inputs = fixture.captured();
    let rendered = render(&inputs);
    let rules = BTreeMap::from([(
        "rule:secret".parse().expect("rule"),
        "Secret Value".to_owned(),
    )]);
    let expected = rendered
        .site()
        .probe_disclosure(&rules, Default::default())
        .expect("native producer probe");
    let actual = output(
        Route::Probe,
        run(Route::Probe, fixture.args(Route::Probe), 1 << 20).expect("tool"),
        false,
    );
    assert_eq!(actual, json!(expected));
    let imported = build::probe_disclosure(
        rendered.site().output(),
        &BTreeMap::new(),
        &rules,
        Default::default(),
    )
    .expect("imported suffix-only comparison");
    assert_ne!(
        actual,
        json!(imported),
        "model.data is generated JSON, not unknown raw imported media"
    );
    assert_eq!(
        fs::read_dir(fixture.parent.path())
            .expect("no staging")
            .count(),
        0
    );
}

#[test]
fn permissions_selection_shapes_and_native_bounds_fail_without_creating_output() {
    let fixture = Fixture::new();
    for route in Route::ALL {
        let args = fixture.args(route);
        let tool = json!(route.descriptor());
        jsonschema::validator_for(&tool["inputSchema"])
            .expect("schema")
            .validate(&args)
            .expect("discoverable request");
        for pointer in [
            "/publication/syntax",
            "/publication/output",
            "/publication/acquisition",
            "/publication/acquisition/sources",
        ] {
            let mut bad = args.clone();
            if pointer.ends_with("/sources") {
                bad["publication"]["acquisition"] = json!({"sources":[]});
            } else {
                let key = pointer.rsplit('/').next().expect("key");
                bad["publication"][key] = json!([]);
            }
            assert!(run(route, bad, 1 << 20).is_err());
        }
        for flag in ["local", "allow_read"] {
            let mut denied = args.clone();
            denied["publication"][flag] = json!(false);
            assert_eq!(
                output(route, run(route, denied, 1 << 20).expect("refusal"), true)["code"],
                "publication_permission"
            );
        }
        let mut many = args.clone();
        let mut recipe: Value =
            serde_json::from_str(many["publication"]["recipe_json"].as_str().expect("recipe"))
                .expect("JSON");
        recipe["sites"]
            .as_array_mut()
            .expect("sites")
            .push(json!({"site":"site:other","model_output":"model.data"}));
        many["publication"]["recipe_json"] = json!(recipe.to_string());
        assert_eq!(
            output(
                route,
                run(route, many.clone(), 1 << 20).expect("unknown selection refusal"),
                true
            )["code"],
            "publication_set_selection"
        );
        // Only a valid complete-set selection can reach the one-site consumer
        // restriction. An unknown site is refused earlier by the recipe owner.
        recipe["workspace"]["source"] = json!(format!(
            "{}\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:other'\nname='Other'\n[[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.invalid/'\n",
            fixture.config
        ));
        many["publication"]["recipe_json"] = json!(recipe.to_string());
        assert_eq!(
            output(
                route,
                run(route, many, 1 << 20).expect("valid multi-site consumer refusal"),
                true
            )["code"],
            "publication_site_selection"
        );
    }
    for (key, value) in [
        ("create_stage", json!(false)),
        ("output_parent", json!("relative")),
        ("directory_bytes", json!(0)),
    ] {
        let mut args = fixture.args(Route::Stage);
        args[key] = value;
        assert!(run(Route::Stage, args, 1 << 20).is_err());
    }
    assert!(run(Route::Stage, fixture.args(Route::Stage), 128).is_err());
    let mut short = fixture.args(Route::Stage);
    short["directory_bytes"] = json!(1);
    output(
        Route::Stage,
        run(Route::Stage, short, 1 << 20).expect("native bound"),
        true,
    );
    let mut overlap = fixture.args(Route::Stage);
    overlap["output_parent"] = json!(fixture.root.path());
    output(
        Route::Stage,
        run(Route::Stage, overlap, 1 << 20).expect("native protection"),
        true,
    );
    let mut rules = fixture.args(Route::Probe);
    rules["rules_json"] =
        json!("{\"format\":\"gazette-disclosure-output-input/1\",\"rules\":[],\"rules\":[]}");
    output(
        Route::Probe,
        run(Route::Probe, rules, 1 << 20).expect("strict native rules"),
        true,
    );
    let mut limited = fixture.args(Route::Probe);
    limited["scan_limits"] = json!({"work_bytes":1});
    output(
        Route::Probe,
        run(Route::Probe, limited, 1 << 20).expect("scanner limit"),
        true,
    );
    assert_eq!(
        fs::read_dir(fixture.parent.path())
            .expect("no outputs")
            .count(),
        0
    );
}

#[test]
fn actual_retention_residue_and_compact_delivery_share_the_original_custody_owner() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let fixture = Fixture::new();
    let inputs = fixture.captured();
    let rendered = render(&inputs);
    let reservation = stage::Reservation::new(1024, 8192).expect("pre-effect reserve");
    let staged =
        build::stage_publication(&rendered, fixture.parent.path(), Default::default(), 1024)
            .expect("real native stage");
    let path = staged.path().to_owned();
    fs::write(path.join("model.data"), b"interference").expect("fixture mutation before retention");
    let retained = staged.retain();
    assert!(matches!(retained, Err(source::StageError::Residue { .. })));
    let residue = output(
        Route::Stage,
        stage::deliver_with(reservation, &retained, |_| Value::Null)
            .expect("original residue delivery"),
        true,
    );
    assert_eq!(
        residue["directory_unix_bytes"],
        json!(path.as_os_str().as_bytes())
    );
    assert_eq!(residue["evidence"]["kind"], "unverified_residue");
    assert_eq!(
        fs::read(path.join("model.data")).expect("preserved residue"),
        b"interference"
    );
    let compact = output(
        Route::Stage,
        run(Route::Stage, fixture.args(Route::Stage), 8192).expect("bounded delivery"),
        true,
    );
    assert_eq!(compact["code"], "stage_receipt_delivery");
    assert_eq!(
        compact["evidence"]["inventory_identity"],
        json!(rendered.site().output().inventory().identity())
    );
    let bytes: Vec<u8> = serde_json::from_value(compact["directory_unix_bytes"].clone())
        .expect("lossless native locator");
    let compact_path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    let captured = source::capture_pinned(
        &compact_path,
        rendered.site().output().inventory().identity(),
        Default::default(),
    )
    .expect("independent compact-receipt recovery readback");
    assert_eq!(captured.inventory(), rendered.site().output().inventory());
    assert_eq!(
        fs::read_dir(fixture.parent.path())
            .expect("custody")
            .count(),
        2
    );
}
