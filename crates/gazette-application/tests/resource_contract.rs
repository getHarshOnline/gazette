//! Application boundary defenses, paired with native JavaScript and executable publication tests.

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use gazette_application::{build, source::PreparationLimits};
use gazette_compiler::toml;
use gazette_core::{
    publication::Key,
    render, resource,
    source::{
        self, CapturedSource, CommitId, Content, EntryPath, Inventory, InventoryBuilder,
        MetadataBudget, Sha256Digest, Subdirectory, TreeIdentity, TreeInput,
    },
};
use gazette_ports::{
    render::{CompiledTemplates, TemplateCompiler},
    resource::ResourceCompiler,
    source::{RevisionReader, SourceCapture, SourceReader},
};
use serde_json::Value;

const CONFIG: &str = r#"
[composition]
theme={package='theme:base'}
[composition.plugins]
'capabilities'={state='enabled'}
[composition.capabilities]
'publication.search/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='shared'},requirement='optional',allow_absence='no-native-acquisition'}
'commerce.selector/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='shared'},requirement='optional',allow_absence='no-native-acquisition'}
[composition.resources]
'module:shared'={state='enabled',resource={owner={kind='plugin',package='capabilities'},resource='module:shared'},output='/assets/shared.js',use='site'}
'module:dependency'={state='enabled',resource={owner={kind='plugin',package='capabilities'},resource='module:dependency'},output='/assets/dependency.js',use='dependency'}
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
authored=[{source='source:content',target='authored'}]
[[sources]]
id='source:theme'
location={kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}
[[sources]]
id='source:plugin'
location={kind='repository',root='/fixture/plugin',commit='1111111111111111111111111111111111111111',subdirectory='input'}
[[sources]]
id='source:content'
location={kind='repository',root='/fixture/content',commit='1111111111111111111111111111111111111111',subdirectory='input'}
[[theme_packages]]
package='theme:base'
source='source:theme'
manifest='theme.toml'
[[plugin_packages]]
package='capabilities'
source='source:plugin'
manifest='plugin.toml'
"#;
const THEME_MANIFEST: &[u8] =
    b"package='theme:base'\nnamespace='base'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";
const PLUGIN_MANIFEST: &[u8] = b"package='capabilities'\n[resources.'module:shared']\ninputs=['main.ts']\nbuild={kind='browser-module',entry='main.ts',imports={'@dependency'='module:dependency'}}\n[resources.'module:dependency']\ninputs=['dependency.ts']\nbuild={kind='browser-module',entry='dependency.ts'}\n[providers.shared]\nimplements=['publication.search/v1','commerce.selector/v1']\n";
const PAGE: &[u8] = b"+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Retained'\nlifecycle='draft'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained body\n";
const SCRIPT: &[u8] = b"export const answer: number = 42;";
const OUTPUT: &[u8] = b"export const answer = 42;";
const DEPENDENCY: &[u8] = b"export const dependency: number = 7;";
const DEPENDENCY_OUTPUT: &[u8] = b"export const dependency = 7;";
const PACKAGE_PAGE: &[u8] = b"+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Package'\nlifecycle='draft'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Package\n";
const PACKAGE_SCRIPT: &[u8] = b"import './lib/dep.ts';\nexport const answer: number = 42;";
const PACKAGE_SCRIPT_DEPENDENCY: &[u8] = b"export const answer: number = 42;";
const PACKAGE_STYLE: &[u8] = b"@use 'styles/tokens';\n.card { color: tokens.$ink; }";
const PACKAGE_STYLE_DEPENDENCY: &[u8] = b"$ink: #123456;";
const PACKAGE_IMAGE: &[u8] = b"retained package image";
const PACKAGE_UNHANDLED: &[u8] = b"no implicit compiler or transport";
const DIAGRAM: &[u8] = b"digraph { a -> b }";
const STALE_DIAGRAM: &[u8] = b"stale native bytes must not become output";
const DIAGRAM_PLUGIN_MANIFEST: &[u8] = b"package='diagram-plugin'\n[extensions.'diagram-plugin.render/v1']\nschema='diagram.schema.json'\n[resources.'image:diagram']\ninputs=['diagram.dot']\nbuild={kind='dot-image',entry='diagram.dot'}\n[providers.diagram]\nimplements=['diagram-plugin.render/v1']\n";

fn path(value: &str) -> EntryPath {
    value.parse().expect("literal entry")
}

fn key(value: &str) -> Key {
    value.parse().expect("literal key")
}

fn captured(root: &Path, limits: source::Limits) -> Result<CapturedSource, source::Error> {
    let mut builder = InventoryBuilder::new(limits);
    let entries: &[(&str, &[u8])] = if root == Path::new("/fixture/theme") {
        builder.insert(path("templates"), Content::Directory)?;
        &[
            ("theme.toml", THEME_MANIFEST),
            ("templates/page.html", b"retained template"),
        ]
    } else if root == Path::new("/fixture/plugin") {
        &[
            ("plugin.toml", PLUGIN_MANIFEST),
            ("main.ts", SCRIPT),
            ("dependency.ts", DEPENDENCY),
        ]
    } else if root == Path::new("/fixture/diagram-plugin") {
        builder.insert(path("assets"), Content::Directory)?;
        &[
            ("plugin.toml", DIAGRAM_PLUGIN_MANIFEST),
            ("diagram.schema.json", br#"{"type":"object"}"#),
            ("diagram.dot", DIAGRAM),
            ("assets/diagram.png", STALE_DIAGRAM),
        ]
    } else if root == Path::new("/fixture/package-content") {
        builder.insert(path("guide"), Content::Directory)?;
        builder.insert(path("guide/lib"), Content::Directory)?;
        builder.insert(path("guide/styles"), Content::Directory)?;
        &[
            ("guide/index.md", PACKAGE_PAGE),
            ("guide/main.ts", PACKAGE_SCRIPT),
            ("guide/lib/dep.ts", PACKAGE_SCRIPT_DEPENDENCY),
            ("guide/main.scss", PACKAGE_STYLE),
            ("guide/styles/_tokens.scss", PACKAGE_STYLE_DEPENDENCY),
            ("guide/image.png", PACKAGE_IMAGE),
        ]
    } else if root == Path::new("/fixture/package-unhandled") {
        builder.insert(path("guide"), Content::Directory)?;
        &[
            ("guide/index.md", PACKAGE_PAGE),
            ("guide/private.bin", PACKAGE_UNHANDLED),
        ]
    } else {
        assert_eq!(root, Path::new("/fixture/content"));
        &[("authored.md", PAGE)]
    };
    let files: BTreeMap<_, _> = entries
        .iter()
        .map(|(name, bytes)| (path(name), bytes.to_vec()))
        .collect();
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
    sealed: Cell<bool>,
}

impl Reader {
    fn observe(&self, subdirectory: &Subdirectory) {
        assert!(
            !self.sealed.get(),
            "rendering cannot reopen acquisition inputs"
        );
        assert_eq!(subdirectory, &"input".parse().expect("subdirectory"));
        self.reads.set(self.reads.get() + 1);
    }
}

impl SourceReader for Reader {
    fn read(
        &self,
        _: &Path,
        _: Option<&Subdirectory>,
        _: source::Limits,
    ) -> Result<Inventory, source::Error> {
        panic!("publication requires retained bytes, not inventory-only reads");
    }
}
impl SourceCapture for Reader {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        self.observe(subdirectory.expect("explicit selection"));
        captured(root, limits)
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
        self.observe(subdirectory);
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        Ok(captured(root, limits)?.into_parts().0)
    }
    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, source::Error> {
        panic!("fixture declares exact repositories, not gitlinks");
    }
}

fn acquired(reader: &Reader) -> build::AcquiredInputs {
    acquired_configuration(reader, CONFIG)
}

fn acquired_configuration(reader: &Reader, configuration: &str) -> build::AcquiredInputs {
    acquired_configuration_with_reads(reader, configuration, 6)
}

fn acquired_configuration_with_reads(
    reader: &Reader,
    configuration: &str,
    reads: usize,
) -> build::AcquiredInputs {
    let inputs = build::acquire_publication(
        reader,
        reader,
        configuration,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        PreparationLimits {
            sources: 3,
            entries: 16,
            bytes: 4096,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("real configuration, source verification and publication acquisition");
    assert_eq!(reader.reads.get(), reads);
    reader.sealed.set(true);
    inputs
}

fn package_configuration(root: &str) -> String {
    format!(
        "[composition]\ntheme={{package='theme:base'}}\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='Example'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\n\
         authored=[{{source='source:content',target='authored',static_assets={{route_prefix='/media/'}}}}]\n\
         [[sources]]\nid='source:theme'\nlocation={{kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\nid='source:content'\nlocation={{kind='repository',root='{root}',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='theme.toml'\n"
    )
}

fn diagram_configuration(
    requirement: &str,
    absence: Option<&str>,
    select_resource: bool,
) -> String {
    let absence = absence
        .map(|reason| format!(",allow_absence='{reason}'"))
        .unwrap_or_default();
    let resource = if select_resource {
        "[composition.resources]\n'image:diagram'={state='enabled',resource={owner={kind='plugin',package='diagram-plugin'},resource='image:diagram'},output='/assets/diagram.png',use='site'}\n"
    } else {
        ""
    };
    format!(
        "dot_renderer={{program='/opt/gazette/bin/dot',executable={{bytes=1,sha256='0000000000000000000000000000000000000000000000000000000000000000'}}}}\n[composition]\ntheme={{package='theme:base'}}\n[composition.plugins]\n'diagram-plugin'={{state='enabled'}}\n[composition.capabilities]\n'diagram-plugin.render/v1'={{state='enabled',provider={{kind='plugin',package='diagram-plugin',provider='diagram'}},requirement='{requirement}'{absence}}}\n{resource}[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{{source='source:content',target='authored'}}]\nmachine={{search='lexical'}}\n[[sources]]\nid='source:theme'\nlocation={{kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n[[sources]]\nid='source:plugin'\nlocation={{kind='repository',root='/fixture/diagram-plugin',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n[[sources]]\nid='source:content'\nlocation={{kind='repository',root='/fixture/content',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n[[theme_packages]]\npackage='theme:base'\nsource='source:theme'\nmanifest='theme.toml'\n[[plugin_packages]]\npackage='diagram-plugin'\nsource='source:plugin'\nmanifest='plugin.toml'\n"
    )
}

struct DeniedDiagramRunner;

impl gazette_ports::process::IsolatedCaptureRunner for DeniedDiagramRunner {
    fn capture_isolated(
        &self,
        _: gazette_core::process::IsolatedCaptureRequest<'_>,
    ) -> Result<gazette_core::process::Captured, gazette_core::process::Error> {
        panic!("execution permission must be checked before the native runner")
    }
}

struct Templates;
struct TemplateProgram(TreeIdentity);
struct CapabilityTemplates;
struct CapabilityTemplateProgram(TreeIdentity);

impl TemplateCompiler for Templates {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        _: render::Limits,
    ) -> Result<Box<dyn CompiledTemplates>, render::Error> {
        assert_eq!(
            source.file(&path("base/page.html")),
            Some(b"retained template".as_slice())
        );
        Ok(Box::new(TemplateProgram(source.inventory().identity())))
    }
}
impl CompiledTemplates for TemplateProgram {
    fn identity(&self) -> TreeIdentity {
        self.0
    }
    fn profile(&self) -> &'static str {
        "test-renderer@1/resource-boundary"
    }
    fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, render::Error> {
        assert_eq!(entry, &path("base/page.html"));
        assert_eq!(context["content"]["id"], "content:one");
        Ok(b"<p>boundary control</p>".to_vec())
    }
}

impl TemplateCompiler for CapabilityTemplates {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        _: render::Limits,
    ) -> Result<Box<dyn CompiledTemplates>, render::Error> {
        assert_eq!(
            source.file(&path("base/page.html")),
            Some(b"retained template".as_slice())
        );
        Ok(Box::new(CapabilityTemplateProgram(
            source.inventory().identity(),
        )))
    }
}

impl CompiledTemplates for CapabilityTemplateProgram {
    fn identity(&self) -> TreeIdentity {
        self.0
    }

    fn profile(&self) -> &'static str {
        "test-renderer@1/optional-degradation"
    }

    fn render(&self, entry: &EntryPath, context: &Value) -> Result<Vec<u8>, render::Error> {
        assert_eq!(entry, &path("base/page.html"));
        let capability = &context["presentation"]["capabilities"]["diagram-plugin.render/v1"];
        Ok(format!(
            "<p data-capability=\"diagram\">{}:{}</p>",
            capability["state"]
                .as_str()
                .expect("typed capability state"),
            capability["reason"].as_str().expect("typed absence reason")
        )
        .into_bytes())
    }
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    WrongIdentity,
    OutsideDependency,
    MissingEntry,
    EmptyTool,
    Oversized,
    OversizedReferences,
    OutsideReference,
    MissingReference,
    InventedImage,
}

struct Resources {
    calls: Cell<usize>,
    fault: Fault,
}

impl ResourceCompiler for Resources {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        links: &resource::ModuleLinks,
        limits: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        assert_eq!(
            links
                .get(&"module:dependency".parse().expect("resource id"))
                .map(String::as_str),
            Some("https://example.test/assets/dependency.js")
        );
        self.calls.set(self.calls.get() + 1);
        assert_eq!(recipe.entry(), &path("main.ts"));
        assert_eq!(
            source.inventory().entries().len(),
            1,
            "only declared compiler inputs reach this port"
        );
        assert_eq!(source.file(recipe.entry()), Some(SCRIPT));
        let mut result = resource::Artifact {
            input: source.inventory().identity(),
            bytes: OUTPUT.to_vec(),
            dependencies: BTreeSet::from([recipe.entry().clone()]),
            references: BTreeSet::new(),
            deferred_references: Default::default(),
            svg: None,
            image: None,
            tool: "boundary-control-v1",
        };
        match self.fault {
            Fault::None => {}
            Fault::WrongIdentity => {
                result.input = InventoryBuilder::new(source::Limits::default())
                    .finish()
                    .identity()
            }
            Fault::OutsideDependency => {
                result.dependencies.insert(path("outside.ts"));
            }
            Fault::MissingEntry => result.dependencies.clear(),
            Fault::EmptyTool => result.tool = "",
            Fault::Oversized => result.bytes = vec![b'x'; limits.output_bytes + 1],
            Fault::OversizedReferences => {
                result
                    .references
                    .insert("x".repeat(limits.output_bytes + 1));
            }
            Fault::OutsideReference => {
                result.references.insert("https://other.test/a".into());
            }
            Fault::MissingReference => {
                result.references.insert("missing.png".into());
            }
            Fault::InventedImage => {
                let size = gazette_core::raster::Dimensions {
                    width: 1,
                    height: 1,
                };
                result.image = Some(gazette_core::raster::Geometry {
                    source: size,
                    oriented: size,
                    output: size,
                });
            }
        }
        Ok(result)
    }
}

struct ReachableResources {
    calls: Cell<usize>,
    deferred: bool,
}

struct PagePackageResources;

impl ResourceCompiler for PagePackageResources {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        links: &resource::ModuleLinks,
        _: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        assert!(links.is_empty());
        let (bytes, dependencies) = match recipe {
            resource::Recipe::BrowserModule { entry, .. } => {
                assert_eq!(entry, &path("guide/main.ts"));
                assert_eq!(source.file(entry), Some(PACKAGE_SCRIPT));
                assert_eq!(
                    source.file(&path("guide/lib/dep.ts")),
                    Some(PACKAGE_SCRIPT_DEPENDENCY)
                );
                (
                    b"export const answer = 42;".to_vec(),
                    BTreeSet::from([entry.clone(), path("guide/lib/dep.ts")]),
                )
            }
            resource::Recipe::Stylesheet { entry } => {
                assert_eq!(entry, &path("guide/main.scss"));
                assert_eq!(source.file(entry), Some(PACKAGE_STYLE));
                assert_eq!(
                    source.file(&path("guide/styles/_tokens.scss")),
                    Some(PACKAGE_STYLE_DEPENDENCY)
                );
                (
                    b".card { color: #123456; }".to_vec(),
                    BTreeSet::from([entry.clone(), path("guide/styles/_tokens.scss")]),
                )
            }
            _ => panic!("page package uses existing script and stylesheet recipes"),
        };
        Ok(resource::Artifact {
            input: source.inventory().identity(),
            bytes,
            dependencies,
            references: BTreeSet::new(),
            deferred_references: Default::default(),
            svg: None,
            image: None,
            tool: "page-package-fixture-v1",
        })
    }
}

impl ResourceCompiler for ReachableResources {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        links: &resource::ModuleLinks,
        _: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        self.calls.set(self.calls.get() + 1);
        let entry = recipe.entry();
        let (bytes, references) = if entry == &path("main.ts") {
            let dependency = links
                .get(&"module:dependency".parse().expect("resource id"))
                .expect("exact selected dependency URL")
                .clone();
            (OUTPUT.to_vec(), BTreeSet::from([dependency]))
        } else {
            assert_eq!(entry, &path("dependency.ts"));
            assert!(links.is_empty());
            (DEPENDENCY_OUTPUT.to_vec(), BTreeSet::new())
        };
        Ok(resource::Artifact {
            input: source.inventory().identity(),
            bytes,
            dependencies: BTreeSet::from([entry.clone()]),
            deferred_references: if self.deferred {
                references.clone()
            } else {
                Default::default()
            },
            references,
            svg: None,
            image: None,
            tool: "reachable-resource-v1",
        })
    }
}

fn render<'a>(
    inputs: &'a build::AcquiredInputs,
    resources: &impl ResourceCompiler,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    let mut limits = build::PublicationRenderLimits::default();
    limits.output.resources.output_bytes = OUTPUT.len();
    inputs.render_publication(
        &Templates,
        resources,
        &gazette_core::raster::unavailable,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&path("model.json")),
            graph: None,
            limits: source::Limits::default(),
        },
        limits,
        &gazette_adapter_markdown::Comrak,
    )
}

fn render_diagram<'a>(
    inputs: &'a build::AcquiredInputs,
    resources: &impl ResourceCompiler,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    inputs.render_publication(
        &CapabilityTemplates,
        resources,
        &gazette_core::raster::unavailable,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&path("model.json")),
            graph: None,
            limits: source::Limits::default(),
        },
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
}

#[test]
fn only_reachable_selected_package_modules_compile_with_exact_resource_evidence() {
    for deferred in [false, true] {
        let reader = Reader::default();
        let inputs = acquired(&reader);
        let resources = ReachableResources {
            calls: Cell::new(0),
            deferred,
        };
        let rendered = inputs
            .render_publication(
                &Templates,
                &resources,
                &gazette_core::raster::unavailable,
                build::SiteRenderOptions {
                    render_year: None,
                    model_output: Some(&path("model.json")),
                    graph: None,
                    limits: source::Limits::default(),
                },
                Default::default(),
                &gazette_adapter_markdown::Comrak,
            )
            .expect("reachable package resources compile through one selected graph");
        assert_eq!(resources.calls.get(), 2);
        assert_eq!(
            rendered.site().output().file(&path("assets/dependency.js")),
            Some(DEPENDENCY_OUTPUT)
        );
        let resource: gazette_core::resource::ResourceId =
            "module:dependency".parse().expect("resource id");
        let evidence = &rendered.site().evidence().resources[&resource];
        assert_eq!(evidence.resource.resource, resource);
        assert_eq!(evidence.source, "source:plugin".parse().expect("source"));
        assert_eq!(reader.reads.get(), 6);
        let model: Value =
            serde_json::from_slice(rendered.site().output().file(&path("model.json")).unwrap())
                .unwrap();
        let graph = &model["browser_modules"];
        assert!(graph["modules"].get("module:dependency").is_some());
        assert_eq!(
            graph["modules"]["module:shared"]["dependencies"],
            serde_json::json!(["module:dependency"])
        );
        for preloads in [&graph["site_preloads"], &graph["pages"]["content:one"]] {
            let values = preloads.as_array().expect("explicit eager closure");
            assert!(values.contains(&serde_json::json!("module:shared")));
            assert_eq!(
                values.contains(&serde_json::json!("module:dependency")),
                !deferred
            );
        }
        assert_eq!(
            graph["modules"]["module:shared"]
                .get("deferred_references")
                .is_some(),
            deferred
        );
    }
}

#[test]
fn retained_leaf_package_reaches_the_complete_application_output_without_another_compiler() {
    let configuration = package_configuration("/fixture/package-content");
    let reader = Reader::default();
    let inputs = acquired_configuration_with_reads(&reader, &configuration, 4);
    let rendered = inputs
        .render_publication(
            &Templates,
            &PagePackageResources,
            &gazette_core::raster::unavailable,
            build::SiteRenderOptions {
                render_year: None,
                model_output: Some(&path("model.json")),
                graph: None,
                limits: source::Limits::default(),
            },
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("retained page package through the complete application build");
    let output = rendered.site().output();
    assert_eq!(
        output.file(&path("guide/main.js")),
        Some(b"export const answer = 42;".as_slice())
    );
    assert_eq!(
        output.file(&path("guide/main.css")),
        Some(b".card { color: #123456; }".as_slice())
    );
    assert_eq!(output.file(&path("guide/lib/dep.js")), None);
    assert_eq!(output.file(&path("guide/image.png")), Some(PACKAGE_IMAGE));
    let evidence = &rendered.site().evidence().page_resources[&key("content:one")];
    assert_eq!(evidence.len(), 2);
    assert_eq!(
        evidence
            .iter()
            .map(|entry| entry.output.as_str())
            .collect::<Vec<_>>(),
        ["guide/main.js", "guide/main.css"]
    );
    assert_eq!(
        rendered.site().evidence().assets[&path("guide/image.png")].path,
        path("authored/guide/image.png")
    );
    let model: Value = serde_json::from_slice(
        output
            .file(&path("model.json"))
            .expect("shared application model"),
    )
    .expect("model JSON");
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["resources"]
            .as_array()
            .expect("page resource projections")
            .len(),
        2
    );
    assert_eq!(
        model["pages"]["content:one"]["assets"][0]["url"],
        "https://example.test/guide/image.png"
    );
}

#[test]
fn unhandled_leaf_package_companion_refuses_the_complete_application_output() {
    let configuration = package_configuration("/fixture/package-unhandled");
    let reader = Reader::default();
    let inputs = acquired_configuration_with_reads(&reader, &configuration, 4);
    let error = inputs
        .render_publication(
            &Templates,
            &PagePackageResources,
            &gazette_core::raster::unavailable,
            build::SiteRenderOptions {
                render_year: None,
                model_output: Some(&path("model.json")),
                graph: None,
                limits: source::Limits::default(),
            },
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("an unhandled retained companion cannot disappear or be copied");
    assert!(matches!(
        error,
        build::PublicationRenderError::Site(build::SiteRenderError::UncompiledAsset(rejected))
            if rejected == path("authored/guide/private.bin")
    ));
}

#[test]
fn malformed_successful_resource_results_cannot_produce_a_publication() {
    let reader = Reader::default();
    let inputs = acquired(&reader);
    for fault in [
        Fault::WrongIdentity,
        Fault::OutsideDependency,
        Fault::MissingEntry,
        Fault::EmptyTool,
        Fault::Oversized,
        Fault::OversizedReferences,
        Fault::OutsideReference,
        Fault::MissingReference,
        Fault::InventedImage,
    ] {
        let resources = Resources {
            calls: Cell::new(0),
            fault,
        };
        let error = render(&inputs, &resources)
            .expect_err("application must independently reject provider contract violation");
        match (&fault, error) {
            (
                Fault::Oversized,
                build::PublicationRenderError::Resource(resource::Error::Limit("output bytes")),
            ) => {}
            (
                Fault::OversizedReferences | Fault::OutsideReference | Fault::MissingReference,
                build::PublicationRenderError::Resource(resource::Error::Input { reason, .. }),
            ) => {
                assert_eq!(
                    reason,
                    "compiled module reference differs from its selected import bindings"
                );
            }
            (
                Fault::InventedImage,
                build::PublicationRenderError::Resource(resource::Error::Input { reason, .. }),
            ) => {
                assert_eq!(
                    reason,
                    "compiler geometry differs from the selected resource contract"
                );
            }
            (
                Fault::OutsideDependency,
                build::PublicationRenderError::Resource(resource::Error::Input {
                    path: rejected,
                    reason,
                }),
            ) => {
                assert_eq!(rejected, path("outside.ts"));
                assert_eq!(reason, "compiler dependency lies outside declared inputs");
            }
            (
                Fault::WrongIdentity | Fault::MissingEntry | Fault::EmptyTool,
                build::PublicationRenderError::Resource(resource::Error::Input {
                    path: rejected,
                    reason,
                }),
            ) => {
                assert_eq!(rejected, path("main.ts"));
                assert_eq!(
                    reason,
                    "compiler evidence differs from the selected input contract"
                );
            }
            (_, other) => panic!("wrong boundary failure for {fault:?}: {other:?}"),
        }
        assert_eq!(
            resources.calls.get(),
            1,
            "failure cannot be retried as another capability"
        );
        assert_eq!(reader.reads.get(), 6);
    }
}

#[test]
fn cached_provider_success_never_bypasses_application_evidence_checks() {
    let reader = Reader::default();
    let inputs = acquired(&reader);
    for fault in [
        Fault::WrongIdentity,
        Fault::OutsideDependency,
        Fault::MissingEntry,
        Fault::EmptyTool,
        Fault::Oversized,
        Fault::OversizedReferences,
        Fault::OutsideReference,
        Fault::MissingReference,
        Fault::InventedImage,
    ] {
        let cache = build::ResourceCache::new(
            Resources {
                calls: Cell::new(0),
                fault,
            },
            Default::default(),
        );
        let first = render(&inputs, &cache).expect_err("cold provider evidence refused");
        let second = render(&inputs, &cache).expect_err("reused provider evidence still refused");
        assert_eq!(
            first.code(),
            second.code(),
            "same rejected boundary for {fault:?}"
        );
        assert_eq!(first.to_string(), second.to_string());
    }
}

#[test]
fn capability_status_and_site_resource_compilation_remain_independent() {
    let reader = Reader::default();
    let inputs = acquired(&reader);
    let resources = Resources {
        calls: Cell::new(0),
        fault: Fault::None,
    };
    let rendered =
        render(&inputs, &resources).expect("control response meets the application contract");
    assert_eq!(
        resources.calls.get(),
        1,
        "one site resource compiles once, independently from two capability selections"
    );
    let tree = rendered.site().output();
    assert_eq!(tree.file(&path("assets/shared.js")), Some(OUTPUT));
    let model: Value =
        serde_json::from_slice(tree.file(&path("model.json")).expect("assembled model"))
            .expect("actual emitted JSON");
    assert_eq!(
        model["capabilities"]
            .as_object()
            .expect("capability map")
            .len(),
        2
    );
    let unavailable = serde_json::json!({
        "state": "unavailable",
        "reason": "no-native-acquisition"
    });
    assert_eq!(model["capabilities"]["publication.search/v1"], unavailable);
    assert_eq!(model["capabilities"]["commerce.selector/v1"], unavailable);
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["capabilities"],
        model["capabilities"]
    );
    let evidence = &rendered.site().evidence().capabilities;
    assert_eq!(evidence.len(), 2);
    let gazette_application::build::CapabilityOutcome::Unavailable { reason } =
        &evidence[&"publication.search/v1".parse().expect("capability")].outcome
    else {
        panic!("missing acquisition remains explicitly unavailable")
    };
    assert_eq!(
        *reason,
        gazette_core::publication::capability_runtime::CapabilityAbsence::NoNativeAcquisition
    );
    let resource =
        &rendered.site().evidence().resources[&"module:shared".parse().expect("resource")];
    assert_eq!(resource.output_sha256, Sha256Digest::of(OUTPUT));
    assert_eq!(
        model["resources"]["module:shared"]["url"],
        "https://example.test/assets/shared.js"
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["theme_resources"],
        serde_json::json!([{
            "id": "module:shared",
            "kind": "browser-module",
            "url": "https://example.test/assets/shared.js",
            "output": "assets/shared.js",
            "sha256": Sha256Digest::of(OUTPUT),
            "bytes": OUTPUT.len()
        }])
    );
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["resources"],
        serde_json::json!([])
    );
    assert_eq!(reader.reads.get(), 6);
}

#[test]
fn unavailable_capability_never_infers_or_compiles_a_resource() {
    let configuration = CONFIG
        .replace(
            "'commerce.selector/v1'={state='enabled',provider={kind='plugin',package='capabilities',provider='shared'},requirement='optional',allow_absence='no-native-acquisition'}",
            "'commerce.selector/v1'={state='disabled'}",
        )
        .replace(
            "'module:shared'={state='enabled',resource={owner={kind='plugin',package='capabilities'},resource='module:shared'},output='/assets/shared.js',use='site'}",
            "'module:shared'={state='disabled'}",
        );
    let reader = Reader::default();
    let inputs = acquired_configuration(&reader, &configuration);
    let resources = Resources {
        calls: Cell::new(0),
        fault: Fault::None,
    };
    let rendered = render(&inputs, &resources).expect("explicit provider unavailability");
    assert_eq!(
        resources.calls.get(),
        0,
        "capability cannot infer a resource"
    );
    assert!(
        rendered
            .site()
            .output()
            .file(&path("assets/shared.js"))
            .is_none()
    );
    assert!(rendered.site().evidence().resources.is_empty());
    let capability: gazette_core::publication::CapabilityId =
        "publication.search/v1".parse().expect("capability");
    let evidence = &rendered.site().evidence().capabilities[&capability];
    let build::CapabilityOutcome::Unavailable { reason } = &evidence.outcome else {
        panic!("missing acquisition remains explicitly unavailable")
    };
    assert_eq!(
        *reason,
        gazette_core::publication::capability_runtime::CapabilityAbsence::NoNativeAcquisition
    );
}

#[test]
fn diagram_acquisition_discovers_the_exact_selected_plugin_resource_before_execution() {
    let reader = Reader::default();
    let configuration = diagram_configuration("required", None, true);
    let site = "site:one".parse().expect("site");
    let mut inputs = build::acquire_publications(
        &reader,
        &reader,
        &configuration,
        &[site],
        toml::Limits::default(),
        Default::default(),
    )
    .expect("package-qualified publication set acquisition");
    reader.sealed.set(true);
    let error = inputs
        .acquire_diagrams(
            &DeniedDiagramRunner,
            &gazette_core::raster::unavailable,
            Default::default(),
            &gazette_adapter_markdown::Comrak,
            Default::default(),
        )
        .expect_err("selected DOT resource requires explicit execution permission");
    assert_eq!(error.code(), "diagram_permission");
    assert!(inputs.diagram_capture().is_none());
}

#[test]
fn exact_missing_native_acquisition_is_shared_retained_and_replayable_without_stale_output() {
    let reader = Reader::default();
    let configuration = diagram_configuration("optional", Some("no-native-acquisition"), false);
    let inputs = acquired_configuration(&reader, &configuration);
    assert_eq!(
        inputs.sources()[&"source:plugin".parse().expect("source key")]
            .file(&path("assets/diagram.png")),
        Some(STALE_DIAGRAM),
        "the captured stale source remains evidence, not an output instruction"
    );
    let resources = Resources {
        calls: Cell::new(0),
        fault: Fault::None,
    };
    let rendered = render_diagram(&inputs, &resources)
        .expect("the exact optional native-acquisition absence is permitted");
    assert_eq!(
        resources.calls.get(),
        0,
        "a missing retained native result never invokes a substitute resource compiler"
    );
    let tree = rendered.site().output();
    assert!(tree.file(&path("assets/diagram.png")).is_none());
    let model: Value =
        serde_json::from_slice(tree.file(&path("model.json")).expect("shared model"))
            .expect("actual emitted JSON");
    let expected = serde_json::json!({
        "state": "unavailable",
        "reason": "no-native-acquisition"
    });
    assert_eq!(model["capabilities"]["diagram-plugin.render/v1"], expected);
    assert_eq!(
        model["pages"]["content:one"]["presentation"]["capabilities"]["diagram-plugin.render/v1"],
        expected
    );
    let html = std::str::from_utf8(tree.file(&path("index.html")).expect("Theme page"))
        .expect("UTF-8 fixture HTML");
    assert!(html.contains("unavailable:no-native-acquisition"));
    assert!(!html.contains("/assets/diagram.png"));
    let search = std::str::from_utf8(tree.file(&path("search.json")).expect("search output"))
        .expect("UTF-8 search JSON");
    assert!(
        !search.contains("/assets/diagram.png"),
        "search cannot retain a URL for an unavailable capability"
    );
    let evidence = &rendered.site().evidence().capabilities
        [&"diagram-plugin.render/v1".parse().expect("capability key")];
    let build::CapabilityOutcome::Unavailable { reason } = &evidence.outcome else {
        panic!("missing acquisition remains explicitly unavailable")
    };
    assert_eq!(
        *reason,
        gazette_core::publication::capability_runtime::CapabilityAbsence::NoNativeAcquisition
    );
    assert!(rendered.site().evidence().resources.is_empty());
    let first = tree.inventory().identity();
    drop(rendered);
    let replay = render_diagram(&inputs, &resources).expect("retained-input replay");
    assert_eq!(replay.site().output().inventory().identity(), first);
    assert_eq!(resources.calls.get(), 0);
}

#[test]
fn missing_native_acquisition_never_degrades_without_the_exact_optional_permission() {
    for (requirement, permission) in [("required", None), ("optional", None)] {
        let reader = Reader::default();
        let configuration = diagram_configuration(requirement, permission, false);
        let inputs = acquired_configuration(&reader, &configuration);
        let resources = Resources {
            calls: Cell::new(0),
            fault: Fault::None,
        };
        let error = render_diagram(&inputs, &resources)
            .expect_err("required or differently permitted native input must fail closed");
        assert_eq!(error.code(), "publication_uncompiled_capability");
        assert_eq!(resources.calls.get(), 0);
    }
}
