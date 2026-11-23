//! Notebook structure and source retention are not execution or HTML admission.

use gazette_compiler::{
    corpus::{self, File, FileError, FileInspection, Input, ParsedCorpus},
    notebook::{self, ParsedNotebook},
};
use gazette_core::source::Sha256Digest;
use serde_json::{Value, json};

fn document(cells: Vec<Value>) -> Value {
    json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":cells})
}

fn markdown(id: &str) -> Value {
    json!({"cell_type":"markdown","id":id,"metadata":{},"source":"# Heading"})
}

fn encoded(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("fixture JSON")
}

fn reject(value: &Value, code: &str) {
    let bytes = encoded(value);
    let error = ParsedNotebook::parse(&bytes, notebook::Limits::default())
        .expect_err("invalid notebook must not be repaired or accepted");
    assert_eq!(error.code(), code, "{error}");
}

fn inputs<'a>(files: &[(&str, &'a [u8])]) -> Vec<Input<'a>> {
    files
        .iter()
        .map(|(path, bytes)| Input {
            path: path.parse().expect("fixture path"),
            bytes,
        })
        .collect()
}

#[test]
fn ordered_cells_outputs_attachments_and_private_metadata_are_retained_not_executed() {
    let mut value = document(vec![
        json!({"cell_type":"raw","id":"raw-first","metadata":{"format":"text/html"},
            "source":"<script>not_a_rendering_grant()</script>"}),
        json!({"cell_type":"markdown","id":"markdown-second","metadata":{},
            "source":["# Analysis\n","![plot](attachment:plot.png)"],
            "attachments":{"plot.png":{"image/png":"aGVsbG8="}}}),
        json!({"cell_type":"code","id":"code-last","metadata":{"collapsed":true},
        "source":["raise RuntimeError('MUST NOT EXECUTE')\n"],"execution_count":null,
        "outputs":[
            {"output_type":"stream","name":"stdout","text":["line one\n","line two"]},
            {"output_type":"stream","name":"stderr","text":"warning\n"},
            {"output_type":"error","ename":"ExampleError","evalue":"stored, not raised",
                "traceback":["private stored traceback"]},
            {"output_type":"display_data","metadata":{},"data":{
                "application/json":{"records":[1,true,null]},"text/plain":["Stored result"]}},
            {"output_type":"execute_result","metadata":{},"execution_count":7,
                "data":{"text/plain":"42","application/vnd.example+json":{"value":42}}}
        ]}),
    ]);
    value["metadata"] = json!({"authors":[{"name":"PRIVATE NOTEBOOK AUTHOR"}],
        "kernelspec":{"name":"unavailable-kernel","display_name":"Not installed"},
        "language_info":{"name":"python"},"private_path":"/private/not-a-public-path"});
    let mut bytes = serde_json::to_vec_pretty(&value).expect("formatted fixture");
    bytes.extend_from_slice(b"\r\n");
    let parsed = ParsedNotebook::parse(&bytes, notebook::Limits::default())
        .expect("schema-valid stored notebook");
    assert_eq!(parsed.source(), bytes);
    assert!(std::ptr::eq(parsed.source().as_ptr(), bytes.as_ptr()));
    assert_eq!(
        parsed.value(),
        &value,
        "no normalization, source joining or output omission"
    );
    assert!(
        std::ptr::eq(parsed.value(), parsed.value()),
        "retained tree, not another parse"
    );
    assert_eq!(parsed.value()["cells"][0]["id"], "raw-first");
    assert_eq!(parsed.value()["cells"][1]["id"], "markdown-second");
    assert_eq!(parsed.value()["cells"][2]["id"], "code-last");
    let inspection = parsed.inspect();
    assert_eq!(inspection.cells, 3);
    assert_eq!(inspection.bytes, bytes.len());
    assert_eq!(inspection.sha256, Sha256Digest::of(&bytes));
    assert_eq!(inspection.nodes, parsed.node_count());
    let report = serde_json::to_value(&inspection).expect("inspection JSON");
    assert_eq!(report.as_object().expect("report object").len(), 5);
    for field in [
        "authors",
        "attributions",
        "language",
        "metadata",
        "source",
        "outputs",
    ] {
        assert!(
            report.get(field).is_none(),
            "syntax report does not infer publication {field}"
        );
    }
    let text = serde_json::to_string(&report).expect("report text");
    assert!(!text.contains("PRIVATE NOTEBOOK AUTHOR"));
    assert!(!text.contains("/private/"));
    let debug = format!("{parsed:?}");
    for private in [
        "PRIVATE NOTEBOOK AUTHOR",
        "/private/",
        "MUST NOT EXECUTE",
        "private stored traceback",
        "aGVsbG8=",
    ] {
        assert!(
            !debug.contains(private),
            "debug must contain structural evidence only"
        );
    }
    let repeated = ParsedNotebook::parse(&bytes, notebook::Limits::default()).expect("repeat");
    assert_eq!(inspection, repeated.inspect());
    let compact = encoded(&value);
    let compact = ParsedNotebook::parse(&compact, notebook::Limits::default())
        .expect("same values, other bytes");
    assert_eq!(compact.value(), parsed.value());
    assert_ne!(compact.inspect().sha256, inspection.sha256);
    assert_eq!(compact.inspect().schema_sha256, inspection.schema_sha256);
}

#[test]
fn arbitrary_precision_stored_results_and_private_token_keys_are_preserved() {
    let source = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{"large":18446744073709551617,"precise":0.12345678901234567890123456789,"$serde_json::private::Number":"ordinary metadata","$ref":"https://example.invalid/not-a-schema"},"cells":[{"id":"one","cell_type":"code","metadata":{},"source":"not executed","execution_count":null,"outputs":[{"output_type":"display_data","metadata":{},"data":{"application/json":{"large":18446744073709551617,"precise":0.12345678901234567890123456789}}}]}]}"#;
    let parsed = ParsedNotebook::parse(source, notebook::Limits::default())
        .expect("schema-conforming exact numbers");
    assert_eq!(parsed.source(), source);
    for record in [
        &parsed.value()["metadata"],
        &parsed.value()["cells"][0]["outputs"][0]["data"]["application/json"],
    ] {
        assert_eq!(record["large"].to_string(), "18446744073709551617");
        assert_eq!(
            record["precise"].to_string(),
            "0.12345678901234567890123456789"
        );
    }
    assert_eq!(
        parsed.value()["metadata"]["$serde_json::private::Number"],
        "ordinary metadata"
    );
    assert_eq!(
        parsed.value()["metadata"]["$ref"],
        "https://example.invalid/not-a-schema"
    );
}

#[test]
fn schema_integer_checks_preserve_precision_and_do_not_panic_on_large_numbers() {
    for (number, accepted) in [
        ("18446744073709551617", true),
        ("1e400", true),
        ("18446744073709551617.1", false),
    ] {
        let source = format!(
            r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{{"id":"one","cell_type":"code","metadata":{{}},"source":"","execution_count":{number},"outputs":[]}}]}}"#
        );
        let parsed = ParsedNotebook::parse(source.as_bytes(), notebook::Limits::default());
        if accepted {
            assert!(parsed.is_ok(), "{number}: {parsed:?}");
        } else {
            assert_eq!(
                parsed
                    .expect_err("fractional count is not an integer")
                    .code(),
                "notebook_structure"
            );
        }
    }
}

#[test]
fn malformed_and_duplicate_decoded_json_members_fail_without_echoing_values() {
    for source in [
        br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[],}"#.as_slice(),
        br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]} {}"#,
        br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"metadata":{},"cells":[]}"#,
        br#"{"nbformat":4,"nbformat_minor":5,"metadata":{"secret":"PRIVATE","\u0073ecret":"OTHER"},"cells":[]}"#,
        br#"{"nbformat":4,"nbformat_minor":5,"metadata":{"nested":[{"key":1,"k\u0065y":2}]},"cells":[]}"#,
        &[0xff, 0],
    ] {
        let error = ParsedNotebook::parse(source, notebook::Limits::default()).expect_err("strict JSON");
        assert_eq!(error.code(), "notebook_json");
        assert!(!error.to_string().contains("PRIVATE"));
        assert!(!error.to_string().contains("OTHER"));
    }
    let deeply_nested = format!(
        "{{\"nbformat\":4,\"nbformat_minor\":5,\"cells\":[],\"metadata\":{{\"nested\":{}0{}}}}}",
        "[".repeat(140),
        "]".repeat(140)
    );
    assert_eq!(
        ParsedNotebook::parse(deeply_nested.as_bytes(), notebook::Limits::default())
            .expect_err("native decoder recursion guard")
            .code(),
        "notebook_json"
    );
}

#[test]
fn cell_identity_and_explicit_names_are_unique_without_automatic_repairs() {
    let mut missing = document(vec![markdown("one")]);
    missing["cells"][0]
        .as_object_mut()
        .expect("cell")
        .remove("id");
    reject(&missing, "notebook_structure");
    for id in ["", "contains space", "café", "../outside", &"a".repeat(65)] {
        reject(&document(vec![markdown(id)]), "notebook_structure");
    }
    let duplicate = document(vec![markdown("same"), markdown("same")]);
    reject(&duplicate, "notebook_duplicate_id");
    let mut names = document(vec![markdown("one"), markdown("two")]);
    names["cells"][0]["metadata"] = json!({"name":"PRIVATE NAME"});
    names["cells"][1]["metadata"] = json!({"name":"PRIVATE NAME"});
    let bytes = encoded(&names);
    let error =
        ParsedNotebook::parse(&bytes, notebook::Limits::default()).expect_err("duplicate names");
    assert!(matches!(error, notebook::Error::DuplicateName(1)));
    assert_eq!(error.code(), "notebook_duplicate_name");
    assert!(!error.to_string().contains("PRIVATE NAME"));
    names["cells"][1]["metadata"]["name"] = json!("Different");
    let bytes = encoded(&names);
    assert!(ParsedNotebook::parse(&bytes, notebook::Limits::default()).is_ok());
}

#[test]
fn known_cell_and_output_shapes_are_checked_by_the_bundled_schema() {
    for (pointer, invalid) in [
        ("/cells/0/source", json!(["valid line", 7])),
        ("/cells/0/metadata", json!([])),
        ("/cells/0/cell_type", json!("unknown")),
        ("/metadata", json!(null)),
        ("/cells", json!({})),
    ] {
        let mut value = document(vec![markdown("one")]);
        *value.pointer_mut(pointer).expect("existing fixture field") = invalid;
        reject(&value, "notebook_structure");
    }
    for output in [
        json!({"output_type":"stream","name":"stdout","text":17}),
        json!({"output_type":"error","ename":"Error","evalue":"bad","traceback":[1]}),
        json!({"output_type":"display_data","metadata":{},"data":{"text/plain":{"not":"text"}}}),
        json!({"output_type":"execute_result","metadata":{},"data":{}}),
    ] {
        reject(
            &document(vec![json!({"cell_type":"code","id":"code","metadata":{},
            "source":"pass","execution_count":null,"outputs":[output]})]),
            "notebook_structure",
        );
    }
}

#[test]
fn unsupported_versions_are_not_converted_or_guessed() {
    for (major, minor) in [(3, 0), (4, 0), (4, 4), (4, 6), (5, 0)] {
        let mut value = document(Vec::new());
        value["nbformat"] = json!(major);
        value["nbformat_minor"] = json!(minor);
        reject(&value, "notebook_version");
    }
    let mut absent = document(Vec::new());
    absent
        .as_object_mut()
        .expect("document")
        .remove("nbformat_minor");
    reject(&absent, "notebook_version");
}

#[test]
fn source_value_and_cell_limits_accept_exact_boundary_and_reject_one_short() {
    let bytes = encoded(&document(vec![markdown("one")]));
    let exact = notebook::Limits {
        source_bytes: bytes.len(),
        nodes: 10,
        cells: 1,
    };
    let parsed = ParsedNotebook::parse(&bytes, exact).expect("ten JSON values, one cell");
    assert_eq!(parsed.node_count(), 10);
    for limits in [
        notebook::Limits {
            source_bytes: bytes.len() - 1,
            ..exact
        },
        notebook::Limits { nodes: 9, ..exact },
        notebook::Limits { cells: 0, ..exact },
    ] {
        assert_eq!(
            ParsedNotebook::parse(&bytes, limits)
                .expect_err("one-short bound")
                .code(),
            "notebook_limit"
        );
    }
    let error = ParsedNotebook::parse(
        b"invalid",
        notebook::Limits {
            source_bytes: 0,
            ..exact
        },
    )
    .expect_err("source limit before decoding");
    assert!(matches!(error, notebook::Error::Limit("source bytes")));
    let empty = encoded(&document(Vec::new()));
    assert_eq!(
        ParsedNotebook::parse(
            &empty,
            notebook::Limits {
                cells: 0,
                ..notebook::Limits::default()
            }
        )
        .expect("empty notebook")
        .node_count(),
        5
    );
}

#[test]
fn corpus_recognizes_notebook_suffixes_without_roles_or_failed_syntax_fallback() {
    let bytes = encoded(&document(vec![markdown("one")]));
    let files = [
        ("b.IPyNb", bytes.as_slice()),
        ("a.ipynb", bytes.as_slice()),
        ("opaque.bin", b"not JSON".as_slice()),
    ];
    let parsed = ParsedCorpus::parse(
        inputs(&files),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("complete corpus");
    let report = parsed.inspect().expect("structural report");
    for path in ["a.ipynb", "b.IPyNb"] {
        let file = &parsed.files()[path];
        match file {
            File::Notebook(notebook) => assert_eq!(notebook.source(), bytes),
            other => panic!("notebook was not classified: {other:?}"),
        }
        assert!(file.body().is_none());
        assert!(file.body_offset().is_none());
        assert!(
            file.metadata().is_none(),
            "notebook metadata is not authored TOML"
        );
        assert_eq!(file.source_sha256(), Sha256Digest::of(&bytes));
        assert!(matches!(&report.files[path], FileInspection::Notebook(_)));
    }
    assert!(matches!(&parsed.files()["opaque.bin"], File::Asset(_)));
    let reversed = [files[2], files[1], files[0]];
    assert_eq!(
        report,
        ParsedCorpus::parse(
            inputs(&reversed),
            corpus::Limits::default(),
            &gazette_adapter_markdown::Comrak
        )
        .expect("reordered inputs")
        .inspect()
        .expect("same report")
    );
    for bad in [
        b"not JSON".as_slice(),
        b"{}",
        b"+++\ntitle='not a notebook'\n+++\nbody",
    ] {
        let selected = [("z.ipynb", bad), ("ok.bin", b"opaque"), ("a.IPYNB", bad)];
        match ParsedCorpus::parse(
            inputs(&selected),
            corpus::Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("no fallback or partial corpus")
        {
            corpus::Error::File {
                path,
                error: FileError::Notebook(_),
            } => assert_eq!(path.as_str(), "a.IPYNB"),
            other => panic!("wrong failure: {other:?}"),
        }
    }
}

#[test]
fn corpus_notebook_budgets_are_shared_and_never_refunded_per_file() {
    let bytes = encoded(&document(Vec::new()));
    let files = [("a.ipynb", bytes.as_slice()), ("b.IPYNB", bytes.as_slice())];
    let exact = corpus::Limits {
        notebook_nodes: 10,
        source_bytes: bytes.len() * 2,
        ..corpus::Limits::default()
    };
    assert!(ParsedCorpus::parse(inputs(&files), exact, &gazette_adapter_markdown::Comrak).is_ok());
    let one_short = corpus::Limits {
        notebook_nodes: 9,
        ..exact
    };
    assert!(
        ParsedCorpus::parse(
            inputs(&files[..1]),
            one_short,
            &gazette_adapter_markdown::Comrak
        )
        .is_ok()
    );
    assert!(
        ParsedCorpus::parse(
            inputs(&files[1..]),
            one_short,
            &gazette_adapter_markdown::Comrak
        )
        .is_ok()
    );
    match ParsedCorpus::parse(inputs(&files), one_short, &gazette_adapter_markdown::Comrak)
        .expect_err("combined JSON value ceiling")
    {
        corpus::Error::File {
            path,
            error: FileError::Notebook(notebook::Error::Limit("JSON values")),
        } => assert_eq!(path.as_str(), "b.IPYNB"),
        other => panic!("wrong aggregate failure: {other:?}"),
    }
    let per_file = corpus::Limits {
        notebook: notebook::Limits {
            nodes: 4,
            ..notebook::Limits::default()
        },
        ..exact
    };
    assert_eq!(
        ParsedCorpus::parse(inputs(&files), per_file, &gazette_adapter_markdown::Comrak)
            .expect_err("per-notebook ceiling")
            .code(),
        "notebook_limit"
    );
    let byte_limit = corpus::Limits {
        source_bytes: exact.source_bytes - 1,
        ..exact
    };
    assert_eq!(
        ParsedCorpus::parse(
            inputs(&files),
            byte_limit,
            &gazette_adapter_markdown::Comrak
        )
        .expect_err("complete source bytes")
        .code(),
        "corpus_limit"
    );
}

fn scan(metadata: Value, cells: Vec<Value>) -> Option<notebook::ScanFinding> {
    let bytes =
        encoded(&json!({"nbformat":4,"nbformat_minor":5,"metadata":metadata,"cells":cells}));
    ParsedNotebook::parse(&bytes, notebook::Limits::default())
        .expect("valid scan fixture")
        .scan_for_download()
}

fn code(id: &str, source: &str, outputs: Vec<Value>) -> Value {
    json!({"cell_type":"code","id":id,"metadata":{},"execution_count":1,"source":source,"outputs":outputs})
}

#[test]
fn download_scan_passes_clean_notebooks_with_near_misses() {
    let clean = scan(
        json!({"kernelspec":{"name":"python3","display_name":"Python 3","language":"python"},
               "language_info":{"name":"python","version":"3.12.1"}}),
        vec![
            json!({"cell_type":"markdown","id":"prose","metadata":{},
                   "source":"A risk-free, task-list method. See https://example.com/data/x and docs/home/notes. Use `a @ b` and @property."}),
            code(
                "fit",
                "from sklearn import svm\ntoken = read_token_from_environment()\nkey: int = 3\nratio = 1/home_count\nmonkey = 'banana'\n",
                vec![
                    json!({"output_type":"stream","name":"stdout","text":"sk-learn tutorial, AKIA too short, 10 items\n"}),
                    json!({"output_type":"display_data","metadata":{},"data":{"image/png":"/9j/AKIAQUJDREVGR0hJSktMTU5PUA+ghp_x=="}}),
                ],
            ),
        ],
    );
    assert_eq!(clean, None);
}

/// Metadata, cells, refused class, cell index, field, and the value it must not echo.
type RefusalCase = (
    Value,
    Vec<Value>,
    notebook::ScanClass,
    Option<usize>,
    &'static str,
    &'static str,
);

#[test]
fn download_scan_refuses_each_class_without_echoing_the_value() {
    let cases: Vec<RefusalCase> = vec![
        (
            json!({}),
            vec![code(
                "a",
                "x = 1",
                vec![
                    json!({"output_type":"error","ename":"OSError","evalue":"missing",
                "traceback":["\u{1b}[0;32mFile \u{1b}[0;32m/Users/privateperson/project/run.py:3\u{1b}[0m"]}),
                ],
            )],
            notebook::ScanClass::LocalPath,
            Some(0),
            "cells[0].outputs[0].traceback",
            "privateperson",
        ),
        (
            json!({"kernelspec":{"name":"python3","display_name":"Python","language":"python","argv":["python","-m","ipykernel","-f","{connection_file}"],"path":"kernels/privatekernel/python3"}}),
            vec![],
            notebook::ScanClass::LocalPath,
            None,
            "metadata.kernelspec.path",
            "privatekernel",
        ),
        (
            json!({}),
            vec![code("a", "open(r'C:\\Users\\winperson\\data.csv')", vec![])],
            notebook::ScanClass::LocalPath,
            Some(0),
            "cells[0].source",
            "winperson",
        ),
        (
            json!({}),
            vec![
                json!({"cell_type":"markdown","id":"m","metadata":{},"source":"Contact privatemail@example.org for data."}),
            ],
            notebook::ScanClass::Identity,
            Some(0),
            "cells[0].source",
            "privatemail@example.org",
        ),
        (
            json!({"execution":{"username":"loginperson"}}),
            vec![],
            notebook::ScanClass::Identity,
            None,
            "metadata.execution.username",
            "loginperson",
        ),
        (
            json!({}),
            vec![code(
                "a",
                "client = Client(api_key='sk-proj9Qx2LmZr7Tb4Wc8Vn1Kd5Hs3')",
                vec![],
            )],
            notebook::ScanClass::Secret,
            Some(0),
            "cells[0].source",
            "sk-proj9Qx2LmZr7Tb4Wc8Vn1Kd5Hs3",
        ),
        (
            json!({}),
            vec![code(
                "a",
                "x",
                vec![
                    json!({"output_type":"stream","name":"stdout","text":"AWS AKIAIOSFODNN7EXAMPLE ready\n"}),
                ],
            )],
            notebook::ScanClass::Secret,
            Some(0),
            "cells[0].outputs[0].text",
            "AKIAIOSFODNN7EXAMPLE",
        ),
        (
            json!({}),
            vec![code("a", "DB_PASSWORD = \"q8Zr2Lx9Vb4Nm7Kt1Wp3\"", vec![])],
            notebook::ScanClass::Secret,
            Some(0),
            "cells[0].source",
            "q8Zr2Lx9Vb4Nm7Kt1Wp3",
        ),
        (
            json!({}),
            vec![
                json!({"cell_type":"markdown","id":"m","metadata":{},"source":"-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\n",
                "attachments":{}}),
            ],
            notebook::ScanClass::Secret,
            Some(0),
            "cells[0].source",
            "b3BlbnNzaC1rZXktdjEAAAAA",
        ),
        (
            json!({}),
            vec![code(
                "a",
                "x",
                vec![
                    json!({"output_type":"stream","name":"stdout","text":"Serving at http://localhost:8888/?token=abc\n"}),
                ],
            )],
            notebook::ScanClass::PrivateLink,
            Some(0),
            "cells[0].outputs[0].text",
            "localhost:8888",
        ),
        (
            json!({}),
            vec![
                json!({"cell_type":"markdown","id":"m","metadata":{},"source":"See [data](http://192.168.4.20/share/report)."}),
            ],
            notebook::ScanClass::PrivateLink,
            Some(0),
            "cells[0].source",
            "192.168.4.20",
        ),
        (
            json!({}),
            vec![
                json!({"cell_type":"markdown","id":"m","metadata":{},"source":"![plot](attachment:file:secretname.png)",
                "attachments":{"file:///srv/secretname.png":{"image/png":"iVBORw0KGgo="}}}),
            ],
            notebook::ScanClass::PrivateLink,
            Some(0),
            "cells[0].attachments{#0}",
            "secretname",
        ),
    ];
    for (metadata, cells, class, cell, field, value) in cases {
        let finding = scan(metadata, cells).expect("refused download");
        assert_eq!(finding.class, class, "{field}");
        assert_eq!(finding.cell, cell, "{field}");
        assert_eq!(finding.field, field);
        assert_eq!(finding.code(), class.code());
        for diagnostic in [
            finding.to_string(),
            format!("{finding:?}"),
            serde_json::to_string(&finding).expect("finding JSON"),
        ] {
            assert!(!diagnostic.contains(value), "{diagnostic} leaks {value}");
        }
    }
}
