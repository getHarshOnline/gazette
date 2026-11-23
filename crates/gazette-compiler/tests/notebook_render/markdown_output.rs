//! Stored Markdown follows the same native projection and notebook-wide budget as cells.

use super::*;

fn stored(source: Value) -> Value {
    json!({"output_type":"display_data","metadata":{},"data":{"text/markdown":source}})
}

#[test]
fn stored_markdown_preserves_exact_multiline_and_every_supported_alternative() {
    let parts = [
        "# Head",
        "ing\n\n**Stored",
        "** report.\n\n|Field|Value|\n|---|---|\n|Result|yes|\n",
    ];
    let source = parts.concat();
    let mut rendered = Vec::new();
    for representation in [json!(source), json!(parts)] {
        let mut output = stored(representation);
        output["data"]["text/plain"] = json!("Original plain alternative");
        output["data"]["application/json"] = json!({"retained":true});
        output["data"]["image/png"] = json!(STANDARD.encode(b"stored transport"));
        let value = document(vec![code_cell(json!("NEVER_EXECUTE()"), vec![output])]);
        let bytes = encoded(&value);
        let parsed = ParsedNotebook::parse(&bytes, Default::default())
            .expect("retained source representation");
        let result = parsed
            .render_static(
                "embed",
                Default::default(),
                &|format, bytes, _| Ok(transport_inspection(format, bytes)),
                &gazette_adapter_markdown::Comrak,
            )
            .expect("shared Markdown and all stored alternatives");
        let expected = markdown::ParsedMarkdown::parse(
            &source,
            Default::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap()
        .render_in_namespace("embed-cell-0-output-0-", |_, _, _| Ok(None))
        .unwrap();
        assert!(result.html.contains(&format!(
            "<div class=\"notebook-markdown-output\">{}</div>",
            expected.body_html
        )));
        assert!(result.text.contains(&expected.body_text));
        assert!(result.html.contains("<strong>Stored</strong>"));
        assert!(result.html.contains("<table>"));
        assert!(result.text.contains("Original plain alternative"));
        assert!(result.html.contains("Original plain alternative"));
        assert!(result.text.contains("{\"retained\":true}"));
        assert!(result.html.contains("notebook-json"));
        assert!(result.text.contains("[Stored notebook image:"));
        assert!(result.html.contains("data:image/png;base64,"));
        assert_eq!(result.images.len(), 1);
        assert_eq!(parsed.value(), &value);
        assert_eq!(result.source_sha256, Sha256Digest::of(&bytes));
        assert_eq!(
            parsed
                .render_static(
                    "embed",
                    Default::default(),
                    &|format, bytes, _| Ok(transport_inspection(format, bytes)),
                    &gazette_adapter_markdown::Comrak
                )
                .unwrap(),
            result
        );
        rendered.push(result);
    }
    assert_eq!(rendered[0].html, rendered[1].html);
    assert_eq!(rendered[0].text, rendered[1].text);
    assert_ne!(rendered[0].source_sha256, rendered[1].source_sha256);
}

#[test]
fn markdown_outputs_namespace_headings_and_footnotes_by_actual_cell_and_output_ordinals() {
    let source = "# Heading\n\n[Here](#heading) note[^n].\n\n[^n]: Retained footnote.\n";
    let mut execution = stored(json!(source));
    execution["output_type"] = json!("execute_result");
    execution["execution_count"] = json!(7);
    let mut later = code_cell(json!(""), vec![stored(json!(source))]);
    later["id"] = json!("program-other");
    let bytes = encoded(&document(vec![
        markdown_cell(
            "program-output-1",
            json!("# Output-1-heading\n\n[Here](#output-1-heading)"),
        ),
        code_cell(
            json!("not executed"),
            vec![
                json!({"output_type":"stream","name":"stdout","text":"before"}),
                stored(json!(source)),
                execution,
            ],
        ),
        later,
    ]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
    let result = parsed
        .render_static(
            "embed",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let ids: Vec<_> = result
        .html
        .split(" id=\"")
        .skip(1)
        .map(|tail| tail.split('"').next().unwrap())
        .collect();
    assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
    for id in [
        "embed-cell-0-output-1-heading",
        "embed-cell-1-output-1-heading",
        "embed-cell-1-output-2-heading",
        "embed-cell-2-output-0-heading",
    ] {
        assert!(ids.contains(&id), "missing {id}");
        assert!(result.html.contains(&format!("href=\"#{id}\"")));
    }
    for fragment in result.html.split("href=\"#").skip(1) {
        let id = fragment.split('"').next().unwrap();
        assert!(ids.contains(&id), "dangling reference {id}");
    }
    assert!(result.html.contains("data-execution-count=\"7\""));
    assert!(
        result.html.find("before").unwrap()
            < result.html.find("embed-cell-1-output-1-heading").unwrap()
    );
    let other = parsed
        .render_static(
            "second",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    assert_ne!(other.html, result.html);
    assert_eq!(other.text, result.text);
}

#[test]
fn markdown_outputs_use_retained_companion_mapping_without_external_or_fragment_callbacks() {
    let source = "# Heading\n\n[Remote](https://example.invalid/) [Mail](mailto:reader@example.invalid) [Here](#heading) [Later](#later) [Data](data.csv#row) ![Plot](images/plot.png)";
    let bytes = encoded(&document(vec![code_cell(
        json!(""),
        vec![stored(json!(source))],
    )]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
    let mut calls = Vec::new();
    let result = parsed
        .render_static_resolving(
            "embed",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            |kind, destination, span| {
                assert!(span.start < span.end && span.end <= source.len());
                calls.push((kind, destination.to_owned()));
                Ok(Some(format!(
                    "https://publication.example/assets/{destination}"
                )))
            },
        )
        .unwrap();
    assert_eq!(
        calls,
        [
            (markdown::LinkKind::Link, "data.csv#row".into()),
            (markdown::LinkKind::Image, "images/plot.png".into())
        ]
    );
    assert!(
        result
            .html
            .contains("href=\"https://publication.example/assets/data.csv#row\"")
    );
    assert!(
        result
            .html
            .contains("src=\"https://publication.example/assets/images/plot.png\"")
    );
    assert!(
        result
            .html
            .contains("href=\"#embed-cell-0-output-0-heading\"")
    );
    assert!(result.text.contains("Data") && result.text.contains("Plot"));
    assert!(
        result
            .html
            .contains("href=\"#embed-cell-0-output-0-later\"")
    );
    for mapped in [
        None,
        Some("javascript:alert(1)".to_owned()),
        Some("data:image/png;base64,Zg==".to_owned()),
    ] {
        assert!(
            parsed
                .render_static_resolving(
                    "embed",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                    |_, _, _| Ok(mapped.clone())
                )
                .is_err()
        );
    }
}

#[test]
fn malformed_and_unsafe_markdown_outputs_fail_without_plaintext_or_partial_fallback() {
    for malformed in [
        json!(false),
        json!({"body":"# heading"}),
        json!(["text", 3]),
    ] {
        let bytes = encoded(&document(vec![code_cell(
            json!(""),
            vec![stored(malformed)],
        )]));
        assert!(
            ParsedNotebook::parse(&bytes, Default::default()).is_err(),
            "native schema owns multiline representation"
        );
    }
    for body in [
        "<script>never()</script>",
        "<b>raw</b>",
        "[unsafe](javascript:alert(1))",
        "![unsafe](data:image/png;base64,Zg==)",
        "![absent](attachment:plot.png)",
        "[missing](relative.csv)",
        "# One {#same}\n\n# Two {#same}",
        "Plain.\n\n[^unused]: <span>hidden raw HTML</span>",
    ] {
        let mut output = stored(json!(body));
        output["data"]["text/plain"] = json!("not a fallback");
        let value = document(vec![
            markdown_cell("earlier", json!("Earlier valid cell.")),
            code_cell(
                json!(""),
                vec![stored(json!("Earlier valid output.")), output],
            ),
        ]);
        let bytes = encoded(&value);
        let parsed = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
        assert!(
            matches!(
                parsed.render_static(
                    "embed",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                ),
                Err(RenderError::Markdown { cell: Some(1), .. })
            ),
            "{body}"
        );
        assert_eq!(parsed.value(), &value);
    }
}

#[test]
fn markdown_output_nodes_and_emitted_bytes_share_the_complete_notebook_allowance() {
    let body = "# Heading\n\nStored body.";
    let count = markdown::ParsedMarkdown::parse(
        body,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
    .node_count();
    let bytes = encoded(&document(vec![
        markdown_cell("first", json!(body)),
        code_cell(json!(""), vec![stored(json!(body)), stored(json!(body))]),
    ]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
    let limits = markdown::Limits {
        nodes: count * 3,
        ..Default::default()
    };
    let result = parsed
        .render_static(
            "embed",
            limits,
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let exact = markdown::Limits {
        source_bytes: bytes.len(),
        output_bytes: result.html.len().max(result.text.len()),
        ..limits
    };
    assert_eq!(
        parsed
            .render_static(
                "embed",
                exact,
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .unwrap(),
        result
    );
    for limited in [
        markdown::Limits {
            nodes: count * 3 - 1,
            ..exact
        },
        markdown::Limits {
            output_bytes: exact.output_bytes - 1,
            ..exact
        },
        markdown::Limits {
            source_bytes: exact.source_bytes - 1,
            ..exact
        },
    ] {
        assert!(
            parsed
                .render_static(
                    "embed",
                    limited,
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err()
        );
    }
}
