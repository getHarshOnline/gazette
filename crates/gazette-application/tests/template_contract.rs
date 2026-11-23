//! Application defenses against renderer contract violations after real metadata admission.

use std::{
    cell::Cell,
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use gazette_application::{build, source::PreparationLimits};
use gazette_compiler::toml;
use gazette_core::{
    render,
    source::{
        self, CapturedSource, CommitId, Content, EntryPath, Inventory, InventoryBuilder,
        MetadataBudget, Sha256Digest, Subdirectory, TreeIdentity, TreeInput,
    },
};
use gazette_ports::{
    render::{CompiledTemplates, TemplateCompiler},
    source::{RevisionReader, SourceCapture, SourceReader},
};
use serde_json::{Value, json};

const CONFIG: &str = r#"
[composition]
theme={package='theme:base'}
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[brands]]
id='brand:one'
name='Example'
[[brands.publications]]
site='site:one'
public_base_url='https://example.test/'
authored=[{source='source:theme',target='content'}]
[[sources]]
id='source:theme'
location={kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}
[[theme_packages]]
package='theme:base'
source='source:theme'
manifest='manifest.toml'
"#;
const MANIFEST: &[u8] = b"package='theme:base'\nnamespace='base'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='parts/page.html',alias='alias.html',graph='alias.html'}\n";
const TEMPLATE: &[u8] = b"{{ title }}";

fn captured(limits: source::Limits) -> Result<CapturedSource, source::Error> {
    let mut builder = InventoryBuilder::new(limits);
    builder.insert("templates".parse()?, Content::Directory)?;
    builder.insert("templates/parts".parse()?, Content::Directory)?;
    let files: BTreeMap<EntryPath, Vec<u8>> = [
        ("manifest.toml", MANIFEST),
        ("unselected.txt", b"not a template".as_slice()),
        ("templates/parts/page.html", TEMPLATE),
        ("templates/alias.html", TEMPLATE),
    ]
    .into_iter()
    .map(|(path, bytes)| Ok((path.parse()?, bytes.to_vec())))
    .collect::<Result<_, source::Error>>()?;
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

#[derive(Default)]
struct Reader {
    reads: Cell<usize>,
}

impl SourceReader for Reader {
    fn read(
        &self,
        _: &Path,
        _: Option<&Subdirectory>,
        _: source::Limits,
    ) -> Result<Inventory, source::Error> {
        panic!("the application must retain source bytes");
    }
}
impl SourceCapture for Reader {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        assert_eq!(root, Path::new("/fixture/theme"));
        assert_eq!(subdirectory, Some(&"input".parse().expect("subdirectory")));
        self.reads.set(self.reads.get() + 1);
        captured(limits)
    }
}
impl RevisionReader for Reader {
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: source::Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, source::Error> {
        assert_eq!(root, Path::new("/fixture/theme"));
        assert_eq!(subdirectory, &"input".parse().expect("subdirectory"));
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        self.reads.set(self.reads.get() + 1);
        Ok(captured(limits)?.into_parts().0)
    }
    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, source::Error> {
        panic!("explicit repository is not a gitlink selection");
    }
}

fn acquired(reader: &Reader) -> build::AcquiredInputs {
    acquired_with(reader, CONFIG)
}

fn acquired_with(reader: &Reader, config: &str) -> build::AcquiredInputs {
    let acquired = build::acquire_composition(
        reader,
        reader,
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        PreparationLimits {
            sources: 1,
            entries: 6,
            bytes: 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("real workspace resolution and complete pinned acquisition");
    assert_eq!(reader.reads.get(), 2);
    acquired
}

struct Compiler {
    calls: Cell<usize>,
    wrong_identity: bool,
    profile: &'static str,
    profile_reads: Arc<AtomicUsize>,
}
struct Program {
    identity: TreeIdentity,
    profile: &'static str,
    profile_reads: Arc<AtomicUsize>,
}

impl TemplateCompiler for Compiler {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        _: render::Limits,
    ) -> Result<Box<dyn CompiledTemplates>, render::Error> {
        self.calls.set(self.calls.get() + 1);
        let paths: Vec<_> = source
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "base",
                "base/alias.html",
                "base/parts",
                "base/parts/page.html"
            ],
            "application mounts only the selected package subtree in its namespace"
        );
        assert_eq!(
            source.file(&"base/parts/page.html".parse().expect("entry")),
            Some(TEMPLATE)
        );
        assert_eq!(
            source.file(&"base/alias.html".parse().expect("entry")),
            Some(TEMPLATE)
        );
        let identity = if self.wrong_identity {
            InventoryBuilder::new(source::Limits::default())
                .finish()
                .identity()
        } else {
            source.inventory().identity()
        };
        Ok(Box::new(Program {
            identity,
            profile: self.profile,
            profile_reads: Arc::clone(&self.profile_reads),
        }))
    }
}
impl CompiledTemplates for Program {
    fn profile(&self) -> &'static str {
        self.profile_reads.fetch_add(1, Ordering::SeqCst);
        self.profile
    }
    fn identity(&self) -> TreeIdentity {
        self.identity
    }
    fn render(&self, entry: &EntryPath, _: &Value) -> Result<Vec<u8>, render::Error> {
        assert_eq!(entry.as_str(), "base/parts/page.html");
        // Deliberately ignore the compiler's ceiling: the application must recheck.
        Ok(b"four".to_vec())
    }
}

#[test]
fn renderer_identity_mismatch_rejects_the_real_selected_subtree() {
    let reader = Reader::default();
    let acquired = acquired(&reader);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("real parsed manifest binding");
    let compiler = Compiler {
        calls: Cell::new(0),
        wrong_identity: true,
        profile: "",
        profile_reads: Arc::default(),
    };
    let result = metadata.compile_templates(&compiler, render::Limits::default());
    let Err(build::TemplateError::IdentityMismatch { expected, observed }) = result else {
        panic!("application must refuse mismatched compiled identity");
    };
    assert_ne!(expected, observed);
    assert_eq!(
        observed,
        InventoryBuilder::new(source::Limits::default())
            .finish()
            .identity()
    );
    assert_eq!(compiler.calls.get(), 1);
    assert_eq!(
        compiler.profile_reads.load(Ordering::SeqCst),
        0,
        "identity mismatch must precede profile admission"
    );
    assert_eq!(reader.reads.get(), 2, "compilation cannot reopen source");
}

#[test]
fn oversized_renderer_success_is_rejected_and_exact_bound_succeeds() {
    let reader = Reader::default();
    let acquired = acquired(&reader);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("real metadata");
    let compiler = Compiler {
        calls: Cell::new(0),
        wrong_identity: false,
        profile: "fixture.renderer/1",
        profile_reads: Arc::default(),
    };
    let small = metadata
        .compile_templates(
            &compiler,
            render::Limits {
                output_bytes: 3,
                ..render::Limits::default()
            },
        )
        .expect("matching renderer identity");
    let exact = metadata
        .compile_templates(
            &compiler,
            render::Limits {
                output_bytes: 4,
                ..render::Limits::default()
            },
        )
        .expect("exact output ceiling");
    assert_eq!(small.evidence().site.as_str(), "site:one");
    assert_eq!(small.evidence().packages.len(), 1);
    assert_eq!(
        small.evidence().packages[0].manifest_sha256,
        Sha256Digest::of(MANIFEST)
    );
    assert_ne!(
        small.evidence().packages[0].source_tree,
        small.evidence().template_tree
    );
    drop(metadata);
    drop(acquired);
    let entry = "base/parts/page.html".parse().expect("entry");
    assert!(matches!(
        small.render(&entry, &json!({})),
        Err(render::Error::Limit("output bytes"))
    ));
    assert_eq!(
        exact.render(&entry, &json!({})).expect("exact ceiling"),
        b"four"
    );
    assert_eq!(compiler.calls.get(), 2);
    assert_eq!(reader.reads.get(), 2);
}

#[test]
fn tighter_subtree_limits_fail_before_the_compiler_is_called() {
    let reader = Reader::default();
    let acquired = acquired(&reader);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("real metadata");
    let compiler = Compiler {
        calls: Cell::new(0),
        wrong_identity: false,
        profile: "fixture.renderer/1",
        profile_reads: Arc::default(),
    };
    let defaults = render::Limits::default();
    for templates in [
        source::Limits {
            entries: 1,
            ..defaults.templates
        },
        source::Limits {
            depth: 1,
            ..defaults.templates
        },
        source::Limits {
            file_bytes: TEMPLATE.len() as u64 - 1,
            ..defaults.templates
        },
        source::Limits {
            total_bytes: TEMPLATE.len() as u64 - 1,
            ..defaults.templates
        },
    ] {
        assert!(matches!(
            metadata.compile_templates(
                &compiler,
                render::Limits {
                    templates,
                    ..defaults
                }
            ),
            Err(build::TemplateError::Namespace(
                build::theme_namespace::Error::Input(source::Error::Limit { .. })
            ))
        ));
        assert_eq!(
            compiler.calls.get(),
            0,
            "subtree limits are enforced before renderer invocation"
        );
    }
    assert_eq!(reader.reads.get(), 2);
}

const PROFILE_256: &str = concat!(
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
);
const PROFILE_257: &str = concat!(
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdefx",
);

#[test]
fn invalid_profile_refuses_the_complete_program_without_exposing_its_payload() {
    assert_eq!(PROFILE_256.len(), 256);
    assert_eq!(PROFILE_257.len(), 257);
    let reader = Reader::default();
    let acquired = acquired(&reader);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("admitted metadata");
    let mut redacted = None;
    for profile in [
        "",
        " ",
        "private renderer",
        "private\trenderer",
        "private\nrenderer",
        "private\0renderer",
        "private\x7frenderer",
        "private-é",
        PROFILE_257,
    ] {
        let compiler = Compiler {
            calls: Cell::new(0),
            wrong_identity: false,
            profile,
            profile_reads: Arc::default(),
        };
        let error = match metadata.compile_templates(&compiler, Default::default()) {
            Err(error) => error,
            Ok(_) => panic!("invalid profile must not produce a usable compiled Theme"),
        };
        assert!(matches!(error, build::TemplateError::Profile));
        assert_eq!(error.code(), "template_profile");
        let message = error.to_string();
        if let Some(expected) = &redacted {
            assert_eq!(
                &message, expected,
                "profile payload never changes the redacted diagnostic"
            );
        } else {
            redacted = Some(message);
        }
        assert_eq!(compiler.calls.get(), 1);
        assert_eq!(compiler.profile_reads.load(Ordering::SeqCst), 1);
    }
    let compiler = Compiler {
        calls: Cell::new(0),
        wrong_identity: false,
        profile: PROFILE_256,
        profile_reads: Arc::default(),
    };
    let theme = metadata
        .compile_templates(&compiler, Default::default())
        .expect("exact 256-byte ASCII graphic profile is valid");
    assert_eq!(theme.evidence().tool, PROFILE_256);
    assert_eq!(
        theme
            .render(&"base/parts/page.html".parse().expect("entry"), &json!({}),)
            .expect("valid compiled Theme remains usable"),
        b"four"
    );
    assert_eq!(
        compiler.profile_reads.load(Ordering::SeqCst),
        1,
        "rendering does not query the profile again"
    );
    assert_eq!(
        reader.reads.get(),
        2,
        "profile checks never reread native inputs"
    );
}

#[test]
fn renderer_profile_and_every_supplied_limit_remain_in_evidence_for_identical_bytes() {
    let reader = Reader::default();
    let acquired = acquired(&reader);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("admitted metadata");
    let limits = render::Limits::default();
    let mut variants = vec![
        ("fixture.renderer/1", limits),
        ("fixture.renderer/2", limits),
    ];
    for templates in [
        source::Limits {
            entries: limits.templates.entries - 1,
            ..limits.templates
        },
        source::Limits {
            depth: limits.templates.depth - 1,
            ..limits.templates
        },
        source::Limits {
            file_bytes: limits.templates.file_bytes - 1,
            ..limits.templates
        },
        source::Limits {
            total_bytes: limits.templates.total_bytes - 1,
            ..limits.templates
        },
    ] {
        variants.push((
            "fixture.renderer/1",
            render::Limits {
                templates,
                ..limits
            },
        ));
    }
    variants.push((
        "fixture.renderer/1",
        render::Limits {
            context_bytes: limits.context_bytes - 1,
            ..limits
        },
    ));
    variants.push((
        "fixture.renderer/1",
        render::Limits {
            output_bytes: limits.output_bytes - 1,
            ..limits
        },
    ));
    let mut baseline: Option<Value> = None;
    let mut baseline_identity = None;
    for (profile, limits) in variants {
        let compiler = Compiler {
            calls: Cell::new(0),
            wrong_identity: false,
            profile,
            profile_reads: Arc::default(),
        };
        let theme = metadata
            .compile_templates(&compiler, limits)
            .expect("same retained templates under valid chosen bounds");
        assert_eq!(theme.evidence().tool, profile);
        assert_eq!(theme.evidence().limits, limits);
        let encoded = serde_json::to_value(theme.evidence())
            .expect("real application evidence serialization");
        assert_eq!(encoded["tool"], profile);
        assert_eq!(
            encoded["limits"],
            serde_json::to_value(limits).expect("complete supplied limits")
        );
        let output = theme
            .render(&"base/parts/page.html".parse().expect("entry"), &json!({}))
            .expect("identical output bytes");
        assert_eq!(output, b"four");
        if let Some(original) = &baseline {
            assert_ne!(
                &encoded, original,
                "a changed profile or bound cannot disappear from evidence"
            );
            let mut normalized = encoded.clone();
            normalized["tool"] = original["tool"].clone();
            normalized["limits"] = original["limits"].clone();
            assert_eq!(
                &normalized, original,
                "only the explicit profile or limits changed"
            );
            assert_eq!(Some(theme.evidence().template_tree), baseline_identity);
        } else {
            baseline_identity = Some(theme.evidence().template_tree);
            baseline = Some(encoded);
        }
        assert_eq!(compiler.calls.get(), 1);
        assert_eq!(
            compiler.profile_reads.load(Ordering::SeqCst),
            1,
            "evidence and rendering reuse the captured profile"
        );
        assert_eq!(
            reader.reads.get(),
            2,
            "all evidence comes from the same retained source acquisition"
        );
    }
}

struct InvalidAliasCompiler;
struct InvalidAliasProgram(TreeIdentity);

impl TemplateCompiler for InvalidAliasCompiler {
    fn compile(
        &self,
        input: &TreeInput<'_>,
        _: render::Limits,
    ) -> Result<Box<dyn CompiledTemplates>, render::Error> {
        Ok(Box::new(InvalidAliasProgram(input.inventory().identity())))
    }
}
impl CompiledTemplates for InvalidAliasProgram {
    fn profile(&self) -> &'static str {
        "fixture.alias/1"
    }
    fn identity(&self) -> TreeIdentity {
        self.0
    }
    fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, render::Error> {
        let kind = context["presentation"]["surface"]["kind"]
            .as_str()
            .expect("closed surface kind");
        if entry.as_str() == "base/alias.html" {
            assert!(matches!(kind, "alias" | "graph"));
            Ok(vec![0xff])
        } else {
            assert_eq!(kind, "page");
            assert!(context["presentation"].get("chrome").is_none());
            assert_eq!(
                context["presentation"]["browser_theme_colors"],
                json!({"light": "#ffffff", "dark": "#000000"})
            );
            Ok(b"valid page".to_vec())
        }
    }
}

#[test]
fn alias_renderer_invalid_utf8_cannot_escape_as_a_successful_site() {
    let reader = Reader::default();
    let acquired = acquired(&reader);
    let metadata = acquired
        .check_metadata(Default::default())
        .expect("metadata");
    let theme = metadata
        .compile_templates(&InvalidAliasCompiler, Default::default())
        .expect("matching source identity");
    let text = b"+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='Example'\nlifecycle='draft'\nroute={path='/page/',aliases=['/old/']}\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\nBody\n";
    let corpus = gazette_compiler::corpus::ParsedCorpus::parse(
        [gazette_compiler::corpus::Input {
            path: "content/page.md".parse().expect("path"),
            bytes: text,
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax");
    let authored =
        gazette_compiler::authored::resolve(acquired.compilation(), &corpus, Default::default())
            .expect("graph");
    let error = theme
        .render_site(
            &authored,
            build::SiteRenderOptions {
                render_year: None,
                model_output: Some(&"model.json".parse().expect("path")),
                graph: None,
                limits: Default::default(),
            },
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("renderer failure must discard complete output");
    assert_eq!(error.code(), "page_output_utf8");
    let graph_error = theme
        .render_site(
            &authored,
            build::SiteRenderOptions {
                render_year: None,
                model_output: Some(&"model.json".parse().expect("path")),
                graph: Some(build::GraphRenderOptions {
                    output: &"graph.html".parse().expect("graph path"),
                    limits: Default::default(),
                }),
                limits: Default::default(),
            },
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("invalid graph bytes cannot enter a successful tree");
    assert_eq!(graph_error.code(), "graph_output_utf8");
    assert_eq!(
        reader.reads.get(),
        2,
        "assembly cannot reread mutable sources"
    );
}

#[test]
fn configured_page_entries_require_the_exact_retained_namespace_file() {
    for (entry, valid) in [
        ("base/parts/page.html", true),
        ("parts/page.html", false),
        ("other/parts/page.html", false),
        ("base/missing.html", false),
        ("base/parts", false),
    ] {
        let config = CONFIG.replace(
            "deployable=false",
            &format!(
                "deployable=false\n[sites.composition.page_templates]\n'article:one'='{entry}'"
            ),
        );
        let reader = Reader::default();
        let acquired = acquired_with(&reader, &config);
        let metadata = acquired.check_metadata(Default::default()).unwrap();
        let compiler = Compiler {
            calls: Cell::new(0),
            wrong_identity: false,
            profile: "fixture.templates/1",
            profile_reads: Arc::default(),
        };
        let result = metadata.compile_templates(&compiler, Default::default());
        assert_eq!(result.is_ok(), valid, "entry: {entry}");
        assert_eq!(
            compiler.calls.get(),
            usize::from(valid),
            "invalid entry fails before provider invocation"
        );
    }
}

struct PageSelectionCompiler(EntryPath);
struct PageSelectionProgram {
    identity: TreeIdentity,
    expected: EntryPath,
}
impl TemplateCompiler for PageSelectionCompiler {
    fn compile(
        &self,
        input: &TreeInput<'_>,
        _: render::Limits,
    ) -> Result<Box<dyn CompiledTemplates>, render::Error> {
        Ok(Box::new(PageSelectionProgram {
            identity: input.inventory().identity(),
            expected: self.0.clone(),
        }))
    }
}
impl CompiledTemplates for PageSelectionProgram {
    fn profile(&self) -> &'static str {
        "fixture.page-selection/1"
    }
    fn identity(&self) -> TreeIdentity {
        self.identity
    }
    fn render(&self, entry: &EntryPath, _: &Value) -> Result<Vec<u8>, render::Error> {
        assert_eq!(
            entry, &self.expected,
            "application must use the effective page entry"
        );
        Ok(b"<p>Selected page</p>".to_vec())
    }
}

#[test]
fn application_renders_authored_override_and_configured_fallback_and_records_each_choice() {
    let config = CONFIG.replace(
        "deployable=false",
        "deployable=false\n[sites.composition.page_templates]\n'article:one'='base/alias.html'",
    );
    let reader = Reader::default();
    let acquired = acquired_with(&reader, &config);
    let metadata = acquired.check_metadata(Default::default()).unwrap();
    for (source_template, expected) in [
        ("template='base/parts/page.html'\n", "base/parts/page.html"),
        ("", "base/alias.html"),
    ] {
        let expected: EntryPath = expected.parse().unwrap();
        let theme = metadata
            .compile_templates(&PageSelectionCompiler(expected.clone()), Default::default())
            .unwrap();
        let text = format!(
            "+++\n{source_template}[content]\nid='article:one'\nsite='site:one'\ntitle='Example'\nlifecycle='draft'\nroute={{path='/page/'}}\n[content.structure]\nkind='page'\nfamily={{kind='article'}}\n+++\nBody\n"
        );
        let corpus = gazette_compiler::corpus::ParsedCorpus::parse(
            [gazette_compiler::corpus::Input {
                path: "content/page.md".parse().unwrap(),
                bytes: text.as_bytes(),
            }],
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let authored = gazette_compiler::authored::resolve(
            acquired.compilation(),
            &corpus,
            Default::default(),
        )
        .unwrap();
        let rendered = theme
            .render_site(
                &authored,
                build::SiteRenderOptions {
                    render_year: None,
                    model_output: None,
                    graph: None,
                    limits: Default::default(),
                },
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert_eq!(
            rendered.evidence().pages[&"article:one".parse().unwrap()].template,
            expected
        );
    }
}
