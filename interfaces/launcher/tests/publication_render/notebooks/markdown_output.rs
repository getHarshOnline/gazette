//! Stored Markdown outputs use the native Markdown and retained publication owners.

use super::*;

fn stored(markdown: &str) -> Value {
    json!({"output_type":"display_data","metadata":{},"data":{"text/markdown":markdown}})
}

fn output_notebook(outputs: Vec<Value>) -> Vec<u8> {
    notebook(vec![json!({"cell_type":"code","id":"stored","metadata":{},
        "source":"raise RuntimeError('MUST NOT EXECUTE')","execution_count":null,"outputs":outputs})])
}

#[test]
fn stored_markdown_output_reaches_theme_machine_assets_and_exact_retained_repeat() {
    let content = output_notebook(vec![
        stored(
            "# Stored result\n\n**Retained computation** and [Result data](r%C3%A9sults.csv#row).\n\n| Field | Value |\n| --- | --- |\n| retained metric | 42 |\n",
        ),
        stored("# Stored result\n\nSecond retained output with [Same data](r%C3%A9sults.csv).\n"),
    ]);
    let page = attach(PAGE, &["analysis/results.ipynb"]);
    let csv = b"name,value\nretained,42\n";
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis/results.ipynb", &content),
        ("one/analysis/résults.csv", csv),
        ("one/résults.csv", b"wrong page-relative data"),
    ]);
    let config = machine(&super::super::static_configuration(&fixture, false));
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("stored Markdown uses configured native renderer");
    let tree = rendered.site().output();
    assert_shared(
        tree,
        "content:one",
        "guide/index.html",
        &[
            "Stored result",
            "Retained computation",
            "retained metric",
            "42",
            "Second retained output",
        ],
    );
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let page = &model["pages"]["content:one"];
    let body = &super::notebook_sections(page);
    assert!(
        body.contains("<table>"),
        "native Markdown table, not literal output text"
    );
    assert!(body.contains("<strong>Retained computation</strong>"));
    let ids: Vec<_> = body
        .split("id=\"")
        .skip(1)
        .filter_map(|value| value.split_once('"').map(|(id, _)| id))
        .filter(|id| id.ends_with("-stored-result"))
        .collect();
    assert_eq!(ids.len(), 2, "both output headings retained");
    assert_ne!(
        ids[0], ids[1],
        "output positions have distinct native anchors"
    );
    assert!(ids.iter().all(|id| id.starts_with("gazette-notebook-")));
    let assets = page["assets"]
        .as_array()
        .expect("shared asset relationships");
    assert_eq!(assets.len(), 1, "repeated output links share one asset");
    let url = assets[0]["url"].as_str().expect("compiled companion URL");
    assert_eq!(
        url,
        "https://one.example/mount/media/authored/analysis/r%C3%A9sults.csv"
    );
    assert_eq!(assets[0], model["assets"][url]);
    assert!(body.contains(&format!("href=\"{url}#row\"")));
    assert!(body.contains(&format!("href=\"{url}\"")));
    assert_eq!(bytes(tree, "media/authored/analysis/résults.csv"), csv);

    let parent = tempdir().expect("disposable CLI stage parent");
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
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("actual CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("actual stage"));
    for path in [
        "guide/index.html",
        "model.json",
        "search.json",
        "media/authored/analysis/résults.csv",
    ] {
        assert_eq!(
            fs::read(directory.join(path)).expect("native read-back"),
            bytes(tree, path)
        );
    }
    for path in ["one/analysis/results.ipynb", "one/analysis/résults.csv"] {
        fs::write(
            fixture.directory.path().join(path),
            b"changed after retained capture",
        )
        .expect("disposable native input change");
    }
    let repeat = render(&inputs).expect("no native notebook/output companion reread");
    let repeated = repeat.site().output();
    assert_eq!(repeated.inventory(), tree.inventory());
    for entry in tree.inventory().entries() {
        assert_eq!(
            repeated.file(&entry.path),
            tree.file(&entry.path),
            "exact retained output bytes"
        );
    }
    assert_cli_failure_preserves_parent(&config, "source_content_mismatch");
}

#[test]
fn stored_markdown_output_missing_and_excluded_companions_refuse_without_fallback() {
    let page = attach(
        &PAGE.replace("lifecycle='draft'", "lifecycle='published'"),
        &["analysis/results.ipynb"],
    );
    let content = output_notebook(vec![stored("# Stored result\n\n[Result data](data.csv)")]);
    let missing = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis/results.ipynb", &content),
        ("other/analysis/data.csv", b"foreign-only,42\n"),
    ]);
    assert_cli_failure_preserves_parent(
        &super::super::static_configuration(&missing, false),
        "markdown_link_resolution",
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/analysis/results.ipynb", &content),
        ("one/analysis/data.csv", b"retained,42\n"),
    ]);
    let inputs = acquire(&super::super::static_configuration(&fixture, false));
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained native syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("resolved owner graph");
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
        .expect("projection owns notebook output links");
    let failure = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect_err("excluded output companion never re-enters publication");
    assert_eq!(failure.code(), "markdown_link_resolution");
}
