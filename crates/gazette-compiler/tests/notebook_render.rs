//! Real retained notebook syntax and shared Markdown projection; no execution fixtures.

use std::{cell::RefCell, collections::BTreeSet};

use base64::{Engine, engine::general_purpose::STANDARD};

use gazette_compiler::{
    markdown,
    notebook::{self, ParsedNotebook, RenderError},
};
use gazette_core::{raster, source::Sha256Digest};
use serde_json::{Value, json};

#[path = "notebook_render/markdown_output.rs"]
mod markdown_output;

#[path = "notebook_render/html_output.rs"]
mod html_output;

#[test]
fn notebook_components_use_one_catalog_across_cells_and_stored_outputs() {
    use gazette_compiler::theme_package::ParsedManifest;
    use gazette_core::markdown::{ComponentBinding, Error, Fragment};
    use gazette_ports::markdown::ComponentRenderer;
    struct Children;
    impl ComponentRenderer for Children {
        fn render(&self, _: &ComponentBinding, body: &Fragment, _: bool) -> Result<String, Error> {
            Ok(body.html.clone())
        }
    }
    let manifest = ParsedManifest::parse(
        "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.note]\ntemplate='note.html'\n[components.counter]\ntemplate='counter.html'\ninteraction={module='module:counter',export='mount',mount='load'}\n",
        Default::default(),
    ).expect("native selected catalog");
    let catalog = markdown::ComponentPresentation {
        definitions: manifest.components(),
        renderer: &Children,
    };
    let input = encoded(&document(vec![
        markdown_cell(
            "authored",
            json!("::::note\n# Cell heading\n\n:::counter\nNested **cell**.\n:::\n::::\n"),
        ),
        code_cell(
            json!("not executed"),
            vec![
                json!({"output_type":"display_data","metadata":{},"data":{"text/markdown":":::counter\n# Output heading\n\nStored **meaning**.\n:::\n"}}),
            ],
        ),
    ]));
    let parsed = ParsedNotebook::parse(&input, Default::default()).unwrap();
    let render = || {
        parsed.render_with_components(
            "book",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            Some(catalog),
            |_, _, _| Ok(None),
        )
    };
    let output = render().expect("real parser and component admission");
    assert_eq!(output, render().unwrap());
    assert_eq!(output.source_sha256, Sha256Digest::of(&input));
    assert_eq!(parsed.source(), input.as_slice());
    assert_eq!(
        output
            .components
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["note", "counter", "counter"]
    );
    assert!(output.components[0].interaction.is_none());
    assert!(
        output.components[1..].iter().all(|item| item
            .interaction
            .as_ref()
            .unwrap()
            .export
            .as_str()
            == "mount")
    );
    assert!(output.html.contains("book-cell-0-cell-heading"));
    assert!(output.html.contains("book-cell-1-output-0-output-heading"));
    assert!(output.text.contains("Nested cell."));
    assert!(output.text.contains("Stored meaning."));
    assert!(!output.text.contains("mount"));
    assert!(
        parsed
            .render_static(
                "book",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err(),
        "standalone never selects a catalog implicitly"
    );

    for stored in [false, true] {
        let invalid = ":::unknown\nRejected late body.\n:::\n";
        let cell = if stored {
            code_cell(
                json!("literal"),
                vec![
                    json!({"output_type":"display_data","metadata":{},"data":{"text/markdown":invalid}}),
                ],
            )
        } else {
            markdown_cell("bad", json!(invalid))
        };
        let bytes = encoded(&document(vec![
            markdown_cell("good", json!(":::note\nValid prefix.\n:::\n")),
            cell,
        ]));
        let notebook = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
        assert!(
            matches!(
                notebook.render_with_components(
                    "book-",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak,
                    Some(catalog),
                    |_, _, _| Ok(None)
                ),
                Err(RenderError::Markdown { cell: Some(1), .. })
            ),
            "invalid cell/output refuses complete projection with exact cell context"
        );
    }
}

fn document(cells: Vec<Value>) -> Value {
    json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":cells})
}

fn markdown_cell(id: &str, source: Value) -> Value {
    json!({"cell_type":"markdown","id":id,"metadata":{},"source":source})
}

fn code_cell(source: Value, outputs: Vec<Value>) -> Value {
    json!({"cell_type":"code","id":"program","metadata":{},"execution_count":null,"source":source,"outputs":outputs})
}

fn encoded(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("native fixture JSON")
}

// This provider deliberately tests only the compiler's transport/binding boundary.
// Real pixel decoding is exercised through the native raster adapter and application.
fn transport_inspection(format: raster::Format, bytes: &[u8]) -> raster::Inspection {
    raster::Inspection {
        display: None,
        format,
        width: 2,
        height: 3,
        bytes: bytes.len(),
        sha256: Sha256Digest::of(bytes),
        tool: "test.transport-only",
    }
}

fn image_document(value: Value) -> Vec<u8> {
    encoded(&document(vec![code_cell(
        json!("not executed"),
        vec![
            json!({"output_type":"display_data","metadata":{"image/png":{"width":"UNTRUSTED","height":0,"onerror":"UNTRUSTED"}},"data":{"image/png":value,"text/plain":"Stored alternative"}}),
        ],
    )]))
}

#[test]
fn raster_transport_preserves_exact_bytes_and_all_alternatives_without_metadata_attributes() {
    let input = b"transport boundary fixture, not a decoded image";
    let base64 = STANDARD.encode(input);
    let bytes = image_document(json!([&base64[..8], "\r\n", &base64[8..], "\n"]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("valid retained syntax");
    let calls = RefCell::new(Vec::new());
    let provider = |format, bytes: &[u8], limits: raster::Limits| {
        assert!(bytes.len() <= limits.encoded_bytes);
        assert_eq!(
            limits.decoded_bytes,
            raster::Limits::default().decoded_bytes
        );
        calls.borrow_mut().push((format, bytes.to_vec()));
        Ok(transport_inspection(format, bytes))
    };
    let output = parsed
        .render_static(
            "image",
            Default::default(),
            &provider,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("bound provider result");
    assert_eq!(*calls.borrow(), [(raster::Format::Png, input.to_vec())]);
    assert!(output.html.contains(&format!(
        "src=\"data:image/png;base64,{base64}\" width=\"2\" height=\"3\""
    )));
    assert!(output.html.contains("Stored alternative"));
    assert!(output.text.contains("Stored alternative"));
    assert!(
        output
            .text
            .contains("[Stored notebook image: image/png, 2 × 3 pixels]")
    );
    assert!(!output.html.contains("UNTRUSTED"));
    assert!(!output.text.contains(&base64));
    assert_eq!(
        output.images,
        [transport_inspection(raster::Format::Png, input)]
    );
    assert_eq!(output.source_sha256, Sha256Digest::of(&bytes));
    assert_eq!(
        parsed
            .render_static(
                "image",
                Default::default(),
                &provider,
                &gazette_adapter_markdown::Comrak
            )
            .expect("repeat"),
        output
    );

    let mut jpeg = document(vec![code_cell(
        json!(""),
        vec![
            json!({"output_type":"execute_result","execution_count":2,"metadata":{},"data":{"image/jpeg":base64,"application/json":{"retained":true}}}),
        ],
    )]);
    jpeg["metadata"] = json!({});
    let jpeg = encoded(&jpeg);
    let jpeg =
        ParsedNotebook::parse(&jpeg, Default::default()).expect("JPEG representation syntax");
    let jpeg = jpeg
        .render_static(
            "jpeg",
            Default::default(),
            &provider,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("JPEG transport");
    assert!(jpeg.html.contains("src=\"data:image/jpeg;base64,"));
    assert!(jpeg.text.contains("{\"retained\":true}"));
    assert_eq!(jpeg.images[0].format, raster::Format::Jpeg);
}

#[test]
fn raster_evidence_must_bind_exact_bytes_format_dimensions_and_profile() {
    let bytes = image_document(json!(STANDARD.encode(b"transport fixture")));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("retained syntax");
    for field in 0..8 {
        let provider = |format, bytes: &[u8], limits: raster::Limits| {
            let mut evidence = transport_inspection(format, bytes);
            match field {
                0 => evidence.format = raster::Format::Jpeg,
                1 => evidence.bytes += 1,
                2 => evidence.sha256 = Sha256Digest::of(b"different bytes"),
                3 => evidence.width = 0,
                4 => evidence.height = 0,
                5 => evidence.tool = "",
                6 => evidence.width = limits.width + 1,
                7 => evidence.height = limits.height + 1,
                _ => unreachable!(),
            }
            Ok(evidence)
        };
        let error = parsed
            .render_static(
                "image",
                Default::default(),
                &provider,
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("invalid provider evidence");
        assert_eq!(
            error.code(),
            if field < 6 {
                "raster_binding"
            } else {
                "raster_limit"
            }
        );
        assert!(matches!(error, RenderError::Raster { cell: 0, .. }));
    }
    assert_eq!(
        parsed
            .render_static(
                "image",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("no selected decoder")
            .code(),
        "raster_unsupported"
    );
    assert_eq!(
        parsed
            .render_static(
                "image",
                Default::default(),
                &|_, _, _| Err(raster::Error::Decode),
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("provider rejected pixels")
            .code(),
        "raster_decode"
    );
}

#[test]
fn malformed_base64_is_rejected_before_the_raster_provider() {
    for input in [
        "",
        "Zg",
        "Zg=",
        "Zh==",
        "Zg===",
        "====",
        "========",
        "Z g==",
        "Zg==\t",
        "__8=",
        "data:image/png;base64,Zg==",
        "Zg==\u{a0}",
    ] {
        let bytes = image_document(json!(input));
        let parsed = ParsedNotebook::parse(&bytes, Default::default())
            .expect("inspectable malformed MIME data");
        let error = parsed
            .render_static(
                "image",
                Default::default(),
                &|_, _, _| panic!("bad Base64 reached pixel decoder"),
                &gazette_adapter_markdown::Comrak,
            )
            .expect_err("strict Base64 required");
        assert_eq!(error.code(), "raster_decode", "{input:?}");
    }
    let bytes = encoded(&document(vec![markdown_cell(
        "unsafe",
        json!("![image](data:image/png;base64,Zg==)"),
    )]));
    let parsed =
        ParsedNotebook::parse(&bytes, Default::default()).expect("data URI remains syntax");
    assert!(
        parsed
            .render_static(
                "image",
                Default::default(),
                &|_, _, _| panic!("Markdown URI reached decoder"),
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
}

#[test]
fn raster_emission_respects_whole_notebook_output_and_source_caps() {
    let bytes = image_document(json!(STANDARD.encode(b"transport fixture")));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("retained syntax");
    let provider = |format, bytes: &[u8], _| Ok(transport_inspection(format, bytes));
    let rendered = parsed
        .render_static(
            "image",
            Default::default(),
            &provider,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("reference output");
    let limits = markdown::Limits {
        output_bytes: rendered.html.len().max(rendered.text.len()),
        ..Default::default()
    };
    assert_eq!(
        parsed
            .render_static(
                "image",
                limits,
                &provider,
                &gazette_adapter_markdown::Comrak
            )
            .expect("exact cap"),
        rendered
    );
    assert_eq!(
        parsed
            .render_static(
                "image",
                markdown::Limits {
                    output_bytes: limits.output_bytes - 1,
                    ..limits
                },
                &provider,
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("whole output overflow")
            .code(),
        "notebook_render_limit"
    );
    assert_eq!(
        parsed
            .render_static(
                "image",
                markdown::Limits {
                    source_bytes: bytes.len() - 1,
                    ..limits
                },
                &|_, _, _| panic!("oversized notebook reached provider"),
                &gazette_adapter_markdown::Comrak
            )
            .expect_err("whole input overflow")
            .code(),
        "notebook_render_limit"
    );
}

#[test]
fn complete_stored_cells_outputs_and_exact_source_identity_are_preserved_without_execution() {
    let mut value = document(vec![
        markdown_cell(
            "prose",
            json!(["# Analysis\n\n", "This is **retained**.\n"]),
        ),
        code_cell(
            json!([
                "raise RuntimeError('NEVER EXECUTE')\n",
                "<script>&not_code()</script>\n"
            ]),
            vec![
                json!({"output_type":"stream","name":"stdout","text":["first\n","<script>stored & escaped</script>"]}),
                json!({"output_type":"stream","name":"stderr","text":"stored warning"}),
                json!({"output_type":"error","ename":"StoredError","evalue":"not raised <here>","traceback":["frame one","frame two & <three>"]}),
                json!({"output_type":"display_data","metadata":{},"data":{"text/plain":["plain ","result"],"application/json":{"a":[1,true,null],"s":"<script>JSON</script>"}}}),
                json!({"output_type":"execute_result","metadata":{},"execution_count":7,"data":{"text/plain":"42"}}),
            ],
        ),
    ]);
    value["metadata"] = json!({"kernelspec":{"name":"missing-kernel","display_name":"Not installed"},"private_note":"NOT A PUBLIC FIELD"});
    let mut bytes = serde_json::to_vec_pretty(&value).expect("exact formatted input");
    bytes.extend_from_slice(b"\r\n");
    let parsed =
        ParsedNotebook::parse(&bytes, notebook::Limits::default()).expect("one notebook parse");
    let original_tree = parsed.value() as *const Value;
    let rendered = parsed
        .render_static(
            "page-one",
            markdown::Limits::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("native stored-output render");
    assert_eq!(parsed.value(), &value);
    assert_eq!(parsed.value() as *const Value, original_tree);
    assert_eq!(parsed.source(), bytes);
    assert_eq!(rendered.source_sha256, Sha256Digest::of(&bytes));
    assert_eq!(rendered.schema_sha256, parsed.inspect().schema_sha256);
    assert_eq!(rendered.tool, "gazette.notebook-static/1;nbformat/4.5");
    for text in [
        "Analysis",
        "retained",
        "NEVER EXECUTE",
        "first",
        "stored warning",
        "StoredError",
        "not raised",
        "frame one",
        "frame two",
        "plain result",
        "42",
    ] {
        assert!(rendered.text.contains(text), "missing text {text}");
        assert!(rendered.html.contains(text), "missing HTML {text}");
    }
    assert!(rendered.html.contains("<strong>retained</strong>"));
    assert!(
        rendered
            .html
            .contains("&lt;script&gt;stored &amp; escaped&lt;/script&gt;")
    );
    assert!(rendered.html.contains("data-execution-count=\"null\""));
    assert!(rendered.html.contains("data-execution-count=\"7\""));
    assert!(
        rendered
            .text
            .contains("{\"a\":[1,true,null],\"s\":\"<script>JSON</script>\"}")
    );
    assert!(!rendered.html.contains("<script>"));
    assert!(!rendered.html.contains("NOT A PUBLIC FIELD"));
    assert!(!rendered.text.contains("NOT A PUBLIC FIELD"));
    assert!(
        rendered.html.find("Analysis").expect("first cell")
            < rendered.html.find("NEVER EXECUTE").expect("second cell")
    );
    assert_eq!(
        parsed
            .render_static(
                "page-one",
                markdown::Limits::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("deterministic repeat"),
        rendered
    );
}

#[test]
fn object_member_order_is_presentation_neutral_but_arrays_and_source_bytes_remain_exact() {
    let source = |data: &str| {
        format!(
            r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[
                {{"cell_type":"markdown","id":"intro","metadata":{{}},"source":"Introduction"}},
                {{"cell_type":"code","id":"program","metadata":{{}},"execution_count":null,"source":"not executed","outputs":[
                    {{"output_type":"display_data","metadata":{{}},"data":{data}}},
                    {{"output_type":"stream","name":"stdout","text":"Last output"}}
                ]}}
            ]}}"#
        ).into_bytes()
    };
    let render = |bytes: &[u8]| {
        let parsed = ParsedNotebook::parse(bytes, notebook::Limits::default())
            .expect("native bounded notebook parser");
        assert_eq!(parsed.source(), bytes);
        let rendered = parsed
            .render_static(
                "stable",
                markdown::Limits::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .expect("native notebook projection");
        assert_eq!(rendered.source_sha256, Sha256Digest::of(bytes));
        rendered
    };
    let original = source(
        r#"{"text/plain":"Plain result","application/json":{"z":{"b":2,"a":1},"sequence":[2,1]}}"#,
    );
    let expected = render(&original);
    for data in [
        r#"{"application/json":{"z":{"b":2,"a":1},"sequence":[2,1]},"text/plain":"Plain result"}"#,
        r#"{"text/plain":"Plain result","application/json":{"sequence":[2,1],"z":{"a":1,"b":2}}}"#,
    ] {
        let bytes = source(data);
        let actual = render(&bytes);
        assert_ne!(bytes, original);
        assert_ne!(actual.source_sha256, expected.source_sha256);
        assert_eq!(actual.html, expected.html);
        assert_eq!(actual.text, expected.text);
    }
    let original_value: Value = serde_json::from_slice(&original).expect("array controls");
    for pointer in [
        "/cells",
        "/cells/1/outputs",
        "/cells/1/outputs/0/data/application~1json/sequence",
    ] {
        let mut changed = original_value.clone();
        changed
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .expect("semantic array")
            .reverse();
        let actual = render(&encoded(&changed));
        assert_ne!(actual.html, expected.html, "{pointer}");
        assert_ne!(actual.text, expected.text, "{pointer}");
    }
}

#[test]
fn string_and_array_sources_have_equal_outputs_but_distinct_original_byte_evidence() {
    let left = encoded(&document(vec![markdown_cell(
        "same",
        json!("# Heading\n\nParagraph."),
    )]));
    let right = encoded(&document(vec![markdown_cell(
        "same",
        json!(["# Heading\n", "\nParagraph."]),
    )]));
    let left = ParsedNotebook::parse(&left, Default::default())
        .expect("string syntax")
        .render_static(
            "same",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("string rendering");
    let right = ParsedNotebook::parse(&right, Default::default())
        .expect("array syntax")
        .render_static(
            "same",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("array rendering");
    assert_eq!(left.html, right.html);
    assert_eq!(left.text, right.text);
    assert_ne!(left.source_sha256, right.source_sha256);
    assert_eq!(left.schema_sha256, right.schema_sha256);
}

#[test]
fn namespaced_headings_footnotes_and_fragments_are_unique_across_cells_and_embeddings() {
    let source =
        "# Heading\n\n[local](#heading) ref[^n].\n\n[^n]: footnote text\n\n<!-- more -->\n\nTail.";
    let bytes = encoded(&document(vec![
        markdown_cell("one", json!(source)),
        markdown_cell("two", json!(source)),
    ]));
    let parsed =
        ParsedNotebook::parse(&bytes, Default::default()).expect("retained two-cell syntax");
    let rendered = parsed
        .render_static(
            "embed",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("namespaced rendering");
    let ids: Vec<_> = rendered
        .html
        .split(" id=\"")
        .skip(1)
        .map(|tail| tail.split('"').next().expect("attribute value"))
        .collect();
    assert_eq!(
        ids.len(),
        ids.iter().collect::<BTreeSet<_>>().len(),
        "no duplicate generated IDs"
    );
    for cell in [0, 1] {
        assert!(
            rendered
                .html
                .contains(&format!("id=\"embed-cell-{cell}-heading\""))
        );
        assert!(
            rendered
                .html
                .contains(&format!("href=\"#embed-cell-{cell}-heading\""))
        );
    }
    for tail in rendered.html.split("href=\"#").skip(1) {
        let target = tail.split('"').next().expect("fragment target");
        assert!(
            ids.contains(&target),
            "dangling generated reference {target}"
        );
    }
    let other = parsed
        .render_static(
            "other",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("independent embedding");
    assert_ne!(other.html, rendered.html);
    assert_eq!(other.text, rendered.text);
    assert_eq!(other.source_sha256, rendered.source_sha256);

    let collision = encoded(&document(vec![
        markdown_cell("a", json!("# b-c")),
        markdown_cell("a-b", json!("# c")),
    ]));
    let collision = ParsedNotebook::parse(&collision, Default::default())
        .expect("similar authored IDs")
        .render_static(
            "embed",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("ordinal prefixes are unambiguous");
    assert!(collision.html.contains("id=\"embed-cell-0-b-c\""));
    assert!(collision.html.contains("id=\"embed-cell-1-c\""));
    assert!(collision.html.contains("data-cell-id=\"a\""));
    assert!(collision.html.contains("data-cell-id=\"a-b\""));
}

#[test]
fn unsupported_rich_representations_never_hide_behind_plaintext_fallback() {
    for (mime, value) in [
        ("text/html", json!("<script>never</script>")),
        ("image/svg+xml", json!("<svg/>")),
        (
            "application/vnd.jupyter.widget-view+json",
            json!({"model_id":"widget"}),
        ),
    ] {
        let mut data = json!({"text/plain":"not a permitted fallback", "text/markdown":"# Also not a fallback"});
        data[mime] = value;
        let bytes = encoded(&document(vec![code_cell(
            json!("1 + 1"),
            vec![json!({"output_type":"display_data","metadata":{},"data":data})],
        )]));
        let parsed =
            ParsedNotebook::parse(&bytes, Default::default()).expect("valid rich notebook syntax");
        assert!(
            matches!(
                parsed.render_static(
                    "page",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                ),
                Err(RenderError::Unsupported {
                    feature,
                    ..
                }) if feature == if mime == "text/html" { "HTML representation" } else { "MIME representation" }
            ),
            "{mime}"
        );
    }
}

#[test]
fn unsupported_attachments_raw_cells_widget_state_and_raw_html_fail_explicitly() {
    let mut attached = markdown_cell("attachment", json!("# Text"));
    attached["attachments"] = json!({"unused.svg":{"image/svg+xml":"<svg/>"}});
    let mut widgets = document(vec![markdown_cell("ordinary", json!("ordinary"))]);
    widgets["metadata"]["widgets"] = json!({});
    for (value, code) in [
        (document(vec![attached]), "notebook_render_unsupported"),
        (widgets, "notebook_render_unsupported"),
        (
            document(vec![
                json!({"cell_type":"raw","id":"raw","metadata":{},"source":"raw"}),
            ]),
            "notebook_render_unsupported",
        ),
        (
            document(vec![markdown_cell(
                "html",
                json!("<script>never()</script>"),
            )]),
            "markdown_raw_html_unavailable",
        ),
    ] {
        let bytes = encoded(&value);
        let parsed =
            ParsedNotebook::parse(&bytes, Default::default()).expect("syntax remains inspectable");
        assert_eq!(
            parsed
                .render_static(
                    "page",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("no silent static substitute")
                .code(),
            code
        );
        assert_eq!(parsed.value(), &value);
    }
}

#[test]
fn embedded_images_are_cell_local_byte_bound_and_do_not_rewrite_markdown_text() {
    let mut first = markdown_cell(
        "first",
        json!("# First\n\n![A *plot*](attachment:plot.png)\n\n`attachment:plot.png`"),
    );
    first["attachments"] = json!({"plot.png":{"image/png":STANDARD.encode(b"first")},"unused.png":{"image/png":STANDARD.encode(b"unused")}});
    let mut second = markdown_cell("second", json!("![Other](attachment:plot.png)"));
    second["attachments"] = json!({"plot.png":{"image/jpeg":STANDARD.encode(b"second")}});
    let bytes = encoded(&document(vec![first, second]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("valid notebook");
    let provider = |format, bytes: &[u8], _| Ok(transport_inspection(format, bytes));
    let output = parsed
        .render_static_resolving(
            "page",
            Default::default(),
            &provider,
            &gazette_adapter_markdown::Comrak,
            |_, _, _| panic!("attachments never leave the cell"),
        )
        .expect("same native Markdown plan");
    assert!(output.html.contains("data:image/png;base64,Zmlyc3Q="));
    assert!(output.html.contains("data:image/jpeg;base64,c2Vjb25k"));
    assert!(!output.html.contains(&STANDARD.encode(b"unused")));
    assert!(output.html.contains("alt=\"A plot\""));
    assert!(output.html.contains("<code>attachment:plot.png</code>"));
    assert!(output.text.contains("A plot"));
    assert!(output.text.contains("Other"));
    assert!(!output.text.contains("data:image"));
    assert_eq!(output.images.len(), 3);
    assert_eq!(output.images[0].sha256, Sha256Digest::of(b"first"));
    assert_eq!(output.images[2].sha256, Sha256Digest::of(b"second"));
    assert_eq!(parsed.source(), bytes);
    assert_eq!(
        parsed
            .render_static(
                "page",
                Default::default(),
                &provider,
                &gazette_adapter_markdown::Comrak
            )
            .expect("repeat"),
        output
    );
    let limits = markdown::Limits {
        output_bytes: output.html.len() - 1,
        ..markdown::Limits::default()
    };
    assert_eq!(
        parsed
            .render_static("page", limits, &provider, &gazette_adapter_markdown::Comrak)
            .expect_err("whole notebook cap")
            .code(),
        "notebook_render_limit"
    );
}

#[test]
fn attachment_exceptions_cannot_admit_links_data_urls_or_missing_cell_values() {
    let provider = |format, bytes: &[u8], _| Ok(transport_inspection(format, bytes));
    for source in [
        "[Link](attachment:plot.png)",
        "![Missing](attachment:other.png)",
        "![Raw](data:image/png;base64,aGVsbG8=)",
    ] {
        let mut cell = markdown_cell("cell", json!(source));
        cell["attachments"] = json!({"plot.png":{"image/png":"aGVsbG8="}});
        let bytes = encoded(&document(vec![cell]));
        let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("syntax");
        assert_eq!(
            parsed
                .render_static(
                    "page",
                    Default::default(),
                    &provider,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("no scheme bypass")
                .code(),
            "markdown_unsafe_url"
        );
    }
    let source = "![Image](attachment:plot.png)";
    let ordinary = markdown::ParsedMarkdown::parse(
        source,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("ordinary syntax");
    assert_eq!(
        ordinary
            .render_html()
            .expect_err("not an ordinary Markdown feature")
            .code(),
        "markdown_unsafe_url"
    );
}

#[test]
fn attachment_transport_and_all_bundle_representations_are_checked_even_when_unused() {
    for (bundle, expected) in [
        (json!({"image/png":"bad!"}), "raster_decode"),
        (
            json!({"image/png":"aGVsbG8=","text/plain":"alternate"}),
            "notebook_render_unsupported",
        ),
        (json!({}), "notebook_render_unsupported"),
    ] {
        let mut cell = markdown_cell("cell", json!("No image reference"));
        cell["attachments"] = json!({"unused":bundle});
        let bytes = encoded(&document(vec![cell]));
        let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("syntax");
        assert_eq!(
            parsed
                .render_static(
                    "page",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("no unused-value bypass")
                .code(),
            expected
        );
    }
}

#[test]
fn relative_assets_and_documents_require_binding_while_external_links_are_not_fetched() {
    for destination in [
        "image.png",
        "../docs/page.md",
        "/media/image.png",
        "//external.test/image.png",
    ] {
        let bytes = encoded(&document(vec![markdown_cell(
            "local",
            json!(format!("[target]({destination})")),
        )]));
        let parsed =
            ParsedNotebook::parse(&bytes, Default::default()).expect("retained relative syntax");
        assert_eq!(
            parsed
                .render_static(
                    "page",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .expect_err("local reference has no mapping")
                .code(),
            "markdown_link_resolution"
        );
    }
    let bytes = encoded(&document(vec![markdown_cell(
        "external",
        json!(
            "[remote](https://does-not-exist.invalid/doc) ![image](https://does-not-exist.invalid/image.png)"
        ),
    )]));
    let parsed =
        ParsedNotebook::parse(&bytes, Default::default()).expect("retained external syntax");
    let rendered = parsed
        .render_static(
            "page",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("no network required");
    assert!(
        rendered
            .html
            .contains("https://does-not-exist.invalid/image.png")
    );
}

#[test]
fn explicit_companion_mapping_uses_shared_markdown_safety_and_namespaces() {
    let bytes = encoded(&document(vec![markdown_cell(
        "local",
        json!(
            "# Plot\n\n![Retained plot](images/plot.png) [Data](data.csv#row) [Here](#plot) [Remote](https://example.invalid/)"
        ),
    )]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("retained notebook");
    let mut calls = Vec::new();
    let rendered = parsed
        .render_static_resolving(
            "page",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            |kind, destination, span| {
                assert!(span.start < span.end);
                calls.push((kind, destination.to_owned()));
                Ok(Some(format!(
                    "https://publication.example/assets/{destination}"
                )))
            },
        )
        .expect("explicit mappings, no raster or remote acquisition");
    assert_eq!(
        calls,
        vec![
            (markdown::LinkKind::Image, "images/plot.png".into()),
            (markdown::LinkKind::Link, "data.csv#row".into()),
        ]
    );
    assert!(
        rendered
            .html
            .contains("src=\"https://publication.example/assets/images/plot.png\"")
    );
    assert!(
        rendered
            .html
            .contains("href=\"https://publication.example/assets/data.csv#row\"")
    );
    assert!(rendered.html.contains("href=\"#page-cell-0-plot\""));
    assert!(
        rendered.images.is_empty(),
        "static companion transport is not stored-raster inspection"
    );
    assert!(
        parsed
            .render_static_resolving(
                "page",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak,
                |_, _, _| Ok(Some("javascript:alert(1)".into())),
            )
            .is_err(),
        "mapped destinations cannot bypass native Markdown URL safety"
    );
}

#[test]
fn whole_notebook_bounds_and_namespace_apply_even_without_markdown_cells() {
    let bytes = encoded(&document(vec![code_cell(
        json!("<&>"),
        vec![json!({"output_type":"stream","name":"stdout","text":"output"})],
    )]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("code-only notebook");
    let rendered = parsed
        .render_static(
            "page",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("reference size");
    let exact = markdown::Limits {
        output_bytes: rendered.html.len().max(rendered.text.len()),
        ..Default::default()
    };
    assert_eq!(
        parsed
            .render_static(
                "page",
                exact,
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("exact complete cap"),
        rendered
    );
    assert!(
        parsed
            .render_static(
                "page",
                markdown::Limits {
                    output_bytes: exact.output_bytes - 1,
                    ..exact
                },
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
    assert!(
        parsed
            .render_static(
                "page",
                markdown::Limits {
                    source_bytes: bytes.len() - 1,
                    ..exact
                },
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
    for namespace in ["bad\"namespace", "../parent", "non-ascii-é"] {
        assert!(
            parsed
                .render_static(
                    namespace,
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .is_err(),
            "code-only namespace {namespace:?}"
        );
    }
    let empty = encoded(&document(vec![]));
    let empty = ParsedNotebook::parse(&empty, Default::default()).expect("empty syntax");
    assert!(
        empty
            .render_static(
                "bad namespace",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
    assert!(
        empty
            .render_static(
                "",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .expect("empty static surface is not an execution placeholder")
            .text
            .is_empty()
    );
}

#[test]
fn markdown_node_budget_is_shared_across_cells_and_late_failure_returns_no_result() {
    let body = "A paragraph.";
    let count = markdown::ParsedMarkdown::parse(
        body,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native event count")
    .node_count();
    let bytes = encoded(&document(vec![
        markdown_cell("one", json!(body)),
        markdown_cell("two", json!(body)),
    ]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).expect("two retained cells");
    assert!(
        parsed
            .render_static(
                "page",
                markdown::Limits {
                    nodes: count,
                    ..Default::default()
                },
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
    parsed
        .render_static(
            "page",
            markdown::Limits {
                nodes: count * 2,
                ..Default::default()
            },
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("exact aggregate event budget");
    let late = encoded(&document(vec![
        markdown_cell("valid", json!(body)),
        code_cell(
            json!("not executed"),
            vec![
                json!({"output_type":"display_data","metadata":{},"data":{"text/html":"<script>late unsupported</script>"}}),
            ],
        ),
    ]));
    let parsed =
        ParsedNotebook::parse(&late, Default::default()).expect("late unsupported syntax retained");
    assert!(
        parsed
            .render_static(
                "page",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak
            )
            .is_err()
    );
}
