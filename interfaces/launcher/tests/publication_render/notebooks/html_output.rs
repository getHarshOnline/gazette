//! Stored inert HTML follows the retained notebook and shared publication projections.

use super::*;

fn output(html: &str) -> Vec<u8> {
    notebook(vec![json!({"cell_type":"code","id":"table","metadata":{},
    "source":"raise RuntimeError('MUST NOT EXECUTE')","execution_count":null,
    "outputs":[{"output_type":"display_data","metadata":{},"data":{
        "text/html":html,"text/plain":"Retained alternate representation"
    }}]})])
}

#[test]
fn retained_html_table_reaches_theme_search_cli_and_staging_without_rereads() {
    let content = output(
        "<table><caption>Stored measurements</caption><thead><tr><th scope=\"col\">Metric</th><th scope=\"col\">Value</th></tr></thead><tbody><tr><td>Unicode 🦀</td><td><strong>42</strong> &amp; 7</td></tr></tbody></table>",
    );
    let page = attach(PAGE, &["analysis/table.ipynb"]);
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis/table.ipynb", &content),
    ]);
    let config = machine(&fixture.config());
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("inert retained table compiles without a kernel");
    let tree = rendered.site().output();
    assert_shared(
        tree,
        "content:one",
        "guide/index.html",
        &[
            "Stored measurements",
            "Metric",
            "Value",
            "Unicode 🦀",
            "42",
            "Retained alternate representation",
        ],
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let body = &super::notebook_sections(&model["pages"]["content:one"]);
    assert!(body.contains("<table>"));
    assert!(body.contains("<th scope=\"col\">Metric</th>"));
    assert!(body.contains("<strong>42</strong> &amp; 7"));
    assert!(
        model["pages"]["content:one"]["notes"]["body_text"]
            .as_str()
            .unwrap()
            .contains("42 & 7")
    );

    let parent = tempdir().expect("private stage parent");
    let mut args = RENDER_ARGUMENTS.to_vec();
    args.extend(["--output-parent", parent.path().to_str().unwrap()]);
    let result = cli(&config, &args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("native CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).unwrap()
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    for path in ["guide/index.html", "model.json", "search.json"] {
        assert_eq!(fs::read(directory.join(path)).unwrap(), bytes(tree, path));
    }
    fs::write(
        fixture.directory.path().join("one/analysis/table.ipynb"),
        b"changed after capture",
    )
    .unwrap();
    let repeated = render(&inputs).expect("retained input, not live notebook reads");
    assert_eq!(repeated.site().output().inventory(), tree.inventory());
    for entry in tree.inventory().entries() {
        assert_eq!(
            repeated.site().output().file(&entry.path),
            tree.file(&entry.path)
        );
    }
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn active_or_incomplete_html_refuses_the_whole_publication_before_staging() {
    for html in [
        "<table><tr><td>retained</td></tr></table><script>alert(1)</script>",
        "<p onclick=\"alert(1)\">retained</p>",
        "<table><tbody><tr><td>unfinished",
    ] {
        let content = output(html);
        let page = attach(PAGE, &["analysis/table.ipynb"]);
        let fixture = Fixture::new(&[
            ("one/page.md", page.as_bytes()),
            ("one/analysis/table.ipynb", &content),
        ]);
        assert_cli_failure_preserves_parent(&fixture.config(), "notebook_render_unsupported");
    }
}
