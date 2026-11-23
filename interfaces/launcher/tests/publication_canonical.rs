//! Retained two-site canonical origin through real rendering and direct CLI staging.

use gazette_sdk::{build, html, source};
use git2::{IndexAddOption, Repository, Signature};
use serde_json::Value;
use std::{
    convert::Infallible,
    fs,
    io::Write,
    process::{Command, Stdio},
};
use tempfile::{TempDir, tempdir};

const ORIGINAL: &str = "https://original.example/mount/work/";
const COPY: &str = "https://copy.example/mount/edition/";
const TEMPLATE: &str = "<!doctype html><html><head><title>{{ content.title }}</title><link rel=\"canonical\" href=\"{{ route.canonical_url }}\"><script type=\"application/ld+json\">{{ json_ld | json_script }}</script></head><body><a href=\"{{ route.url }}\">This edition</a><p>publisher={{ site.publisher }}</p>{% for binding in attributions %}<p>{{ binding.role }}={{ binding.principal.id }}</p>{% endfor %}{{ body_html | safe }}</body></html>";

struct Fixture {
    directory: TempDir,
    configuration: String,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempdir().expect("owned retained sources");
        let repository = Repository::init(directory.path()).unwrap();
        for (path, text) in [
            (
                "theme/theme.toml",
                "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html',graph='graph.json'}\n",
            ),
            ("theme/templates/page.html", TEMPLATE),
            (
                "theme/templates/alias.html",
                "<meta http-equiv=\"refresh\" content=\"0; url={{ route.canonical_url }}\"><a href=\"{{ route.canonical_url }}\">Moved</a>",
            ),
            ("theme/templates/graph.json", "{{ __tera_context }}"),
            (
                "original/page.md",
                "+++\n[content]\nid='content:original'\nsite='site:original'\ntitle='Original work'\ndescription='The original retained work and its author.'\nlifecycle='published'\nroute={path='/work/'}\nattributions=[{principal='principal:author',role='author'}]\nstructure={kind='page',family={kind='article'}}\n+++\n# Original body\n",
            ),
            (
                "copy/page.md",
                "+++\n[content]\nid='content:copy'\nsite='site:copy'\ntitle='Syndicated edition'\nlifecycle='published'\nroute={path='/edition/',canonical='content:original'}\nattributions=[{principal='principal:rights',role='rights-holder'}]\nstructure={kind='page',family={kind='article'}}\n+++\n# Independently retained edition\n",
            ),
        ] {
            let path = directory.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let mut index = repository.index().unwrap();
        index
            .add_all(["theme", "original", "copy"], IndexAddOption::DEFAULT, None)
            .unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0)).unwrap();
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "retained sources",
                &tree,
                &[],
            )
            .unwrap();
        let mut configuration = String::from("[composition]\ntheme={package='theme:one'}\n");
        for name in ["original", "copy", "rights"] {
            configuration.push_str(&format!(
                "[[principals]]\nid='principal:{name}'\nkind='person'\n"
            ));
        }
        configuration.push_str(
            "[[principals]]\nid='principal:author'\nkind='person'\nname='Fixture author'\n",
        );
        for name in ["original", "copy"] {
            configuration.push_str(&format!(
                "[[sites]]\nid='site:{name}'\npublisher='principal:{name}'\ndeployable=false\n"
            ));
        }
        configuration.push_str("[[brands]]\nid='brand:publication'\nname='Explicit publication'\n");
        for name in ["original", "copy"] {
            let curated = if name == "original" {
                ",llms={enabled=true,description_from='content:original',representation='markdown-alternate',include_human_link=true,sections=[{title='Original work',contents=['content:original']}]},markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}"
            } else {
                ""
            };
            configuration.push_str(&format!("[[brands.publications]]\nsite='site:{name}'\npublic_base_url='https://{name}.example/mount/'\nauthored=[{{source='source:{name}',target='{name}'}}]\nmachine={{sitemap=true,search='lexical',json_ld=true{curated}}}\n"));
        }
        for name in ["theme", "original", "copy"] {
            configuration.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='{name}'}}\n", directory.path().to_str().unwrap()));
        }
        configuration.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n");
        Self {
            directory,
            configuration,
        }
    }
}

fn bytes<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().unwrap()).expect("complete output")
}

fn acquisition_limits() -> source::PreparationLimits {
    source::PreparationLimits {
        sources: 16,
        entries: 1024,
        bytes: 1024 * 1024,
        per_source: source::RevisionLimits::default(),
    }
}

fn render(inputs: &build::AcquiredInputs) -> build::PublicationRender<'_> {
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: Some(build::GraphRenderOptions {
                output: &"graph.json".parse().unwrap(),
                limits: Default::default(),
            }),
            model_output: Some(&"model.json".parse().unwrap()),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("connected native rendering")
}

#[test]
fn canonical_copy_keeps_native_home_roles_and_bytes_across_sdk_cli_and_machine_outputs() {
    let fixture = Fixture::new();
    let inputs = build::acquire_publication(
        &fixture.configuration,
        &"site:copy".parse().unwrap(),
        Default::default(),
        acquisition_limits(),
    )
    .expect("exact native source captures");
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    for name in ["copy", "original"] {
        let binding = resolved
            .authored()
            .source_binding(&format!("content:{name}").parse().unwrap())
            .unwrap();
        assert_eq!(binding.source_key().as_str(), format!("source:{name}"));
        assert_eq!(binding.site.as_str(), format!("site:{name}"));
    }
    let review = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let reviewed_copy = review.request().surfaces().iter().find(|surface| matches!(surface.target(), build::ReviewTarget::Page { content } if content.as_str() == "content:copy")).expect("copy has its own policy review surface");
    assert_eq!(
        reviewed_copy.value()["content"]["route"]["canonical"],
        "content:original"
    );
    assert_eq!(reviewed_copy.value()["site"]["publisher"], "principal:copy");
    let rendered = render(&inputs);
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:copy"];
    assert_eq!(page["route"]["url"], COPY);
    assert_eq!(page["route"]["canonical_url"], ORIGINAL);
    assert_eq!(page["content"]["route"]["canonical"], "content:original");
    assert_eq!(page["site"]["publisher"], "principal:copy");
    assert_eq!(page["site"]["roles"], serde_json::json!([]));
    assert_eq!(page["provenance"]["site"], "site:copy");
    assert_eq!(page["attributions"].as_array().unwrap().len(), 1);
    assert_eq!(
        page["attributions"][0]["principal"]["id"],
        "principal:rights"
    );
    assert_eq!(
        model["graph"]["references"]["content:original"]["site"],
        "site:original"
    );
    assert_eq!(model["graph"]["routes"]["content:copy"], page["route"]);
    assert!(model["pages"].get("content:original").is_none());
    let mut attributes = Vec::new();
    let mut linked = Vec::new();
    html::inspect(
        bytes(tree, "edition/index.html"),
        Default::default(),
        |kind, value| {
            match kind {
                html::TextKind::Attribute => attributes.push(value.to_owned()),
                html::TextKind::JsonScript => {
                    linked.push(serde_json::from_str::<Value>(value).unwrap())
                }
                _ => (),
            }
            Ok::<_, Infallible>(())
        },
    )
    .expect("native HTML decoding, not raw escaped URL matching");
    assert_eq!(
        attributes
            .iter()
            .filter(|value| value.as_str() == COPY)
            .count(),
        1
    );
    assert_eq!(
        attributes
            .iter()
            .filter(|value| value.as_str() == ORIGINAL)
            .count(),
        1
    );
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0]["url"], COPY);
    assert_eq!(linked[0]["mainEntityOfPage"]["url"], ORIGINAL);
    assert_eq!(linked[0]["mainEntityOfPage"]["@type"], "WebPage");
    assert_eq!(
        linked[0]["mainEntityOfPage"]["identifier"],
        "content:original"
    );
    for field in ["breadcrumb", "publisher", "author", "copyrightHolder"] {
        assert!(
            linked[0]["mainEntityOfPage"].get(field).is_none(),
            "copy {field} must not leak into original page identity"
        );
    }
    assert!(linked[0].get("gazette:canonical").is_none());
    assert_eq!(linked[0]["publisher"]["identifier"], "principal:copy");
    assert!(linked[0].get("author").is_none());
    let site_linked: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).unwrap();
    assert_eq!(site_linked["@graph"].as_array().unwrap().len(), 1);
    assert_eq!(
        site_linked["@graph"][0]["mainEntityOfPage"],
        linked[0]["mainEntityOfPage"]
    );
    let sitemap = std::str::from_utf8(bytes(tree, "sitemap.xml")).unwrap();
    assert!(
        !sitemap.contains("<loc>"),
        "noncanonical copy has no sitemap entry"
    );
    let search: Value = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    assert_eq!(search[0]["route"]["url"], COPY);
    assert_eq!(search[0]["route"]["canonical_url"], ORIGINAL);

    let original_inputs = build::acquire_publication(
        &fixture.configuration,
        &"site:original".parse().unwrap(),
        Default::default(),
        acquisition_limits(),
    )
    .unwrap();
    let original_render = render(&original_inputs);
    let original_model: Value =
        serde_json::from_slice(bytes(original_render.site().output(), "model.json")).unwrap();
    let original_page = &original_model["pages"]["content:original"];
    let original_linked: Value =
        serde_json::from_slice(bytes(original_render.site().output(), "site.jsonld")).unwrap();
    assert_eq!(
        linked[0]["mainEntityOfPage"]["@id"],
        original_linked["@graph"][0]["mainEntityOfPage"]["@id"],
        "both publication outputs reference the same native original page identity"
    );
    assert_eq!(original_page["site"]["publisher"], "principal:original");
    assert_eq!(original_page["provenance"]["site"], "site:original");
    assert_eq!(
        original_page["attributions"][0]["principal"]["id"],
        "principal:author"
    );
    assert!(
        std::str::from_utf8(bytes(original_render.site().output(), "sitemap.xml"))
            .unwrap()
            .contains(ORIGINAL)
    );
    assert!(
        std::str::from_utf8(bytes(original_render.site().output(), "llms.txt"))
            .unwrap()
            .contains("## Original work")
    );
    let alternate =
        std::str::from_utf8(bytes(original_render.site().output(), "work/index.md")).unwrap();
    assert!(
        alternate.contains("Fixture author"),
        "Markdown projection carries the native attribution instead of synthesizing an llms author catalogue"
    );
    assert!(
        tree.file(&"llms.txt".parse().unwrap()).is_none(),
        "a syndicated-only site cannot curate its noncanonical copy"
    );

    let stage = tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "render-site",
            "--render-year",
            "2040",
            "--local",
            "--site",
            "site:copy",
            "--model-output",
            "model.json",
            "--graph-output",
            "graph.json",
            "--output-parent",
            stage.path().to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(fixture.configuration.as_bytes())
        .unwrap();
    let cli = child.wait_with_output().unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let receipt: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).unwrap()
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    for entry in tree.inventory().entries() {
        if let Some(expected) = tree.file(&entry.path) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).unwrap(),
                expected
            );
        }
    }
    fixture
        .directory
        .close()
        .expect("remove only owned native source fixture");
    assert_eq!(
        render(&inputs).site().output().inventory(),
        tree.inventory(),
        "rebuild uses retained captures after native sources disappear"
    );
}
