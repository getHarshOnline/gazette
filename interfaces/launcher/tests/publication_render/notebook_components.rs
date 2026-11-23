//! Notebook components share page meaning, captured templates and the existing island path.

use super::*;
use gazette_sdk::{authored, publication::content::Disposition};
use serde_json::json;

const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n[components.note]\ntemplate='components/note.html'\narguments={type='object',required=['label'],additionalProperties=false,properties={label={type='string'}}}\n[components.counter]\ntemplate='components/note.html'\narguments={type='object',required=['label'],additionalProperties=false,properties={label={type='string'}}}\ninteraction={module='module:counter',export='mount',mount='load'}\n";
const PLUGIN: &str = "package='fixture-plugin'\n[resources.'module:counter']\ninputs=['counter.ts']\nbuild={kind='browser-module',entry='counter.ts'}\n";
const SELECTION: &str = "[composition.resources]\n'module:counter'={state='enabled',resource={owner={kind='plugin',package='fixture-plugin'},resource='module:counter'},output='/assets/counter.js',use='dependency'}\n";
const COMPONENT: &str = "<aside class=\"notebook-note\">{{ component.body.html | safe }}</aside>";
const TEMPLATE: &str = "<main>{{ body_html | safe }}</main>{% if notes is defined and notes.notebooks is defined %}<aside>{% for notebook in notes.notebooks %}{% for cell in notebook.cells %}{% if cell.kind == \"markdown\" %}{{ cell.html | safe }}{% else %}<details><summary>Show code</summary>{{ cell.source_html | safe }}</details>{{ cell.outputs_html | safe }}{% endif %}{% endfor %}{% endfor %}</aside>{% endif %}{% if presentation.interaction is defined %}<script type=\"module\" src=\"{{ presentation.interaction.url }}\"></script>{% endif %}";
const CELL: &str = "::::note {label='cell'}\n# Cell heading\n\n:::counter {label='nested'}\nNested **meaning**.\n:::\n::::\n";
const STORED: &str = ":::counter {label='stored'}\n# Output heading\n\nStored **meaning**.\n:::\n";

fn fixture(cell: &str, stored: &str) -> (Fixture, Vec<u8>) {
    let manifest = MANIFEST.replace(
        "render={page='page.html'}",
        "render={page='page.html',alias='page.html'}",
    );
    let notebook = serde_json::to_vec(&json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[
        {"id":"authored","cell_type":"markdown","metadata":{},"source":cell},
        {"id":"output","cell_type":"code","metadata":{},"execution_count":null,"source":"raise RuntimeError('never execute')","outputs":[
            {"output_type":"display_data","metadata":{},"data":{"text/markdown":stored}}
        ]}
    ]})).unwrap();
    let page = PAGE
        .replacen("+++\n", "+++\nnotebooks=['analysis.ipynb']\n", 1)
        .replace("lifecycle='draft'", "lifecycle='published'")
        .replace(
            "**Retained** body\n",
            ":::note {label='primary'}\nPrimary meaning.\n:::\n",
        );
    (
        Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("one/analysis.ipynb", &notebook),
            ("theme/theme.toml", manifest.as_bytes()),
            ("theme/plugin.toml", PLUGIN.as_bytes()),
            ("theme/templates/page.html", TEMPLATE.as_bytes()),
            ("theme/templates/components/note.html", COMPONENT.as_bytes()),
            (
                "theme/counter.ts",
                b"export function mount({element,props}) { element.textContent=props.label; }",
            ),
        ]),
        notebook,
    )
}

fn config(fixture: &Fixture, selection: &str) -> String {
    let mut config = fixture
        .config()
        .replacen(
            "[[principals]]",
            &format!("[composition.plugins]\n'fixture-plugin'={{state='enabled'}}\n{selection}[[principals]]"),
            1,
        )
        .replacen(
            "public_base_url=",
            "machine={search='lexical'}\npublic_base_url=",
            1,
        );
    config.push_str(
        "[[plugin_packages]]\npackage='fixture-plugin'\nsource='source:theme'\nmanifest='plugin.toml'\n",
    );
    config
}

#[test]
fn cells_and_stored_components_reach_shared_review_islands_cli_and_retained_rebuild() {
    let (fixture, notebook) = fixture(CELL, STORED);
    let config = config(&fixture, SELECTION);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete notebook component publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:one"];
    // The essay's own component stays on the page; notebook components join the Notes.
    let records: Vec<&Value> = page["components"]
        .as_array()
        .unwrap()
        .iter()
        .chain(page["notes"]["components"].as_array().unwrap())
        .collect();
    assert_eq!(page["components"].as_array().unwrap().len(), 1);
    assert_eq!(
        records
            .iter()
            .map(|item| item["arguments"]["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["primary", "cell", "nested", "stored"]
    );
    assert_eq!(
        records
            .iter()
            .filter(|item| item.get("interaction").is_some())
            .count(),
        2
    );
    let html = std::str::from_utf8(bytes(tree, "guide/index.html")).unwrap();
    assert!(html.contains(page["body_html"].as_str().unwrap()));
    assert_eq!(html.matches("class=\"notebook-note\"").count(), 4);
    assert_eq!(
        html.matches("data-gazette-component=\"counter\"").count(),
        2
    );
    assert!(html.contains("Nested <strong>meaning</strong>."));
    assert!(html.contains("Stored <strong>meaning</strong>."));
    let resource = &page["presentation"]["interaction"];
    let bootstrap = resource["output"].as_str().unwrap();
    assert_eq!(
        resource["references"],
        json!({"https://one.example/mount/assets/counter.js":"assets/counter.js"})
    );
    assert_eq!(html.matches("<script type=\"module\"").count(), 1);
    assert!(
        std::str::from_utf8(bytes(tree, bootstrap))
            .unwrap()
            .contains("https://one.example/mount/assets/counter.js")
    );
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    let search = search
        .iter()
        .find(|item| item["content"]["id"] == "content:one")
        .unwrap();
    assert_eq!(search["components"], page["components"]);
    assert_eq!(search["body"], page["body_text"]);
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    assert_eq!(
        evidence["pages"]["content:one"]["notebooks"][0]["source_sha256"],
        source::Sha256Digest::of(&notebook).to_string()
    );
    assert_eq!(evidence["islands"].as_object().unwrap().len(), 1);

    let parsed = build::parse_publication(&inputs, Default::default()).unwrap();
    let resolved = parsed.resolve(Default::default()).unwrap();
    let private = build::review(&resolved, Default::default()).unwrap();
    let selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: [("one/analysis.ipynb".parse().unwrap(), Disposition::Include)]
            .into_iter()
            .collect(),
    };
    let selected = resolved.into_selected(selection).unwrap();
    let candidate = build::review_selected(&selected, Default::default()).unwrap();
    for review in [&private, &candidate] {
        let reviewed = review.request().surfaces().iter().find(|surface| matches!(surface.target(), build::ReviewTarget::Page {content} if content.as_str() == "content:one")).unwrap().value();
        assert_eq!(reviewed["components"], page["components"]);
        assert_eq!(reviewed["body_text"], page["body_text"]);
        assert!(
            !reviewed["body_html"]
                .as_str()
                .unwrap()
                .contains("notebook-note"),
            "review does not execute Theme presentation"
        );
    }

    let parent = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["inventory"], json!(tree.inventory()));
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    for entry in tree.inventory().entries() {
        if let Some(expected) = tree.file(&entry.path) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).unwrap(),
                expected
            );
        }
    }
    fixture.directory.close().unwrap();
    let replay = render(&inputs).expect("retained sources, components and modules");
    assert_eq!(
        replay.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        bytes(replay.site().output(), "model.json"),
        bytes(tree, "model.json")
    );
    assert_eq!(
        bytes(replay.site().output(), bootstrap),
        bytes(tree, bootstrap)
    );
}

#[test]
fn invalid_cell_output_or_missing_module_refuses_before_staging() {
    for (cell, stored, selection) in [
        (
            ":::unknown\nNo inferred definition.\n:::\n",
            STORED,
            SELECTION,
        ),
        (
            CELL,
            ":::counter {label=42}\nBad stored arguments.\n:::\n",
            SELECTION,
        ),
        (CELL, STORED, ""),
    ] {
        let (fixture, _) = fixture(cell, stored);
        let config = config(&fixture, selection);
        assert!(render(&acquire(&config)).is_err());
        let parent = tempdir().unwrap();
        fs::write(parent.path().join("keep"), b"untouched").unwrap();
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend(["--output-parent", parent.path().to_str().unwrap()]);
        let output = cli(&config, &arguments);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
        assert_eq!(fs::read(parent.path().join("keep")).unwrap(), b"untouched");
    }
}
