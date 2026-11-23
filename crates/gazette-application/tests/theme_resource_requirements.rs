//! Theme logical-resource requirements joined to independent workspace selections.

use std::{cell::Cell, collections::BTreeMap, path::Path};

use gazette_application::{build, source::PreparationLimits};
use gazette_compiler::toml;
use gazette_core::{
    resource,
    source::{
        self, CapturedSource, CommitId, Content, EntryPath, Inventory, InventoryBuilder,
        MetadataBudget, Sha256Digest, Subdirectory, TreeInput,
    },
};
use gazette_ports::{
    resource::ResourceCompiler,
    source::{RevisionReader, SourceCapture, SourceReader},
};
use serde_json::Value;

const THEME_MANIFEST: &[u8] = b"package='theme:base'\nnamespace='base'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\nresource_requirements=['module:theme-required']\n";
const THEME_TEMPLATE: &[u8] = br#"{% for resource in presentation.theme_resources %}{{ resource.id }}|{{ resource.kind }}|{{ resource.url }}|{{ resource.output }}|{{ resource.bytes }};{% endfor %}"#;
const PLUGIN_MANIFEST: &[u8] = b"package='resource-plugin'\n[resources.'module:provided']\ninputs=['required.ts']\nbuild={kind='browser-module',entry='required.ts'}\n[resources.'stylesheet:extra']\ninputs=['extra.css']\nbuild={kind='stylesheet',entry='extra.css'}\n";
const PAGE: &[u8] = b"+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='Retained'\nlifecycle='draft'\nroute={path='/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained body\n";
const REQUIRED_SOURCE: &[u8] = b"export const required = true;";
const REQUIRED_OUTPUT: &[u8] = b"export const required=true;";
const EXTRA_SOURCE: &[u8] = b"body { color: black; }";
const EXTRA_OUTPUT: &[u8] = b"body{color:#000}";
const DEPENDENCY_ROOT_MANIFEST: &[u8] = b"package='theme:root'\nnamespace='root'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n[[dependencies]]\npackage='theme:foundation'\n";
const DEPENDENCY_THEME_MANIFEST: &[u8] = b"package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\nresource_requirements=['stylesheet:foundation','module:foundation']\n[[resources]]\nid='stylesheet:base'\ninputs=['foundation.css']\nbuild={kind='stylesheet',entry='foundation.css'}\n[[resources]]\nid='module:base'\ninputs=['foundation.ts']\nbuild={kind='browser-module',entry='foundation.ts'}\n";
const DEPENDENCY_TEMPLATE: &[u8] = br#"{% for resource in presentation.theme_resources %}{{ resource.id }}|{{ resource.kind }};{% endfor %}{% for module in presentation.browser_modules.preloads %}preload={{ module.url }};{% endfor %}"#;
const FOUNDATION_STYLESHEET_SOURCE: &[u8] = b":root { --accent: navy; }";
const FOUNDATION_STYLESHEET_OUTPUT: &[u8] = b":root{--accent:navy}";
const FOUNDATION_MODULE_SOURCE: &[u8] = b"export const foundation = true;";
const FOUNDATION_MODULE_OUTPUT: &[u8] = b"export const foundation=true;";

fn path(value: &str) -> EntryPath {
    value.parse().expect("literal entry path")
}

fn configuration(activate_plugin: bool, required: RequiredSelection) -> String {
    let activation = if activate_plugin {
        "[composition.plugins]\n'resource-plugin'={state='enabled'}\n"
    } else {
        ""
    };
    let required = match required {
        RequiredSelection::Enabled => {
            "'module:theme-required'={state='enabled',resource={owner={kind='plugin',package='resource-plugin'},resource='module:provided'},output='/assets/required.js',use='dependency'}\n"
        }
        RequiredSelection::Disabled => "'module:theme-required'={state='disabled'}\n",
        RequiredSelection::Missing => "",
    };
    let resources = if required.is_empty() && !activate_plugin {
        String::new()
    } else {
        format!(
            "[composition.resources]\n{required}'stylesheet:extra-site'={{state='enabled',resource={{owner={{kind='plugin',package='resource-plugin'}},resource='stylesheet:extra'}},output='/assets/extra.css',use='site'}}\n"
        )
    };
    format!(
        "[composition]\n\
         theme={{package='theme:base'}}\n\
         {activation}\
         {resources}\
         [[principals]]\n\
         id='owner:one'\n\
         kind='person'\n\
         [[sites]]\n\
         id='site:one'\n\
         publisher='owner:one'\n\
         deployable=false\n\
         [[brands]]\n\
         id='brand:one'\n\
         name='Example'\n\
         [[brands.publications]]\n\
         site='site:one'\n\
         public_base_url='https://example.test/'\n\
         authored=[{{source='source:content',target='authored'}}]\n\
         [[sources]]\n\
         id='source:theme'\n\
         location={{kind='repository',root='/fixture/theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\n\
         id='source:plugin'\n\
         location={{kind='repository',root='/fixture/plugin',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\n\
         id='source:content'\n\
         location={{kind='repository',root='/fixture/content',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[theme_packages]]\n\
         package='theme:base'\n\
         source='source:theme'\n\
         manifest='theme.toml'\n\
         [[plugin_packages]]\n\
         package='resource-plugin'\n\
         source='source:plugin'\n\
         manifest='plugin.toml'\n"
    )
}

#[derive(Clone, Copy)]
enum RequiredSelection {
    Enabled,
    Disabled,
    Missing,
}

#[derive(Clone, Copy)]
enum DependencySelection {
    Complete,
    Undeclared,
    Collision,
}

fn dependency_configuration(selection: DependencySelection) -> String {
    let stylesheet_resource = if matches!(selection, DependencySelection::Undeclared) {
        "stylesheet:missing"
    } else {
        "stylesheet:base"
    };
    let stylesheet_output = if matches!(selection, DependencySelection::Collision) {
        "/assets/foundation.js"
    } else {
        "/assets/foundation.css"
    };
    format!(
        "[composition]\n\
         theme={{package='theme:root'}}\n\
         [composition.resources]\n\
         'stylesheet:foundation'={{state='enabled',resource={{owner={{kind='theme',package='theme:foundation'}},resource='{stylesheet_resource}'}},output='{stylesheet_output}',use='site'}}\n\
         'module:foundation'={{state='enabled',resource={{owner={{kind='theme',package='theme:foundation'}},resource='module:base'}},output='/assets/foundation.js',use='site'}}\n\
         [[principals]]\nid='owner:one'\nkind='person'\n\
         [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
         [[brands]]\nid='brand:one'\nname='Example'\n\
         [[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{{source='source:content',target='authored'}}]\n\
         [[sources]]\nid='source:root-theme'\nlocation={{kind='repository',root='/fixture/dependency-root',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\nid='source:foundation-theme'\nlocation={{kind='repository',root='/fixture/dependency-theme',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[sources]]\nid='source:content'\nlocation={{kind='repository',root='/fixture/content',commit='1111111111111111111111111111111111111111',subdirectory='input'}}\n\
         [[theme_packages]]\npackage='theme:root'\nsource='source:root-theme'\nmanifest='theme.toml'\n\
         [[theme_packages]]\npackage='theme:foundation'\nsource='source:foundation-theme'\nmanifest='theme.toml'\n"
    )
}

fn captured(root: &Path, limits: source::Limits) -> Result<CapturedSource, source::Error> {
    let entries: &[(&str, &[u8])] = if root == Path::new("/fixture/theme") {
        &[
            ("theme.toml", THEME_MANIFEST),
            ("templates/page.html", THEME_TEMPLATE),
        ]
    } else if root == Path::new("/fixture/plugin") {
        &[
            ("plugin.toml", PLUGIN_MANIFEST),
            ("required.ts", REQUIRED_SOURCE),
            ("extra.css", EXTRA_SOURCE),
        ]
    } else if root == Path::new("/fixture/dependency-root") {
        &[
            ("theme.toml", DEPENDENCY_ROOT_MANIFEST),
            ("templates/page.html", DEPENDENCY_TEMPLATE),
        ]
    } else if root == Path::new("/fixture/dependency-theme") {
        &[
            ("theme.toml", DEPENDENCY_THEME_MANIFEST),
            ("templates/base.html", b"{% block body %}{% endblock %}"),
            ("foundation.css", FOUNDATION_STYLESHEET_SOURCE),
            ("foundation.ts", FOUNDATION_MODULE_SOURCE),
        ]
    } else {
        assert_eq!(root, Path::new("/fixture/content"));
        &[("authored.md", PAGE)]
    };
    let files = entries
        .iter()
        .map(|(name, bytes)| (path(name), bytes.to_vec()))
        .collect::<BTreeMap<_, _>>();
    let mut builder = InventoryBuilder::new(limits);
    if matches!(
        root,
        path if path == Path::new("/fixture/theme")
            || path == Path::new("/fixture/dependency-root")
            || path == Path::new("/fixture/dependency-theme")
    ) {
        builder.insert(path("templates"), Content::Directory)?;
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

#[derive(Default)]
struct Reader;

impl SourceReader for Reader {
    fn read(
        &self,
        _: &Path,
        _: Option<&Subdirectory>,
        _: source::Limits,
    ) -> Result<Inventory, source::Error> {
        panic!("fixture requires retained source capture")
    }
}

impl SourceCapture for Reader {
    fn capture(
        &self,
        root: &Path,
        _: Option<&Subdirectory>,
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        captured(root, limits)
    }
}

impl RevisionReader for Reader {
    fn read_revision(
        &self,
        root: &Path,
        _: &Subdirectory,
        commit: &CommitId,
        limits: source::Limits,
        _: &mut MetadataBudget,
    ) -> Result<Inventory, source::Error> {
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
        panic!("fixture uses exact repository revisions")
    }
}

fn acquired(configuration: &str) -> Result<build::AcquiredInputs, build::CompositionError> {
    let reader = Reader;
    build::acquire_publication(
        &reader,
        &reader,
        configuration,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        PreparationLimits {
            sources: 3,
            entries: 10,
            bytes: 4096,
            per_source: source::RevisionLimits::default(),
        },
    )
}

fn acquired_dependency(
    selection: DependencySelection,
) -> Result<build::AcquiredInputs, build::CompositionError> {
    let reader = Reader;
    build::acquire_publication(
        &reader,
        &reader,
        &dependency_configuration(selection),
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        PreparationLimits {
            sources: 3,
            entries: 12,
            bytes: 4096,
            per_source: source::RevisionLimits::default(),
        },
    )
}

struct Resources {
    calls: Cell<usize>,
}

impl ResourceCompiler for Resources {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &resource::Recipe,
        _: &resource::ModuleLinks,
        _: resource::Limits,
    ) -> Result<resource::Artifact, resource::Error> {
        self.calls.set(self.calls.get() + 1);
        let (entry, output) = match recipe.entry().as_str() {
            "required.ts" => (path("required.ts"), REQUIRED_OUTPUT),
            "extra.css" => (path("extra.css"), EXTRA_OUTPUT),
            "foundation.css" => (path("foundation.css"), FOUNDATION_STYLESHEET_OUTPUT),
            "foundation.ts" => (path("foundation.ts"), FOUNDATION_MODULE_OUTPUT),
            other => panic!("unexpected retained Theme resource entry {other}"),
        };
        assert_eq!(source.inventory().entries().len(), 1);
        Ok(resource::Artifact {
            input: source.inventory().identity(),
            bytes: output.to_vec(),
            dependencies: [entry].into(),
            references: Default::default(),
            deferred_references: Default::default(),
            svg: None,
            image: None,
            tool: "private-compiler-tool",
        })
    }
}

fn render<'a>(
    inputs: &'a build::AcquiredInputs,
    resources: &Resources,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    inputs.render_publication(
        &gazette_adapter_tera::Compiler,
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
fn plugin_resource_satisfies_ordered_theme_requirement_without_exposing_binding_details() {
    let inputs = acquired(&configuration(true, RequiredSelection::Enabled))
        .expect("explicit plugin activation and independent resource selections");
    let resources = Resources {
        calls: Cell::new(0),
    };
    let rendered = render(&inputs, &resources).expect("logical Theme requirement compiles");
    assert_eq!(resources.calls.get(), 2);
    let tree = rendered.site().output();
    assert_eq!(
        tree.file(&path("assets/required.js")),
        Some(REQUIRED_OUTPUT)
    );
    assert_eq!(tree.file(&path("assets/extra.css")), Some(EXTRA_OUTPUT));
    let model_bytes = tree.file(&path("model.json")).expect("site model");
    let model: Value = serde_json::from_slice(model_bytes).expect("site model JSON");
    assert_eq!(
        model["resources"]
            .as_object()
            .expect("site resources")
            .len(),
        2
    );
    let theme = &model["pages"]["content:one"]["presentation"]["theme_resources"];
    assert_eq!(theme.as_array().expect("ordered Theme resources").len(), 2);
    assert_eq!(theme[1]["id"], "stylesheet:extra-site");
    assert_eq!(theme[1]["url"], "https://example.test/assets/extra.css");
    assert_eq!(theme[0]["id"], "module:theme-required");
    assert_eq!(theme[0]["url"], "https://example.test/assets/required.js");
    assert!(
        model["pages"]["content:one"]["presentation"]
            .get("site_resources")
            .is_none()
    );
    assert_eq!(model["capabilities"], serde_json::json!({}));
    let public = String::from_utf8_lossy(model_bytes);
    for private in [
        "resource-plugin",
        "module:provided",
        "private-compiler-tool",
        "source:plugin",
        "selection",
        "recipe",
    ] {
        assert!(!public.contains(private), "public model leaked {private}");
    }
    let html = std::str::from_utf8(tree.file(&path("index.html")).expect("Tera output"))
        .expect("UTF-8 HTML");
    assert!(html.contains("module:theme-required"));
    assert!(html.contains("stylesheet:extra-site"));
    assert!(!html.contains("resource-plugin"));
}

#[test]
fn missing_or_disabled_theme_requirement_fails_before_resource_compilation() {
    for selection in [RequiredSelection::Missing, RequiredSelection::Disabled] {
        let inputs = acquired(&configuration(true, selection)).expect("resolved composition");
        let resources = Resources {
            calls: Cell::new(0),
        };
        let error = render(&inputs, &resources).expect_err("Theme requirement must fail closed");
        assert_eq!(error.code(), "package_selection");
        assert_eq!(resources.calls.get(), 0);
    }
}

#[test]
fn theme_requirement_does_not_activate_a_cataloged_plugin_or_select_a_resource() {
    let inputs = acquired(&configuration(false, RequiredSelection::Missing))
        .expect("catalog membership is not activation");
    let resources = Resources {
        calls: Cell::new(0),
    };
    let error = render(&inputs, &resources).expect_err("requirement is not a selection");
    assert_eq!(error.code(), "package_selection");
    assert_eq!(resources.calls.get(), 0);
}

#[test]
fn dependency_theme_stylesheet_and_module_compile_replay_and_reach_theme_context() {
    let inputs = acquired_dependency(DependencySelection::Complete)
        .expect("root and dependency Theme captures");
    let resources = Resources {
        calls: Cell::new(0),
    };
    let first = render(&inputs, &resources).expect("dependency Theme resources compile");
    assert_eq!(resources.calls.get(), 2);
    let tree = first.site().output();
    assert_eq!(
        tree.file(&path("assets/foundation.css")),
        Some(FOUNDATION_STYLESHEET_OUTPUT)
    );
    assert_eq!(
        tree.file(&path("assets/foundation.js")),
        Some(FOUNDATION_MODULE_OUTPUT)
    );
    let identity = tree.inventory().identity();
    let model: Value = serde_json::from_slice(tree.file(&path("model.json")).expect("site model"))
        .expect("site model JSON");
    let presentation = &model["pages"]["content:one"]["presentation"];
    assert_eq!(
        presentation["theme_resources"],
        serde_json::json!([
            {
                "id": "stylesheet:foundation",
                "kind": "stylesheet",
                "url": "https://example.test/assets/foundation.css",
                "output": "assets/foundation.css",
                "sha256": Sha256Digest::of(FOUNDATION_STYLESHEET_OUTPUT),
                "bytes": FOUNDATION_STYLESHEET_OUTPUT.len(),
            },
            {
                "id": "module:foundation",
                "kind": "browser-module",
                "url": "https://example.test/assets/foundation.js",
                "output": "assets/foundation.js",
                "sha256": Sha256Digest::of(FOUNDATION_MODULE_OUTPUT),
                "bytes": FOUNDATION_MODULE_OUTPUT.len(),
            }
        ])
    );
    assert_eq!(
        presentation["browser_modules"]["preloads"][0]["url"],
        "https://example.test/assets/foundation.js"
    );
    let html = std::str::from_utf8(tree.file(&path("index.html")).expect("Theme output"))
        .expect("UTF-8 Theme output");
    assert_eq!(
        html,
        "stylesheet:foundation|stylesheet;module:foundation|browser-module;preload=https:&#x2F;&#x2F;example.test&#x2F;assets&#x2F;foundation.js;"
    );
    assert!(
        first
            .site()
            .evidence()
            .resources
            .values()
            .all(|evidence| matches!(
                &evidence.resource.owner,
                gazette_compiler::workspace::ResourceOwner::Theme(package)
                    if package.as_str() == "theme:foundation"
            ))
    );
    drop(first);

    let replay = render(&inputs, &resources).expect("retained deterministic replay");
    assert_eq!(replay.site().output().inventory().identity(), identity);
    assert_eq!(resources.calls.get(), 4);
}

#[test]
fn dependency_theme_resource_selection_rejects_undeclared_and_colliding_bindings() {
    for (selection, code) in [
        (DependencySelection::Undeclared, "package_selection"),
        (DependencySelection::Collision, "resource_input"),
    ] {
        let inputs = acquired_dependency(selection).expect("resolved dependency Theme graph");
        let resources = Resources {
            calls: Cell::new(0),
        };
        let error = render(&inputs, &resources)
            .expect_err("invalid dependency Theme resource selection fails closed");
        assert_eq!(error.code(), code);
        assert_eq!(resources.calls.get(), 0);
    }
}

#[path = "theme_resource_requirements/social_cards.rs"]
mod social_cards;
