//! Notebook math uses the same explicit acquisition and retained publication path.

use super::*;

fn fixture() -> Fixture {
    with_cells(json!([
        {"cell_type":"markdown", "id":"formula-cell", "metadata":{},
         "source":"Notebook formula $x_1$ and `$literal$`.\n\n```math\ny^2\n```\n"},
        {"cell_type":"code", "id":"stored-output", "metadata":{},
         "source":"print('$not-executed$')", "execution_count":1,
         "outputs":[{"output_type":"display_data", "metadata":{},
                     "data":{"text/markdown":"Stored formula $z_3$.\n"}}]}
    ]))
}

fn with_cells(cells: Value) -> Fixture {
    let notebook = serde_json::to_vec(&json!({
        "nbformat":4, "nbformat_minor":5, "metadata":{}, "cells":cells
    }))
    .expect("valid retained notebook");
    let page = PAGE.replacen("+++\n", "+++\nnotebooks=['analysis.ipynb']\n", 1);
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis.ipynb", &notebook),
        (
            "math/render.mjs",
            include_bytes!("../../../../../adapters/math/render.mjs"),
        ),
        (
            "math/vendor/katex-0.18.4/katex.mjs",
            include_bytes!("../../../../../adapters/math/vendor/katex-0.18.4/katex.mjs"),
        ),
    ])
}

#[test]
fn native_batch_boundary_within_a_notebook_cell_preserves_following_cell_results() {
    let first = (0..258)
        .map(|i| format!("$x_{{{i}}}$\n\n"))
        .collect::<String>();
    let second = (258..260)
        .map(|i| format!("$x_{{{i}}}$\n\n"))
        .collect::<String>();
    let fixture = with_cells(json!([
        {"cell_type":"markdown", "id":"large-cell", "metadata":{}, "source":first},
        {"cell_type":"markdown", "id":"following-cell", "metadata":{}, "source":second}
    ]));
    let directory = tempdir().expect("disposable typesetter cwd");
    let command = command(directory.path());
    let mut inputs =
        super::super::sites::acquire_set(&config(&fixture, Some(&command)), &["site:one"]);
    build::acquire_math(&mut inputs, Default::default(), permitted())
        .expect("both real native batches are retained");
    let calls: Vec<_> = inputs.math_capture().unwrap().invocations().collect();
    assert_eq!(calls.len(), 2);
    let mut observed = Vec::new();
    for (batch, (request, response, _, exit)) in calls.iter().enumerate() {
        assert_eq!(exit.code, Some(0));
        let request: Value = serde_json::from_str(request).expect("complete batch request");
        let response: Value = serde_json::from_str(response).expect("complete batch response");
        let formulas = request["formulas"].as_array().unwrap();
        assert_eq!(formulas.len(), if batch == 0 { 256 } else { 4 });
        assert_eq!(response["mathml"].as_array().unwrap().len(), formulas.len());
        observed.extend(formulas.iter().cloned());
    }
    assert_eq!(
        observed,
        (0..260)
            .map(|i| json!({"tex":format!("x_{{{i}}}"), "display":false}))
            .collect::<Vec<_>>()
    );
    let site = inputs.sites()[&"site:one".parse::<Key>().unwrap()]
        .as_ref()
        .expect("complete site capture");
    let rendered = render(site).expect("complete cross-batch notebook rendering");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:one"];
    let html = super::super::notebooks::notebook_sections(page);
    assert_eq!(html.matches("<math ").count(), 260);
    for i in 0..260 {
        assert!(html.contains(&format!(
            "<annotation encoding=\"application/x-tex\">x_{{{i}}}</annotation>"
        )));
    }
    assert!(
        std::str::from_utf8(bytes(tree, "guide/index.html"))
            .unwrap()
            .contains(&html)
    );
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    assert_eq!(
        search
            .iter()
            .find(|record| record["content"]["id"] == "content:one")
            .unwrap()["body"],
        page["body_text"]
    );
}

fn assert_shared(tree: &source::CapturedSource, selected: bool) {
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let page = &model["pages"]["content:one"];
    // Notebook math reaches the Notes cells and Notes text, not the essay body.
    let html = &super::super::notebooks::notebook_sections(page);
    let text = page["notes"]["body_text"]
        .as_str()
        .expect("shared Notes text");
    let theme = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme HTML");
    assert!(theme.contains(html));
    assert!(html.contains("<code>$literal$</code>"));
    assert!(html.contains("$not-executed$"));
    for formula in ["x_1", "y^2", "z_3"] {
        assert!(text.contains(formula), "missing formula text {formula}");
    }
    assert!(!text.contains("<math"));
    if selected {
        assert_eq!(html.matches("<math ").count(), 3);
        for formula in ["x_1", "y^2\n", "z_3"] {
            assert!(html.contains(&format!(
                "<annotation encoding=\"application/x-tex\">{formula}</annotation>"
            )));
        }
        assert!(!html.contains("language-math"));
    } else {
        assert!(!html.contains("<math"));
        assert!(html.contains("$x_1$"));
        assert!(html.contains("$z_3$"));
        assert!(html.contains("language-math"));
    }
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("search records");
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == "content:one")
        .expect("owning page search record");
    assert_eq!(record["notes"]["body_text"], text);
    assert!(tree.file(&"analysis.ipynb".parse().unwrap()).is_none());
}

#[test]
fn selected_notebook_cells_and_stored_markdown_share_math_capture_and_retained_outputs() {
    let fixture = fixture();
    let runtime_directory = tempdir().expect("disposable typesetter working directory");
    let command = command(runtime_directory.path());
    let mut inputs =
        super::super::sites::acquire_set(&config(&fixture, Some(&command)), &["site:one"]);
    build::acquire_math(&mut inputs, Default::default(), permitted())
        .expect("native Node/KaTeX acquisition includes notebook Markdown");
    let capture = inputs.math_capture().expect("one retained math capture");
    let calls: Vec<_> = capture.invocations().collect();
    assert_eq!(calls.len(), 1);
    let (request, response, _, exit) = calls[0];
    assert_eq!(exit.code, Some(0));
    let request: Value = serde_json::from_str(request).expect("exact native request");
    assert_eq!(
        request,
        json!({"formulas":[
            {"tex":"x_1", "display":false},
            {"tex":"y^2\n", "display":true},
            {"tex":"z_3", "display":false}
        ]})
    );
    let response: Value = serde_json::from_str(response).expect("complete native response");
    assert_eq!(response["mathml"].as_array().unwrap().len(), 3);
    let site = inputs.sites()[&"site:one".parse::<Key>().unwrap()]
        .as_ref()
        .expect("selected site");
    let rendered = render(site).expect("captured notebook math renders");
    let tree = rendered.site().output();
    assert_shared(tree, true);

    runtime_directory
        .close()
        .expect("retire only disposable cwd");
    assert!(!Path::new(&command.directory).exists());
    let replay = render(site).expect("no native reread or process during compilation");
    let replay_tree = replay.site().output();
    assert_eq!(
        tree.inventory().identity(),
        replay_tree.inventory().identity()
    );
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(tree.file(&entry.path), replay_tree.file(&entry.path));
        }
    }
    assert_shared(replay_tree, true);
    assert_eq!(inputs.math_capture().unwrap().invocations().count(), 1);
}

#[test]
fn unselected_notebook_math_stays_literal_without_a_typesetter() {
    let fixture = fixture();
    let config = config(&fixture, None).replace(",markdown_profile='publication-math'", "");
    let mut inputs = super::super::sites::acquire_set(&config, &["site:one"]);
    build::acquire_math(&mut inputs, Default::default(), Default::default())
        .expect("unselected notebook math needs no process permission or typesetter");
    assert!(inputs.math_capture().is_none());
    let site = inputs.sites()[&"site:one".parse::<Key>().unwrap()]
        .as_ref()
        .expect("selected site");
    let rendered = render(site).expect("literal notebook publication");
    assert_shared(rendered.site().output(), false);
}

#[test]
fn selected_notebook_math_requires_typesetter_and_cannot_render_without_capture() {
    let fixture = fixture();
    let mut inputs = super::super::sites::acquire_set(&config(&fixture, None), &["site:one"]);
    let error = build::acquire_math(&mut inputs, Default::default(), permitted())
        .expect_err("notebook-only formulas still require an explicitly configured typesetter");
    assert_eq!(error.code(), "math_unconfigured");
    assert_eq!(error.completed().count(), 0);
    assert!(error.active_request().is_none());
    assert!(inputs.math_capture().is_none());
    let site = inputs.sites()[&"site:one".parse::<Key>().unwrap()]
        .as_ref()
        .expect("retained selected site");
    assert!(
        render(site).is_err(),
        "selected math cannot become literal fallback"
    );
}
