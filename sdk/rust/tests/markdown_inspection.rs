//! Pure SDK reports preserve native inspection, framing, safety and wire contracts.

use gazette_sdk::{
    document, markdown,
    source::{Precision, Sha256Digest},
};
use serde_json::{Value, json};

const BODY: &str = "# Héllo 😀\r\n\r\n- [x] done\r\n\r\n[link](https://example.test/path) and note[^n].\r\n\r\n[^n]: retained **footnote**\r\n";

fn output_schema<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(
        schemars::generate::SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
    .expect("native emitted report schema")
}

#[test]
fn markdown_report_matches_retained_native_inspection_and_optional_html() {
    let parsed = markdown::parse(BODY, Default::default()).expect("native adapter parse");
    let inspection = parsed.inspect().expect("native inspection");
    let html = parsed.render_html().expect("native safe rendering");
    for include_html in [false, true] {
        let report =
            markdown::inspect(BODY, Default::default(), include_html).expect("shared report");
        assert_eq!(report.inspection, inspection);
        assert_eq!(
            report.html.as_deref(),
            include_html.then_some(html.as_str())
        );
        assert_eq!(
            report.inspection.source_sha256,
            Sha256Digest::of(BODY.as_bytes())
        );
        let heading = &report.inspection.headings[0];
        assert_eq!(heading.span.slice(BODY), Some("# Héllo 😀"));
        assert_eq!(heading.precision, Precision::Construct);
        let link = &report.inspection.links[0];
        assert_eq!(
            link.span.slice(BODY),
            Some("[link](https://example.test/path)")
        );
        assert_eq!(report.inspection.footnotes.len(), 1);
        assert_eq!(report.inspection.footnote_references.len(), 1);
    }
}

#[test]
fn framed_report_preserves_full_metadata_and_body_relative_unicode_coordinates() {
    let metadata = "title='Métadata 😀 only'\r\n";
    let source = format!("+++\r\n{metadata}+++\r\n{BODY}");
    let parsed = document::parse(&source, Default::default()).expect("explicit framed parse");
    let report = document::inspect(&source, Default::default(), true).expect("framed report");
    assert_eq!(
        report.document,
        parsed.inspect().expect("same native document evidence")
    );
    assert_eq!(
        report.html.as_deref(),
        Some(parsed.render_html().expect("same body HTML").as_str())
    );
    assert_eq!(report.document.metadata_span.slice(&source), Some(metadata));
    assert_eq!(report.document.body_span.slice(&source), Some(BODY));
    assert_eq!(
        report.document.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(
        report.document.metadata.source_sha256,
        Sha256Digest::of(metadata.as_bytes())
    );
    assert_eq!(
        report.document.markdown.source_sha256,
        Sha256Digest::of(BODY.as_bytes())
    );
    let heading = &report.document.markdown.headings[0];
    assert_eq!(heading.span.slice(BODY), Some("# Héllo 😀"));
    let full = heading
        .span
        .in_parent(&source, report.document.body_span)
        .expect("checked body-to-document translation");
    assert_eq!(
        full.start,
        source.find("# Héllo").expect("exact original heading")
    );
    assert_eq!(full.slice(&source), Some("# Héllo 😀"));
    assert!(
        !report
            .html
            .as_ref()
            .expect("requested HTML")
            .contains("Métadata")
    );
    let body = markdown::inspect(BODY, Default::default(), true).expect("same body-only owner");
    assert_eq!(report.document.markdown, body.inspection);
    assert_eq!(report.html, body.html);
}

#[test]
fn report_serialization_and_schema_distinguish_required_nulls_from_omitted_html() {
    let markdown_schema = output_schema::<markdown::Report>();
    let markdown_validator =
        jsonschema::validator_for(&markdown_schema).expect("Markdown output schema");
    let document_schema = output_schema::<document::Report>();
    let document_validator =
        jsonschema::validator_for(&document_schema).expect("document output schema");
    for include_html in [false, true] {
        let markdown = json!(
            markdown::inspect("# Parent\n", Default::default(), include_html)
                .expect("native report")
        );
        let document = json!(
            document::inspect(
                "+++\ntitle='A'\n+++\n# Parent\n",
                Default::default(),
                include_html
            )
            .expect("native document report")
        );
        for (value, validator, inspection_key) in [
            (markdown, &markdown_validator, "inspection"),
            (document, &document_validator, "document"),
        ] {
            assert!(validator.is_valid(&value), "{value}");
            assert_eq!(value.get("html").is_some(), include_html);
            let pointer = if inspection_key == "inspection" {
                "/inspection/headings/0"
            } else {
                "/document/markdown/headings/0"
            };
            assert_eq!(
                value.pointer(&format!("{pointer}/parent")),
                Some(&Value::Null)
            );
            let mut missing_null = value.clone();
            missing_null
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .expect("actual heading")
                .remove("parent");
            assert!(
                !validator.is_valid(&missing_null),
                "serialized parent is required even when null"
            );
            let mut missing_report = value.clone();
            missing_report
                .as_object_mut()
                .expect("report object")
                .remove(inspection_key);
            assert!(!validator.is_valid(&missing_report));
            let mut wrong_html = value;
            wrong_html["html"] = json!(42);
            assert!(!validator.is_valid(&wrong_html));
        }
    }
}

#[test]
fn all_markdown_limits_preserve_native_failures_and_byte_boundary_is_exact() {
    let limits = markdown::Limits {
        source_bytes: BODY.len(),
        ..Default::default()
    };
    assert!(markdown::inspect(BODY, limits, false).is_ok());
    let defaults = markdown::Limits::default();
    for limits in [
        markdown::Limits {
            source_bytes: BODY.len() - 1,
            ..defaults
        },
        markdown::Limits {
            nodes: 0,
            ..defaults
        },
        markdown::Limits {
            depth: 0,
            ..defaults
        },
        markdown::Limits {
            fact_bytes: 0,
            ..defaults
        },
        markdown::Limits {
            output_bytes: 0,
            ..defaults
        },
    ] {
        let expected = markdown::parse(BODY, limits)
            .and_then(|parsed| parsed.inspect())
            .expect_err("native ceiling refusal");
        for include_html in [false, true] {
            let actual = markdown::inspect(BODY, limits, include_html)
                .expect_err("no partial report at a native ceiling");
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn document_limits_keep_native_metadata_and_body_error_coordinates() {
    let source = format!("+++\r\ntitle='Metadata'\r\n+++\r\n{BODY}");
    let defaults = document::Limits::default();
    assert!(
        document::inspect(
            &source,
            document::Limits {
                source_bytes: source.len(),
                ..defaults
            },
            false
        )
        .is_ok()
    );
    for limits in [
        document::Limits {
            source_bytes: source.len() - 1,
            ..defaults
        },
        document::Limits {
            metadata: gazette_sdk::toml::Limits {
                source_bytes: 0,
                ..defaults.metadata
            },
            ..defaults
        },
        document::Limits {
            metadata: gazette_sdk::toml::Limits {
                nodes: 0,
                ..defaults.metadata
            },
            ..defaults
        },
        document::Limits {
            metadata: gazette_sdk::toml::Limits {
                depth: 0,
                ..defaults.metadata
            },
            ..defaults
        },
        document::Limits {
            markdown: markdown::Limits {
                nodes: 0,
                ..defaults.markdown
            },
            ..defaults
        },
    ] {
        let expected = document::parse(&source, limits)
            .and_then(|parsed| parsed.inspect())
            .expect_err("native document ceiling");
        for include_html in [false, true] {
            let actual = document::inspect(&source, limits, include_html)
                .expect_err("no partial framed report");
            assert_eq!(actual.code(), expected.code());
            assert_eq!(
                actual.to_string(),
                expected.to_string(),
                "preserve native document offsets"
            );
        }
    }
}

#[test]
fn optional_html_failure_never_returns_an_earlier_successful_inspection() {
    for source in [
        "# Safe first\n\n<script>private()</script>\n",
        "# Safe first\n\n[bad](javascript:private)\n",
    ] {
        let inspected = markdown::inspect(source, Default::default(), false)
            .expect("syntax inspection may retain unsafe evidence");
        assert!(inspected.html.is_none());
        let expected = markdown::parse(source, Default::default())
            .expect("native source parse")
            .render_html()
            .expect_err("native HTML gate");
        let error = markdown::inspect(source, Default::default(), true)
            .expect_err("no partial report after HTML refusal");
        assert_eq!(error, expected);
        let framed = format!("+++\r\ntitle='A'\r\n+++\r\n{source}");
        assert!(document::inspect(&framed, Default::default(), false).is_ok());
        let error =
            document::inspect(&framed, Default::default(), true).expect_err("framed HTML refusal");
        match error {
            document::Error::Markdown { error, offset } => {
                assert_eq!(error, expected);
                assert_eq!(offset, framed.find(source).expect("exact body start"));
            }
            other => panic!("expected native body error, got {other}"),
        }
    }
    let source = "*a* ".repeat(128);
    let inspection = markdown::parse(&source, Default::default())
        .expect("native parse")
        .inspect()
        .expect("native text");
    let limits = markdown::Limits {
        output_bytes: inspection.text.len() + 16,
        ..Default::default()
    };
    assert!(
        markdown::inspect(&source, limits, false).is_ok(),
        "inspection fits while expanded HTML does not"
    );
    let expected = markdown::parse(&source, limits)
        .expect("bounded parse")
        .render_html()
        .expect_err("actual HTML expansion exceeds output ceiling");
    assert_eq!(
        markdown::inspect(&source, limits, true).expect_err("no earlier report leaked"),
        expected
    );
}

#[test]
fn framing_is_explicit_and_inspection_does_not_resolve_link_paths() {
    let source = "+++\ntitle='uninterpreted body text'\n+++\n# Body\n";
    let report = markdown::inspect(source, Default::default(), false)
        .expect("ordinary Markdown without framing inference");
    assert_eq!(
        report.inspection.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert!(report.inspection.text.contains("uninterpreted body text"));
    assert!(matches!(
        document::inspect("# Plain Markdown\n", Default::default(), false),
        Err(document::Error::Framing(_))
    ));
    let links = "[local](./does-not-exist.md) ![image](./does-not-exist.png)\n";
    let report = markdown::inspect(links, Default::default(), true)
        .expect("pure syntax/HTML never opens either path");
    assert_eq!(report.inspection.links.len(), 2);
    assert!(
        report
            .html
            .expect("requested HTML")
            .contains("does-not-exist.md")
    );
}
