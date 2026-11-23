//! Whole-source syntax compilation: ordered, bounded, lossless and non-authorizing.

use gazette_compiler::corpus::{self, File, FileError, FileInspection, Input, ParsedCorpus};

fn inputs<'a>(files: &[(&str, &'a [u8])]) -> Vec<Input<'a>> {
    files
        .iter()
        .map(|(path, bytes)| Input {
            path: path.parse().expect("path"),
            bytes,
        })
        .collect()
}

#[test]
fn retained_syntax_and_assets_are_complete_and_order_independent() {
    let sources: [(&str, &[u8]); 3] = [
        ("docs/README.MD", b"# Native\n\n[Guide](guide.markdown)\n"),
        (
            "docs/guide.markdown",
            b"+++\r\ntitle = 'Guide'\r\n+++\r\n## Usage\n",
        ),
        ("docs/image.bin", &[0xff, 0, 0x80]),
    ];
    let parsed = ParsedCorpus::parse(
        inputs(&sources),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("corpus");
    let report = parsed.inspect().expect("report");
    let mut reversed = sources;
    reversed.reverse();
    let other = ParsedCorpus::parse(
        inputs(&reversed),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("reverse");
    assert_eq!(report, other.inspect().expect("report"));
    assert_eq!(report.files.len(), 3);
    assert_eq!(
        report.source_bytes,
        sources.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
    );
    let native = &parsed.files()["docs/README.MD"];
    assert!(native.metadata().is_none(), "no synthesized frontmatter");
    assert_eq!(
        native.body().expect("AST").render_html().expect("fragment"),
        "<h1 id=\"native\">Native</h1>\n<p><a href=\"guide.markdown\">Guide</a></p>\n"
    );
    let document = &parsed.files()["docs/guide.markdown"];
    assert_eq!(
        document.metadata().expect("metadata").inspect().keys,
        ["title"]
    );
    assert_eq!(
        document
            .body()
            .expect("AST")
            .render_html()
            .expect("fragment"),
        "<h2 id=\"usage\">Usage</h2>\n"
    );
    match &report.files["docs/guide.markdown"] {
        FileInspection::Document(syntax) => {
            assert_eq!(syntax.body_span.start, 27);
            assert_eq!(syntax.metadata_span.start, 5);
        }
        other => panic!("expected framed document: {other:?}"),
    }
    match &parsed.files()["docs/image.bin"] {
        File::Asset(bytes) => assert_eq!(*bytes, sources[2].1),
        other => panic!("expected binary asset: {other:?}"),
    }
}

#[test]
fn explicit_bad_framing_never_falls_back_and_failure_is_path_ordered() {
    for source in [
        b"+++".as_slice(),
        b"+++\nkey='value'\n",
        b"+++\na = [\n+++\nbody",
    ] {
        let error = ParsedCorpus::parse(
            inputs(&[("bad.md", source)]),
            corpus::Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("framing or TOML");
        assert!(matches!(
            error,
            corpus::Error::File {
                error: FileError::Document(_),
                ..
            }
        ));
    }
    let sources: [(&str, &[u8]); 3] = [("z.md", &[0xff]), ("ok.md", b"valid"), ("a.md", &[0xfe])];
    let mut reversed = sources;
    reversed.reverse();
    for selected in [&sources, &reversed] {
        match ParsedCorpus::parse(
            inputs(selected),
            corpus::Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("no subset")
        {
            corpus::Error::File {
                path,
                error: FileError::Utf8(error),
            } => {
                assert_eq!(path.as_str(), "a.md");
                assert_eq!(error.valid_up_to(), 0);
            }
            other => panic!("expected first source diagnostic: {other:?}"),
        }
    }
}

#[test]
fn duplicate_and_complete_input_budgets_precede_syntax() {
    let duplicate = ParsedCorpus::parse(
        inputs(&[("a.md", b"bad"), ("a.md", b"bad")]),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect_err("duplicate");
    assert!(matches!(duplicate, corpus::Error::Duplicate(_)));
    for limits in [
        corpus::Limits {
            files: 1,
            ..corpus::Limits::default()
        },
        corpus::Limits {
            source_bytes: 2,
            ..corpus::Limits::default()
        },
        corpus::Limits {
            path_bytes: 3,
            ..corpus::Limits::default()
        },
    ] {
        let error = ParsedCorpus::parse(
            inputs(&[("a.md", &[0xff]), ("z.bin", b"asset")]),
            limits,
            &gazette_adapter_markdown::Comrak,
        )
        .expect_err("preflight before UTF8");
        if limits.source_bytes == 2 {
            assert!(
                matches!(
                    error,
                    corpus::Error::SourceBytesLimit {
                        maximum: 2,
                        attempted: Some(6)
                    }
                ),
                "{error}"
            );
            assert_eq!(error.code(), "corpus_limit");
            assert!(
                error
                    .to_string()
                    .contains("corpus.source_bytes (--max-corpus-bytes)")
            );
        } else {
            assert!(matches!(error, corpus::Error::Limit(_)), "{error}");
        }
    }
}

#[test]
fn aggregate_ast_budgets_cannot_be_reset_per_file() {
    let sources: [(&str, &[u8]); 2] =
        [("a.md", b"+++\nk=1\n+++\na"), ("b.md", b"+++\nk=1\n+++\nb")];
    let one = ParsedCorpus::parse(
        inputs(&sources[..1]),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("one file");
    let file = &one.files()["a.md"];
    let markdown_nodes = file.body().expect("body").node_count();
    let nodes = file.metadata().expect("metadata").node_count();
    assert!(markdown_nodes > 0 && nodes > 0);
    for limits in [
        corpus::Limits {
            markdown_nodes,
            ..corpus::Limits::default()
        },
        corpus::Limits {
            metadata_nodes: nodes,
            ..corpus::Limits::default()
        },
    ] {
        assert!(
            ParsedCorpus::parse(
                inputs(&sources[..1]),
                limits,
                &gazette_adapter_markdown::Comrak
            )
            .is_ok()
        );
        assert!(
            ParsedCorpus::parse(
                inputs(&sources[1..]),
                limits,
                &gazette_adapter_markdown::Comrak
            )
            .is_ok()
        );
        match ParsedCorpus::parse(inputs(&sources), limits, &gazette_adapter_markdown::Comrak)
            .expect_err("shared ceiling")
        {
            corpus::Error::File { path, .. } => assert_eq!(path.as_str(), "b.md"),
            other => panic!("expected later document failure: {other}"),
        }
    }
}

#[test]
fn assets_are_not_omitted_and_syntax_is_not_html_admission() {
    let sources: [(&str, &[u8]); 2] = [
        ("private.Md", b"<script>unsafe()</script>"),
        ("opaque.toml", &[0xff]),
    ];
    let parsed = ParsedCorpus::parse(
        inputs(&sources),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax only");
    let report = parsed.inspect().expect("explicit unsafe syntax");
    match &report.files["private.Md"] {
        FileInspection::Markdown(syntax) => assert!(!syntax.raw_html.is_empty()),
        other => panic!("expected Markdown: {other:?}"),
    }
    assert!(
        parsed.files()["private.Md"]
            .body()
            .expect("AST")
            .render_html()
            .is_err()
    );
    let changed = ParsedCorpus::parse(
        inputs(&[("opaque.toml", &[0xfe])]),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("changed asset")
    .inspect()
    .expect("evidence");
    assert_ne!(report.files["opaque.toml"], changed.files["opaque.toml"]);
}

#[test]
fn projection_exhaustion_preserves_the_ast_but_returns_no_partial_report() {
    let mut limits = corpus::Limits::default();
    limits.document.markdown.output_bytes = 4;
    let parsed = ParsedCorpus::parse(
        inputs(&[("a.md", b"a"), ("z.md", b"more than four bytes")]),
        limits,
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax fits");
    match parsed.inspect().expect_err("whole inspection must fail") {
        corpus::Error::File {
            path,
            error: FileError::Markdown(gazette_compiler::markdown::Error::Limit(_)),
        } => assert_eq!(path.as_str(), "z.md"),
        other => panic!("wrong projection failure: {other:?}"),
    }
    assert_eq!(
        parsed.files().len(),
        2,
        "projection does not mutate retained syntax"
    );
}

#[test]
fn structured_suffix_is_explicit_bounded_and_keeps_original_source_coordinates() {
    use gazette_core::source::{Location, Precision, Sha256Digest};
    let source = "format: gazette-landing/v1\ncontent: {title: Example}\nsections:\n  - heading: {level: 2, text: 'नमस्ते'}\n    body: Text\n";
    let parsed = ParsedCorpus::parse(
        inputs(&[
            ("index.landing.yaml", source.as_bytes()),
            ("retained.yaml", b"not a landing"),
        ]),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let file = &parsed.files()["index.landing.yaml"];
    assert!(matches!(file, File::Landing(_)));
    assert!(matches!(parsed.files()["retained.yaml"], File::Asset(_)));
    assert!(
        file.body_offset().is_none(),
        "normalized body has no affine source offset"
    );
    assert_eq!(file.source_sha256(), Sha256Digest::of(source.as_bytes()));
    let heading = &file.body().unwrap().headings().unwrap()[0];
    let location = file
        .body_location(Location {
            span: heading.span,
            precision: heading.precision,
        })
        .unwrap();
    assert_eq!(location.precision, Precision::Construct);
    assert!(location.span.slice(source).unwrap().contains("नमस्ते"));
    let limits = corpus::Limits {
        metadata_nodes: 1,
        ..Default::default()
    };
    assert!(
        ParsedCorpus::parse(
            inputs(&[("index.landing.yaml", source.as_bytes())]),
            limits,
            &gazette_adapter_markdown::Comrak
        )
        .is_err()
    );
    assert!(
        ParsedCorpus::parse(
            inputs(&[("bad.landing.yaml", b"not a landing")]),
            Default::default(),
            &gazette_adapter_markdown::Comrak
        )
        .is_err()
    );
}
