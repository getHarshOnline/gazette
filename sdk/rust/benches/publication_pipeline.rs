//! Portable scaling evidence for the complete local publication path.

use std::{collections::BTreeMap, fs, path::Path, time::Instant};

use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const SCALES: [usize; 3] = [1, 10, 100];
const SAMPLES: usize = 3;
const MULTI_SITE_COUNT: usize = 3;
const BODY: &str = "A retained paragraph with **emphasis**, `code`, and a [reference](https://example.invalid/).\n\n";
const MANIFEST: &str = "package='theme:benchmark'\nnamespace='benchmark'\ntemplates='templates'\nrender={page='page.html'}\n";
const TEMPLATE: &str = "<article><h1>{{ content.title }}</h1>{{ body_html | safe }}{% for item in related %}<span>{{ item.id }}:{{ item.site }}</span>{% endfor %}</article>";

fn main() {
    let supplied = std::env::args().skip(1).collect::<Vec<_>>();
    let cargo_test = supplied
        .iter()
        .any(|argument| argument.starts_with("--test-threads="));
    let arguments = supplied
        .into_iter()
        .filter(|argument| argument != "--bench" && !argument.starts_with("--test-threads="))
        .collect::<Vec<_>>();
    let (shapes, scales) = if cargo_test && arguments.is_empty() {
        // `cargo test --all-targets` executes harness-free benchmarks. Keep that
        // portable validation bounded; ordinary `cargo bench` still runs the matrix.
        (vec![Shape::Single, Shape::Multi], vec![1])
    } else {
        selection(&arguments)
    };
    for shape in shapes {
        for scale in &scales {
            run(shape, *scale);
        }
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Single,
    Multi,
}

impl Shape {
    fn label(self) -> &'static str {
        match self {
            Self::Single => "single-site",
            Self::Multi => "multi-site",
        }
    }

    fn sites(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Multi => MULTI_SITE_COUNT,
        }
    }
}

fn selection(arguments: &[String]) -> (Vec<Shape>, Vec<usize>) {
    let shapes = match arguments.first().map(String::as_str) {
        None => vec![Shape::Single, Shape::Multi],
        Some("single") => vec![Shape::Single],
        Some("multi") => vec![Shape::Multi],
        Some("1" | "10" | "100") => vec![Shape::Single, Shape::Multi],
        Some(_) => usage(),
    };
    let scale_argument = if arguments
        .first()
        .is_some_and(|argument| matches!(argument.as_str(), "single" | "multi"))
    {
        arguments.get(1)
    } else {
        arguments.first()
    };
    if arguments.len()
        > usize::from(scale_argument.is_some())
            + usize::from(
                arguments
                    .first()
                    .is_some_and(|argument| matches!(argument.as_str(), "single" | "multi")),
            )
    {
        usage();
    }
    let scales = match scale_argument.map(String::as_str) {
        None => SCALES.to_vec(),
        Some("1") => vec![1],
        Some("10") => vec![10],
        Some("100") => vec![100],
        Some(_) => usage(),
    };
    (shapes, scales)
}

fn usage() -> ! {
    eprintln!("usage: publication_pipeline [single|multi] [1|10|100]");
    std::process::exit(2);
}

fn run(shape: Shape, scale: usize) {
    let fixture = Fixture::new(shape.sites(), scale);
    let recipe = build::LocalPublicationRecipe::parse_json(
        fixture.recipe.as_bytes(),
        fixture.recipe.len(),
        fixture.sites,
    )
    .expect("benchmark-owned recipe");
    let mut expected = None;
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let (observation, wall_us) = measured(|| {
            recipe
                .with_rendered(
                    toml::Limits::default(),
                    build::PublicationSetAcquisitionLimits::default(),
                    source::Limits::default(),
                    build::PublicationSetRenderLimits::default(),
                    observe,
                )
                .expect("complete current SDK publication path")
        });
        assert_eq!(observation.sites, fixture.sites);
        assert_eq!(observation.pages, fixture.pages);
        assert_eq!(
            observation.output,
            *expected.get_or_insert_with(|| observation.output.clone())
        );
        let peak_retained_bytes_proxy = u64::try_from(fixture.tracked_source_bytes)
            .expect("tracked source bytes fit u64")
            .checked_add(observation.output_bytes)
            .and_then(|bytes| {
                bytes.checked_add(
                    u64::try_from(observation.receipt_bytes).expect("receipt bytes fit u64"),
                )
            })
            .expect("retained-byte proxy fits u64");
        samples.push(json!({
            "sample": sample,
            "wall_us": wall_us,
            "output_entries": observation.output_entries,
            "output_bytes": observation.output_bytes,
            "receipt_bytes": observation.receipt_bytes,
            "peak_retained_bytes_proxy": peak_retained_bytes_proxy,
            "output_sha256": source::Sha256Digest::of(&observation.output),
        }));
    }
    println!(
        "{}",
        json!({
            "format": "gazette.publication-pipeline-benchmark.v1",
            "shape": shape.label(),
            "scale": scale,
            "sites": fixture.sites,
            "pages_per_site": scale,
            "pages": fixture.pages,
            "tracked_source_bytes": fixture.tracked_source_bytes,
            "memory_measurement": "coexisting retained source payloads + generated output payloads + serialized receipt; portable deterministic lower-bound proxy, not allocator or RSS peak",
            "samples": samples,
        })
    );
}

struct Observation {
    sites: usize,
    pages: usize,
    output_entries: usize,
    output_bytes: u64,
    receipt_bytes: usize,
    output: Vec<u8>,
}

fn observe(rendered: &build::RenderedPublicationSet<'_>) -> Observation {
    assert!(
        rendered.is_complete(),
        "{}",
        serde_json::to_string_pretty(&rendered.inspect()).expect("incomplete inspection")
    );
    let mut pages = 0;
    let mut output_entries = 0;
    let mut output_bytes = 0_u64;
    let mut output = BTreeMap::new();
    for (site, result) in rendered.sites() {
        let tree = result
            .as_ref()
            .expect("complete site output")
            .site()
            .output();
        for required in [
            "model.json",
            "search.json",
            "sitemap.xml",
            "llms.txt",
            "site.jsonld",
        ] {
            assert!(
                tree.file(&required.parse().expect("literal output path"))
                    .is_some(),
                "{site} omitted required representative output {required}"
            );
        }
        let model: Value = serde_json::from_slice(
            tree.file(&"model.json".parse().expect("model output"))
                .expect("representative site model"),
        )
        .expect("valid shared site model");
        pages += model["pages"].as_object().expect("page map").len();
        output_entries += tree.inventory().entries().len();
        output_bytes += tree.inventory().total_bytes();
        output.insert(
            site.to_string(),
            serde_json::to_value(tree.inventory()).expect("deterministic inventory"),
        );
    }
    let output = serde_json::to_vec(&output).expect("deterministic output identity envelope");
    let receipt_bytes = serde_json::to_vec(&rendered.inspect())
        .expect("complete private inspection")
        .len();
    Observation {
        sites: rendered.sites().len(),
        pages,
        output_entries,
        output_bytes,
        receipt_bytes,
        output,
    }
}

struct Fixture {
    _directory: TempDir,
    recipe: String,
    sites: usize,
    pages: usize,
    tracked_source_bytes: usize,
}

impl Fixture {
    fn new(sites: usize, pages_per_site: usize) -> Self {
        let directory = tempdir().expect("disposable publication fixture");
        let repository = Repository::init(directory.path()).expect("fixture Git repository");
        let mut tracked_source_bytes = 0;
        write(
            directory.path(),
            "theme/theme.toml",
            MANIFEST.as_bytes(),
            &mut tracked_source_bytes,
        );
        write(
            directory.path(),
            "theme/templates/page.html",
            TEMPLATE.as_bytes(),
            &mut tracked_source_bytes,
        );
        for site in 0..sites {
            for page in 0..pages_per_site {
                let related = if sites > 1 && page == 0 {
                    format!("related=['content:s{}-p0000']\n", (site + 1) % sites)
                } else {
                    String::new()
                };
                let document = format!(
                    "+++\n{related}[content]\nid='content:s{site}-p{page:04}'\nsite='site:s{site}'\ntitle='Site {site} page {page}'\ndescription='Retained benchmark publication {site}.'\nlifecycle='published'\nroute={{path='/page-{page:04}/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Site {site} page {page}\n\n{}",
                    BODY.repeat(4)
                );
                write(
                    directory.path(),
                    &format!("content/s{site}/page-{page:04}.md"),
                    document.as_bytes(),
                    &mut tracked_source_bytes,
                );
            }
        }
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(["content", "theme"], IndexAddOption::DEFAULT, None)
            .expect("fixture tracked files");
        let tree = repository
            .find_tree(index.write_tree().expect("fixture tree OID"))
            .expect("fixture tree");
        let signature = Signature::new(
            "Gazette benchmark",
            "benchmark@example.invalid",
            &git2::Time::new(0, 0),
        )
        .expect("fixed signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("fixture commit");
        let root = directory.path().to_str().expect("UTF-8 fixture root");
        let workspace = workspace(root, commit, sites);
        let recipe = json!({
            "format": "gazette-local-publication-set/1",
            "workspace": {"kind": "supplied", "source": workspace},
            "sites": (0..sites).map(|site| json!({
                "site": format!("site:s{site}"),
                "model_output": "model.json",
                "graph": null,
            })).collect::<Vec<_>>(),
        })
        .to_string();
        Self {
            _directory: directory,
            recipe,
            sites,
            pages: sites * pages_per_site,
            tracked_source_bytes,
        }
    }
}

fn workspace(root: &str, commit: git2::Oid, sites: usize) -> String {
    let mut value = String::from(
        "[composition]\ntheme={package='theme:benchmark'}\n\
         [[principals]]\nid='owner:benchmark'\nkind='person'\nname='Benchmark owner'\n",
    );
    for site in 0..sites {
        value.push_str(&format!(
            "[[sites]]\nid='site:s{site}'\npublisher='owner:benchmark'\ndeployable=false\n\
             [[brands]]\nid='brand:s{site}'\nname='Benchmark site {site}'\n\
             [[brands.publications]]\nsite='site:s{site}'\nmachine={{json_ld=true,search='lexical',sitemap=true,llms={{enabled=true,description_from='content:s{site}-p0000',representation='markdown-alternate',include_human_link=true,sections=[{{title='Start here',contents=['content:s{site}-p0000']}}]}},markdown_alternates={{enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}}}\n\
             public_base_url='https://s{site}.example/'\nauthored=[{{source='source:s{site}',target='s{site}'}}]\n"
        ));
    }
    value.push_str(&format!(
        "[[sources]]\nid='source:theme'\n\
         location={{kind='repository',root={root:?},commit='{commit}',subdirectory='theme'}}\n\
         [[theme_packages]]\npackage='theme:benchmark'\nsource='source:theme'\nmanifest='theme.toml'\n"
    ));
    for site in 0..sites {
        value.push_str(&format!(
            "[[sources]]\nid='source:s{site}'\n\
             location={{kind='repository',root={root:?},commit='{commit}',subdirectory='content/s{site}'}}\n"
        ));
    }
    value
}

fn write(root: &Path, relative: &str, bytes: &[u8], total: &mut usize) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture file parent")).expect("fixture directory");
    fs::write(path, bytes).expect("fixture bytes");
    *total = total
        .checked_add(bytes.len())
        .expect("source bytes fit usize");
}

fn measured<T>(operation: impl FnOnce() -> T) -> (T, u128) {
    let started = Instant::now();
    let result = operation();
    (result, started.elapsed().as_micros())
}
