//! Native capture, complete local rendering and fresh staging, never publication approval.

#[path = "publication_render/attributions.rs"]
mod attributions;
#[path = "publication_render/native_documents.rs"]
mod native_documents;
#[path = "publication_render/static_assets.rs"]
mod static_assets;

#[path = "support/browser.rs"]
mod browser_carrier;

#[path = "support/browser_sdk.rs"]
mod browser_sdk;

#[path = "publication_render/browsing.rs"]
mod browsing;

#[path = "support/mcp.rs"]
mod mcp_carrier;

use gazette_test_support as native;

#[path = "publication_render/ambient_media.rs"]
mod ambient_media;
#[path = "publication_render/article_notes.rs"]
mod article_notes;
#[path = "publication_render/components.rs"]
mod components;
#[path = "publication_render/container_contents.rs"]
mod container_contents;
#[path = "publication_render/discoverability.rs"]
mod discoverability;
#[path = "publication_render/discovery.rs"]
mod discovery;
#[path = "publication_render/discussion.rs"]
mod discussion;
#[path = "publication_render/documentation_mounts.rs"]
mod documentation_mounts;
#[path = "publication_render/documentation_reading.rs"]
mod documentation_reading;
#[path = "publication_render/editorial.rs"]
mod editorial;
#[path = "publication_render/feeds.rs"]
mod feeds;
#[path = "publication_render/invitations.rs"]
mod invitations;
#[path = "publication_render/islands.rs"]
mod islands;
#[path = "publication_render/journal_worktree.rs"]
mod journal_worktree;
#[path = "publication_render/json_ld.rs"]
mod json_ld;
#[path = "publication_render/json_script.rs"]
mod json_script;
#[path = "publication_render/landings.rs"]
mod landings;
#[path = "publication_render/legal_processing.rs"]
mod legal_processing;
#[path = "publication_render/license.rs"]
mod license;
#[path = "publication_render/listing_artwork.rs"]
mod listing_artwork;
#[path = "publication_render/listings.rs"]
mod listings;
#[path = "publication_render/markdown_alternates.rs"]
mod markdown_alternates;
#[path = "publication_render/materialization.rs"]
mod materialization;
#[path = "publication_render/math.rs"]
mod math;
#[path = "publication_render/mirrors.rs"]
mod mirrors;
#[path = "publication_render/navigation.rs"]
mod navigation;
#[path = "publication_render/notebook_components.rs"]
mod notebook_components;
#[path = "publication_render/notebook_images.rs"]
mod notebook_images;
#[path = "publication_render/notebooks.rs"]
mod notebooks;
#[path = "publication_render/output_plan.rs"]
mod output_plan;
#[path = "publication_render/output_set.rs"]
mod output_set;
#[path = "publication_render/policy.rs"]
mod policy;
#[path = "publication_render/preparation.rs"]
mod preparation;
#[path = "publication_render/probe.rs"]
mod probe;
#[path = "publication_render/publication_mcp.rs"]
mod publication_mcp;
#[path = "publication_render/publication_roles.rs"]
mod publication_roles;
#[path = "publication_render/registry.rs"]
mod registry;
#[path = "publication_render/registry_store.rs"]
mod registry_store;
#[path = "publication_render/rendered_site_mcp.rs"]
mod rendered_site_mcp;
#[path = "publication_render/resource_defaults.rs"]
mod resource_defaults;
#[path = "publication_render/resource_mounts.rs"]
mod resource_mounts;
#[path = "publication_render/search.rs"]
mod search;
#[path = "publication_render/selection.rs"]
mod selection;
#[path = "publication_render/site_principals.rs"]
mod site_principals;
#[path = "publication_render/sites.rs"]
mod sites;
#[path = "publication_render/summary.rs"]
mod summary;
#[path = "publication_render/taxonomies.rs"]
mod taxonomies;
#[path = "publication_render/templates.rs"]
mod templates;
#[path = "publication_render/webmcp.rs"]
mod webmcp;
#[path = "publication_render/worktree.rs"]
mod worktree;

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html',graph='graph.html'}\n";
const NOTEBOOK: &[u8] = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"id":"retained","cell_type":"code","metadata":{},"source":"raise RuntimeError('not executed')","execution_count":null,"outputs":[]}]}"#;
const TEMPLATE: &str = "<h1>{{ content.title }}</h1>{{ body_html | safe }}{% if notes is defined and notes.notebooks is defined %}{% for notebook in notes.notebooks %}<section class=\"gazette-notebook\">{% for cell in notebook.cells %}{% if cell.kind == \"markdown\" %}<div class=\"notebook-markdown\" data-cell-id=\"{{ cell.cell_id }}\">{{ cell.html | safe }}</div>{% else %}<div class=\"notebook-code\" data-cell-id=\"{{ cell.cell_id }}\">{{ cell.source_html | safe }}{{ cell.outputs_html | safe }}</div>{% endif %}{% endfor %}</section>{% endfor %}{% endif %}<a href=\"{{ route.canonical_url }}\">canonical</a>{% for item in related %}<i>{{ item.id }}:{{ item.site }}</i>{% endfor %}";
const ALIAS: &str = "<meta http-equiv=\"refresh\" content=\"0; url={{ route.canonical_url }}\"><a href=\"{{ route.canonical_url }}\">Moved</a>";
const HOME: &str = "+++\n[content]\nid='collection:home'\nsite='site:one'\ntitle='Home'\ndescription='The explicit fixture collection and its retained articles.'\nlifecycle='published'\nroute={path='/'}\nstructure={kind='collection'}\n+++\n# Home body\n";
const PAGE: &str = "+++\ncollections=['collection:home']\nrelated=['content:other']\n[content]\nid='content:one'\nsite='site:one'\ntitle='One <escaped>'\ndescription='The retained local article.'\nlifecycle='draft'\nroute={path='/guide/',aliases=['/old.html']}\nstructure={kind='page',family={kind='article'}}\n+++\n**Retained** body\n";
const OTHER: &str = "+++\n[content]\nid='content:other'\nsite='site:other'\ntitle='Other'\ndescription='The independently selected documentation page.'\nlifecycle='archived'\nroute={path='/elsewhere/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Other body\n";
const PROJECT_DOC: &str = "+++\ncollections=['collection:home']\nrelated=['content:other']\nprojects=[{project='project:subject',kind='about'}]\n[content]\nid='doc:native'\nsite='site:one'\ntitle='Native documentation'\nlifecycle='published'\nroute={path='/docs/native/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Retained native documentation\n";
// Parity comparisons supply the same calendar input to every carrier. The CLI
// captures today's year when omitted; the deterministic SDK never reads a clock.
const RENDER_ARGUMENTS: [&str; 8] = [
    "render-site",
    "--local",
    "--site",
    "site:one",
    "--model-output",
    "model.json",
    "--render-year",
    "2040",
];

/// Test declarations are curated per fixture, never inferred from the page graph.
fn curated_llms(description_from: &str, sections: &[(&str, &[&str])]) -> String {
    let sections = sections
        .iter()
        .map(|(title, contents)| format!("{{title={title:?},contents={contents:?}}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "llms={{enabled=true,description_from={description_from:?},representation='markdown-alternate',include_human_link=true,sections=[{sections}]}},markdown_alternates={{enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}"
    )
}

struct Fixture {
    directory: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(overrides: &[(&str, &[u8])]) -> Self {
        let directory = tempdir().expect("disposable native input");
        let repository = Repository::init(directory.path()).expect("fixture Git");
        for (path, bytes) in [
            ("theme/theme.toml", MANIFEST.as_bytes()),
            ("theme/templates/page.html", TEMPLATE.as_bytes()),
            ("theme/templates/alias.html", ALIAS.as_bytes()),
            ("theme/templates/graph.html", b"{{ __tera_context }}"),
            ("one/home.md", HOME.as_bytes()),
            ("one/page.md", PAGE.as_bytes()),
            ("other/page.md", OTHER.as_bytes()),
        ]
        .into_iter()
        .chain(overrides.iter().copied())
        {
            let path = directory.path().join(path);
            fs::create_dir_all(path.parent().expect("fixture parent")).expect("directories");
            fs::write(path, bytes).expect("fixture source bytes");
        }
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(
                ["theme", "one", "other", "project", "math"],
                IndexAddOption::DEFAULT,
                None,
            )
            .expect("fixture tracked files");
        let tree = repository
            .find_tree(index.write_tree().expect("tree OID"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("local fixture commit");
        Self { directory, commit }
    }

    fn config(&self) -> String {
        let mut text = String::from(
            "[composition]\ntheme={package='theme:one'}\n\
            [[principals]]\nid='owner:one'\nkind='person'\n\
            [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
            [[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n\
            [[brands]]\nid='brand:one'\nname='Explicit bindings'\n\
            [[brands.publications]]\nsite='site:one'\npublic_base_url='https://one.example/mount/'\nauthored=[{source='source:one',target='one'}]\n\
            [[brands.publications]]\nsite='site:other'\npublic_base_url='https://other.example/'\nauthored=[{source='source:other',target='other'}]\n",
        );
        for (key, subdirectory) in [("theme", "theme"), ("one", "one"), ("other", "other")] {
            text.push_str(&format!("[[sources]]\nid='source:{key}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{subdirectory}'}}\n", self.directory.path().to_str().expect("path"), self.commit));
        }
        text.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n");
        text
    }

    fn project_config(&self, selected: bool) -> String {
        format!(
            "{}[[sources]]\nid='source:native'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='project'}}\n\
            [[projects]]\nid='project:native'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
            [[projects]]\nid='project:subject'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
            [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:native'\nrender={selected}\ndiscoverable=true\ndocuments={{source='source:native',target='native-docs'}}\n",
            self.config(),
            self.directory.path().to_str().expect("native root"),
            self.commit
        )
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    acquire_result(config).expect("complete native capture")
}

fn acquire_result(config: &str) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_publication(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 8,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
}

fn render(
    inputs: &build::AcquiredInputs,
) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: source::Limits::default(),
        },
        build::PublicationRenderLimits::default(),
    )
}

fn bytes<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("complete expected output")
}

#[test]
fn native_capture_renders_complete_site_and_machine_parity_then_stages_exact_bytes() {
    let fixture = Fixture::new(&[("theme/templates/context.json", b"{{ __tera_context }}")]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("complete local rendering");
    assert!(std::ptr::eq(rendered.inputs(), &inputs));
    let site = rendered.site();
    let tree = site.output();
    assert_eq!(
        tree.inventory()
            .entries()
            .iter()
            .filter(|entry| matches!(entry.content, source::Content::File { .. }))
            .count(),
        4
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("machine artifact");
    assert_eq!(model["format"], "gazette-local-site-model/1");
    assert_eq!(model["site"], "site:one");
    assert_eq!(model["pages"].as_object().expect("page map").len(), 2);
    let page = &model["pages"]["content:one"];
    assert_eq!(page["content"]["lifecycle"], "draft");
    assert_eq!(page["content"]["title"], "One <escaped>");
    assert_eq!(page["related"][0]["id"], "content:other");
    assert_eq!(page["related"][0]["site"], "site:other");
    assert_eq!(page["related"][0]["lifecycle"], "archived");
    assert!(page.get("source").is_none());
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("HTML");
    assert!(html.starts_with("<h1>One &lt;escaped&gt;</h1><p><strong>Retained</strong> body</p>"));
    assert!(html.contains("<i>content:other:site:other</i>"));
    let attribute = html
        .split_once("<a href=\"")
        .expect("canonical link")
        .1
        .split_once('"')
        .expect("closed attribute")
        .0;
    assert_eq!(
        attribute,
        "https:&#x2F;&#x2F;one.example&#x2F;mount&#x2F;guide&#x2F;"
    );
    assert_eq!(
        attribute.replace("&#x2F;", "/"),
        page["route"]["canonical_url"]
            .as_str()
            .expect("same canonical")
    );
    assert_eq!(
        std::str::from_utf8(bytes(tree, "old.html")).expect("alias HTML"),
        "<meta http-equiv=\"refresh\" content=\"0; url=https:&#x2F;&#x2F;one.example&#x2F;mount&#x2F;guide&#x2F;\"><a href=\"https:&#x2F;&#x2F;one.example&#x2F;mount&#x2F;guide&#x2F;\">Moved</a>"
    );
    assert!(
        tree.file(&"elsewhere/index.html".parse().expect("other site route"))
            .is_none()
    );
    let evidence = &site.evidence().pages[&"content:one".parse().expect("content identity")];
    assert_eq!(
        evidence.html_sha256,
        source::Sha256Digest::of(bytes(tree, "guide/index.html"))
    );
    let context_manifest = MANIFEST.replace("page='page.html'", "page='context.json'");
    let observed_fixture = Fixture::new(&[
        ("theme/theme.toml", context_manifest.as_bytes()),
        ("theme/templates/context.json", b"{{ __tera_context }}"),
    ]);
    let observed_inputs = acquire(&observed_fixture.config());
    let observed = build::render_publication(
        &observed_inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect("actual Tera receives the direct page context");
    let context: Value =
        serde_json::from_slice(bytes(observed.site().output(), "guide/index.html"))
            .expect("Tera's complete actual context, not a test-side Model reconstruction");
    let context_sha256 = source::Sha256Digest::of(
        &serde_json::to_vec(&context).expect("compact actual direct context"),
    );
    assert_eq!(evidence.model_sha256, context_sha256);
    let observed_evidence =
        &observed.site().evidence().pages[&"content:one".parse().expect("content identity")];
    assert_eq!(observed_evidence.model_sha256, context_sha256);
    assert_ne!(observed_evidence.html_sha256, evidence.html_sha256);
    assert_ne!(
        bytes(observed.site().output(), "guide/index.html"),
        bytes(tree, "guide/index.html"),
        "the selected probe template changes output, not the supplied page model"
    );
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("same retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same retained semantic owner");
    let authored = resolved.authored();
    let discovery = authored.discovery().expect("source-bound discovery role");
    let mut projected = authored
        .project_discovery_page(
            &"content:one".parse().expect("content"),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
            &discovery,
        )
        .expect("native compiler projection");
    projected
        .data
        .site
        .metadata
        .as_mut()
        .expect("configured publication metadata")
        .render_year = Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"));
    let mut aggregate = page.as_object().expect("emitted listing").clone();
    assert_eq!(
        aggregate
            .remove("presentation")
            .expect("shared presentation envelope"),
        context["presentation"]
    );
    assert_eq!(
        Value::Object(aggregate),
        serde_json::to_value(
            projected
                .listing(&discovery)
                .expect("eligible borrowed listing")
        )
        .expect("exact compiler listing serialization")
    );

    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed after capture",
    )
    .expect("disposable mutation");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "changed template",
    )
    .expect("disposable mutation");
    let repeated = render(&inputs).expect("retained rebuild without native reread");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    let parent = tempdir().expect("disjoint output parent");
    let stage = build::stage_publication(
        &rendered,
        parent.path(),
        source::Limits::default(),
        16 << 10,
    )
    .expect("fresh native stage");
    assert_eq!(stage.inventory().identity(), tree.inventory().identity());
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(stage.path().join(entry.path.as_str())).expect("native read-back"),
                tree.file(&entry.path).expect("expected file")
            );
        }
    }
    stage.close().expect("scoped fixture cleanup");
    assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 0);
    assert!(
        matches!(
            build::stage_publication(
                &rendered,
                fixture.directory.path(),
                source::Limits::default(),
                16 << 10
            ),
            Err(source::StageError::Preflight(_))
        ),
        "consumed source cannot become output parent"
    );
}

#[test]
fn configured_home_routes_and_alias_requirements_fail_before_any_successful_tree() {
    // Keep the graph valid so this case reaches the independent source-home gate.
    let wrong_home = PAGE
        .replace("collections=['collection:home']\n", "")
        .replace("site='site:one'", "site='site:other'");
    let fixture = Fixture::new(&[("one/page.md", wrong_home.as_bytes())]);
    assert!(matches!(
        render(&acquire(&fixture.config())),
        Err(build::PublicationRenderError::Source(error)) if error.code() == "publication_home"
    ));
    let no_route = PAGE.replace("route={path='/guide/',aliases=['/old.html']}\n", "");
    let fixture = Fixture::new(&[("one/page.md", no_route.as_bytes())]);
    assert!(matches!(
        render(&acquire(&fixture.config())),
        Err(build::PublicationRenderError::Site(error)) if error.code() == "site_route_required"
    ));
    let no_alias = MANIFEST.replace(",alias='alias.html'", "");
    let fixture = Fixture::new(&[("theme/theme.toml", no_alias.as_bytes())]);
    let inputs = acquire(&fixture.config());
    let error = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect_err("alias output cannot silently disappear");
    assert!(matches!(
        error,
        build::PublicationRenderError::Site(build::SiteRenderError::AliasTemplateRequired(_))
    ));
}

#[test]
fn model_collisions_and_output_budgets_reject_the_complete_site() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire(&fixture.config());
    for (model, limits) in [
        ("index.html", source::Limits::default()),
        ("guide", source::Limits::default()),
        (
            "model.json",
            source::Limits {
                entries: 1,
                ..source::Limits::default()
            },
        ),
        (
            "model.json",
            source::Limits {
                file_bytes: 1,
                ..source::Limits::default()
            },
        ),
        (
            "model.json",
            source::Limits {
                total_bytes: 1,
                ..source::Limits::default()
            },
        ),
    ] {
        let error = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: None,
                graph: None,
                model_output: Some(&model.parse().expect("model")),
                limits,
            },
            Default::default(),
        )
        .expect_err("complete output must obey layout/bounds");
        assert!(
            match &error {
                build::PublicationRenderError::Site(
                    build::SiteRenderError::Output(_) | build::SiteRenderError::Render(_),
                ) => true,
                build::PublicationRenderError::Resource(gazette_sdk::resource::Error::Limit(
                    "resource metadata bytes",
                )) => limits.total_bytes == 1,
                _ => false,
            },
            "{model}: {error:?}"
        );
    }
}

#[test]
fn unsupported_assets_and_undeclared_capabilities_are_not_silently_omitted() {
    let foreign = Fixture::new(&[("other/notebook.ipynb", NOTEBOOK)]);
    let foreign_inputs = acquire(&foreign.config());
    let foreign_render =
        render(&foreign_inputs).expect("foreign assets do not defeat site isolation");
    assert!(
        foreign_render
            .site()
            .output()
            .file(&"notebook.ipynb".parse().expect("asset path"))
            .is_none()
    );
    let fixture = Fixture::new(&[("one/notebook.ipynb", NOTEBOOK)]);
    assert!(matches!(
        render(&acquire(&fixture.config())),
        Err(build::PublicationRenderError::Site(
            build::SiteRenderError::UncompiledAsset(_)
        ))
    ));
    let fixture = Fixture::new(&[(
        "theme/plugin.toml",
        b"package='fixture-plugin'\n[providers.fixture]\nimplements=['publication.search/v1']\n",
    )]);
    for requirement in ["required", "optional"] {
        let config = fixture.config().replace("[[principals]]", &format!("[composition.plugins]\n'fixture-plugin'={{state='enabled'}}\n[composition.capabilities]\n'publication.search/v1'={{state='enabled',provider={{kind='plugin',package='fixture-plugin',provider='missing'}},requirement='{requirement}'}}\n[[principals]]"))
            + "[[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n";
        assert!(
            matches!(
                acquire_result(&config),
                Err(build::CompositionError::Capability(error))
                    if error.code() == "package_selection"
            ),
            "optional cannot silently imply accepted degradation"
        );
    }
}

fn native_configuration(fixture: &Fixture) -> String {
    format!(
        "{}native_markdown={{route_prefix='/docs/library/',lifecycle='published'}}\n",
        fixture.project_config(true)
    )
}

fn page_at<'a>(model: &'a Value, route: &str) -> &'a Value {
    model["pages"]
        .as_object()
        .expect("complete page model")
        .values()
        .find(|page| page["route"]["canonical_url"] == route)
        .expect("exact expected native route")
}

fn cli(config: &str, arguments: &[&str]) -> Output {
    let mut arguments = arguments.to_vec();
    if arguments.first() == Some(&"render-site") && !arguments.contains(&"--render-year") {
        arguments.extend(["--render-year", "2040"]);
    }
    if arguments.first() == Some(&"check")
        && !matches!(arguments.get(1), Some(&"build" | &"workspace"))
        && !arguments.contains(&"--workspace-root")
    {
        arguments.push("--stdin");
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("direct Gazette process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("workspace transport");
    child.wait_with_output().expect("direct process result")
}

#[test]
fn publication_check_and_resolved_render_reuse_one_retained_semantic_phase() {
    let fixture = Fixture::new(&[("project/guide.md", PROJECT_DOC.as_bytes())]);
    let config = fixture.project_config(true);
    let inputs = acquire(&config);
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("one retained syntax phase");
    let resolved = parsed
        .resolve(Default::default())
        .expect("one semantic phase");
    assert!(std::ptr::eq(parsed.inputs(), &inputs));
    assert!(std::ptr::eq(resolved.inputs(), &inputs));
    assert!(std::ptr::eq(resolved.authored().corpus(), parsed.corpus()));
    let native_id = "doc:native".parse().expect("project document ID");
    let gazette_sdk::corpus::File::Document(original) =
        &parsed.corpus().files()["native-docs/guide.md"]
    else {
        panic!("native framed document must retain its parsed form");
    };
    assert!(std::ptr::eq(
        resolved
            .authored()
            .document(&native_id)
            .expect("same parsed document"),
        original.as_ref()
    ));
    let inspection = resolved.inspect();
    assert!(std::ptr::eq(
        inspection.graph,
        resolved.authored().content().graph()
    ));
    let expected = serde_json::to_value(&inspection).expect("private semantic inspection");
    assert_eq!(expected["status"], "resolved-unadmitted");
    assert_eq!(
        expected["graph"]["nodes"]["content:one"]["lifecycle"],
        "draft"
    );
    assert_eq!(
        expected["graph"]["nodes"]["content:other"]["lifecycle"],
        "archived"
    );
    assert_eq!(
        expected["graph"]["nodes"]
            .as_object()
            .expect("complete graph")
            .len(),
        4
    );
    assert_eq!(
        expected["acquisition"],
        serde_json::to_value(inputs.inspect()).expect("same exact capture evidence")
    );
    assert_eq!(
        serde_json::to_value(
            resolved
                .authored()
                .source_binding(&native_id)
                .expect("native home binding")
        )
        .expect("public source identity"),
        serde_json::json!({"site":"site:one","project":"project:native"})
    );
    let before = ["one", "other", "project", "theme"].map(|path| {
        source::inspect(
            &fixture.directory.path().join(path),
            None,
            source::Limits::default(),
        )
        .expect("native input before checking")
        .identity()
    });
    let checked = cli(&config, &["check", "--publication", "site:one"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(checked.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&checked.stdout).expect("CLI semantic report"),
        expected
    );
    for (path, identity) in ["one", "other", "project", "theme"].into_iter().zip(before) {
        assert_eq!(
            source::inspect(
                &fixture.directory.path().join(path),
                None,
                source::Limits::default()
            )
            .expect("native input after checking")
            .identity(),
            identity,
            "semantic checking must not mutate {path}"
        );
    }
    let render_resolved = || {
        build::render_resolved_publication(
            &resolved,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                graph: None,
                model_output: Some(&"model.json".parse().expect("model output")),
                limits: source::Limits::default(),
            },
            build::PublicationOutputLimits::default(),
        )
    };
    let retained = render_resolved().expect("render existing semantics");
    assert!(std::ptr::eq(retained.inputs(), &inputs));
    let convenience = render(&inputs).expect("convenience composes the same phases");
    assert_eq!(
        retained.site().output().inventory(),
        convenience.site().output().inventory()
    );
    for entry in retained.site().output().inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                retained.site().output().file(&entry.path),
                convenience.site().output().file(&entry.path)
            );
        }
    }
    fs::write(
        fixture.directory.path().join("project/guide.md"),
        b"source changed after capture",
    )
    .expect("isolated source edit");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        b"{% broken",
    )
    .expect("isolated template edit");
    assert_eq!(
        render_resolved()
            .expect("no recapture or reparse")
            .site()
            .output()
            .inventory()
            .identity(),
        retained.site().output().inventory().identity()
    );
    assert_eq!(
        serde_json::to_value(resolved.inspect()).expect("unchanged semantic phase"),
        expected
    );
    let failed = build::acquire_publication(
        &config,
        &"site:one".parse().expect("site"),
        Default::default(),
        source::PreparationLimits {
            sources: 8,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect_err("fresh acquisition must reject changed pinned bytes");
    let checked = cli(&config, &["check", "--publication", "site:one"]);
    assert!(!checked.status.success());
    assert!(checked.stdout.is_empty());
    assert!(String::from_utf8_lossy(&checked.stderr).contains(failed.code()));
}

#[test]
fn publication_check_fails_with_render_semantics_before_partial_output_or_writes() {
    let late = PAGE
        .replace("id='content:one'", "id='content:late'")
        .replace(
            "route={path='/guide/',aliases=['/old.html']}",
            "route={path='/late/'}",
        );
    for document in [
        late.replace("title='One <escaped>'", "title=["),
        late.replace("site='site:one'", "site='site:other'"),
        late.replace("related=['content:other']", "related=['content:missing']"),
    ] {
        let fixture = Fixture::new(&[("one/zz-late.md", document.as_bytes())]);
        let config = fixture.config();
        let inputs = acquire(&config);
        let error = render(&inputs).expect_err("whole retained corpus is invalid");
        let before = ["one", "other", "theme"].map(|path| {
            source::inspect(
                &fixture.directory.path().join(path),
                None,
                source::Limits::default(),
            )
            .expect("source before failed check")
            .identity()
        });
        let checked = cli(&config, &["check", "--publication", "site:one"]);
        assert!(!checked.status.success());
        assert!(checked.stdout.is_empty(), "no partially successful graph");
        let diagnostic = String::from_utf8_lossy(&checked.stderr);
        assert!(
            diagnostic.contains(&format!("gazette: {}:", error.code())),
            "{diagnostic}"
        );
        assert!(
            diagnostic.contains(&error.to_string()),
            "same semantic failure: {diagnostic}"
        );
        for (path, identity) in ["one", "other", "theme"].into_iter().zip(before) {
            assert_eq!(
                source::inspect(
                    &fixture.directory.path().join(path),
                    None,
                    source::Limits::default()
                )
                .expect("source after failed check")
                .identity(),
                identity
            );
        }
        assert_cli_failure_preserves_parent(&config, error.code());
    }
}

#[test]
fn publication_check_does_not_claim_template_rendering_or_public_admission() {
    let fixture = Fixture::new(&[("theme/templates/page.html", b"{% if broken")]);
    let config = fixture.config();
    let checked = cli(&config, &["check", "--publication", "site:one"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let report: Value = serde_json::from_slice(&checked.stdout).expect("semantic report only");
    assert_eq!(report["status"], "resolved-unadmitted");
    assert_eq!(
        report["graph"]["nodes"]["content:one"]["lifecycle"],
        "draft"
    );
    assert_eq!(
        report["graph"]["nodes"]["content:other"]["lifecycle"],
        "archived"
    );
    let error =
        render(&acquire(&config)).expect_err("rendering must still reject the broken Theme");
    assert!(matches!(error, build::PublicationRenderError::Template(_)));
    assert_cli_failure_preserves_parent(&config, error.code());
}

#[test]
fn publication_check_honors_corpus_limits_and_excludes_incompatible_modes() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let limited = cli(
        &config,
        &[
            "check",
            "--publication",
            "site:one",
            "--max-corpus-files",
            "1",
        ],
    );
    assert!(!limited.status.success());
    assert!(limited.stdout.is_empty());
    assert!(String::from_utf8_lossy(&limited.stderr).contains("gazette: corpus_limit:"));
    for mode in [
        vec!["--acquire-site", "site:one"],
        vec!["--acquire-composition", "site:one"],
        vec!["--parse-documents"],
        vec!["--check-composition"],
        vec!["--compile-templates"],
        vec!["--origin", "{}"],
        vec!["--composition-origin", "{}"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(["check", "--publication", "site:one"])
            .args(&mode)
            .stdin(Stdio::null())
            .output()
            .expect("actual argument parser");
        assert!(!output.status.success(), "incompatible selector {mode:?}");
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("cannot be used with"),
            "{mode:?}: {diagnostic}"
        );
        assert!(diagnostic.contains("--publication"), "{diagnostic}");
    }
}

#[test]
fn direct_cli_requires_local_effect_and_matches_sdk_output_before_fresh_retention() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let expected_inputs = acquire(&config);
    let expected = render(&expected_inputs).expect("SDK site");
    let arguments = RENDER_ARGUMENTS;
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete CLI report");
    assert_eq!(report["status"], "local_rendered");
    assert!(report["directory"].is_null());
    assert_eq!(
        report["inventory"],
        serde_json::to_value(expected.site().output().inventory()).expect("SDK inventory")
    );
    assert_eq!(
        report["evidence"],
        serde_json::to_value(expected.site().evidence()).expect("SDK evidence")
    );
    assert_eq!(report["evidence"]["alias_template"], "fixture/alias.html");
    let reacquired = acquire(&config);
    assert_eq!(
        serde_json::to_value(reacquired.inspect()).expect("reobserved source"),
        serde_json::to_value(expected_inputs.inspect()).expect("original inputs")
    );
    assert_eq!(expected.site().evidence().pages.len(), 2);

    let parent = tempdir().expect("explicit output parent");
    fs::write(parent.path().join("unrelated.txt"), b"preserve me").expect("unrelated fixture");
    let mut retained_arguments = arguments.to_vec();
    retained_arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("parent path"),
    ]);
    let output = cli(&config, &retained_arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let retained: Value = serde_json::from_slice(&output.stdout).expect("retained CLI receipt");
    assert_eq!(retained["status"], "local_retained");
    assert_eq!(retained["inventory"], report["inventory"]);
    assert_eq!(retained["evidence"], report["evidence"]);
    let directory =
        std::path::Path::new(retained["directory"].as_str().expect("retained directory"));
    assert_eq!(
        directory.parent().expect("native parent"),
        parent
            .path()
            .canonicalize()
            .expect("canonical fixture parent")
    );
    for entry in expected.site().output().inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("retained output"),
                expected
                    .site()
                    .output()
                    .file(&entry.path)
                    .expect("SDK bytes")
            );
        }
    }
    assert_eq!(
        fs::read(parent.path().join("unrelated.txt")).expect("unrelated read-back"),
        b"preserve me"
    );
    let mut overlap_arguments = arguments.to_vec();
    overlap_arguments.extend([
        "--output-parent",
        fixture.directory.path().to_str().expect("source parent"),
    ]);
    let overlap = cli(&config, &overlap_arguments);
    assert!(!overlap.status.success());
    assert!(overlap.stdout.is_empty());
    assert!(String::from_utf8_lossy(&overlap.stderr).contains("publication_stage"));
    let denied = cli(
        &config,
        &[
            "render-site",
            "--site",
            "site:one",
            "--model-output",
            "model.json",
        ],
    );
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
}

#[test]
fn direct_cli_without_model_output_retains_the_same_pages_without_a_debug_artifact() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let explicit = render(&inputs).expect("existing explicit debug model render");
    let omitted = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            model_output: None,
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("shared model remains available to the Theme");
    let parent = tempdir().expect("explicit disposable retention parent");
    let output = cli(
        &config,
        &[
            "render-site",
            "--local",
            "--site",
            "site:one",
            "--output-parent",
            parent.path().to_str().expect("parent path"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("native CLI report");
    assert_eq!(report["status"], "local_retained");
    assert_eq!(
        report["inventory"],
        serde_json::to_value(omitted.site().output().inventory()).unwrap()
    );
    assert_eq!(
        report["evidence"],
        serde_json::to_value(omitted.site().evidence()).unwrap()
    );
    assert!(report["evidence"].get("model_output").is_none());
    let retained = std::path::Path::new(report["directory"].as_str().expect("actual stage"));
    assert!(
        !retained.join("model.json").exists(),
        "no invented default path"
    );
    assert_eq!(
        omitted.site().output().inventory().entries().len() + 1,
        explicit.site().output().inventory().entries().len()
    );
    for entry in omitted.site().output().inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                omitted.site().output().file(&entry.path),
                explicit.site().output().file(&entry.path)
            );
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("retained output"),
                omitted
                    .site()
                    .output()
                    .file(&entry.path)
                    .expect("SDK bytes")
            );
        }
    }
    assert_eq!(
        serde_json::to_value(&explicit.site().evidence().pages).unwrap(),
        serde_json::to_value(&omitted.site().evidence().pages).unwrap()
    );
}

fn assert_cli_failure_preserves_parent(config: &str, expected_code: &str) {
    assert_cli_failure_with_arguments_preserves_parent(config, &RENDER_ARGUMENTS, expected_code);
}

fn assert_cli_failure_with_arguments_preserves_parent(
    config: &str,
    render_arguments: &[&str],
    expected_code: &str,
) {
    let parent = tempdir().expect("explicit output parent");
    fs::create_dir(parent.path().join("unrelated")).expect("unrelated directory");
    fs::write(parent.path().join("unrelated/keep.txt"), b"preserve me").expect("unrelated fixture");
    let before = source::inspect(parent.path(), None, source::Limits::default())
        .expect("parent inventory before failure");
    let mut arguments = render_arguments.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("parent path"),
    ]);
    let output = cli(config, &arguments);
    assert!(!output.status.success(), "{expected_code}: must fail");
    assert!(
        output.stdout.is_empty(),
        "{expected_code}: no success receipt"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains(&format!("gazette: {expected_code}:")),
        "wrong failure: {error}"
    );
    assert_eq!(
        source::inspect(parent.path(), None, source::Limits::default())
            .expect("parent inventory after failure")
            .identity(),
        before.identity(),
        "failure must not create any stage or change unrelated files"
    );
    assert_eq!(
        fs::read(parent.path().join("unrelated/keep.txt")).expect("unrelated read-back"),
        b"preserve me"
    );
}

#[test]
fn direct_cli_rejects_late_render_and_metadata_failures_without_creating_a_stage() {
    let missing_route = PAGE.replace("route={path='/guide/',aliases=['/old.html']}\n", "");
    let raw_html = PAGE.replace("**Retained** body", "<script>unadmitted()</script>");
    // The home page renders before content:one. Only its alias needs this missing
    // include, so this exercises failure after primary page bytes were produced.
    let missing_alias = "{% include 'missing.html' %}";
    for (path, replacement, code) in [
        ("one/page.md", missing_route.as_str(), "site_route_required"),
        (
            "theme/templates/alias.html",
            missing_alias,
            "render_template",
        ),
        (
            "one/page.md",
            raw_html.as_str(),
            "markdown_raw_html_unavailable",
        ),
        ("theme/theme.toml", "package=[", "toml_invalid"),
    ] {
        let fixture = Fixture::new(&[(path, replacement.as_bytes())]);
        assert_cli_failure_preserves_parent(&fixture.config(), code);
    }
}

#[test]
fn direct_cli_rejects_a_stale_source_pin_without_creating_a_stage() {
    let fixture = Fixture::new(&[]);
    let configuration = fixture.config();
    let repository = Repository::open(fixture.directory.path()).expect("fixture repository");
    let previous = repository
        .find_commit(fixture.commit)
        .expect("configured commit");
    let tree = previous.tree().expect("same source bytes");
    let signature = Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(1, 0))
        .expect("fixed successor signature");
    let successor = repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "advance fixture",
            &tree,
            &[&previous],
        )
        .expect("disposable successor commit");
    assert_ne!(successor, fixture.commit);
    assert_cli_failure_preserves_parent(&configuration, "source_revision_mismatch");
    assert_eq!(
        repository.head().expect("source HEAD preserved").target(),
        Some(successor),
        "Gazette must not check out the old pin"
    );
}

#[test]
fn direct_cli_lost_receipt_pipe_keeps_exact_retained_output_and_reports_recovery_location() {
    let fixture = Fixture::new(&[]);
    let configuration = fixture.config();
    let inputs = acquire(&configuration);
    let expected = render(&inputs).expect("independent expected local tree");
    let parent = tempdir().expect("explicit output parent");
    fs::write(parent.path().join("unrelated.txt"), b"preserve me").expect("unrelated fixture");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(RENDER_ARGUMENTS)
        .arg("--output-parent")
        .arg(parent.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("direct Gazette process");
    // Close the sole reader before sending any input: receipt delivery cannot
    // race with the fixture, and the child must still finish its explicit effect.
    drop(child.stdout.take().expect("sole receipt reader"));
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(configuration.as_bytes())
        .expect("workspace transport");
    let output = child.wait_with_output().expect("direct process result");
    assert!(
        !output.status.success(),
        "retention makes a lost receipt consequential"
    );
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("gazette: output_io:"), "{error}");
    assert!(
        error.contains("receipt delivery failed; inspect before retrying; no cleanup attempted"),
        "{error}"
    );
    let directories: Vec<_> = fs::read_dir(parent.path())
        .expect("output parent")
        .map(|entry| entry.expect("parent entry"))
        .filter(|entry| entry.file_type().expect("native type").is_dir())
        .map(|entry| entry.path())
        .collect();
    assert_eq!(directories.len(), 1, "exactly one retained effect");
    let retained = directories[0]
        .canonicalize()
        .expect("retained native location");
    assert!(
        error.contains(&format!("local tree retained at {retained:?}")),
        "recovery diagnostic must identify the actual retained tree: {error}"
    );
    let actual = source::inspect(&retained, None, source::Limits::default())
        .expect("independent retained-tree inventory");
    assert_eq!(
        actual.identity(),
        expected.site().output().inventory().identity()
    );
    for entry in actual.entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(retained.join(entry.path.as_str())).expect("retained file"),
                expected
                    .site()
                    .output()
                    .file(&entry.path)
                    .expect("expected bytes")
            );
        }
    }
    assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 2);
    assert_eq!(
        fs::read(parent.path().join("unrelated.txt")).expect("unrelated read-back"),
        b"preserve me"
    );
}

fn static_configuration(fixture: &Fixture, native: bool) -> String {
    fixture
        .project_config(native)
        .replace(
            "authored=[{source='source:one',target='one'}]",
            "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/authored/'}}]",
        )
        .replace(
            "documents={source='source:native',target='native-docs'}",
            "documents={source='source:native',target='native-docs',static_assets={route_prefix='/media/native/'}}",
        )
}
