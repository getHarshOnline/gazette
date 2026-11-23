//! Explicit local graph views share the resolved publication model, not a public admission path.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml};
use git2::{IndexAddOption, Oid, Repository, Signature};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html',graph='graph.json'}\n";
const PAGE_TEMPLATE: &str = "<h1>{{ content.title }}</h1>{{ body_html | safe }}";
// Tera's native context dump observes its complete input without a test-side
// graph resolver or a second list of fields to keep in sync.
const GRAPH_TEMPLATE: &str = "{{ __tera_context }}";
const GRAPH_HTML: &str = "<h1>{{ site }}</h1>{% for id, node in nodes %}<p>{{ id }}:{{ node.lifecycle }}:{{ node.title }}</p>{% endfor %}{% for id, target in references %}<a href=\"{{ target.canonical_url }}\">{{ target.title }}</a>{% endfor %}";
const HOME: &str = "+++\nprojects=[{project='project:context',kind='context'}]\n[content]\nid='collection:home'\nsite='site:one'\ntitle='Home'\nlifecycle='published'\nroute={path='/'}\nstructure={kind='collection'}\n+++\n# Local home\n";
const SECTION: &str = "+++\n[content]\nid='collection:section'\nsite='site:one'\ntitle='Section'\nlifecycle='archived'\nroute={path='/section/'}\nstructure={kind='collection',parent='collection:home'}\n+++\n# Local section\n";
const PAGE: &str = "+++\nrelated=['content:foreign']\nprojects=[{project='project:subject',kind='about'}]\n[content]\nid='content:page'\nsite='site:one'\ntitle='Draft <escaped> Ω'\nlifecycle='draft'\nattributions=[{principal='person:author',role='author'},{principal='person:rights',role='rights-holder'}]\nroute={path='/article/',aliases=['/old.html']}\nstructure={kind='page',family={kind='article'}}\n+++\n# Local page body\n";
const JOURNEY: &str = "+++\n[content]\nid='journey:reading'\nsite='site:one'\ntitle='Read in order'\nlifecycle='published'\nroute={path='/reading/'}\nstructure={kind='journey',collection='collection:section',members=[{content='content:guide',primary=true},{content='content:page',primary=true}]}\n+++\n# Local journey\n";
const GUIDE: &str = "+++\n[content]\nid='content:guide'\nsite='site:one'\ntitle='Local guide'\nlifecycle='archived'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\n# Local guide body\n";
const DIRECT: &str = "+++\ncollections=['collection:section']\n[content]\nid='content:direct'\nsite='site:one'\ntitle='Direct collection member'\nlifecycle='published'\nroute={path='/direct/'}\nstructure={kind='page',family={kind='utility'}}\n+++\n# Direct member body\n";
const FOREIGN: &str = "+++\n[content]\nid='content:foreign'\nsite='site:other'\ntitle='FOREIGN TITLE SECRET'\nlifecycle='archived'\nattributions=[{principal='person:foreign',role='author'}]\nroute={path='/external/'}\nstructure={kind='page',family={kind='documentation'}}\n+++\n# FOREIGN BODY SECRET\n";
// SDK and CLI comparisons select the same year instead of consulting the CLI clock.
const RENDER_YEAR: u16 = 2040;
const ARGUMENTS: [&str; 8] = [
    "render-site",
    "--local",
    "--site",
    "site:one",
    "--model-output",
    "model.json",
    "--render-year",
    "2040",
];

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new(overrides: &[(&str, &[u8])]) -> Self {
        let root = tempdir().expect("isolated native repository");
        let repository = Repository::init(root.path()).expect("fixture Git");
        for (path, bytes) in [
            ("theme/manifest.toml", MANIFEST.as_bytes()),
            ("theme/templates/page.html", PAGE_TEMPLATE.as_bytes()),
            ("theme/templates/alias.html", b"Moved".as_slice()),
            ("theme/templates/graph.json", GRAPH_TEMPLATE.as_bytes()),
            ("theme/templates/graph.html", GRAPH_HTML.as_bytes()),
            ("one/home.md", HOME.as_bytes()),
            ("one/section.md", SECTION.as_bytes()),
            ("one/page.md", PAGE.as_bytes()),
            ("one/journey.md", JOURNEY.as_bytes()),
            ("one/guide.md", GUIDE.as_bytes()),
            ("one/direct.md", DIRECT.as_bytes()),
            ("other/page.md", FOREIGN.as_bytes()),
        ]
        .into_iter()
        .chain(overrides.iter().copied())
        {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("fixture directories");
            fs::write(path, bytes).expect("fixture bytes");
        }
        let mut index = repository.index().expect("index");
        index
            .add_all(["theme", "one", "other"], IndexAddOption::DEFAULT, None)
            .expect("fixture files");
        let tree = repository
            .find_tree(index.write_tree().expect("tree identity"))
            .expect("tree");
        let signature =
            Signature::new("Fixture", "fixture@example.invalid", &git2::Time::new(0, 0))
                .expect("fixed signature");
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "graph fixture",
                &tree,
                &[],
            )
            .expect("isolated commit");
        Self { root, commit }
    }

    fn config(&self) -> String {
        let mut config = String::from("[composition]\ntheme={package='theme:one'}\n");
        for id in [
            "owner:publisher",
            "person:author",
            "person:rights",
            "person:foreign",
            "person:unused",
        ] {
            config.push_str(&format!("[[principals]]\nid='{id}'\nkind='person'\n"));
        }
        for id in ["one", "other"] {
            config.push_str(&format!(
                "[[sites]]\nid='site:{id}'\npublisher='owner:publisher'\ndeployable=false\n"
            ));
        }
        for id in ["subject", "context", "unused"] {
            config.push_str(&format!("[[projects]]\nid='project:{id}'\nlifecycle='done'\nvisibility='private'\navailability='unavailable'\n"));
        }
        config.push_str("[[brands]]\nid='brand:one'\nname='Fixture'\n");
        for (site, base) in [
            ("one", "https://one.example/mount/"),
            ("other", "https://other.example/"),
        ] {
            config.push_str(&format!("[[brands.publications]]\nsite='site:{site}'\npublic_base_url='{base}'\nauthored=[{{source='source:{site}',target='{site}'}}]\n"));
        }
        for name in ["theme", "one", "other"] {
            config.push_str(&format!("[[sources]]\nid='source:{name}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='{name}'}}\n",self.root.path().to_str().expect("root"),self.commit));
        }
        config.push_str("[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='manifest.toml'\n");
        config
    }
}

fn acquire(config: &str) -> build::AcquiredInputs {
    build::acquire_publication(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 3,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("complete captured inputs")
}

fn render<'a>(
    inputs: &'a build::AcquiredInputs,
    graph: Option<(&str, &str)>,
) -> Result<build::PublicationRender<'a>, build::PublicationRenderError> {
    let output =
        graph.map(|(_, output)| output.parse::<source::EntryPath>().expect("graph output"));
    build::render_publication(
        inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(RENDER_YEAR).unwrap()),
            model_output: Some(&"model.json".parse().expect("model output")),
            graph: output.as_ref().map(|output| build::GraphRenderOptions {
                output,
                limits: Default::default(),
            }),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
}

fn bytes<'a>(tree: &'a source::CapturedSource, path: &str) -> &'a [u8] {
    tree.file(&path.parse().expect("output path"))
        .expect("expected output")
}

fn cli(config: &str, parent: &Path, extra: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(ARGUMENTS)
        .args(extra)
        .args(["--output-parent", parent.to_str().expect("parent")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(config.as_bytes())
        .expect("config transport");
    child.wait_with_output().expect("CLI completion")
}

fn assert_cli_failure(config: &str, extra: &[&str], code: &str) {
    let parent = tempdir().expect("explicit output parent");
    fs::write(parent.path().join("unrelated.txt"), b"preserve").expect("unrelated fixture");
    let before = source::inspect(parent.path(), None, source::Limits::default()).expect("before");
    let output = cli(config, parent.path(), extra);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no success receipt");
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with(&format!("gazette: {code}")),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        source::inspect(parent.path(), None, source::Limits::default())
            .expect("after")
            .identity(),
        before.identity(),
        "no stage or unrelated changes"
    );
}

#[test]
fn selected_graph_is_the_exact_template_value_and_retained_site_model() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let rendered =
        render(&inputs, Some(("graph.json", "views/graph.json"))).expect("complete local graph");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("machine model");
    let graph = &model["graph"];
    assert_eq!(
        serde_json::from_slice::<Value>(bytes(tree, "views/graph.json"))
            .expect("actual graph template context"),
        *graph
    );
    assert_eq!(graph["format"], "gazette-discovery-site-graph/1");
    assert_eq!(graph["site"], "site:one");
    assert_eq!(graph["publisher"], "owner:publisher");
    assert_eq!(graph["nodes"].as_object().expect("local nodes").len(), 6);
    for (id, page) in model["pages"].as_object().expect("pages") {
        assert_eq!(graph["nodes"][id], page["content"]);
        assert_eq!(graph["routes"][id], page["route"]);
        assert_eq!(graph["provenance"][id], page["provenance"]);
    }
    assert_eq!(graph["nodes"]["content:page"]["lifecycle"], "draft");
    assert_eq!(
        graph["nodes"]["collection:section"]["lifecycle"],
        "archived"
    );
    assert_eq!(
        graph["references"]["content:foreign"],
        json!({
            "id":"content:foreign",
            "site":"site:other",
            "title":"FOREIGN TITLE SECRET",
            "canonical_url":"https://other.example/external/",
            "presentation":{"theme":"theme:one"}
        })
    );
    assert_eq!(
        graph["related"],
        json!([{"source":"content:page","target":"content:foreign"}])
    );
    assert_eq!(
        graph["memberships"],
        json!([{"content":"content:direct","collection":"collection:section"}])
    );
    assert_eq!(
        graph["containers"],
        json!([{"child":"collection:section","parent":"collection:home"},{"child":"journey:reading","parent":"collection:section"}])
    );
    let steps = graph["journeys"]
        .as_array()
        .expect("ordered traversal edges");
    assert_eq!(
        steps
            .iter()
            .map(|step| step["content"].as_str().expect("member"))
            .collect::<Vec<_>>(),
        ["content:guide", "content:page"]
    );
    assert_eq!(steps[0]["primary"], true);
    assert_eq!(steps[1]["primary"], true);
    assert_eq!(steps[0]["position"], 0);
    assert_eq!(steps[1]["position"], 1);
    assert_eq!(
        graph["project_links"]["content:page"]["project:subject"]["relation"],
        "about"
    );
    assert_eq!(
        graph["project_links"]["content:page"]["project:context"]["relation"],
        "mentions"
    );
    for id in ["project:context", "project:subject"] {
        assert_eq!(
            graph["projects"][id],
            json!({"id":id,"lifecycle":"done","visibility":"private","availability":"unavailable"})
        );
    }
    assert_eq!(
        graph["principals"]
            .as_object()
            .expect("scoped principals")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["owner:publisher", "person:author", "person:rights"]
    );
    let serialized = serde_json::to_string(graph).expect("graph bytes");
    for excluded in [
        "FOREIGN BODY SECRET",
        "person:foreign",
        "person:unused",
        "project:unused",
        "body_html",
        "source:theme",
        fixture.root.path().to_str().expect("root"),
    ] {
        assert!(
            !serialized.contains(excluded),
            "unscoped graph value: {excluded}"
        );
    }
    assert!(
        tree.file(&"external/index.html".parse().expect("foreign output"))
            .is_none()
    );
    let parent = tempdir().expect("retained parent");
    for _ in 0..2 {
        let output = cli(
            &config,
            parent.path(),
            &["--graph-output", "views/graph.json"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).expect("local receipt");
        assert_eq!(receipt["status"], "local_retained");
        let retained = Path::new(receipt["directory"].as_str().expect("retained location"));
        assert_eq!(
            source::inspect(retained, None, source::Limits::default())
                .expect("readback")
                .identity(),
            tree.inventory().identity()
        );
        for entry in tree.inventory().entries() {
            if matches!(entry.content, source::Content::File { .. }) {
                assert_eq!(
                    fs::read(retained.join(entry.path.as_str())).expect("retained bytes"),
                    tree.file(&entry.path).expect("SDK bytes")
                );
            }
        }
    }
    fs::write(
        fixture.root.path().join("one/page.md"),
        b"changed after capture",
    )
    .expect("isolated source change");
    fs::write(
        fixture.root.path().join("theme/templates/graph.json"),
        b"changed after capture",
    )
    .expect("isolated template change");
    assert_eq!(
        render(&inputs, Some(("graph.json", "views/graph.json")))
            .expect("retained source graph")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    assert_cli_failure(
        &config,
        &["--graph-output", "views/graph.json"],
        "source_content_mismatch",
    );
}

#[test]
fn human_graph_view_and_absent_selection_preserve_local_mode() {
    let manifest = MANIFEST.replace("graph='graph.json'", "graph='graph.html'");
    let foreign = FOREIGN.replace("FOREIGN TITLE SECRET", "Foreign <em> & Ω");
    let fixture = Fixture::new(&[
        ("theme/manifest.toml", manifest.as_bytes()),
        ("other/page.md", foreign.as_bytes()),
    ]);
    let config = fixture.config();
    let parent = tempdir().expect("output parent");
    let output = cli(
        &config,
        parent.path(),
        &["--graph-output", "graph/index.html"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
    let retained = Path::new(receipt["directory"].as_str().expect("directory"));
    let html = fs::read_to_string(retained.join("graph/index.html")).expect("human graph");
    assert!(html.contains("content:page:draft:Draft &lt;escaped&gt; Ω"));
    assert!(
        html.replace("&#x2F;", "/")
            .contains("href=\"https://other.example/external/\"")
    );
    assert!(html.contains("Foreign &lt;em&gt; &amp; Ω"));
    assert!(!html.contains("Foreign <em>"));
    assert!(!html.contains("FOREIGN BODY SECRET"));
    let inputs = acquire(&config);
    let without = render(&inputs, None).expect("unchanged local rendering");
    let model: Value =
        serde_json::from_slice(bytes(without.site().output(), "model.json")).expect("model");
    assert!(
        model.get("graph").is_none(),
        "not a null or implicitly selected graph"
    );
    assert!(
        without
            .site()
            .output()
            .file(&"graph/index.html".parse().expect("graph output"))
            .is_none()
    );
}

#[test]
fn graph_output_rejects_removed_template_selection_and_cannot_bypass_local_acknowledgement() {
    let removed = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(ARGUMENTS)
        .args(["--graph-template", "graph.json"])
        .stdin(Stdio::null())
        .output()
        .expect("clap check");
    assert!(!removed.status.success());
    assert!(removed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&removed.stderr).contains("unexpected argument"));
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(ARGUMENTS.into_iter().filter(|arg| *arg != "--local"))
        .args(["--graph-output", "graph.json"])
        .stdin(Stdio::null())
        .output()
        .expect("local acknowledgement check");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--local"));
}

#[test]
fn graph_output_collisions_and_template_failures_leave_no_stage() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    for output in [
        "index.html",
        "old.html",
        "model.json",
        "model.json/graph.json",
        "article",
    ] {
        assert_cli_failure(&config, &["--graph-output", output], "source_inventory");
    }
    let missing = MANIFEST.replace("graph='graph.json'", "graph='missing.html'");
    let fixture = Fixture::new(&[("theme/manifest.toml", missing.as_bytes())]);
    assert_cli_failure(
        &fixture.config(),
        &["--graph-output", "graph.html"],
        "template_binding",
    );
    let fixture = Fixture::new(&[("theme/templates/graph.json", b"{{ nonexistent.field }}")]);
    assert_cli_failure(
        &fixture.config(),
        &["--graph-output", "graph.json"],
        "render_",
    );
}

#[test]
fn graph_expansion_and_generated_tree_remain_independently_bounded() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let mut graph = build::GraphRenderOptions {
        output: &"graph.json".parse().expect("output"),
        limits: Default::default(),
    };
    graph.limits.nodes = 0;
    let error = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.json".parse().expect("model")),
            graph: Some(graph),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect_err("bounded graph expansion");
    assert_eq!(error.code(), "graph_limit");
    assert_cli_failure(
        &config,
        &["--graph-output", "graph.json", "--max-total-bytes", "1"],
        "",
    );
}

const TAXONOMIES: &str = "[[taxonomies]]\nid='taxonomy:topic'\nlabel='主題 <&>'\nlanguage='en-US'\nterms=[{id='term:shared',label='工程 Ω'}]\n[[taxonomies]]\nid='taxonomy:audience'\nlabel='पाठक'\nterms=[{id='term:shared',label='Readers & writers'}]\n[[taxonomies]]\nid='taxonomy:foreign'\nlabel='FOREIGN CATALOG LABEL'\nlanguage='ja'\nterms=[{id='term:foreign',label='FOREIGN TERM LABEL'}]\n[[taxonomies]]\nid='taxonomy:unused'\nlabel='UNUSED CATALOG LABEL'\nterms=[{id='term:unused',label='UNUSED TERM LABEL'}]\n";

#[test]
fn language_and_composite_taxonomies_share_theme_page_and_graph_without_inheritance() {
    for related in [true, false] {
        assert_language_and_composite_taxonomies(related);
    }
}

fn assert_language_and_composite_taxonomies(related: bool) {
    // Deliberately reversed declarations: the compiler owns normalized ordering.
    // Identical term keys in different taxonomies retain different identities/labels.
    let declarations = "[{taxonomy='taxonomy:topic',term='term:shared'},{taxonomy='taxonomy:audience',term='term:shared'}]";
    let page = PAGE.replace(
        "[content]\n",
        &format!("[content]\nlanguage='EN-us'\ntaxonomies={declarations}\n"),
    );
    let page = if related {
        page
    } else {
        page.replace("related=['content:foreign']\n", "")
    };
    let home = HOME.replace(
        "[content]\n",
        "[content]\nlanguage='hi'\ntaxonomies=[{taxonomy='taxonomy:audience',term='term:shared'}]\n",
    );
    let foreign = FOREIGN.replace(
        "[content]\n",
        "[content]\nlanguage='ja'\ntaxonomies=[{taxonomy='taxonomy:foreign',term='term:foreign'}]\n",
    );
    let template = "<!doctype html><html{% if content.language %} lang=\"{{ content.language }}\"{% endif %}><body>{{ body_html | safe }}{% for item in classifications %}<p data-taxonomy=\"{{ item.taxonomy.id }}\" data-term=\"{{ item.term.id }}\">{{ item.taxonomy.label }}:{{ item.term.label }}</p>{% endfor %}</body></html>";
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/home.md", home.as_bytes()),
        ("other/page.md", foreign.as_bytes()),
        ("theme/templates/page.html", template.as_bytes()),
    ]);
    let large_label = format!("主題 <&>{}", " Ω".repeat(512));
    let catalog = TAXONOMIES.replace("主題 <&>", &large_label);
    let config = format!("{}{catalog}", fixture.config());
    let inputs = acquire(&config);
    let rendered = render(&inputs, Some(("graph.json", "graph.json")))
        .expect("shared language and taxonomy resolution");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("local model");
    let graph = &model["graph"];
    let expected = json!([
        {"taxonomy":{"id":"taxonomy:audience","label":"पाठक"},"term":{"id":"term:shared","label":"Readers & writers"}},
        {"taxonomy":{"id":"taxonomy:topic","label":large_label,"language":"en-US"},"term":{"id":"term:shared","label":"工程 Ω"}}
    ]);
    let projected = &model["pages"]["content:page"];
    let foreign_recommendations: Vec<_> = projected["recommendations"]["items"]
        .as_array()
        .expect("ranked recommendations")
        .iter()
        .filter(|item| item["id"] == "content:foreign")
        .collect();
    if related {
        assert_eq!(foreign_recommendations.len(), 1);
        let recommendation = foreign_recommendations[0];
        assert_eq!(recommendation["site"], "site:other");
        assert_eq!(recommendation["title"], "FOREIGN TITLE SECRET");
        assert_eq!(recommendation["language"], "ja");
        assert_eq!(recommendation["score"], 120);
        assert_eq!(recommendation["reasons"], json!(["outgoing-related"]));
        assert_eq!(
            recommendation["route"]["url"],
            "https://other.example/external/"
        );
        assert_eq!(
            recommendation["route"]["canonical_url"],
            "https://other.example/external/"
        );
        assert_eq!(recommendation["destination"]["kind"], "cross-site");
        assert_eq!(recommendation["destination"]["content"], "content:foreign");
        assert_eq!(recommendation["destination"]["site"], "site:other");
        assert_eq!(
            recommendation["destination"]["relationships"],
            json!(["related"])
        );
        assert_eq!(
            recommendation["classifications"],
            json!([
                {"taxonomy":{"id":"taxonomy:foreign","label":"FOREIGN CATALOG LABEL","language":"ja"},"term":{"id":"term:foreign","label":"FOREIGN TERM LABEL"}}
            ])
        );
    } else {
        assert!(foreign_recommendations.is_empty());
    }
    assert_eq!(projected["content"]["language"], "en-US");
    assert_eq!(projected["classifications"], expected);
    assert_eq!(
        projected["content"]["taxonomies"],
        json!([
            {"taxonomy":"taxonomy:audience","term":"term:shared"},
            {"taxonomy":"taxonomy:topic","term":"term:shared"}
        ])
    );
    for (id, page_model) in model["pages"].as_object().expect("local pages") {
        assert_eq!(graph["nodes"][id], page_model["content"]);
        assert_eq!(graph["classifications"][id], page_model["classifications"]);
        assert!(
            page_model["classifications"]
                .as_array()
                .expect("local classifications")
                .iter()
                .all(|item| item["taxonomy"]["id"] != "taxonomy:foreign")
        );
    }
    for id in ["collection:section", "journey:reading"] {
        assert!(model["pages"][id]["content"]["language"].is_null());
        assert_eq!(model["pages"][id]["classifications"], json!([]));
    }
    assert_eq!(
        model["pages"]["collection:home"]["content"]["language"],
        "hi"
    );
    assert_eq!(
        model["pages"]["collection:home"]["classifications"]
            .as_array()
            .expect("own classification")
            .len(),
        1
    );
    assert!(graph["classifications"].get("content:foreign").is_none());
    assert_eq!(
        serde_json::from_slice::<Value>(bytes(tree, "graph.json")).expect("Theme graph context"),
        *graph
    );
    let html = std::str::from_utf8(bytes(tree, "article/index.html")).expect("page HTML");
    assert!(html.contains("<html lang=\"en-US\">"));
    assert!(html.contains("data-taxonomy=\"taxonomy:audience\" data-term=\"term:shared\">पाठक:Readers &amp; writers</p>"));
    assert!(html.contains(&format!(
        "data-taxonomy=\"taxonomy:topic\" data-term=\"term:shared\">主題 &lt;&amp;&gt;{}:工程 Ω</p>",
        " Ω".repeat(512)
    )));
    // Count the emitted records, not the workspace's unused catalog. Every
    // assignment emits two records and one edge, including repeated terms.
    let assignments = graph["classifications"]
        .as_object()
        .expect("classifications")
        .values()
        .map(|items| items.as_array().expect("assignments").len())
        .sum::<usize>();
    assert_eq!(assignments, 3);
    let nodes = ["nodes", "references", "projects", "principals"]
        .into_iter()
        .map(|field| graph[field].as_object().expect("emitted records").len())
        .sum::<usize>()
        + 2 * assignments;
    let edges = [
        "memberships",
        "related",
        "containers",
        "journeys",
        "declarations",
    ]
    .into_iter()
    .map(|field| graph[field].as_array().expect("emitted edges").len())
    .sum::<usize>()
        + graph["provenance"].as_object().expect("source edges").len()
        + graph["nodes"]
            .as_object()
            .expect("content nodes")
            .values()
            .map(|node| {
                node.get("attributions")
                    .map_or(0, |items| items.as_array().expect("attributions").len())
            })
            .sum::<usize>()
        + graph["project_links"]
            .as_object()
            .expect("project relationships")
            .values()
            .flat_map(|links| links.as_object().expect("project links").values())
            .map(|link| 1 + link["evidence"].as_array().expect("link evidence").len())
            .sum::<usize>()
        + assignments;
    // The graph ceiling belongs to the compiler-owned semantic graph. The
    // application adds the independently bounded presentation envelope only
    // after that graph has passed its own limits.
    let mut semantic_graph = graph.clone();
    semantic_graph
        .as_object_mut()
        .expect("graph object")
        .remove("presentation");
    let encoded_bytes = serde_json::to_vec(&semantic_graph)
        .expect("exact graph encoding")
        .len();
    let bounded = |nodes, edges, text_bytes| {
        let mut graph = build::GraphRenderOptions {
            output: &"graph.json".parse().expect("graph output"),
            limits: Default::default(),
        };
        graph.limits.nodes = nodes;
        graph.limits.edges = edges;
        graph.limits.text_bytes = text_bytes;
        build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(gazette_sdk::authored::RenderYear::new(RENDER_YEAR).unwrap()),
                model_output: Some(&"model.json".parse().expect("model output")),
                graph: Some(graph),
                limits: source::Limits::default(),
            },
            Default::default(),
        )
    };
    assert_eq!(
        bounded(nodes, edges, encoded_bytes)
            .expect("exact complete graph budget includes large Unicode labels")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    for (nodes, edges, text_bytes) in [
        (nodes - 1, edges, encoded_bytes),
        (nodes, edges - 1, encoded_bytes),
        (nodes, edges, encoded_bytes - 1),
    ] {
        let outcome = bounded(nodes, edges, text_bytes);
        assert!(
            outcome
                .as_ref()
                .is_err_and(|error| error.code() == "graph_limit"),
            "classification records and labels cannot escape graph limits; accepted nodes={nodes}, edges={edges}, text_bytes={text_bytes}"
        );
    }
    assert!(
        !std::str::from_utf8(bytes(tree, "section/index.html"))
            .expect("unclassified section")
            .contains(" lang=")
    );
    let machine = std::str::from_utf8(bytes(tree, "model.json")).expect("machine bytes");
    for foreign_label in ["FOREIGN CATALOG LABEL", "FOREIGN TERM LABEL"] {
        assert_eq!(machine.contains(foreign_label), related);
        assert!(!graph.to_string().contains(foreign_label));
        assert!(!html.contains(foreign_label));
    }
    for excluded in [
        "FOREIGN BODY SECRET",
        "UNUSED CATALOG LABEL",
        "UNUSED TERM LABEL",
    ] {
        assert!(
            !machine.contains(excluded),
            "unselected catalog leaked: {excluded}"
        );
        assert!(!html.contains(excluded));
    }
    // Source metadata is read, not rewritten to its normalized projection.
    assert_eq!(
        fs::read(fixture.root.path().join("one/page.md")).expect("source unchanged"),
        page.as_bytes()
    );
    assert_eq!(
        fs::read(fixture.root.path().join("one/home.md")).expect("source unchanged"),
        home.as_bytes()
    );
    let parent = tempdir().expect("retained output parent");
    let output = cli(&config, parent.path(), &["--graph-output", "graph.json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("actual CLI receipt");
    let retained = Path::new(receipt["directory"].as_str().expect("retained location"));
    assert_eq!(
        source::inspect(retained, None, source::Limits::default())
            .expect("read-back")
            .identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        fs::read(retained.join("model.json")).expect("CLI machine bytes"),
        bytes(tree, "model.json")
    );
    assert_eq!(
        fs::read(retained.join("article/index.html")).expect("CLI HTML bytes"),
        html.as_bytes()
    );
    assert_eq!(
        fs::read(retained.join("graph.json")).expect("CLI graph bytes"),
        bytes(tree, "graph.json")
    );
    fs::write(
        fixture.root.path().join("one/page.md"),
        b"language changed after capture",
    )
    .expect("isolated mutation");
    assert_eq!(
        render(&inputs, Some(("graph.json", "graph.json")))
            .expect("retained classified corpus")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
}

#[test]
fn late_taxonomy_failures_do_not_invent_selection_or_leave_partial_output() {
    for (metadata, diagnostic) in [
        (
            "language='en-US'\ntaxonomies=[{taxonomy='taxonomy:topic',term='term:shared'},{taxonomy='taxonomy:topic',term='term:shared'}]\n",
            "content_duplicate_taxonomy",
        ),
        (
            "language='en-US'\ntaxonomies=[{taxonomy='taxonomy:missing',term='term:shared'}]\n",
            "content_taxonomy_reference",
        ),
        (
            "language='en-US'\ntaxonomies=[{taxonomy='taxonomy:topic',term='term:Shared'}]\n",
            "content_taxonomy_term_reference",
        ),
        (
            "language='en-GB'\ntaxonomies=[{taxonomy='taxonomy:topic',term='term:shared'}]\n",
            "content_taxonomy_language",
        ),
        (
            "taxonomies=[{taxonomy='taxonomy:topic',term='term:shared'}]\n",
            "content_taxonomy_language",
        ),
    ] {
        let late = PAGE
            .replace("id='content:page'", "id='content:late'")
            .replace(
                "route={path='/article/',aliases=['/old.html']}",
                "route={path='/late/'}",
            )
            .replace("[content]\n", &format!("[content]\n{metadata}"));
        let fixture = Fixture::new(&[("one/zz-invalid.md", late.as_bytes())]);
        let config = format!("{}{TAXONOMIES}", fixture.config());
        let inputs = acquire(&config);
        let error = render(&inputs, Some(("graph.json", "graph.json")))
            .expect_err("whole input must fail, not substitute another taxonomy or term");
        assert!(
            error.to_string().contains(diagnostic),
            "{diagnostic}: {error}"
        );
        assert_cli_failure(
            &config,
            &["--graph-output", "graph.json"],
            "publication_authored",
        );
        assert_eq!(
            fs::read(fixture.root.path().join("one/zz-invalid.md"))
                .expect("invalid source unchanged"),
            late.as_bytes()
        );
    }
}

#[test]
fn graph_presentation_reuses_publication_metadata_and_only_emitted_destinations() {
    let fixture = Fixture::new(&[]);
    let base = fixture.config().replace("site='site:one'\npublic_base_url=", "site='site:one'\nhome='collection:home'\ncontacts={discovery='hello@example.test'}\npublic_base_url=");
    for enabled in [true, false] {
        let config = base.replace(
            "home='collection:home'",
            &format!(
                "machine.graph={{enabled={enabled},path='/connections/'}}\nhome='collection:home'"
            ),
        );
        let inputs = acquire(&config);
        let rendered = render(&inputs, None).expect("configured graph selection");
        let model: Value =
            serde_json::from_slice(bytes(rendered.site().output(), "model.json")).expect("model");
        for page in model["pages"].as_object().expect("pages").values() {
            if enabled {
                assert_eq!(
                    page["presentation"]["graph"]["url"],
                    "https://one.example/mount/connections/"
                );
            } else {
                assert!(page["presentation"].get("graph").is_none());
            }
        }
        if enabled {
            let metadata = &model["graph"]["presentation"]["publication"];
            assert_eq!(
                metadata,
                &model["pages"]["content:page"]["site"]["metadata"]
            );
            assert_eq!(metadata["brand"]["name"], "Fixture");
            assert_eq!(metadata["home"]["url"], "https://one.example/mount/");
            assert_eq!(metadata["contacts"]["discovery"], "hello@example.test");
            assert_eq!(model["graph"]["site"], "site:one");
            assert!(
                rendered
                    .site()
                    .output()
                    .file(&"connections/index.html".parse().expect("path"))
                    .is_some()
            );
        } else {
            assert!(model.get("graph").is_none());
        }
    }
    let inputs = acquire(&base);
    let rendered =
        render(&inputs, Some(("graph.json", "views/connections.json"))).expect("explicit output");
    let model: Value =
        serde_json::from_slice(bytes(rendered.site().output(), "model.json")).expect("model");
    assert_eq!(
        model["pages"]["content:page"]["presentation"]["graph"]["url"],
        "https://one.example/mount/views/connections.json"
    );
    let absent = render(&inputs, None).expect("unselected graph");
    let model: Value =
        serde_json::from_slice(bytes(absent.site().output(), "model.json")).expect("model");
    assert!(
        model["pages"]["content:page"]["presentation"]
            .get("graph")
            .is_none()
    );
}

#[test]
fn supplied_render_year_and_independent_rights_holder_reach_page_and_graph_metadata() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config().replace(
        "id='site:one'\n",
        "id='site:one'\nrights_holder='person:rights'\n",
    );
    let inputs = acquire(&config);
    for year in [2040, 2041] {
        let rendered = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(gazette_sdk::authored::RenderYear::new(year).unwrap()),
                model_output: Some(&"model.json".parse().unwrap()),
                graph: Some(build::GraphRenderOptions {
                    output: &"views/graph.json".parse().unwrap(),
                    limits: Default::default(),
                }),
                limits: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
        let model: Value =
            serde_json::from_slice(bytes(rendered.site().output(), "model.json")).unwrap();
        let page = &model["pages"]["content:page"]["site"]["metadata"];
        let graph = &model["graph"]["presentation"]["publication"];
        assert_eq!(page, graph);
        assert_eq!(page["render_year"], year);
        assert_eq!(page["rights_holder"]["id"], "person:rights");
        assert_ne!(
            page["rights_holder"]["id"],
            model["pages"]["content:page"]["site"]["publisher"]
        );
    }
}
