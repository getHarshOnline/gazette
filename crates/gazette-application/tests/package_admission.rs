//! Captured Theme/plugin package admission through the real application boundary.

use std::{collections::BTreeMap, path::Path};

use gazette_application::{build, source::PreparationLimits};
use gazette_compiler::toml;
use gazette_core::{
    plugin::PluginPackageId,
    source::{
        CapturedSource, CommitId, Content, EntryPath, Error, Inventory, InventoryBuilder, Limits,
        MetadataBudget, RevisionLimits, Sha256Digest, Subdirectory, TreeIdentity, TreeInput,
    },
};
use gazette_ports::{
    render::{CompiledTemplates, TemplateCompiler},
    source::{RevisionReader, SourceCapture, SourceReader},
};

const CONFIG: &str = r#"
[composition]
theme={package='theme:root',palette='palette:plain'}
[composition.plugins]
'commerce-plugin'={state='enabled'}
[composition.capabilities]
'commerce.catalog/v1'={state='enabled',provider={kind='plugin',package='commerce-plugin',provider='catalog'},requirement='required'}
[composition.resources]
'module:catalog'={state='enabled',resource={owner={kind='plugin',package='commerce-plugin'},resource='module:catalog'},output='/assets/catalog.js',use='site'}
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sources]]
id='source:foundation'
location={kind='repository',root='/fixture/foundation',commit='1111111111111111111111111111111111111111',subdirectory='package'}
[[sources]]
id='source:root'
location={kind='repository',root='/fixture/root',commit='1111111111111111111111111111111111111111',subdirectory='package'}
[[sources]]
id='source:plugin'
location={kind='repository',root='/fixture/plugin',commit='1111111111111111111111111111111111111111',subdirectory='package'}
[[theme_packages]]
package='theme:foundation'
source='source:foundation'
manifest='theme.toml'
[[theme_packages]]
package='theme:root'
source='source:root'
manifest='theme.toml'
[[plugin_packages]]
package='commerce-plugin'
source='source:plugin'
manifest='plugin.toml'
"#;

const FOUNDATION: &[u8] =
    b"package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\n";
const ROOT: &[u8] = b"package='theme:root'\nnamespace='root'\ntemplates='templates'\npalettes=[{id='palette:plain',browser_theme_colors={light='#ffffff',dark='#000000'}}]\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:foundation'\n[components.notice]\ntemplate='notice.html'\n";
const PLUGIN: &[u8] = b"package='commerce-plugin'\n[resources.'module:catalog']\ninputs=['catalog.ts']\nbuild={kind='browser-module',entry='catalog.ts'}\n[providers.catalog]\nimplements=['commerce.catalog/v1']\n";
const PLUGIN_WITH_UNEXPORTED_RESOURCE: &[u8] =
    b"package='commerce-plugin'\n[providers.catalog]\nimplements=['commerce.catalog/v1']\n";

const EXPLICIT_PLUGIN_SELECTION_CONFIG: &str = r#"
[composition]
theme={package='theme:root',palette='palette:plain'}
[composition.plugins]
'component-plugin'={state='enabled'}
'extension-plugin'={state='enabled'}
[[principals]]
id='owner:one'
kind='person'
[[sites]]
id='site:one'
publisher='owner:one'
deployable=false
[[sources]]
id='source:foundation'
location={kind='repository',root='/fixture/foundation',commit='1111111111111111111111111111111111111111',subdirectory='package'}
[[sources]]
id='source:root'
location={kind='repository',root='/fixture/root',commit='1111111111111111111111111111111111111111',subdirectory='package'}
[[sources]]
id='source:plugins'
location={kind='repository',root='/fixture/plugins',commit='1111111111111111111111111111111111111111',subdirectory='package'}
[[theme_packages]]
package='theme:foundation'
source='source:foundation'
manifest='theme.toml'
[[theme_packages]]
package='theme:root'
source='source:root'
manifest='theme.toml'
[[plugin_packages]]
package='component-plugin'
source='source:plugins'
manifest='component.toml'
[[plugin_packages]]
package='extension-plugin'
source='source:plugins'
manifest='extension.toml'
[[plugin_packages]]
package='unselected-plugin'
source='source:plugins'
manifest='unselected.toml'
"#;

const COMPONENT_PLUGIN: &[u8] = b"package='component-plugin'\n[components.card]\narguments='card.arguments.json'\nmodel='card.model.json'\n";
const EXTENSION_PLUGIN: &[u8] = b"package='extension-plugin'\n[extensions.'extension-plugin.record/v1']\nschema='record.schema.json'\n[providers.record]\nimplements=['extension-plugin.record/v1']\n";

struct Reader {
    plugin: &'static [u8],
    faq: Option<&'static [u8]>,
}

impl Reader {
    fn new(plugin: &'static [u8]) -> Self {
        Self { plugin, faq: None }
    }
    fn captured(&self, root: &Path, limits: Limits) -> Result<CapturedSource, Error> {
        let (manifest, mut entries): (&[u8], Vec<(&str, &[u8])>) = match root.to_str() {
            Some("/fixture/foundation") => (
                FOUNDATION,
                vec![("templates/base.html", b"foundation".as_slice())],
            ),
            Some("/fixture/root") if self.faq.is_some() => (
                b"package='theme:root'\nnamespace='root'\ntemplates='templates'\npalettes=[{id='palette:plain',browser_theme_colors={light='#ffffff',dark='#000000'}}]\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:foundation'\n[components.notice]\ntemplate='notice.html'\nsemantic={kind='faq-item',question_argument='question'}\narguments={type='object',required=['question'],properties={question={type='string'}}}\n",
                vec![("templates/page.html", b"{{ body_html | safe }}".as_slice()), ("templates/notice.html", self.faq.unwrap())],
            ),
            Some("/fixture/root") => (
                ROOT,
                vec![
                    (
                        "templates/page.html",
                        b"{% extends 'foundation/base.html' %}",
                    ),
                    (
                        "templates/notice.html",
                        b"{{ component.context.address }}|{{ component.context.host.content }}|{{ component.context.host.site }}|{{ component.context.host.surface.kind }}|{{ component.context.host.surface.family.kind }}",
                    ),
                ],
            ),
            Some("/fixture/plugin") => (self.plugin, vec![("catalog.ts", b"export {}")]),
            _ => panic!("unexpected fixture root"),
        };
        entries.push(("theme.toml", manifest));
        if root == Path::new("/fixture/plugin") {
            entries.pop();
            entries.push(("plugin.toml", manifest));
        }
        captured(entries, limits)
    }
}

impl SourceReader for Reader {
    fn read(&self, _: &Path, _: Option<&Subdirectory>, _: Limits) -> Result<Inventory, Error> {
        panic!("package admission must use retained captures")
    }
}

impl SourceCapture for Reader {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        assert_eq!(subdirectory, Some(&"package".parse().unwrap()));
        self.captured(root, limits)
    }
}

impl RevisionReader for Reader {
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        assert_eq!(subdirectory, &"package".parse().unwrap());
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        Ok(self.captured(root, limits)?.into_parts().0)
    }

    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, Error> {
        panic!("direct repository fixtures do not discover gitlinks")
    }
}

fn captured(entries: Vec<(&str, &[u8])>, limits: Limits) -> Result<CapturedSource, Error> {
    let mut builder = InventoryBuilder::new(limits);
    if entries
        .iter()
        .any(|(path, _)| path.starts_with("templates/"))
    {
        builder.insert("templates".parse()?, Content::Directory)?;
    }
    let files = entries
        .into_iter()
        .map(|(path, bytes)| Ok((path.parse::<EntryPath>()?, bytes.to_vec())))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
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

fn acquire(plugin: &'static [u8]) -> build::AcquiredInputs {
    let reader = Reader::new(plugin);
    build::acquire_composition(
        &reader,
        &reader,
        CONFIG,
        &"site:one".parse().unwrap(),
        toml::Limits::default(),
        PreparationLimits {
            sources: 3,
            entries: 32,
            bytes: 16 << 10,
            per_source: RevisionLimits::default(),
        },
    )
    .expect("exact package capture")
}

#[test]
fn package_roles_graph_and_exact_identities_are_admitted_without_live_reads() {
    let acquired = acquire(PLUGIN);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("complete package admission");
    let inspection = metadata.inspect();
    assert_eq!(inspection.root_theme.as_str(), "theme:root");
    assert_eq!(
        inspection
            .theme_order
            .iter()
            .map(|package| package.as_str())
            .collect::<Vec<_>>(),
        ["theme:foundation", "theme:root"]
    );
    assert_eq!(inspection.theme_packages.len(), 2);
    assert_eq!(inspection.plugin_packages.len(), 1);
    let plugin = inspection.plugin_packages.values().next().unwrap();
    assert_eq!(plugin.identity.package.as_str(), "commerce-plugin");
    assert_eq!(plugin.identity.manifest, Sha256Digest::of(PLUGIN));

    let theme = metadata
        .compile_templates(&Compiler, Default::default())
        .expect("one compiled virtual namespace");
    assert_eq!(theme.render_recipes().page.as_str(), "root/page.html");
    assert_eq!(theme.evidence().packages.len(), 2);
    assert_eq!(
        theme.evidence().packages[0].namespace.as_str(),
        "foundation"
    );
    assert_eq!(theme.evidence().packages[1].namespace.as_str(), "root");
    assert_eq!(
        theme.evidence().browser_theme_colors.light.as_str(),
        "#ffffff"
    );
    assert_eq!(
        theme.evidence().browser_theme_colors.dark.as_str(),
        "#000000"
    );
    assert_eq!(
        theme
            .render(&theme.render_recipes().page, &serde_json::json!({}))
            .expect("qualified root render"),
        b"rendered"
    );
}

#[test]
fn tera_components_receive_only_the_compiler_owned_semantic_host_context() {
    use gazette_core::{
        markdown::{
            ComponentBinding, ComponentContext, ComponentHost, ComponentSurface, Directive,
            Fragment,
        },
        publication::content::PageFamily,
        source::{Location, Precision, Span},
    };
    use gazette_ports::markdown::ComponentRenderer;

    let acquired = acquire(PLUGIN);
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("complete package admission");
    let theme = metadata
        .compile_templates(&gazette_adapter_tera::Compiler, Default::default())
        .expect("real Tera compilation");
    let location = Location {
        span: Span { start: 0, end: 9 },
        precision: Precision::Construct,
    };
    let binding = ComponentBinding {
        container_contents: None,
        reading_prefix: None,
        presentation_only: false,
        occurrence: Directive {
            location,
            header: location,
            info: "notice".into(),
            emitted: true,
        },
        name: "notice".parse().unwrap(),
        arguments: BTreeMap::new(),
        context: Some(ComponentContext {
            address: "content:one#component-1".into(),
            occurrence: 1,
            host: ComponentHost {
                content: "content:one".parse().unwrap(),
                site: "site:one".parse().unwrap(),
                project_resources: None,
                surface: ComponentSurface::Page {
                    family: PageFamily::Article,
                },
            },
        }),
    };
    let output = ComponentRenderer::render(
        &theme,
        &binding,
        &Fragment {
            link_destinations: Vec::new(),
            images: Vec::new(),
            html: "<p>Body</p>".into(),
            text: "Body".into(),
        },
        false,
    )
    .expect("component render with semantic context");
    assert_eq!(
        output,
        "content:one#component-1|content:one|site:one|page|article"
    );
    for private in ["/fixture/", "source:root", "span"] {
        assert!(
            !output.contains(private),
            "private coordinate leaked: {private}"
        );
    }
}

#[test]
fn selected_resource_must_be_exported_by_its_package_independently_of_capability_provider() {
    let acquired = acquire(PLUGIN_WITH_UNEXPORTED_RESOURCE);
    let error = acquired
        .check_metadata(toml::Limits::default())
        .expect_err("resource relationship must close");
    assert_eq!(error.code(), "package_selection");
    assert!(error.to_string().contains("not exported by its package"));
}

#[test]
fn actual_mounted_theme_resource_refuses_exact_retained_only_origin_before_compilation() {
    let directory = tempfile::tempdir().expect("one owned native admission tree");
    let theme = directory.path().join("theme");
    let content = directory.path().join("content");
    std::fs::create_dir_all(theme.join("templates")).unwrap();
    std::fs::create_dir(&content).unwrap();
    std::fs::write(theme.join("templates/page.html"), "{{ body_html | safe }}").unwrap();
    std::fs::write(
        theme.join("theme.toml"),
        "package='theme:mounted'\nnamespace='mounted'\ntemplates='templates'\n\
         browser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n\
         [[resources]]\nid='module:shared'\ninputs=['shared/dep.ts']\n\
         build={kind='browser-module',entry='shared/dep.ts'}\n",
    )
    .unwrap();
    std::fs::write(content.join("dep.ts"), b"export const answer=42;").unwrap();
    std::fs::write(content.join("page.md"), b"+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Page'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# Page\n").unwrap();
    for retained in [true, false] {
        let source = format!(
            "[design_inputs]\ntokens='source:content'\n\
             [composition]\ntheme={{package='theme:mounted'}}\n\
             [composition.resources]\n'module:shared'={{state='enabled',resource={{owner={{kind='theme',package='theme:mounted'}},resource='module:shared'}},output='/assets/shared.js',use='site'}}\n\
             [[principals]]\nid='owner:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
             [[brands]]\nid='brand:one'\nname='Example'\n\
             [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/'\n\
             authored=[{{source='source:content',target='authored',retained_only={}}}]\n\
             [[sources]]\nid='source:theme'\nlocation={{kind='filesystem',root={:?}}}\n\
             [[sources]]\nid='source:content'\nlocation={{kind='filesystem',root={:?}}}\n\
             [[theme_packages]]\npackage='theme:mounted'\nsource='source:theme'\nmanifest='theme.toml'\n\
             resource_inputs={{'shared/dep.ts'={{input='tokens',entry='dep.ts'}}}}\n",
            if retained { "['dep.ts']" } else { "[]" },
            theme.to_str().unwrap(),
            content.to_str().unwrap(),
        );
        // Filesystem inputs use the actual native adapter. The existing revision
        // fixture is never consulted; no Git revision or external provider exists.
        let acquired = build::acquire_publication(
            &gazette_adapter_fs::FileSystem,
            &Reader::new(PLUGIN),
            &source,
            &"site:one".parse().unwrap(),
            Default::default(),
            PreparationLimits {
                sources: 2,
                entries: 16,
                bytes: 16 << 10,
                per_source: RevisionLimits::default(),
            },
        )
        .expect("same complete native captures with either explicit disposition");
        let content_key = "source:content".parse().unwrap();
        let entry = "dep.ts".parse().unwrap();
        let capture = &acquired.sources()[&content_key];
        assert_eq!(
            capture.file(&entry),
            Some(b"export const answer=42;".as_slice())
        );
        assert!(
            capture
                .inventory()
                .entries()
                .iter()
                .any(|record| record.path == entry)
        );
        if retained {
            let build::MetadataError::Input {
                source,
                path,
                reason,
            } = acquired.check_metadata(Default::default()).unwrap_err()
            else {
                panic!("the exact mounted origin must fail before any compiler/provider is invoked")
            };
            assert_eq!(source, content_key);
            assert_eq!(path, entry);
            assert_eq!(reason, "Theme resource input is explicitly retained-only");
        } else {
            let metadata = acquired
                .check_metadata(Default::default())
                .expect("same bytes are usable without retention-only policy");
            let inspection = metadata.inspect();
            let package: gazette_compiler::theme_package::ThemePackageId =
                "theme:mounted".parse().unwrap();
            let target: EntryPath = "shared/dep.ts".parse().unwrap();
            let origin = &inspection.theme_packages[&package].input_origins[&target];
            assert_eq!(origin.source, content_key);
            assert_eq!(origin.entry, entry);
            assert_eq!(origin.sha256, Sha256Digest::of(b"export const answer=42;"));
        }
    }
}

struct ExplicitSelectionReader;

impl ExplicitSelectionReader {
    fn captured(root: &Path, limits: Limits) -> Result<CapturedSource, Error> {
        match root.to_str() {
            Some("/fixture/foundation" | "/fixture/root") => {
                Reader::new(PLUGIN).captured(root, limits)
            }
            Some("/fixture/plugins") => captured(
                vec![
                    ("component.toml", COMPONENT_PLUGIN),
                    ("extension.toml", EXTENSION_PLUGIN),
                    ("unselected.toml", b"not valid TOML"),
                    ("card.arguments.json", b"{}"),
                    ("card.model.json", b"{}"),
                    ("record.schema.json", b"{}"),
                ],
                limits,
            ),
            _ => panic!("unexpected fixture root"),
        }
    }
}

impl SourceReader for ExplicitSelectionReader {
    fn read(&self, _: &Path, _: Option<&Subdirectory>, _: Limits) -> Result<Inventory, Error> {
        panic!("package admission must use retained captures")
    }
}

impl SourceCapture for ExplicitSelectionReader {
    fn capture(
        &self,
        root: &Path,
        subdirectory: Option<&Subdirectory>,
        limits: Limits,
    ) -> Result<CapturedSource, Error> {
        assert_eq!(subdirectory, Some(&"package".parse().unwrap()));
        Self::captured(root, limits)
    }
}

impl RevisionReader for ExplicitSelectionReader {
    fn read_revision(
        &self,
        root: &Path,
        subdirectory: &Subdirectory,
        commit: &CommitId,
        limits: Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, Error> {
        assert_eq!(subdirectory, &"package".parse().unwrap());
        assert_eq!(commit.as_str(), "1111111111111111111111111111111111111111");
        Ok(Self::captured(root, limits)?.into_parts().0)
    }

    fn read_gitlink(
        &self,
        _: &Path,
        _: &CommitId,
        _: &Subdirectory,
        _: &mut MetadataBudget,
    ) -> Result<CommitId, Error> {
        panic!("direct repository fixtures do not discover gitlinks")
    }
}

#[test]
fn explicit_component_and_extension_packages_are_admitted_without_export_inference() {
    let reader = ExplicitSelectionReader;
    let acquired = build::acquire_composition(
        &reader,
        &reader,
        EXPLICIT_PLUGIN_SELECTION_CONFIG,
        &"site:one".parse().unwrap(),
        toml::Limits::default(),
        PreparationLimits {
            sources: 3,
            entries: 64,
            bytes: 32 << 10,
            per_source: RevisionLimits::default(),
        },
    )
    .expect("explicit package capture");
    let composition = acquired
        .compilation()
        .workspace
        .composition_for(&"site:one".parse().unwrap())
        .expect("site composition");
    assert_eq!(composition.plugins().len(), 2);
    assert!(composition.capabilities().is_empty());
    assert!(composition.resources().is_empty());

    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("component and extension manifests are admitted from retained bytes");
    let inspection = metadata.inspect();
    let component: PluginPackageId = "component-plugin".parse().unwrap();
    let extension: PluginPackageId = "extension-plugin".parse().unwrap();
    assert_eq!(
        inspection
            .plugin_packages
            .keys()
            .map(|package| package.as_str())
            .collect::<Vec<_>>(),
        ["component-plugin", "extension-plugin"]
    );
    assert!(
        inspection
            .plugin_packages
            .get(&component)
            .expect("component package")
            .declaration
            .components
            .contains_key(&"card".parse().unwrap())
    );
    assert!(
        inspection
            .plugin_packages
            .get(&extension)
            .expect("extension package")
            .declaration
            .extensions
            .contains_key(&"extension-plugin.record/v1".parse().unwrap())
    );
    assert!(
        !inspection
            .plugin_packages
            .keys()
            .any(|package| package.as_str() == "unselected-plugin"),
        "a catalog binding sharing the captured source is not admission"
    );
}

struct Compiler;
struct Program(TreeIdentity);

impl TemplateCompiler for Compiler {
    fn profile(&self) -> &'static str {
        "gazette-test-templates/v1"
    }

    fn compile(
        &self,
        source: &TreeInput<'_>,
        _: gazette_core::render::Limits,
    ) -> Result<Box<dyn CompiledTemplates>, gazette_core::render::Error> {
        for path in ["foundation/base.html", "root/page.html", "root/notice.html"] {
            assert!(source.file(&path.parse().unwrap()).is_some(), "{path}");
        }
        Ok(Box::new(Program(source.inventory().identity())))
    }
}

impl CompiledTemplates for Program {
    fn identity(&self) -> TreeIdentity {
        self.0
    }
    fn profile(&self) -> &'static str {
        "gazette-test-templates/v1"
    }
    fn render(
        &self,
        entry: &EntryPath,
        _: &serde_json::Value,
    ) -> Result<Vec<u8>, gazette_core::render::Error> {
        assert_eq!(entry.as_str(), "root/page.html");
        Ok(b"rendered".to_vec())
    }
}

#[test]
fn actual_compiled_faq_template_refuses_changed_or_missing_authored_wording() {
    use gazette_core::{
        markdown::{ComponentBinding, Directive, Fragment},
        source::{Location, Precision, Span},
    };
    use gazette_ports::markdown::ComponentRenderer;
    for (template, accepted) in [
        (b"<details><summary>{{ component.arguments.question }}</summary><div>{{ component.body.html | safe }}</div></details>".as_slice(), true),
        (b"<details><summary>Different question?</summary><div>{{ component.body.html | safe }}</div></details>".as_slice(), false),
        (b"<details><summary>{{ component.arguments.question }}</summary><div>Different answer.</div></details>".as_slice(), false),
        (b"<details><summary>{{ component.arguments.question }}</summary></details>".as_slice(), false),
    ] {
        let reader = Reader { plugin: PLUGIN, faq: Some(template) };
        let acquired = build::acquire_composition(&reader, &reader, CONFIG, &"site:one".parse().unwrap(), toml::Limits::default(), PreparationLimits { sources: 3, entries: 32, bytes: 16 << 10, per_source: RevisionLimits::default() }).unwrap();
        let metadata = acquired.check_metadata(toml::Limits::default()).unwrap();
        let theme = metadata.compile_templates(&gazette_adapter_tera::Compiler, Default::default()).unwrap();
        let location = Location { span: Span {start:0,end:9}, precision:Precision::Construct };
        let binding = ComponentBinding { container_contents: None, reading_prefix: None, presentation_only:false, occurrence:Directive {location,header:location,info:"notice".into(),emitted:true},name:"notice".parse().unwrap(),arguments:BTreeMap::from([("question".into(),serde_json::json!("What & why?"))]),context:None };
        let body = Fragment {link_destinations: Vec::new(), images: Vec::new(), html:"<p>The <strong>authored</strong> answer.</p>".into(),text:"The authored answer.".into()};
        let result = ComponentRenderer::render(&theme, &binding, &body, false);
        assert_eq!(result.is_ok(), accepted, "{result:?}");
        if accepted { assert!(result.unwrap().contains("What &amp; why?")); }
    }
}
