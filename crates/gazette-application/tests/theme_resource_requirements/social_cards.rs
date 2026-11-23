//! Card capture uses one retained model, exact assets and an explicit native boundary.

use super::*;
use gazette_core::{process, raster};
use gazette_ports::process::{CaptureRunner, RetainedCaptureRunner};
use std::cell::RefCell;

const CARD: &[u8] = br#"<title>{{ content.title }}</title><h1>{{ content.title }}</h1><img data-card-media src="https://example.test/image.png">{% for resource in presentation.theme_resources %}{{ resource.id }};{% endfor %}{{ card.width }}x{{ card.height }}"#;

struct CardReader {
    manifest: String,
    published: bool,
    manual: bool,
    variants: bool,
}
impl CardReader {
    fn tree(&self, root: &Path, limits: source::Limits) -> Result<CapturedSource, source::Error> {
        let original = captured(root, limits)?;
        let (inventory, contents) = original.into_parts();
        let mut files = contents.into_files();
        let mut builder = InventoryBuilder::new(limits);
        if root == Path::new("/fixture/theme") {
            files.insert(path("theme.toml"), self.manifest.as_bytes().to_vec());
            files.insert(path("templates/card.html"), CARD.to_vec());
            let mut page = THEME_TEMPLATE.to_vec();
            page.extend_from_slice(br#"{% if presentation.social_card %}{{ presentation.social_card.title }}|{{ presentation.social_card.url }}{% endif %}"#);
            files.insert(path("templates/page.html"), page);
        } else if root == Path::new("/fixture/content") && self.published {
            for bytes in files.values_mut() {
                *bytes = String::from_utf8(bytes.clone())
                    .unwrap()
                    .replace("lifecycle='draft'", "lifecycle='published'")
                    .into_bytes();
            }
        } else if root == Path::new("/fixture/plugin") && self.manual {
            let mut manifest = String::from_utf8(files[&path("plugin.toml")].clone()).unwrap();
            manifest.push_str("\n[resources.'image:manual']\ninputs=['manual.png']\nbuild={kind='raster-image',entry='manual.png',input_format='png',resize={operation='fit',width=1,height=1},encoding={format='png'},apply_orientation=false}\n");
            files.insert(path("plugin.toml"), manifest.into_bytes());
            files.insert(path("manual.png"), b"png".to_vec());
        }
        if root == Path::new("/fixture/content") && self.variants {
            let page = String::from_utf8(files[&path("authored.md")].clone()).unwrap();
            let page = page.replacen(
                "+++\n",
                "+++\n[[images]]\ndefault='image.png'\nlight='image.png'\ndark='image-dark.png'\n",
                1,
            ) + "\n![Declared portrait](image.png)\n";
            files.insert(path("authored.md"), page.into_bytes());
            files.insert(path("image.png"), b"png".to_vec());
            files.insert(path("image-dark.png"), b"png".to_vec());
        }
        for entry in inventory.entries() {
            if matches!(entry.content, Content::Directory) {
                builder.insert(entry.path.clone(), Content::Directory)?;
            }
        }
        for (path, bytes) in &files {
            builder.insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )?;
        }
        CapturedSource::from_parts(builder.finish(), files)
    }
}
impl SourceReader for CardReader {
    fn read(
        &self,
        _: &Path,
        _: Option<&Subdirectory>,
        _: source::Limits,
    ) -> Result<Inventory, source::Error> {
        panic!("capture required")
    }
}
impl SourceCapture for CardReader {
    fn capture(
        &self,
        root: &Path,
        _: Option<&Subdirectory>,
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        self.tree(root, limits)
    }
}
impl RevisionReader for CardReader {
    fn read_revision(
        &self,
        root: &Path,
        _: &Subdirectory,
        _: &CommitId,
        limits: source::Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, source::Error> {
        Ok(self.tree(root, limits)?.into_parts().0)
    }
    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, source::Error> {
        panic!("no gitlinks")
    }
}
fn fixture(width: u32, base: &str) -> (CardReader, String) {
    let manifest = String::from_utf8(THEME_MANIFEST.to_vec()).unwrap()
        .replace("render={page='page.html'}", &format!("render={{page='page.html',social_card={{template='card.html',width={width},height=1,media_selector='[data-card-media]',media_kind='image',resource_requirements=['module:theme-required','stylesheet:extra-site']}}}}"))
        .replace("resource_requirements=['module:theme-required']\n", "");
    let reader = CardReader {
        manifest,
        published: false,
        manual: false,
        variants: false,
    };
    let pin = "{bytes=1,sha256='0000000000000000000000000000000000000000000000000000000000000000'}";
    let config = format!(
        "social_card_renderer={{node={{program='/fixture/node',executable={pin}}},browser={{program='/fixture/browser',executable={pin}}}}}\n{}",
        configuration(true, RequiredSelection::Enabled)
            .replace(
                "output='/assets/extra.css',use='site'",
                "output='/assets/extra.css',use='dependency'"
            )
            .replace(
                "public_base_url='https://example.test/'",
                &format!("home='content:one'\npublic_base_url='{base}'")
            )
    );
    (reader, config)
}
fn acquire(reader: &CardReader, config: &str) -> build::AcquiredInputs {
    build::acquire_publication(
        reader,
        reader,
        config,
        &"site:one".parse().unwrap(),
        Default::default(),
        PreparationLimits {
            sources: 3,
            entries: 32,
            bytes: 16 << 10,
            per_source: Default::default(),
        },
    )
    .unwrap()
}
fn inputs(width: u32, base: &str) -> build::AcquiredInputs {
    let (reader, config) = fixture(width, base);
    acquire(&reader, &config)
}

fn helper() -> CapturedSource {
    let mut builder = InventoryBuilder::new(Default::default());
    builder
        .insert(
            path("helper.mjs"),
            Content::File {
                bytes: 1,
                sha256: Sha256Digest::of(b"x"),
            },
        )
        .unwrap();
    CapturedSource::from_parts(
        builder.finish(),
        [(path("helper.mjs"), b"x".to_vec())].into(),
    )
    .unwrap()
}
#[derive(Default)]
struct Runner {
    requests: RefCell<Vec<Value>>,
    wrong_identity: bool,
    failure: Option<&'static str>,
    checks: Cell<usize>,
}
impl gazette_ports::process::Runner for Runner {
    fn run(&self, request: process::Request<'_>) -> Result<process::Exit, process::Error> {
        assert!(
            self.requests.borrow().is_empty(),
            "candidate check must precede capture"
        );
        assert_eq!(request.command.program, "/fixture/checker");
        self.checks.set(self.checks.get() + 1);
        Ok(process::Exit { code: Some(0) })
    }
}
struct CardResources;
impl ResourceCompiler for CardResources {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        links: &resource::ModuleLinks,
        limits: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        if recipe.entry().as_str() != "manual.png" {
            return Resources {
                calls: Cell::new(0),
            }
            .compile(source, recipe, links, limits);
        }
        let extent = raster::Dimensions {
            width: 1,
            height: 1,
        };
        Ok(resource::Artifact {
            input: source.inventory().identity(),
            bytes: b"png".to_vec(),
            dependencies: [path("manual.png")].into(),
            references: Default::default(),
            deferred_references: Default::default(),
            svg: None,
            image: Some(raster::Geometry {
                source: extent,
                oriented: extent,
                output: extent,
            }),
            tool: "fixture-image",
        })
    }
}
impl CaptureRunner for Runner {
    fn capture(&self, _: process::CaptureRequest<'_>) -> Result<process::Captured, process::Error> {
        panic!("retained execution required")
    }
}
impl RetainedCaptureRunner for Runner {
    fn capture_retained(
        &self,
        request: process::CaptureRequest<'_>,
        tree: TreeInput<'_>,
        entry: &EntryPath,
    ) -> Result<process::Captured, process::Error> {
        assert_eq!(entry, &path("helper.mjs"));
        assert!(tree.file(entry).is_some());
        let value: Value = serde_json::from_slice(request.request.input).unwrap();
        assert_eq!(
            value["limits"]["timeout_ms"].as_u64().unwrap() as u128 + 3000,
            request.request.options.timeout.as_millis()
        );
        for asset in value["assets"].as_array().unwrap() {
            let entry: EntryPath = asset["entry"].as_str().unwrap().parse().unwrap();
            let bytes = tree.file(&entry).unwrap();
            assert_eq!(Sha256Digest::of(bytes).to_string(), asset["sha256"]);
        }
        assert!(
            value["html"]
                .as_str()
                .unwrap()
                .contains("module:theme-required;")
        );
        assert!(
            value["html"]
                .as_str()
                .unwrap()
                .contains("stylesheet:extra-site;")
        );
        self.requests.borrow_mut().push(value.clone());
        if let Some(diagnostic) = self.failure {
            return Ok(process::Captured {
                exit: process::Exit { code: Some(1) },
                stdout: vec![],
                stderr: Some(diagnostic.as_bytes().to_vec()),
            });
        }
        let identity = if self.wrong_identity {
            serde_json::to_value(Sha256Digest::of(b"wrong")).unwrap()
        } else {
            value["request_sha256"].clone()
        };
        Ok(process::Captured {exit:process::Exit {code:Some(0)},stderr:Some(vec![]),stdout:serde_json::to_vec(&serde_json::json!({"version":1,"request_sha256":identity,"title":"Authored H1 replaces initial title","png_base64":"cG5n"})).unwrap()})
    }
}
fn inspect(
    format: raster::Format,
    bytes: &[u8],
    limits: raster::Limits,
) -> Result<raster::Inspection, raster::Error> {
    assert_eq!(bytes, b"png");
    let result = raster::Inspection {
        display: None,
        format,
        width: 1,
        height: 1,
        bytes: bytes.len(),
        sha256: Sha256Digest::of(bytes),
        tool: "fixture-decoder",
    };
    result.validate(format, bytes, limits)?;
    Ok(result)
}
fn generated<'a>(
    inputs: &'a build::AcquiredInputs,
    runner: &Runner,
    options: build::SocialCardGenerationOptions,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    generated_with_decoder(inputs, runner, options, &inspect)
}
fn generated_with_decoder<'a>(
    inputs: &'a build::AcquiredInputs,
    runner: &Runner,
    options: build::SocialCardGenerationOptions,
    decoder: &raster::Inspect<'_>,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    let helper = helper();
    inputs.render_publication_with_generations(
        &gazette_adapter_tera::Compiler,
        &CardResources,
        decoder,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&path("model.json")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
        &gazette_adapter_markdown::Comrak,
        None,
        build::SocialCardGeneration {
            runner,
            helper: &helper,
            entrypoint: &path("helper.mjs"),
            options,
        },
    )
}

#[test]
fn native_image_occurrence_and_resolved_variants_enter_the_retained_capture_request() {
    let (mut reader, config) = fixture(1, "https://example.test/");
    reader.variants = true;
    let config = config.replace(
        "target='authored'",
        "target='authored',static_assets={route_prefix='/media/'}",
    );
    let inputs = acquire(&reader, &config);
    let runner = Runner::default();
    let output = generated(
        &inputs,
        &runner,
        build::SocialCardGenerationOptions {
            execution: process::Options {
                allow_execute: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    let requests = runner.requests.borrow();
    assert_eq!(requests.len(), 1);
    let declarations = &requests[0]["image_variants"];
    assert_eq!(
        declarations,
        &serde_json::json!([{
            "address": "content:one#image-1",
            "destination": "https://example.test/media/image.png",
            "variants": { "light": "https://example.test/media/image.png", "dark": "https://example.test/media/image-dark.png" }
        }])
    );
    let assets = requests[0]["assets"].as_array().unwrap();
    for name in ["image.png", "image-dark.png"] {
        assert_eq!(
            output.site().output().file(&path(&format!("media/{name}"))),
            Some(b"png".as_slice())
        );
        assert!(assets.iter().any(|asset| asset["role"] == "media"
            && asset["url"] == format!("https://example.test/media/{name}")));
    }
}
#[test]
fn pure_requires_capture_effectful_resumes_once_and_card_resources_stay_local() {
    let inputs = inputs(1, "https://example.test/");
    let error = render(
        &inputs,
        &Resources {
            calls: Cell::new(0),
        },
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            build::PublicationRenderError::Site(build::SiteRenderError::SocialCardCaptureRequired(
                _
            ))
        ),
        "{error}"
    );
    let runner = Runner::default();
    let output = generated(
        &inputs,
        &runner,
        build::SocialCardGenerationOptions {
            execution: process::Options {
                allow_execute: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(runner.requests.borrow().len(), 1);
    let files = output.site().output();
    assert_eq!(
        files.file(&path("assets/social-card.png")),
        Some(b"png".as_slice())
    );
    let html = std::str::from_utf8(files.file(&path("index.html")).unwrap()).unwrap();
    assert!(
        !html.contains("module:theme-required") && !html.contains("stylesheet:extra-site"),
        "card-only dependency must not become an ordinary page root"
    );
    assert!(html.contains("Authored H1 replaces initial title"));
    let model: Value = serde_json::from_slice(files.file(&path("model.json")).unwrap()).unwrap();
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["social_card"]["url"],
        "https://example.test/assets/social-card.png"
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["social_card"]["title"],
        "Authored H1 replaces initial title"
    );
    assert!(output.site().evidence().social_card.is_some());
}
#[test]
fn wrong_response_binding_and_dimensions_fail_without_recapture() {
    for (width, wrong_identity) in [(1, true), (2, false)] {
        let inputs = inputs(width, "https://example.test/");
        let runner = Runner {
            wrong_identity,
            ..Default::default()
        };
        assert!(
            generated(
                &inputs,
                &runner,
                build::SocialCardGenerationOptions {
                    execution: process::Options {
                        allow_execute: true,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert_eq!(runner.requests.borrow().len(), 1);
    }
}
#[test]
fn zero_capture_budget_fails_before_native_invocation() {
    let inputs = inputs(1, "https://example.test/");
    let runner = Runner::default();
    assert!(
        generated(
            &inputs,
            &runner,
            build::SocialCardGenerationOptions {
                cards: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(runner.requests.borrow().is_empty());
}
#[test]
fn selected_url_base_changes_the_bound_request() {
    let first = inputs(1, "https://example.test/");
    let second = inputs(1, "http://127.0.0.1:4242/preview/");
    let identity = |inputs: &build::AcquiredInputs| match render(
        inputs,
        &Resources {
            calls: Cell::new(0),
        },
    )
    .unwrap_err()
    {
        build::PublicationRenderError::Site(build::SiteRenderError::SocialCardCaptureRequired(
            request,
        )) => request.identity(),
        error => panic!("{error}"),
    };
    assert_ne!(identity(&first), identity(&second));
}

#[test]
fn insufficient_cleanup_deadline_fails_before_invocation() {
    let inputs = inputs(1, "https://example.test/");
    let runner = Runner::default();
    assert!(
        generated(
            &inputs,
            &runner,
            build::SocialCardGenerationOptions {
                execution: process::Options {
                    allow_execute: true,
                    timeout: std::time::Duration::from_secs(3),
                    ..Default::default()
                },
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(runner.requests.borrow().is_empty());
}

#[test]
fn candidate_checks_once_before_capture_and_resumption() {
    let (mut reader, config) = fixture(1, "https://example.test/");
    reader.published = true;
    let config =
        format!("policy_check={{program='/fixture/checker',directory='/fixture'}}\n{config}");
    let key = "site:one".parse().unwrap();
    let inputs = build::acquire_publications(
        &reader,
        &reader,
        &config,
        std::slice::from_ref(&key),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let runner = Runner::default();
    let helper = helper();
    let execution = process::Options {
        allow_execute: true,
        ..Default::default()
    };
    let rendered = inputs
        .check_and_render_local_with_generations(
            &gazette_adapter_tera::Compiler,
            &CardResources,
            &inspect,
            build::PublicationSetRenderOptions {
                derive_selection: false,
                sites: [(
                    key.clone(),
                    build::SiteRenderOptions {
                        render_year: None,
                        model_output: None,
                        graph: None,
                        limits: Default::default(),
                    },
                )]
                .into(),
                selection: Some(gazette_compiler::authored::Selection {
                    content: [(
                        "content:one".parse().unwrap(),
                        gazette_core::publication::content::Disposition::Include,
                    )]
                    .into(),
                    assets: Default::default(),
                }),
                review_responses: None,
                preview_urls: None,
                preview_link_bases: None,
            },
            Default::default(),
            &gazette_adapter_markdown::Comrak,
            &runner,
            build::CandidateCheckOptions {
                requirements: None,
                process: execution,
                review: Default::default(),
                bytes: 16 << 20,
                response: Default::default(),
                response_bytes: 16 << 20,
            },
            None,
            build::SocialCardGeneration {
                runner: &runner,
                helper: &helper,
                entrypoint: &path("helper.mjs"),
                options: build::SocialCardGenerationOptions {
                    execution,
                    ..Default::default()
                },
            },
        )
        .unwrap();
    assert!(
        rendered.sites()[&key].is_ok(),
        "{:?}",
        rendered.sites()[&key]
    );
    assert_eq!(runner.checks.get(), 1);
    assert_eq!(runner.requests.borrow().len(), 1);
}
#[test]
fn manual_preview_skips_capture_but_not_template_or_resource_validation() {
    for invalid in ["", "template", "resource"] {
        let (mut reader, config) = fixture(1, "https://example.test/");
        reader.manual = true;
        if invalid == "template" {
            reader.manifest = reader
                .manifest
                .replace("template='card.html'", "template='missing.html'");
        }
        if invalid == "resource" {
            reader.manifest = reader.manifest.replace(
                "resource_requirements=['module:theme-required','stylesheet:extra-site']",
                "resource_requirements=['module:absent']",
            );
        }
        let config=config.replace("[composition.resources]","[composition.resources]\n'image:manual'={state='enabled',resource={owner={kind='plugin',package='resource-plugin'},resource='image:manual'},output='/assets/manual.png',use='site'}")
            .replace("home='content:one'","home='content:one'\nidentity_assets={link_preview='image:manual'}");
        let inputs = acquire(&reader, &config);
        let runner = Runner::default();
        let result = generated(&inputs, &runner, Default::default());
        assert!(runner.requests.borrow().is_empty());
        if invalid.is_empty() {
            let output = result.unwrap();
            assert!(
                output
                    .site()
                    .output()
                    .file(&path("assets/social-card.png"))
                    .is_none()
            );
            assert!(output.site().evidence().social_card.is_none());
        } else {
            assert!(result.is_err(), "invalid {invalid} must still reject");
        }
    }
}

#[test]
fn decoder_receipt_must_bind_exact_png_bytes() {
    let inputs = inputs(1, "https://example.test/");
    let runner = Runner::default();
    let lying_decoder = |format, bytes: &[u8], limits| {
        let mut image = inspect(format, bytes, limits)?;
        image.sha256 = Sha256Digest::of(b"different bytes");
        Ok(image)
    };
    let result = generated_with_decoder(
        &inputs,
        &runner,
        build::SocialCardGenerationOptions {
            execution: process::Options {
                allow_execute: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &lying_decoder,
    );
    assert!(
        matches!(result,Err(build::PublicationRenderError::Site(build::SiteRenderError::SocialCard(error))) if matches!(*error,build::SocialCardGenerationError::Raster(raster::Error::Binding)))
    );
    assert_eq!(runner.requests.borrow().len(), 1);
}

#[test]
fn undeclared_non_module_dependency_remains_invalid() {
    let (mut reader, config) = fixture(1, "https://example.test/");
    reader.manifest = reader.manifest.replace(
        ",resource_requirements=['module:theme-required','stylesheet:extra-site']",
        "",
    );
    let inputs = acquire(&reader, &config);
    let runner = Runner::default();
    let result = generated(&inputs, &runner, Default::default());
    assert!(result.is_err());
    assert!(runner.requests.borrow().is_empty());
}

#[test]
fn capture_failure_reports_only_the_fixed_helper_code_without_retry() {
    let inputs = inputs(1, "https://example.test/");
    for (diagnostic, expected) in [
        ("social-card: blocked-request\n", Some("blocked-request")),
        ("social-card: /private/fixture-secret\n", None),
        ("unexpected private diagnostic", None),
    ] {
        let runner = Runner {
            failure: Some(diagnostic),
            ..Default::default()
        };
        let error = generated(
            &inputs,
            &runner,
            build::SocialCardGenerationOptions {
                execution: process::Options {
                    allow_execute: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_err();
        let build::PublicationRenderError::Site(build::SiteRenderError::SocialCard(error)) = error
        else {
            panic!("expected native capture failure");
        };
        let build::SocialCardGenerationError::Exit { exit, reason } = error.as_ref() else {
            panic!("expected process status");
        };
        assert_eq!(exit.code, Some(1));
        assert_eq!(reason.as_deref(), expected);
        assert!(!error.to_string().contains("private"));
        assert_eq!(runner.requests.borrow().len(), 1);
    }
}
