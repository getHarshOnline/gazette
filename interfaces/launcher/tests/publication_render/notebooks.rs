//! Retained static notebook attachments through real CLI/SDK publication rendering.

#[path = "notebooks/markdown_output.rs"]
mod markdown_output;

#[path = "notebooks/html_output.rs"]
mod html_output;

use gazette_sdk::{authored, publication::content::Disposition};
use serde_json::json;

use super::{
    Fixture, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, Value, acquire,
    assert_cli_failure_preserves_parent, build, bytes, cli, fs, render, source, tempdir,
};

/// The notebook sections the shared fixture template composes from `notes.notebooks`.
pub(super) fn notebook_sections(page: &Value) -> String {
    let mut html = String::new();
    for notebook in page["notes"]["notebooks"].as_array().into_iter().flatten() {
        html.push_str("<section class=\"gazette-notebook\">");
        for cell in notebook["cells"].as_array().expect("notebook cells") {
            let id = cell["cell_id"].as_str().expect("cell ID");
            if cell["kind"] == "markdown" {
                html.push_str(&format!(
                    "<div class=\"notebook-markdown\" data-cell-id=\"{id}\">{}</div>",
                    cell["html"].as_str().expect("Markdown cell HTML")
                ));
            } else {
                html.push_str(&format!(
                    "<div class=\"notebook-code\" data-cell-id=\"{id}\">{}{}</div>",
                    cell["source_html"].as_str().expect("code source HTML"),
                    cell["outputs_html"].as_str().expect("stored outputs HTML")
                ));
            }
        }
        html.push_str("</section>");
    }
    html
}

fn notebook(cells: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({"nbformat":4,"nbformat_minor":5,"metadata":{
        "authors":[{"name":"PRIVATE NOTEBOOK AUTHOR"}],
        "kernelspec":{"name":"absent-kernel","display_name":"Must not launch"},
        "private_path":"/private/notebook-metadata"
    },"cells":cells}))
    .expect("valid retained notebook JSON")
}

fn markdown(id: &str, text: &str) -> Value {
    json!({"cell_type":"markdown","id":id,"metadata":{},"source":text})
}

fn attach(page: &str, paths: &[&str]) -> String {
    page.replacen(
        "+++\n",
        &format!(
            "+++\nnotebooks={}\n",
            serde_json::to_string(paths).expect("TOML-compatible string array")
        ),
        1,
    )
}

fn machine(config: &str) -> String {
    config.replace(
        "public_base_url=",
        "machine={search='lexical'}\npublic_base_url=",
    )
}

fn assert_shared(tree: &source::CapturedSource, id: &str, output: &str, tokens: &[&str]) {
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("machine page model");
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("shared search documents");
    let page = &model["pages"][id];
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == id)
        .expect("owning page search document");
    let html = std::str::from_utf8(bytes(tree, output)).expect("native rendered HTML");
    let body_html = page["body_html"].as_str().expect("shared rendered body");
    let body_text = page["body_text"].as_str().expect("shared retained text");
    assert!(
        html.contains(body_html),
        "Theme receives exact compiled body"
    );
    assert_eq!(record["body"], body_text);
    assert_eq!(record["content"], page["content"]);
    // Notebooks join the Notes: their text is the Notes text, their HTML the cells.
    let notes_text = page["notes"]["body_text"].as_str().expect("Notes text");
    assert_eq!(record["notes"]["body_text"], notes_text);
    assert!(
        record["notes"].get("notebooks").is_none(),
        "search keeps Notes text, not notebook cells"
    );
    let cells: String = page["notes"]["notebooks"]
        .as_array()
        .expect("structured notebooks")
        .iter()
        .flat_map(|notebook| notebook["cells"].as_array().expect("cells"))
        .flat_map(|cell| {
            ["html", "source_html", "outputs_html"].map(|key| cell[key].as_str().unwrap_or(""))
        })
        .collect();
    for token in tokens {
        assert!(
            notes_text.contains(token),
            "missing retained text {token}: {notes_text}"
        );
        assert!(cells.contains(token), "missing rendered text {token}");
    }
    assert!(
        !body_html.contains("notebook"),
        "the essay holds only the essay"
    );
    assert!(
        html.contains(&notebook_sections(page)),
        "Theme receives the structured cells"
    );
    assert!(
        !record
            .as_object()
            .expect("search object")
            .contains_key("body_html")
    );
    assert!(
        !record["body"]
            .as_str()
            .expect("search text")
            .contains("<pre")
    );
    for private in [
        "PRIVATE NOTEBOOK AUTHOR",
        "/private/notebook-metadata",
        "absent-kernel",
    ] {
        assert!(
            !serde_json::to_string(&model)
                .expect("model bytes")
                .contains(private)
        );
        assert!(!html.contains(private));
    }
    assert!(
        tree.inventory()
            .entries()
            .iter()
            .all(|entry| !entry.path.as_str().ends_with(".ipynb")),
        "no implicit raw notebook download"
    );
}

#[test]
fn ordered_notebooks_reach_shared_body_search_cli_and_stage_without_execution_or_rereads() {
    let sentinel_parent = tempdir().expect("private execution sentinel parent");
    let sentinel = sentinel_parent.path().join("MUST_NOT_EXIST");
    let code = format!(
        "from pathlib import Path\nPath({:?}).write_text('executed')\nraise RuntimeError('MUST NOT EXECUTE')\n",
        sentinel.to_str().expect("sentinel path")
    );
    let second = notebook(vec![
        markdown("intro", "# Second notebook first\n\n**Stored analysis**.\n"),
        json!({"cell_type":"code","id":"code","metadata":{},"source":code,"execution_count":null,"outputs":[
            {"output_type":"stream","name":"stdout","text":["stdout first\n","stdout second\n"]},
            {"output_type":"stream","name":"stderr","text":"stderr retained\n<script>globalThis.BAD=1</script>\n"},
            {"output_type":"error","ename":"StoredError","evalue":"stored error value","traceback":["stored trace line"]},
            {"output_type":"display_data","metadata":{},"data":{"text/plain":"plain retained result","application/json":{"retained_answer":42}}}
        ]}),
    ]);
    let first = notebook(vec![markdown(
        "intro",
        "# First notebook second\n\nFinal retained cell.\n",
    )]);
    let foreign = notebook(vec![markdown("foreign", "FOREIGN NOTEBOOK BODY")]);
    let page = attach(PAGE, &["second.ipynb", "first.ipynb"]);
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/second.ipynb", &second),
        ("one/first.ipynb", &first),
        ("other/foreign.ipynb", &foreign),
    ]);
    let config = machine(&fixture.config());
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete supported static notebooks");
    let tree = rendered.site().output();
    assert_shared(
        tree,
        "content:one",
        "guide/index.html",
        &[
            "Second notebook first",
            "First notebook second",
            "stdout first",
            "stdout second",
            "stderr retained",
            "stored error value",
            "plain retained result",
            "retained_answer",
        ],
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("body model");
    assert_eq!(
        model["pages"]
            .as_object()
            .expect("original page identities")
            .len(),
        2
    );
    let text = model["pages"]["content:one"]["notes"]["body_text"]
        .as_str()
        .expect("complete Notes text");
    assert!(
        text.contains("<script>globalThis.BAD=1</script>"),
        "stored output remains literal searchable text"
    );
    let page_html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("rendered page");
    assert!(
        !page_html.contains("<script>globalThis.BAD=1</script>"),
        "text output never becomes executable HTML"
    );
    let mut scripts = 0;
    gazette_sdk::html::inspect(page_html.as_bytes(), Default::default(), |kind, _| {
        if matches!(
            kind,
            gazette_sdk::html::TextKind::Script
                | gazette_sdk::html::TextKind::ModuleScript
                | gazette_sdk::html::TextKind::DataScript
                | gazette_sdk::html::TextKind::JsonScript
        ) {
            scripts += 1;
        }
        Ok::<_, std::convert::Infallible>(())
    })
    .expect("actual generated HTML token contexts");
    assert_eq!(
        scripts, 0,
        "static notebook introduces no script execution context"
    );
    let ordered = [
        "Second notebook first",
        "stdout first",
        "stdout second",
        "stderr retained",
        "stored error value",
        "First notebook second",
    ];
    let mut cursor = 0;
    for token in ordered {
        cursor += text[cursor..]
            .find(token)
            .expect("notebook, cell and output order")
            + token.len();
    }
    for entry in tree.inventory().entries() {
        if let Some(bytes) = tree.file(&entry.path) {
            assert!(!String::from_utf8_lossy(bytes).contains("FOREIGN NOTEBOOK BODY"));
        }
    }
    assert!(
        !sentinel.exists(),
        "code source was displayed, never executed"
    );
    let parent = tempdir().expect("native stage parent");
    let staged = build::stage_publication(&rendered, parent.path(), Default::default(), 16 << 10)
        .expect("real SDK stage");
    assert_eq!(staged.inventory().identity(), tree.inventory().identity());
    assert_eq!(
        fs::read(staged.path().join("guide/index.html")).expect("native page read-back"),
        bytes(tree, "guide/index.html")
    );
    staged.close().expect("release disposable stage");
    let cli_parent = tempdir().expect("direct CLI stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        cli_parent.path().to_str().expect("CLI parent"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("real CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory =
        std::path::Path::new(receipt["directory"].as_str().expect("CLI stage directory"));
    assert_eq!(
        fs::read(directory.join("model.json")).expect("CLI model read-back"),
        bytes(tree, "model.json")
    );
    assert!(!sentinel.exists());
    fs::write(
        fixture.directory.path().join("one/second.ipynb"),
        b"changed after capture",
    )
    .expect("disposable source mutation");
    assert_eq!(
        render(&inputs)
            .expect("retained notebook rebuild")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn project_document_attachments_use_capture_root_not_page_directory_or_other_source() {
    let content = notebook(vec![markdown("native", "Native notebook facts\n")]);
    let page = attach(PROJECT_DOC, &["analysis.ipynb"]);
    let fixture = Fixture::new(&[
        ("project/nested/guide.md", page.as_bytes()),
        ("project/analysis.ipynb", &content),
    ]);
    let inputs = acquire(&machine(&fixture.project_config(true)));
    let rendered = render(&inputs).expect("capture-relative project notebook binding");
    assert_shared(
        rendered.site().output(),
        "doc:native",
        "docs/native/index.html",
        &["Native notebook facts"],
    );
    let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
        .expect("native model");
    assert_eq!(
        model["pages"]["doc:native"]["provenance"]["project"],
        "project:native"
    );
    assert_eq!(model["pages"]["doc:native"]["content"]["id"], "doc:native");
}

#[test]
fn missing_wrong_kind_repeated_and_cross_source_attachments_fail_before_any_stage() {
    let content = notebook(vec![markdown("foreign", "Foreign only")]);
    for declarations in [
        vec!["missing.ipynb"],
        vec!["wrong.txt"],
        vec!["foreign.ipynb"],
        vec!["good.ipynb", "good.ipynb"],
        vec!["../other/foreign.ipynb"],
        vec!["/absolute.ipynb"],
    ] {
        let page = attach(PAGE, &declarations);
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("one/wrong.txt", b"not a notebook"),
            ("one/good.ipynb", &content),
            ("other/foreign.ipynb", &content),
        ]);
        let config = fixture.config();
        assert_eq!(
            render(&acquire(&config))
                .expect_err("no attachment fallback")
                .code(),
            "publication_authored"
        );
        assert_cli_failure_preserves_parent(&config, "publication_authored");
    }
}

#[test]
fn candidate_must_include_bound_notebook_and_excluded_owner_cannot_discharge_unbound_inputs() {
    let content = notebook(vec![markdown("selected", "Selected notebook")]);
    let page = attach(
        &PAGE.replace("lifecycle='draft'", "lifecycle='published'"),
        &["analysis.ipynb"],
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis.ipynb", &content),
    ]);
    let inputs = acquire(&fixture.config());
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("whole bound graph");
    let mut selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: [(
            "one/analysis.ipynb".parse().expect("logical notebook"),
            Disposition::Exclude,
        )]
        .into_iter()
        .collect(),
    };
    assert_eq!(
        resolved
            .into_selected(selection.clone())
            .expect_err("included owner needs included notebook")
            .code(),
        "selection_asset"
    );
    selection
        .content
        .insert("content:one".parse().expect("owner"), Disposition::Exclude);
    let resolved = parsed
        .resolve(Default::default())
        .expect("same retained syntax");
    let selected = resolved
        .into_selected(selection)
        .expect("owner and notebook explicitly excluded");
    let result = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("excluded notebook has no render obligation");
    assert!(
        result
            .site()
            .output()
            .file(&"guide/index.html".parse().expect("excluded owner route"))
            .is_none()
    );
    let unbound = Fixture::new(&[("one/analysis.ipynb", &content)]);
    assert_cli_failure_preserves_parent(&unbound.config(), "site_uncompiled_asset");
}

#[test]
fn unsupported_later_cells_fail_whole_render_without_placeholder_or_raw_download() {
    let page = attach(PAGE, &["analysis.ipynb"]);
    let mut unsupported = vec![
        json!({"cell_type":"raw","id":"late","metadata":{},"source":"RAW MUST NOT BECOME HTML"}),
        json!({"cell_type":"markdown","id":"late","metadata":{},"source":"![plot](attachment:plot.svg)","attachments":{"plot.svg":{"image/svg+xml":"<svg/>"}}}),
    ];
    for mime in [
        "text/html",
        "image/svg+xml",
        "application/vnd.jupyter.widget-view+json",
    ] {
        let value = if mime.ends_with("+json") {
            json!({"model_id":"must-not-run"})
        } else {
            json!("<script>must_not_run()</script>")
        };
        let mut data = serde_json::Map::new();
        data.insert(
            "text/plain".into(),
            json!("TEXT FALLBACK MUST NOT HIDE UNSUPPORTED REPRESENTATION"),
        );
        data.insert(mime.into(), value);
        unsupported.push(json!({"cell_type":"code","id":"late","metadata":{},"source":"raise RuntimeError('must not execute')","execution_count":null,"outputs":[{"output_type":"display_data","metadata":{},"data":data}]}));
    }
    for late in unsupported {
        let content = notebook(vec![
            markdown(
                "first",
                "# Valid earlier cell\n\nEarlier output is not a partial success.\n",
            ),
            late,
        ]);
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("one/analysis.ipynb", &content),
        ]);
        let config = fixture.config();
        let inputs = acquire(&config);
        inputs
            .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
            .expect("unsupported rendering is still valid retained notebook syntax");
        let error = render(&inputs)
            .expect_err("later unsupported representation invalidates complete rendered site");
        assert_eq!(error.code(), "notebook_render_unsupported", "{error}");
        assert_cli_failure_preserves_parent(&config, "notebook_render_unsupported");
    }
    let local_reference = notebook(vec![markdown("local", "[Unbound](local.md)\n")]);
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis.ipynb", &local_reference),
    ]);
    assert_cli_failure_preserves_parent(&fixture.config(), "markdown_link_resolution");
}

#[test]
fn notebook_companions_use_notebook_directory_shared_assets_and_retained_bytes() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==").expect("stored PNG fixture");
    let csv = b"name,value\nretained,42\n";
    let content = notebook(vec![
        markdown(
            "first",
            "![Retained plot](images/plot.png) [Retained data](r%C3%A9sults.csv#row)",
        ),
        markdown(
            "again",
            "![Repeated plot](images/plot.png) [Again](r%C3%A9sults.csv)",
        ),
    ]);
    let page = attach(PAGE, &["analysis/results.ipynb"]);
    let native = attach(PROJECT_DOC, &["analysis/results.ipynb"]);
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis/results.ipynb", &content),
        ("one/analysis/images/plot.png", &png),
        ("one/analysis/résults.csv", csv),
        ("one/images/plot.png", b"wrong page-relative bytes"),
        ("project/nested/guide.md", native.as_bytes()),
        ("project/analysis/results.ipynb", &content),
        ("project/analysis/images/plot.png", &png),
        ("project/analysis/résults.csv", csv),
    ]);
    let config = machine(&super::static_configuration(&fixture, true));
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("both exact source-bound notebook companion paths");
    let tree = rendered.site().output();
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("shared machine model");
    for (id, prefix, output) in [
        ("content:one", "authored", "guide/index.html"),
        ("doc:native", "native", "docs/native/index.html"),
    ] {
        assert_shared(
            tree,
            id,
            output,
            &["Retained plot", "Retained data", "Repeated plot"],
        );
        let assets = model["pages"][id]["assets"]
            .as_array()
            .expect("page asset relationships");
        assert_eq!(
            assets.len(),
            2,
            "same companion across cells is one relationship"
        );
        // Companion links live in the notebook cells, which now sit in Notes.
        let body = &notebook_sections(&model["pages"][id]);
        for asset in assets {
            let url = asset["url"].as_str().expect("asset address");
            assert_eq!(asset, &model["assets"][url]);
            assert!(url.starts_with(&format!(
                "https://one.example/mount/media/{prefix}/analysis/"
            )));
            assert!(body.contains(url));
        }
        assert_eq!(
            bytes(tree, &format!("media/{prefix}/analysis/images/plot.png")),
            png
        );
        assert_eq!(
            bytes(tree, &format!("media/{prefix}/analysis/résults.csv")),
            csv
        );
        assert!(!body.contains("/media/authored/images/plot.png"));
    }
    let parent = tempdir().expect("disposable stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("staged directory"));
    assert_eq!(
        fs::read(directory.join("media/authored/analysis/images/plot.png"))
            .expect("native asset read-back"),
        png
    );
    fs::write(
        fixture
            .directory
            .path()
            .join("one/analysis/images/plot.png"),
        b"changed after capture",
    )
    .expect("disposable source mutation");
    assert_eq!(
        render(&inputs)
            .expect("no native source reread")
            .site()
            .output()
            .inventory()
            .identity(),
        tree.inventory().identity()
    );
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn unmapped_escaping_foreign_and_excluded_notebook_companions_fail_without_fallback() {
    let page = attach(
        &PAGE.replace("lifecycle='draft'", "lifecycle='published'"),
        &["analysis/results.ipynb"],
    );
    for destination in [
        "missing.png",
        "foreign.png",
        "../../other/foreign.png",
        "%2e%2e/foreign.png",
        "images%2fplot.png",
        "raw.svg",
        "local.md",
    ] {
        let content = notebook(vec![markdown(
            "local",
            &format!("![Companion]({destination})"),
        )]);
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("one/analysis/results.ipynb", &content),
            ("other/analysis/foreign.png", b"foreign-only"),
        ]);
        assert_cli_failure_preserves_parent(
            &super::static_configuration(&fixture, false),
            "markdown_link_resolution",
        );
    }
    let content = notebook(vec![markdown("local", "[Data](data.csv)")]);
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis/results.ipynb", &content),
        ("one/analysis/data.csv", b"retained,42\n"),
    ]);
    assert_cli_failure_preserves_parent(&fixture.config(), "markdown_link_resolution");
    let inputs = acquire(&super::static_configuration(&fixture, false));
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed.resolve(Default::default()).expect("bound graph");
    let selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: [
            (
                "one/analysis/results.ipynb".parse().expect("notebook"),
                Disposition::Include,
            ),
            (
                "one/analysis/data.csv".parse().expect("companion"),
                Disposition::Exclude,
            ),
        ]
        .into_iter()
        .collect(),
    };
    let selected = resolved
        .into_selected(selection)
        .expect("notebook cells resolved during projection");
    let error = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect_err("excluded companion cannot re-enter selected output");
    assert_eq!(error.code(), "markdown_link_resolution");
}

#[test]
fn notebook_namespace_rejects_heading_collisions_but_not_encoded_footnote_labels() {
    let content = notebook(vec![markdown("heading", "# Notebook heading\n")]);
    let page = attach(PAGE, &["analysis.ipynb"]);
    let seed = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis.ipynb", &content),
    ]);
    let inputs = acquire(&seed.config());
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained seed syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("actual configured notebook binding");
    let owner = "content:one".parse().expect("authored owner");
    let bound = resolved
        .authored()
        .notebooks(&owner)
        .expect("ordered resolved attachments");
    assert_eq!(bound.len(), 1);
    let namespace = format!(
        "gazette-notebook-{}-",
        source::Sha256Digest::of(bound[0].path.as_str().as_bytes())
    );
    let generated_id = format!("{namespace}-cell-0-notebook-heading");
    let projected = resolved
        .authored()
        .project_page(
            &owner,
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("nonconflicting actual notebook projection");
    let notes = serde_json::to_value(projected.data.notes.as_ref().expect("notebook Notes"))
        .expect("Notes model");
    assert!(
        notebook_sections(&json!({ "notes": notes })).contains(&format!("id=\"{generated_id}\"")),
        "the negative fixture targets a real generated anchor"
    );
    let conflicting = format!("{page}\n# Authored collision {{#{generated_id}}}\n");
    let fixture = Fixture::new(&[
        ("one/page.md", conflicting.as_bytes()),
        ("one/analysis.ipynb", &content),
    ]);
    let config = fixture.config();
    let error = render(&acquire(&config))
        .expect_err("primary anchors cannot collide with a notebook namespace");
    assert_eq!(error.code(), "authored_projection_binding", "{error}");
    assert_cli_failure_preserves_parent(&config, "authored_projection_binding");

    // A source label is not its DOM ID: encoded footnotes and notebook headings
    // must coexist, without restoring predecessor aliases or dropping either.
    let footnote = format!(
        "{page}\nAuthored footnote[^{generated_id}].\n\n[^{generated_id}]: Retained footnote.\n"
    );
    let fixture = Fixture::new(&[
        ("one/page.md", footnote.as_bytes()),
        ("one/analysis.ipynb", &content),
    ]);
    let config = fixture.config();
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("distinct actual anchor namespaces");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let essay = model["pages"]["content:one"]["body_html"]
        .as_str()
        .expect("body");
    let notebook = notebook_sections(&model["pages"]["content:one"]);
    // The essay's footnotes and the notebook's heading share one page document.
    let body = &format!("{essay}{notebook}");
    let encoded: String = generated_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    for id in [
        &generated_id,
        &format!("fn-note-{encoded}"),
        &format!("fnref-note-{encoded}"),
    ] {
        assert_eq!(
            body.matches(&format!("id=\"{id}\"")).count(),
            1,
            "unique {id}"
        );
    }
    assert!(body.contains(&format!("href=\"#fn-note-{encoded}\"")));
    assert!(body.contains(&format!("href=\"#fnref-note-{encoded}\"")));
    let theme = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme HTML");
    assert!(theme.contains(essay) && theme.contains(&notebook));
    let output = cli(&config, &RENDER_ARGUMENTS);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
}
