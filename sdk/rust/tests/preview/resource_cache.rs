//! Real retained JS compilation reused across sites and live preview generations.

use super::*;
use gazette_application::build::{ResourceCache, ResourceCacheLimits};
use gazette_core::{
    resource,
    source::{self, TreeInput},
};
use gazette_ports::resource::ResourceCompiler;
use std::{cell::Cell, os::unix::fs::PermissionsExt, rc::Rc};

struct Counting(Rc<Cell<usize>>);

impl ResourceCompiler for Counting {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        links: &resource::ModuleLinks,
        limits: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        self.0.set(self.0.get() + 1);
        gazette_adapter_javascript::Compiler.compile(source, recipe, links, limits)
    }
}

struct ChangedProfile<'a>(&'a ResourceCache<Counting>);

impl ResourceCompiler for ChangedProfile<'_> {
    fn profile(&self) -> &'static str {
        "adversarial-changed-resource-profile"
    }

    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        links: &resource::ModuleLinks,
        limits: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        self.0.compile(source, recipe, links, limits)
    }
}

fn cache(calls: &Rc<Cell<usize>>) -> ResourceCache<Counting> {
    ResourceCache::new(
        Counting(Rc::clone(calls)),
        ResourceCacheLimits {
            entries: 8,
            bytes: 1 << 20,
        },
    )
}

fn fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let root = fixture.directory.path();
    let old = git(root, &["rev-parse", "HEAD"]);
    fs::create_dir(root.join("plugin")).unwrap();
    fs::write(
        root.join("plugin/plugin.toml"),
        "package='publication-resources'\n[resources.'module:shared']\ninputs=['main.ts','dep.ts']\nbuild={kind='browser-module',entry='main.ts'}\n",
    )
    .unwrap();
    fs::write(
        root.join("plugin/main.ts"),
        "import { value } from './dep.ts'; export const answer: number = value;\n",
    )
    .unwrap();
    fs::write(
        root.join("plugin/dep.ts"),
        "export const value: number = 41;\n",
    )
    .unwrap();
    let theme_manifest = fs::read_to_string(root.join("theme/theme.toml")).unwrap();
    fs::write(
        root.join("theme/theme.toml"),
        format!("{theme_manifest}resource_requirements=['module:shared']\n"),
    )
    .unwrap();
    fs::write(root.join("theme/templates/page.html"), format!("{TEMPLATE}{{% for resource in presentation.theme_resources %}}<script type=\"module\" src=\"{{{{ resource.url }}}}\"></script>{{% endfor %}}")).unwrap();
    git(root, &["add", "theme", "plugin"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "retained resource fixture",
        ],
    );
    let current = git(root, &["rev-parse", "HEAD"]);
    fixture.config = fixture
        .config
        .replace(old.trim(), current.trim())
        .replacen(
            "[[principals]]",
            "[composition.plugins]\n'publication-resources'={state='enabled'}\n[composition.resources]\n'module:shared'={state='enabled',resource={owner={kind='plugin',package='publication-resources'},resource='module:shared'},output='/assets/shared.js',use='site'}\n[[principals]]",
            1,
        );
    fixture.config.push_str(&format!(
        "[[sources]]\nid='source:plugin'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='plugin'}}\n[[plugin_packages]]\npackage='publication-resources'\nsource='source:plugin'\nmanifest='plugin.toml'\n",
        root.to_str().expect("UTF-8 disposable root"),
        current.trim()
    ));
    fixture
}

fn options(model: &source::EntryPath) -> build::PublicationSetRenderOptions<'_> {
    build::PublicationSetRenderOptions {
        derive_selection: false,
        preview_link_bases: None,
        selection: None,
        review_responses: None,
        preview_urls: None,
        sites: SITES
            .into_iter()
            .map(|site| {
                (
                    key(site),
                    build::SiteRenderOptions {
                        render_year: None,
                        model_output: Some(model),
                        graph: None,
                        limits: Default::default(),
                    },
                )
            })
            .collect(),
    }
}

fn render_with<'a>(
    inputs: &'a build::AcquiredPublicationSet,
    compiler: &impl ResourceCompiler,
) -> build::RenderedPublicationSet<'a> {
    let model = "model.json".parse().unwrap();
    let output = inputs
        .render_local(
            &gazette_adapter_tera::Compiler,
            compiler,
            &gazette_sdk::raster::inspect,
            options(&model),
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert!(
        output.is_complete(),
        "every explicitly selected site must render: {output:?}"
    );
    output
}

fn assert_same(
    left: &gazette_application::build::RenderedSite,
    right: &gazette_application::build::RenderedSite,
) {
    assert_eq!(
        left.output().inventory().identity(),
        right.output().inventory().identity()
    );
    for entry in left.output().inventory().entries() {
        if matches!(entry.content, Content::File { .. }) {
            assert_eq!(
                left.output().file(&entry.path),
                right.output().file(&entry.path)
            );
        }
    }
    assert_eq!(
        serde_json::to_value(left.evidence()).unwrap(),
        serde_json::to_value(right.evidence()).unwrap(),
        "cache state must not alter deterministic build evidence"
    );
}

#[test]
fn persistent_warm_hit_is_planned_before_render_and_bypasses_resource_compilation() {
    let fixture = fixture();
    let inputs = fixture.acquire(&SITES);
    let calls = Rc::new(Cell::new(0));
    let resources = cache(&calls);
    let model = "model.json".parse().unwrap();
    let render_options = options(&model);
    let plan = inputs
        .plan_build_cache(
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &resources,
                raster: &gazette_sdk::raster::inspect,
                raster_profile: gazette_sdk::raster::PROFILE,
                markdown: &gazette_adapter_markdown::Comrak,
            },
            &render_options,
            Default::default(),
        )
        .expect("exact pre-render plan");
    assert_eq!(calls.get(), 0, "planning must not compile page resources");

    let journal = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private cache store");
    let read = gazette_sdk::store::ReadOptions {
        allow_coordination_files: true,
        vm_steps: 1_000_000,
        ..Default::default()
    };
    let session = gazette_sdk::store::SessionOptions {
        allow_local_reservation: true,
        read,
    };
    let initialized = gazette_sdk::store::initialize(
        journal.path(),
        gazette_sdk::store::InitializeOptions {
            allow_create: true,
            session,
            ..Default::default()
        },
    )
    .expect("fresh rebuildable cache");
    assert!(matches!(
        build::schedule_build_cache_site(
            &initialized.database,
            Some(&initialized.header),
            plan.key(&key("site:one")).unwrap(),
            read,
        )
        .expect("empty lookup"),
        build::BuildCacheSchedule::Clean
    ));

    let clean = plan
        .render_clean(
            &inputs,
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &resources,
                raster: &gazette_sdk::raster::inspect,
                raster_profile: gazette_sdk::raster::PROFILE,
                markdown: &gazette_adapter_markdown::Comrak,
            },
            render_options,
            Default::default(),
        )
        .expect("clean render");
    assert_eq!(
        calls.get(),
        1,
        "clean build compiles the shared resource once"
    );
    let receipt = build::reconcile_build_cache(
        &clean,
        journal.path(),
        initialized.header,
        build::BuildCacheReconcileOptions {
            cache: gazette_sdk::store::BuildCacheWriteOptions {
                allow_replace: true,
            },
            writer: gazette_sdk::store::WriterOptions {
                allow_generation_advance: true,
                session,
            },
        },
    )
    .expect("verified complete cache replacement");
    let build::BuildCacheSchedule::Warm(warm) = build::schedule_build_cache_site(
        &initialized.database,
        Some(&receipt.journal),
        clean.plan().key(&key("site:one")).unwrap(),
        read,
    )
    .expect("warm lookup") else {
        panic!("expected exact warm hit")
    };
    assert_eq!(
        calls.get(),
        1,
        "warm scheduling bypasses resource rendering"
    );
    let expected = clean.rendered().sites()[&key("site:one")]
        .as_ref()
        .unwrap()
        .site();
    assert_eq!(warm.output.inventory(), expected.output().inventory());
    for entry in expected.output().inventory().entries() {
        if matches!(entry.content, Content::File { .. }) {
            assert_eq!(
                warm.output.file(&entry.path),
                expected.output().file(&entry.path)
            );
        }
    }
    assert_eq!(warm.media_types, *expected.media_types());
}

#[test]
fn clean_cache_render_refuses_changed_compiler_limits_resource_selection_and_semantics() {
    let mut fixture = fixture();
    let inputs = fixture.acquire(&SITES);
    let calls = Rc::new(Cell::new(0));
    let resources = cache(&calls);
    let model = "model.json".parse().unwrap();
    let make_plan = || {
        inputs
            .plan_build_cache(
                build::BuildCacheAdapters {
                    templates: &gazette_adapter_tera::Compiler,
                    resources: &resources,
                    raster: &gazette_sdk::raster::inspect,
                    raster_profile: gazette_sdk::raster::PROFILE,
                    markdown: &gazette_adapter_markdown::Comrak,
                },
                &options(&model),
                Default::default(),
            )
            .unwrap()
    };

    assert!(matches!(
        make_plan().render_clean(
            &inputs,
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &ChangedProfile(&resources),
                raster: &gazette_sdk::raster::inspect,
                raster_profile: gazette_sdk::raster::PROFILE,
                markdown: &gazette_adapter_markdown::Comrak,
            },
            options(&model),
            Default::default(),
        ),
        Err(build::BuildCacheRunError::Changed)
    ));
    assert!(matches!(
        make_plan().render_clean(
            &inputs,
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &resources,
                raster: &gazette_sdk::raster::inspect,
                raster_profile: "adversarial-changed-raster-profile",
                markdown: &gazette_adapter_markdown::Comrak,
            },
            options(&model),
            Default::default(),
        ),
        Err(build::BuildCacheRunError::Changed)
    ));
    let changed_limits = build::PublicationSetRenderLimits {
        bytes: build::PublicationSetRenderLimits::<gazette_sdk::corpus::Limits>::default().bytes
            - 1,
        ..Default::default()
    };
    assert!(matches!(
        make_plan().render_clean(
            &inputs,
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &resources,
                raster: &gazette_sdk::raster::inspect,
                raster_profile: gazette_sdk::raster::PROFILE,
                markdown: &gazette_adapter_markdown::Comrak,
            },
            options(&model),
            changed_limits,
        ),
        Err(build::BuildCacheRunError::Changed)
    ));
    let mut changed_semantics = options(&model);
    changed_semantics.selection = Some(
        serde_json::from_value(serde_json::json!({
            "content": {"content:one": "exclude", "content:two": "exclude"},
            "assets": {}
        }))
        .unwrap(),
    );
    assert!(matches!(
        make_plan().render_clean(
            &inputs,
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &resources,
                raster: &gazette_sdk::raster::inspect,
                raster_profile: gazette_sdk::raster::PROFILE,
                markdown: &gazette_adapter_markdown::Comrak,
            },
            changed_semantics,
            Default::default(),
        ),
        Err(build::BuildCacheRunError::Changed)
    ));
    fixture.config = fixture.config.replace(
        "output='/assets/shared.js'",
        "output='/assets/renamed-shared.js'",
    );
    let changed_resource_selection = fixture.acquire(&SITES);
    assert!(matches!(
        make_plan().render_clean(
            &changed_resource_selection,
            build::BuildCacheAdapters {
                templates: &gazette_adapter_tera::Compiler,
                resources: &resources,
                raster: &gazette_sdk::raster::inspect,
                raster_profile: gazette_sdk::raster::PROFILE,
                markdown: &gazette_adapter_markdown::Comrak,
            },
            options(&model),
            Default::default(),
        ),
        Err(build::BuildCacheRunError::Changed)
    ));
    assert_eq!(
        calls.get(),
        0,
        "all mismatches stop before resource rendering"
    );
}

#[test]
fn distinct_sites_share_native_compilation_with_empty_warm_and_sdk_output_parity() {
    let fixture = fixture();
    let inputs = fixture.acquire(&SITES);
    let calls = Rc::new(Cell::new(0));
    let resources = cache(&calls);
    let empty = render_with(&inputs, &resources);
    assert_eq!(
        calls.get(),
        1,
        "shared source/recipe compiles once across both sites"
    );
    let warm = render_with(&inputs, &resources);
    assert_eq!(
        calls.get(),
        1,
        "retained repeat uses actual compiled result"
    );
    let native = render(&inputs, false);
    assert!(native.is_complete());
    let mut urls = Vec::new();
    for (name, site) in [("one", "site:one"), ("two", "site:two")] {
        let first = empty.sites()[&key(site)].as_ref().unwrap().site();
        assert_same(first, warm.sites()[&key(site)].as_ref().unwrap().site());
        assert_same(first, native.sites()[&key(site)].as_ref().unwrap().site());
        let model: serde_json::Value =
            serde_json::from_slice(first.output().file(&"model.json".parse().unwrap()).unwrap())
                .unwrap();
        let url = model["resources"]["module:shared"]["url"].as_str().unwrap();
        assert_eq!(
            url,
            format!("https://{name}.example.invalid/pr%C3%A9fix/assets/shared.js")
        );
        let html =
            std::str::from_utf8(first.output().file(&"index.html".parse().unwrap()).unwrap())
                .unwrap();
        let scripts: Vec<_> = html.split("<script type=\"module\" src=\"").collect();
        assert_eq!(scripts.len(), 2, "one actual selected module script");
        let (source, suffix) = scripts[1].split_once('"').unwrap();
        assert!(
            suffix.starts_with("></script>"),
            "closed source and empty inline body"
        );
        let mut decoded = Vec::new();
        gazette_compiler::html::inspect(
            format!("<span title=\"{source}\"></span>").as_bytes(),
            Default::default(),
            |kind, value| {
                if kind == gazette_compiler::html::TextKind::Attribute {
                    decoded.push(value.to_owned());
                }
                Ok::<_, std::convert::Infallible>(())
            },
        )
        .expect("native entity decoding of the actual emitted script source");
        assert_eq!(decoded, [url], "HTML script source and model URL agree");
        assert!(
            std::str::from_utf8(
                first
                    .output()
                    .file(&"assets/shared.js".parse().unwrap())
                    .unwrap()
            )
            .unwrap()
            .contains("41")
        );
        let evidence = serde_json::to_value(first.evidence()).unwrap();
        let resource = &evidence["resources"]["module:shared"];
        assert_eq!(resource["source"], "source:plugin");
        assert_eq!(resource["resource"]["owner"]["kind"], "plugin");
        assert_eq!(
            resource["resource"]["owner"]["package"],
            "publication-resources"
        );
        assert_eq!(resource["resource"]["resource"], "module:shared");
        assert_eq!(resource["recipe"]["entry"], "main.ts");
        assert_eq!(resource["use"], "site");
        urls.push(url.to_owned());
    }
    assert_ne!(
        urls[0], urls[1],
        "shared program never conflates site output addresses"
    );
}

#[test]
fn live_body_only_rebuild_hits_then_retained_import_change_misses() {
    use gazette_application::preview::watch::{self, Adapters, Control, Event};
    let fixture = fixture();
    let calls = Rc::new(Cell::new(0));
    let resources = cache(&calls);
    let model = "model.json".parse().unwrap();
    let body = fixture.directory.path().join("one/page.md");
    let original = fs::read_to_string(&body).unwrap();
    let dependency = fixture.directory.path().join("plugin/dep.ts");
    let mut stages = Vec::new();
    let mut first_resource = None;
    let deadline = Instant::now() + Duration::from_secs(20);
    let configuration =
        gazette_application::configuration::resolve(&fixture.config, Default::default())
            .expect("fixed workspace compilation");
    watch::run(
        Adapters {
            host_files: None,
            social_card_helper: None,
            source: &gazette_adapter_fs::FileSystem,
            revisions: &gazette_adapter_git::Git,
            templates: &gazette_adapter_tera::Compiler,
            resources: &resources,
            raster: &gazette_sdk::raster::inspect,
            markdown: &gazette_adapter_markdown::Comrak,
            runner: &gazette_adapter_process::Runner,
            server: &gazette_adapter_http::Http,
        },
        watch::Request {
            workspace: &configuration,
            build_selection: configuration
                .compilation()
                .workspace
                .build_selection(Some(&SITES.map(key)))
                .expect("exact invocation sites"),
            render: options(&model),
            addresses: addresses(&SITES),
        },
        watch::Options {
            local_urls: false,
            interval: Duration::from_millis(100),
            listeners: permission(),
            ..Default::default()
        },
        |event| {
            assert!(Instant::now() < deadline, "bounded real watch progression");
            match event {
                Event::Ready {
                    generation,
                    current,
                    ..
                } => {
                    assert_eq!(generation, 1);
                    assert_eq!(calls.get(), 1);
                    first_resource = Some(
                        current.sites()[&key("site:one")]
                            .output()
                            .file(&"assets/shared.js".parse().unwrap())
                            .unwrap()
                            .to_vec(),
                    );
                    stages.push("ready");
                    fs::write(&body, original.replace("Retained one", "Body-only update"))?;
                }
                Event::Rebuilt {
                    generation,
                    current,
                    ..
                } => {
                    let cold_calls = Rc::new(Cell::new(0));
                    let cold = render_with(current.inputs(), &cache(&cold_calls));
                    assert_eq!(
                        cold_calls.get(),
                        1,
                        "independent empty cache compiles shared program"
                    );
                    for site in SITES {
                        assert_same(
                            &current.sites()[&key(site)],
                            cold.sites()[&key(site)].as_ref().unwrap().site(),
                        );
                    }
                    let resource = current.sites()[&key("site:one")]
                        .output()
                        .file(&"assets/shared.js".parse().unwrap())
                        .unwrap();
                    let html = std::str::from_utf8(
                        current.sites()[&key("site:one")]
                            .output()
                            .file(&"index.html".parse().unwrap())
                            .unwrap(),
                    )
                    .unwrap();
                    assert!(html.contains("Body-only update"));
                    match generation {
                        2 => {
                            assert_eq!(
                                calls.get(),
                                1,
                                "body change must not recompile unchanged JS"
                            );
                            assert_eq!(Some(resource), first_resource.as_deref());
                            stages.push("body");
                            fs::write(&dependency, "export const value: number = 9001;\n")?;
                        }
                        3 => {
                            assert_eq!(
                                calls.get(),
                                2,
                                "changed imported bytes require actual compilation"
                            );
                            assert_ne!(Some(resource), first_resource.as_deref());
                            assert!(std::str::from_utf8(resource).unwrap().contains("9001"));
                            stages.push("dependency");
                            return Ok(Control::Stop);
                        }
                        _ => panic!("unexpected generation {generation}"),
                    }
                }
                Event::RebuildFailed { error, .. } => panic!("native rebuild failed: {error}"),
                Event::Recovered { .. } => panic!("fixture has no failed generation"),
                Event::Closed => stages.push("closed"),
                Event::Poll => (),
            }
            Ok(Control::Continue)
        },
    )
    .expect("real native capture, compiler, complete rendering and held listeners");
    assert_eq!(stages, ["ready", "body", "dependency", "closed"]);
}
