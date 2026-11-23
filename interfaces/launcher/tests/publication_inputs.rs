//! Real native capture joins authored roots and Theme packages without publishing either.

#[path = "publication_inputs/disclosure.rs"]
mod disclosure;
#[path = "publication_inputs/legal.rs"]
mod legal;
#[path = "publication_inputs/review_responses.rs"]
mod review_responses;

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, corpus, source, toml};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const THEME_MANIFEST: &str = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n";
const PAGE: &str = "+++\n[content]\nid='content:one'\nsite='site:one'\ntitle='One'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained one\n";
const OTHER: &str = "+++\nrelated=['content:one']\n[content]\nid='content:other'\nsite='site:other'\ntitle='Other'\nlifecycle='archived'\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Retained other\n";
const PROJECT_PAGE: &str = "+++\n[content]\nid='content:project'\nsite='site:one'\ntitle='Project reference'\nlifecycle='published'\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Native project documentation\n";

struct Fixture {
    directory: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let directory = tempdir().expect("disposable native repository");
        let repository = Repository::init(directory.path()).expect("fixture Git");
        fs::create_dir(directory.path().join("selected")).expect("selected source");
        for (path, bytes) in files {
            let path = directory.path().join("selected").join(path);
            fs::create_dir_all(path.parent().expect("fixture parent"))
                .expect("fixture directories");
            fs::write(path, bytes).expect("fixture bytes");
        }
        let mut index = repository.index().expect("fixture index");
        index
            .add_all(["selected"], IndexAddOption::DEFAULT, None)
            .expect("fixture entries");
        let tree = repository
            .find_tree(index.write_tree().expect("fixture tree"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("local fixture commit");
        Self { directory, commit }
    }

    fn declaration(&self, id: &str) -> String {
        declaration(id, self.directory.path(), &self.commit.to_string())
    }
}

fn declaration(id: &str, root: &Path, commit: &str) -> String {
    format!(
        "[[sources]]\nid='{id}'\nlocation={{kind='repository',root={:?},commit='{commit}',subdirectory='selected'}}\n",
        root.to_str().expect("native path")
    )
}

fn configuration(one: &Fixture, other: &Fixture) -> String {
    format!(
        "[composition]\ntheme={{package='theme:one'}}\n\
        [[principals]]\nid='owner:one'\nkind='person'\n\
        [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
        [[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n\
        [[brands]]\nid='brand:one'\nname='Not a source path'\n\
        [[brands.publications]]\nsite='site:one'\nauthored=[{{source='source:one',target='one'}}]\n\
        [[brands.publications]]\nsite='site:other'\nauthored=[{{source='source:other',target='other'}}]\n{}{}{}\
        [[theme_packages]]\npackage='theme:one'\nsource='source:one'\nmanifest='theme.toml'\n",
        one.declaration("source:one"),
        other.declaration("source:other"),
        declaration(
            "source:unused",
            &one.directory.path().join("absent"),
            &"0".repeat(40)
        )
    )
}

fn fixtures() -> (Fixture, Fixture) {
    (
        Fixture::new(&[
            ("theme.toml", THEME_MANIFEST.as_bytes()),
            ("templates/page.html", b"{{ content.title }}"),
            ("page.md", PAGE.as_bytes()),
        ]),
        Fixture::new(&[
            ("page.md", OTHER.as_bytes()),
            ("opaque.bin", &[0xff, 0, 0x80]),
            ("theme.toml", b"invalid = ["),
        ]),
    )
}

fn acquire(
    config: &str,
    limits: source::PreparationLimits,
) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_publication(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits,
    )
}

fn acquisition_limits() -> source::PreparationLimits {
    source::PreparationLimits {
        sources: 16,
        entries: 1024,
        bytes: 1024 * 1024,
        per_source: source::RevisionLimits::default(),
    }
}

#[test]
fn multiple_owner_roots_are_captured_once_and_keep_declared_public_routes() {
    let (one, other) = fixtures();
    let text = "+++\n[content]\nid='content:descendant'\nsite='site:one'\ntitle='Descendant essay'\nlifecycle='draft'\nroute={path='/blog/descendant/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Descendant essay\n";
    let descendant = Fixture::new(&[("essay.md", text.as_bytes())]);
    let config = configuration(&one, &other).replace(
        "authored=[{source='source:one',target='one'}]",
        "public_base_url='https://one.example/'\nauthored=[{source='source:one',target='one'},{source='source:descendant',target='owners/descendant'}]",
    ) + &descendant.declaration("source:descendant");
    let acquired = acquire(&config, acquisition_limits()).expect("complete native acquisition");
    assert_eq!(
        acquired.sources().len(),
        3,
        "Theme and authored root reuse one capture"
    );
    fs::write(
        descendant.directory.path().join("selected/essay.md"),
        "changed after capture",
    )
    .expect("disposable source mutation");
    let parsed = acquired
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained distributed corpus");
    let publication = parsed.resolve(Default::default()).expect("shared graph");
    let resolved = publication.authored();
    resolved
        .validate_source_bindings()
        .expect("exact home/source bindings");
    let id = "content:descendant".parse().expect("identity");
    assert_eq!(
        resolved.source_path(&id).expect("source path").as_str(),
        "owners/descendant/essay.md"
    );
    let binding = resolved.source_binding(&id).expect("descendant binding");
    assert!(
        binding.project.is_none(),
        "mounting is not a semantic project relation"
    );
    assert_eq!(binding.source_key().as_str(), "source:descendant");
    assert_eq!(
        resolved
            .content()
            .route(&id)
            .expect("declared route")
            .canonical_url,
        "https://one.example/blog/descendant/"
    );
    let page = resolved
        .project_page(
            &id,
            &gazette_sdk::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("rendered page projection");
    assert!(page.body_html.contains("Descendant essay"));
    assert!(!page.body_html.contains("changed after capture"));
}

#[test]
fn native_sources_deduplicate_roles_and_feed_complete_cross_site_retained_corpus() {
    let (one, other) = fixtures();
    let config = configuration(&one, &other);
    let acquired = acquire(
        &config,
        source::PreparationLimits {
            sources: 2,
            ..acquisition_limits()
        },
    )
    .expect("one budget for two exact keys");
    assert_eq!(
        acquired.sources().len(),
        2,
        "Theme/authored shared key captured once"
    );
    let metadata = acquired
        .check_metadata(toml::Limits::default())
        .expect("authored-only source needs no package metadata");
    assert_eq!(metadata.inspect().theme_packages.len(), 1);
    assert!(metadata.inspect().plugin_packages.is_empty());
    fs::write(
        one.directory.path().join("selected/page.md"),
        "changed after capture",
    )
    .expect("disposable mutation");
    fs::write(
        other.directory.path().join("selected/page.md"),
        "changed other capture",
    )
    .expect("disposable mutation");
    let parsed = acquired
        .parse_publication(corpus::Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained authored universe");
    let publication = parsed
        .resolve(Default::default())
        .expect("real cross-site graph");
    let resolved = publication.authored();
    assert_eq!(resolved.content().graph().nodes().len(), 2);
    assert_eq!(
        resolved
            .document(&"content:one".parse().expect("key"))
            .expect("one document")
            .source(),
        PAGE
    );
    assert_eq!(
        resolved
            .document(&"content:other".parse().expect("key"))
            .expect("other document")
            .source(),
        OTHER
    );
    assert_eq!(resolved.content().graph().related().len(), 1);
    let source_key = "source:other".parse().expect("source key");
    let path = "opaque.bin".parse().expect("source-relative path");
    let retained = acquired.sources()[&source_key]
        .file(&path)
        .expect("retained opaque bytes");
    match &parsed.corpus().files()["other/opaque.bin"] {
        corpus::File::Asset(bytes) => {
            assert_eq!(*bytes, retained);
            assert!(std::ptr::eq(bytes.as_ptr(), retained.as_ptr()));
        }
        other => panic!("asset must remain opaque: {other:?}"),
    }
    assert_eq!(parsed.corpus().files().len(), 6);
    assert!(
        matches!(
            acquire(&config, acquisition_limits()),
            Err(build::CompositionError::Source { .. })
        ),
        "new acquisition cannot hide changed native bytes"
    );
}

#[test]
fn composition_only_does_not_read_other_authored_sources_or_skip_required_publication_capture() {
    let (one, other) = fixtures();
    let config = configuration(&one, &other);
    fs::write(
        other.directory.path().join("selected/page.md"),
        "wrong pin bytes",
    )
    .expect("disposable mutation");
    let composition = build::acquire_composition(
        &config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        acquisition_limits(),
    )
    .expect("composition-only capture");
    assert_eq!(composition.sources().len(), 1);
    assert!(matches!(
        composition.parse_publication(corpus::Limits::default(), &gazette_adapter_markdown::Comrak),
        Err(build::PublicationInputError::Binding(_))
    ));
    match acquire(&config, acquisition_limits()).expect_err("every authored root participates") {
        build::CompositionError::Source { binding, .. } => {
            assert_eq!(binding.as_str(), "source:other")
        }
        error => panic!("exact native failure required: {error:?}"),
    }
}

#[test]
fn content_universe_not_a_dummy_authored_folder_is_required_before_capture() {
    let (one, other) = fixtures();
    let config = configuration(&one, &other);
    let missing = config.replace("authored=[{source='source:one',target='one'}]", "");
    let acquired = acquire(&missing, acquisition_limits())
        .expect("configured consumer can use the complete foreign content universe");
    let parsed = acquired
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("same rule during retained parsing");
    assert!(
        parsed
            .corpus()
            .files()
            .keys()
            .all(|path| path.as_str().starts_with("other/"))
    );
    assert_eq!(parsed.corpus().files().len(), 3);
    let no_content = missing.replace("authored=[{source='source:other',target='other'}]", "");
    assert!(matches!(
        acquire(&no_content, acquisition_limits()),
        Err(build::CompositionError::MissingContent)
    ));
    assert!(matches!(
        acquire(
            &config,
            source::PreparationLimits {
                sources: 1,
                ..acquisition_limits()
            }
        ),
        Err(build::CompositionError::TooManySources { limit: 1 })
    ));
    let alias = format!(
        "{config}{}[[sites]]\nid='site:third'\npublisher='owner:one'\ndeployable=false\n[[brands.publications]]\nsite='site:third'\nauthored=[{{source='source:alias',target='third'}}]\n",
        one.declaration("source:alias")
    );
    assert!(
        matches!(
            acquire(
                &alias,
                source::PreparationLimits {
                    sources: 2,
                    ..acquisition_limits()
                }
            ),
            Err(build::CompositionError::TooManySources { limit: 2 })
        ),
        "different keys with identical repository coordinates remain independently budgeted"
    );
}

#[test]
fn complete_capture_and_mapping_enforce_aggregate_limits_without_subset() {
    let (one, other) = fixtures();
    let config = configuration(&one, &other);
    for limits in [
        source::PreparationLimits {
            entries: 1,
            ..acquisition_limits()
        },
        source::PreparationLimits {
            bytes: 1,
            ..acquisition_limits()
        },
    ] {
        assert!(matches!(
            acquire(&config, limits),
            Err(build::CompositionError::Source { .. } | build::CompositionError::Aggregate { .. })
        ));
    }
    let acquired = acquire(&config, acquisition_limits()).expect("captures");
    for (limits, resource) in [
        (
            corpus::Limits {
                files: 0,
                ..Default::default()
            },
            "files",
        ),
        (
            corpus::Limits {
                path_bytes: 0,
                ..Default::default()
            },
            "path bytes",
        ),
    ] {
        match acquired
            .parse_publication(limits, &gazette_adapter_markdown::Comrak)
            .expect_err("mapping bounds")
        {
            build::PublicationInputError::Document(build::DocumentError::Corpus(
                corpus::Error::Limit(actual),
            )) => assert_eq!(actual, resource),
            error => panic!("exact mapping limit required: {error:?}"),
        }
    }
}

#[test]
fn equal_and_nested_authored_prefixes_fail_without_coincident_file_names() {
    let one = Fixture::new(&[
        ("theme.toml", THEME_MANIFEST.as_bytes()),
        ("templates/page.html", b"content"),
    ]);
    let other = Fixture::new(&[(".keep", b"")]);
    for target in ["one", "one/nested"] {
        let config =
            configuration(&one, &other).replace("target='other'", &format!("target='{target}'"));
        match acquire(&config, acquisition_limits()).expect_err("prefix preflight before capture") {
            build::CompositionError::Configuration(gazette_sdk::workspace::Error::Invalid(
                errors,
            )) => {
                assert!(
                    errors
                        .iter()
                        .any(|error| error.code == "workspace_content_overlap")
                );
            }
            error => panic!("compiler owns prefix conflicts: {error:?}"),
        }
    }
}

fn project_placement(source: &str, target: &str, render: bool) -> String {
    format!(
        "[[projects]]\nid='project:one'\nlifecycle='active'\nvisibility='public'\navailability='available'\n\
        [[placements]]\nrole='primary'\nsite='site:one'\nproject='project:one'\nrender={render}\ndiscoverable=false\n\
        documents={{source='{source}',target='{target}'}}\n"
    )
}

#[test]
fn framed_project_documentation_joins_retained_authored_corpus_without_synthesis() {
    let (one, other) = fixtures();
    let project = Fixture::new(&[("guide.md", PROJECT_PAGE.as_bytes())]);
    let config = format!(
        "{}{}{}",
        configuration(&one, &other),
        project.declaration("source:project"),
        project_placement("source:project", "project-docs", true)
    );
    let acquired =
        acquire(&config, acquisition_limits()).expect("all configured publication roles");
    assert_eq!(acquired.sources().len(), 3);
    fs::write(
        project.directory.path().join("selected/guide.md"),
        "changed native source",
    )
    .expect("disposable post-capture edit");
    let corpus = acquired
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("one retained corpus");
    let corpus = corpus.corpus();
    assert_eq!(corpus.files().len(), 7);
    let corpus::File::Document(document) = &corpus.files()["project-docs/guide.md"] else {
        panic!("framed native documentation must retain its original parser form");
    };
    assert_eq!(document.source(), PROJECT_PAGE);
    assert!(corpus.files().contains_key("one/page.md"));
    assert!(corpus.files().contains_key("other/page.md"));
    assert!(matches!(
        acquire(&config, acquisition_limits()),
        Err(build::CompositionError::Source { .. })
    ));
}

#[test]
fn disabled_project_documentation_is_neither_captured_nor_mapped() {
    let (one, other) = fixtures();
    let config = format!(
        "{}{}",
        configuration(&one, &other),
        project_placement("source:unused", "disabled", false)
    );
    let acquired = acquire(
        &config,
        source::PreparationLimits {
            sources: 2,
            ..acquisition_limits()
        },
    )
    .expect("absent disabled native repository is not read");
    assert_eq!(acquired.sources().len(), 2);
    let corpus = acquired
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("selected roles only");
    let corpus = corpus.corpus();
    assert!(
        !corpus
            .files()
            .keys()
            .any(|path| path.as_str().starts_with("disabled/"))
    );
    let enabled = config.replace("render=false", "render=true");
    match acquire(&enabled, acquisition_limits()).expect_err("required docs cannot silently vanish")
    {
        build::CompositionError::Source { binding, .. } => {
            assert_eq!(binding.as_str(), "source:unused")
        }
        error => panic!("exact missing native source failure required: {error:?}"),
    }
}

#[test]
fn one_source_key_shared_by_authored_and_project_roles_is_captured_once() {
    let (one, other) = fixtures();
    let config = format!(
        "{}{}",
        configuration(&one, &other),
        project_placement("source:other", "project-docs", true)
    );
    let acquired = acquire(
        &config,
        source::PreparationLimits {
            sources: 2,
            ..acquisition_limits()
        },
    )
    .expect("two source keys, three content bindings");
    assert_eq!(acquired.sources().len(), 2);
    let corpus = acquired
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("both logical mappings");
    let corpus = corpus.corpus();
    let corpus::File::Document(authored) = &corpus.files()["other/page.md"] else {
        panic!("authored document");
    };
    let corpus::File::Document(project) = &corpus.files()["project-docs/page.md"] else {
        panic!("project document");
    };
    assert!(std::ptr::eq(
        authored.source().as_ptr(),
        project.source().as_ptr()
    ));
    assert_eq!(corpus.files().len(), 9);
    // Acquisition/mapping does not invent different semantic IDs for repeated content.
    // The later semantic compiler independently decides whether declarations conflict.
}

#[test]
fn project_and_authored_capture_share_source_count_and_payload_budget() {
    let (one, other) = fixtures();
    let project = Fixture::new(&[("guide.md", PROJECT_PAGE.as_bytes())]);
    let base = configuration(&one, &other);
    let config = format!(
        "{base}{}{}",
        project.declaration("source:project"),
        project_placement("source:project", "project-docs", true)
    );
    assert!(matches!(
        acquire(
            &config,
            source::PreparationLimits {
                sources: 2,
                ..acquisition_limits()
            }
        ),
        Err(build::CompositionError::TooManySources { limit: 2 })
    ));
    let authored_bytes = THEME_MANIFEST.len()
        + b"{{ content.title }}".len()
        + PAGE.len()
        + OTHER.len()
        + 3
        + b"invalid = [".len();
    let limit = source::PreparationLimits {
        bytes: authored_bytes as u64,
        ..acquisition_limits()
    };
    acquire(&base, limit).expect("exact authored and Theme-package budget");
    assert!(
        matches!(
            acquire(&config, limit),
            Err(build::CompositionError::Source { .. } | build::CompositionError::Aggregate { .. })
        ),
        "adding project bytes cannot receive a second independent allowance"
    );
}

#[test]
fn mixed_role_prefix_collisions_fail_before_unavailable_native_source_is_read() {
    let (one, other) = fixtures();
    for target in ["one", "one/nested"] {
        let config = format!(
            "{}{}",
            configuration(&one, &other),
            project_placement("source:unused", target, true)
        );
        assert!(
            matches!(
                acquire(&config, acquisition_limits()),
                Err(build::CompositionError::Configuration(
                    gazette_sdk::workspace::Error::Invalid(_)
                ))
            ),
            "logical overlap must be rejected before the nonexistent project source"
        );
    }
}

fn review_fixture() -> (Fixture, Fixture, String) {
    review_fixture_with(&[])
}

fn review_fixture_with(overrides: &[(&str, &[u8])]) -> (Fixture, Fixture, String) {
    let page = PAGE
        .replace(
            "[content]\n",
            "related=['content:foreign~/one']\n[content]\n",
        )
        .replace("title='One'", "title='One'\nroute={path='/review/'}")
        + "\n![Retained pixel](pixel.png)\n";
    let foreign = OTHER
        .replace("content:other", "content:foreign~/one")
        .replace(
            "title='Other'",
            "title='Foreign private title'\nroute={path='/foreign/'}",
        );
    let files = [
        ("theme.toml", THEME_MANIFEST.as_bytes()),
        ("templates/page.html", b"{{ content.title }}"),
        ("page.md", page.as_bytes()),
        ("pixel.png", b"\x89PNG\r\n\x1a\n\0retained"),
        (
            "unreferenced.pdf",
            b"%PDF-1.7\nunreferenced retained download",
        ),
        ("opaque.ipynb", br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"id":"one","cell_type":"code","metadata":{},"source":"raise RuntimeError('never executed')","execution_count":null,"outputs":[]}]}"#),
    ];
    let one = Fixture::new(
        &files
            .into_iter()
            .chain(overrides.iter().copied())
            .collect::<Vec<_>>(),
    );
    let other = Fixture::new(&[("page.md", foreign.as_bytes())]);
    let config = configuration(&one, &other)
        .replace(
            "site='site:one'\nauthored=[{source='source:one',target='one'}]",
            "site='site:one'\npublic_base_url='https://one.example/'\nauthored=[{source='source:one',target='one',static_assets={route_prefix='/media/'}}]",
        )
        .replace(
            "site='site:other'\nauthored=",
            "site='site:other'\npublic_base_url='https://other.example/'\nauthored=",
        );
    (one, other, config)
}

fn review_cli(config: &str, extra: &[&str]) -> Output {
    publication_cli(config, &[&["--review"], extra].concat())
}

fn publication_cli(config: &str, extra: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["check", "--stdin", "--publication", "site:one"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual review CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("configuration transport");
    child.wait_with_output().expect("review CLI result")
}

fn json_nodes(value: &Value) -> usize {
    1 + match value {
        Value::Array(items) => items.iter().map(json_nodes).sum(),
        Value::Object(items) => items.values().map(json_nodes).sum(),
        _ => 0,
    }
}

#[test]
fn review_preserves_exact_shared_surfaces_and_every_nested_use_without_admission() {
    let (one, other, config) = review_fixture();
    let inputs = acquire(&config, acquisition_limits()).expect("captured review inputs");
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("retained semantics");
    let review = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete private review request");
    let report = serde_json::to_value(&review).expect("review report");
    assert_eq!(report["request"]["format"], "gazette-publication-review/1");
    assert_eq!(report["request"]["site"], "site:one");
    assert_eq!(
        report["sha256"],
        serde_json::to_value(review.sha256()).expect("request identity")
    );
    assert_eq!(
        review.sha256(),
        source::Sha256Digest::of(
            &serde_json::to_vec(review.request()).expect("exact request bytes")
        )
    );
    assert_eq!(
        report["request"]["configuration_sha256"],
        serde_json::to_value(source::Sha256Digest::of(config.as_bytes()))
            .expect("exact configuration bytes")
    );
    assert_eq!(
        report["request"]["sources"]
            .as_object()
            .expect("captured source set")
            .len(),
        2
    );
    for (key, captured) in inputs.sources() {
        assert_eq!(
            report["request"]["sources"][key.as_str()],
            serde_json::to_value(captured.inventory().identity()).expect("captured tree identity")
        );
    }
    assert!(report.get("approved").is_none());
    assert!(report.get("authorization").is_none());
    let surfaces = review.request().surfaces();
    let page = surfaces
        .iter()
        .find(|surface| {
            serde_json::to_value(surface.target()).expect("target")
                == json!({"kind":"page","content":"content:one"})
        })
        .expect("explicit local page surface");
    let expected_page = serde_json::to_value(
        resolved
            .authored()
            .project_page(
                &"content:one".parse().expect("key"),
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("shared page producer"),
    )
    .expect("shared page payload");
    assert_eq!(*page.value(), expected_page);
    assert_eq!(page.value()["content"]["lifecycle"], "draft");
    assert_eq!(page.value()["related"][0]["id"], "content:foreign~/one");
    assert_eq!(page.value()["related"][0]["lifecycle"], "archived");
    assert_eq!(page.value()["related"][0]["title"], "Foreign private title");
    assert_eq!(
        page.value()["assets"][0]["url"],
        "https://one.example/media/pixel.png"
    );
    let graph = surfaces
        .iter()
        .find(|surface| {
            serde_json::to_value(surface.target()).expect("target") == json!({"kind":"site-graph"})
        })
        .expect("site graph surface");
    assert_eq!(
        *graph.value(),
        serde_json::to_value(
            resolved
                .authored()
                .project_site_graph(inputs.site(), Default::default())
                .expect("shared graph producer")
        )
        .expect("shared graph payload")
    );
    assert!(graph.value()["nodes"].get("content:foreign~/one").is_none());
    assert_eq!(
        graph.value()["references"]["content:foreign~/one"]["site"],
        "site:other"
    );
    let asset = surfaces
        .iter()
        .find(|surface| {
            serde_json::to_value(surface.target()).expect("target")
                == json!({"kind":"asset","path":"one/pixel.png"})
        })
        .expect("exact logical static source surface");
    assert_eq!(*asset.value(), page.value()["assets"][0]);
    assert_eq!(
        asset.value()["sha256"],
        serde_json::to_value(source::Sha256Digest::of(b"\x89PNG\r\n\x1a\n\0retained"))
            .expect("retained asset digest")
    );
    let uncompiled = surfaces
        .iter()
        .find(|surface| {
            serde_json::to_value(surface.target()).expect("target") == json!({"kind":"uncompiled"})
        })
        .expect("explicit uncompiled obligations");
    assert_eq!(
        *uncompiled.value(),
        serde_json::to_value(
            resolved
                .authored()
                .assets_for(inputs.site())
                .expect("shared asset plan")
                .uncompiled()
        )
        .expect("exact uncompiled source paths")
    );
    assert!(
        uncompiled
            .value()
            .as_array()
            .expect("uncompiled entries")
            .contains(&json!("one/opaque.ipynb"))
    );
    let composition = surfaces
        .iter()
        .find(|surface| {
            serde_json::to_value(surface.target()).expect("target") == json!({"kind":"composition"})
        })
        .expect("selected composition evidence");
    assert_eq!(
        *composition.value(),
        serde_json::to_value(
            inputs
                .compilation()
                .workspace
                .composition_for(inputs.site())
        )
        .expect("same effective selection owner")
    );
    assert_eq!(
        surfaces.len(),
        6,
        "foreign content is nested evidence, not an invented local page"
    );
    let unused = surfaces
        .iter()
        .find(|surface| {
            serde_json::to_value(surface.target()).expect("target")
                == json!({"kind":"asset","path":"one/unreferenced.pdf"})
        })
        .expect("unreferenced planned static bytes must remain reviewable");
    assert_eq!(
        unused.value()["url"],
        "https://one.example/media/unreferenced.pdf"
    );
    assert_eq!(
        unused.value()["sha256"],
        serde_json::to_value(source::Sha256Digest::of(
            b"%PDF-1.7\nunreferenced retained download"
        ))
        .expect("exact unreferenced bytes")
    );
    assert_eq!(
        page.value()["assets"]
            .as_array()
            .expect("page references")
            .len(),
        1
    );
    assert_ne!(page.value()["assets"][0]["url"], unused.value()["url"]);
    let mut escaped_foreign_reference = false;
    for surface in surfaces {
        let mut pointers = BTreeSet::new();
        let mut uses = 0;
        for usage in surface.uses() {
            uses += 1;
            assert!(
                pointers.insert(usage.pointer.clone()),
                "each exact use occurs once"
            );
            assert!(std::ptr::eq(
                surface
                    .value()
                    .pointer(&usage.pointer)
                    .expect("valid RFC 6901 pointer"),
                usage.value
            ));
            escaped_foreign_reference |= usage.pointer == "/references/content:foreign~0~1one/site";
        }
        assert!(
            pointers.contains(""),
            "root object/container is a review use"
        );
        assert_eq!(
            uses,
            json_nodes(surface.value()),
            "all containers and scalar uses retained"
        );
    }
    assert!(
        escaped_foreign_reference,
        "keys containing '~' and '/' require exact pointer escaping"
    );
    let checked = review_cli(&config, &[]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&checked.stdout).expect("actual CLI review"),
        report
    );
    let commented = format!("{config}\n# Same publication meaning; distinct exact input bytes.\n");
    let commented_inputs = acquire(&commented, acquisition_limits()).expect("same native pins");
    let commented_parsed = commented_inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("same syntax");
    let commented_resolved = commented_parsed
        .resolve(Default::default())
        .expect("same semantics");
    let commented_review = commented_resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("new configuration identity");
    assert_ne!(
        commented_review.sha256(),
        review.sha256(),
        "review binds literal configuration, not portable artifact identity"
    );
    let commented_report =
        serde_json::to_value(commented_review).expect("commented configuration review");
    assert_ne!(
        commented_report["request"]["configuration_sha256"],
        report["request"]["configuration_sha256"]
    );
    assert_eq!(
        commented_report["request"]["sources"],
        report["request"]["sources"]
    );
    assert_eq!(
        commented_report["request"]["surfaces"],
        report["request"]["surfaces"]
    );
    fs::write(
        one.directory.path().join("selected/page.md"),
        b"changed after capture",
    )
    .expect("isolated source mutation");
    fs::write(
        other.directory.path().join("selected/page.md"),
        b"changed foreign input",
    )
    .expect("isolated foreign mutation");
    assert_eq!(
        serde_json::to_value(
            resolved
                .review(
                    Default::default(),
                    &gazette_sdk::raster::inspect,
                    &gazette_adapter_markdown::Comrak
                )
                .expect("retained review repeat")
        )
        .expect("repeated report"),
        report
    );
    assert!(matches!(
        acquire(&config, acquisition_limits()),
        Err(build::CompositionError::Source { .. })
    ));
    let stale = review_cli(&config, &[]);
    assert!(!stale.status.success());
    assert!(stale.stdout.is_empty());
}

#[test]
fn review_limits_fail_closed_and_cli_requires_explicit_review_mode() {
    let (one, other, config) = review_fixture();
    let inputs = acquire(&config, acquisition_limits()).expect("review inputs");
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("syntax");
    let resolved = parsed.resolve(Default::default()).expect("semantics");
    let complete = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete request");
    let uses = complete
        .request()
        .surfaces()
        .iter()
        .map(|surface| surface.uses().count())
        .sum::<usize>();
    let pointers = complete
        .request()
        .surfaces()
        .iter()
        .flat_map(|surface| surface.uses())
        .map(|usage| usage.pointer.len())
        .sum::<usize>();
    let bytes = serde_json::to_vec(complete.request())
        .expect("request bytes")
        .len()
        .max(pointers);
    assert_eq!(
        resolved
            .review(
                build::ReviewLimits {
                    bytes,
                    uses,
                    graph: Default::default()
                },
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak
            )
            .expect("exact full request and pointer budget")
            .sha256(),
        complete.sha256()
    );
    for (bytes, uses) in [(bytes - 1, uses), (bytes, uses - 1)] {
        assert_eq!(
            resolved
                .review(
                    build::ReviewLimits {
                        bytes,
                        uses,
                        graph: Default::default()
                    },
                    &gazette_sdk::raster::inspect,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("no truncated inventory at exact boundary")
                .code(),
            "publication_review_limit"
        );
    }
    for (field, extra) in [
        ("bytes", ["--max-review-bytes", "1"]),
        ("uses", ["--max-review-uses", "1"]),
    ] {
        let mut limits = build::ReviewLimits::default();
        if field == "bytes" {
            limits.bytes = 1;
        } else {
            limits.uses = 1;
        }
        let error = resolved
            .review(
                limits,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("no incomplete successful review");
        let before = [&one, &other].map(|fixture| {
            source::inspect(
                &fixture.directory.path().join("selected"),
                None,
                source::Limits::default(),
            )
            .expect("before failed review")
            .identity()
        });
        let output = review_cli(&config, &extra);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "no partial review or digest");
        assert!(String::from_utf8_lossy(&output.stderr).contains(error.code()));
        for (fixture, identity) in [&one, &other].into_iter().zip(before) {
            assert_eq!(
                source::inspect(
                    &fixture.directory.path().join("selected"),
                    None,
                    source::Limits::default()
                )
                .expect("after failed review")
                .identity(),
                identity
            );
        }
    }
    for arguments in [
        vec!["check", "--review"],
        vec![
            "check",
            "--publication",
            "site:one",
            "--max-review-bytes",
            "1",
        ],
        vec![
            "check",
            "--publication",
            "site:one",
            "--max-review-uses",
            "1",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args(&arguments)
            .stdin(Stdio::null())
            .output()
            .expect("actual argument validation");
        assert!(
            !output.status.success(),
            "required review selector: {arguments:?}"
        );
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("required"));
    }
}

#[test]
fn review_projects_body_references_without_claiming_template_or_metadata_compilation() {
    let broken = format!("{PAGE}\n[Missing retained file](missing.pdf)\n");
    // Keep the native fixture guards alive for the complete acquisition/check.
    let (one, other, config) = review_fixture_with(&[("page.md", broken.as_bytes())]);
    let inputs = acquire(&config, acquisition_limits()).expect("complete retained bad-link input");
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("valid syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("structural graph remains valid");
    let checked = publication_cli(&config, &[]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let error = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("review cannot omit a failed body projection");
    assert_eq!(error.code(), "markdown_link_resolution");
    let before = [&one, &other].map(|fixture| {
        source::inspect(
            &fixture.directory.path().join("selected"),
            None,
            source::Limits::default(),
        )
        .expect("source before review failure")
        .identity()
    });
    let reviewed = review_cli(&config, &[]);
    assert!(!reviewed.status.success());
    assert!(reviewed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&reviewed.stderr).contains(error.code()));
    for (fixture, identity) in [&one, &other].into_iter().zip(before) {
        assert_eq!(
            source::inspect(
                &fixture.directory.path().join("selected"),
                None,
                source::Limits::default()
            )
            .expect("source after failed review")
            .identity(),
            identity
        );
    }
    let (_one, _other, config) =
        review_fixture_with(&[("templates/page.html", b"{% if broken".as_slice())]);
    let inputs = acquire(&config, acquisition_limits()).expect("captured invalid Theme package");
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("content syntax unaffected");
    let resolved = parsed
        .resolve(Default::default())
        .expect("valid semantic graph");
    let review = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("review does not execute Theme templates");
    let checked = review_cli(&config, &[]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&checked.stdout).expect("CLI review"),
        serde_json::to_value(review).expect("SDK review")
    );
    let error = build::render_resolved_publication(
        &resolved,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model output")),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect_err("review identity is not rendering readiness");
    assert!(matches!(error, build::PublicationRenderError::Template(_)));

    let (_one, _other, config) = review_fixture_with(&[("theme.toml", b"package = [".as_slice())]);
    assert!(matches!(
        acquire(&config, acquisition_limits()),
        Err(build::CompositionError::Capability(error))
            if error.code() == "toml_invalid"
    ));
}

#[test]
fn review_and_review_limits_conflict_with_every_legacy_check_selector() {
    for legacy in [
        vec!["--acquire-site", "site:one"],
        vec!["--acquire-composition", "site:one"],
        vec!["--parse-documents"],
        vec!["--check-composition"],
        vec!["--compile-templates"],
        vec!["--origin", "{}"],
        vec!["--composition-origin", "{}"],
    ] {
        for review in [
            vec!["--review"],
            vec!["--max-review-bytes", "1"],
            vec!["--max-review-uses", "1"],
            vec![
                "--review",
                "--max-review-bytes",
                "1",
                "--max-review-uses",
                "1",
            ],
        ] {
            // Deliberately omit --publication: explicit conflicts must survive
            // Clap's handling of indirect requirements, not just acquisition groups.
            let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg("check")
                .args(&legacy)
                .args(&review)
                .stdin(Stdio::null())
                .output()
                .expect("actual legacy/review conflict validation");
            assert!(!output.status.success(), "{legacy:?} + {review:?}");
            assert!(output.stdout.is_empty());
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            assert!(
                diagnostic.contains("cannot be used with"),
                "{legacy:?} + {review:?}: {diagnostic}"
            );
        }
    }
}
