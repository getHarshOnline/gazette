//! Cross-document source navigation shares native parsing and emitted anchor identity.

use gazette_compiler::corpus::{self, Error, Input, ParsedCorpus};
use gazette_core::source::Precision;

fn parse<'a>(sources: &[(&str, &'a str)]) -> ParsedCorpus<'a> {
    ParsedCorpus::parse(
        sources.iter().map(|(path, source)| Input {
            path: path.parse().expect("logical path"),
            bytes: source.as_bytes(),
        }),
        corpus::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("selected corpus")
}

#[test]
fn selected_heading_and_body_definitions_retain_native_coordinates() {
    let origin = "+++\r\ntitle = 'Origin'\r\n+++\r\n[heading](../guide%20one.md?view=full#caf%C3%A9) [body](../guide%20one.md)\r\n";
    let target = "+++\r\ntitle = 'Guide'\r\n+++\r\n# Unicode {#\"café\"}\r\n\r\nBody.\r\n";
    let corpus = parse(&[
        ("docs/nested/start.md", origin),
        ("docs/guide one.md", target),
    ]);
    let records = corpus
        .document_references(10, 4096)
        .expect("complete relations");
    assert_eq!(records.len(), 2);
    let source_body = corpus.files()["docs/nested/start.md"].body().expect("body");
    let target_body = corpus.files()["docs/guide one.md"].body().expect("body");
    for record in &records {
        assert_eq!(record.path.as_str(), "docs/nested/start.md");
        assert_eq!(record.target.as_str(), "docs/guide one.md");
        assert_eq!(record.location.precision, Precision::Construct);
        assert!(
            record
                .location
                .span
                .slice(source_body.source())
                .expect("link")
                .starts_with('[')
        );
    }
    let heading = &target_body.headings().expect("same anchor allocation")[0];
    assert_eq!(records[0].definition.span, heading.span);
    assert_eq!(records[0].definition.precision, heading.precision);
    assert_eq!(
        records[1].definition.span.slice(target_body.source()),
        Some(target_body.source())
    );
    assert_eq!(
        records,
        corpus.document_references(10, 4096).expect("repeat")
    );
    assert!(
        corpus.files()["docs/guide one.md"]
            .body_offset()
            .expect("frame")
            > 0
    );
}

#[test]
fn exact_selected_paths_and_target_allocations_prevent_namesake_inference() {
    let corpus = parse(&[
        (
            "a.md",
            "[first](one.md#hello-1) [second](two.md#hello) [self](a.md#start)\n# Start\n",
        ),
        ("one.md", "# Hello\n# Hello\n# Manual {#hello}\n"),
        ("two.md", "# Hello\n"),
    ]);
    let references = corpus.document_references(3, 4096).expect("three targets");
    assert_eq!(
        references
            .iter()
            .map(|r| r.target.as_str())
            .collect::<Vec<_>>(),
        ["one.md", "two.md", "a.md"]
    );
    for reference in references {
        let target = corpus.files()[reference.target]
            .body()
            .expect("selected body");
        assert_eq!(
            reference.definition.span,
            target.headings().expect("headings")[0].span
        );
    }
}

#[test]
fn out_of_scope_destinations_do_not_invent_associations() {
    let corpus = parse(&[
        (
            "a.md",
            "# Local\n[local](#local) ![image](b.md#target) [external](https://example.com/b.md#target) [root](/b.md#target) [unselected](absent.md#missing) [directory](guide) [asset](asset.txt) [footnote](b.md#fn-note-6e6f7465)\n",
        ),
        ("b.md", "# Target\nText[^note].\n\n[^note]: Note\n"),
        ("guide/index.md", "# Not inferred\n"),
        ("asset.txt", "# Not Markdown\n"),
    ]);
    assert!(
        corpus
            .document_references(0, 0)
            .expect("outside scope")
            .is_empty()
    );
    assert_eq!(
        corpus.files()["a.md"]
            .body()
            .expect("body")
            .heading_references()
            .expect("existing local owner")
            .len(),
        1
    );
}

#[test]
fn invalid_lexical_paths_and_selected_fragments_refuse_the_whole_graph() {
    for destination in [
        "../b.md",
        "%2e%2e/b.md",
        "b%2f.md",
        "b%5c.md",
        "b%zz.md",
        "b%00.md",
        "./b.md#missing",
        "b.md#bad%",
        "b.md#%FF",
        "a//b.md",
    ] {
        let source = format!("[valid](b.md#target) [invalid]({destination})\n");
        let corpus = parse(&[("a.md", &source), ("b.md", "# Target\n")]);
        assert!(
            matches!(corpus.document_references(10, 4096), Err(Error::File { path, .. }) if path.as_str() == "a.md"),
            "{destination}"
        );
    }
}

#[test]
fn complete_projection_is_bounded_before_returning_records() {
    let corpus = parse(&[
        ("a.md", "[one](b.md) [two](b.md#target)\n"),
        ("b.md", "# Target\n"),
    ]);
    assert!(matches!(
        corpus.document_references(1, usize::MAX),
        Err(Error::Limit("document reference occurrences"))
    ));
    assert!(matches!(
        corpus.document_references(10, 0),
        Err(Error::Limit("document reference bytes"))
    ));
    assert_eq!(
        corpus.document_references(2, 4096).expect("complete").len(),
        2
    );
}

#[test]
fn reference_definitions_share_native_link_resolution_and_corpus_order() {
    let sources = [
        ("z.md", "[read][guide]\n\n[guide]: ./a.md#target\n"),
        ("a.md", "# Target\n[back](z.md)\n"),
    ];
    let forward = parse(&sources);
    let reversed = parse(&[sources[1], sources[0]]);
    assert_eq!(
        forward.document_references(10, 4096).expect("forward"),
        reversed.document_references(10, 4096).expect("reverse")
    );
    let references = forward
        .document_references(10, 4096)
        .expect("resolved reference link");
    assert_eq!(references.len(), 2);
    assert_eq!(references[1].path.as_str(), "z.md");
    assert_eq!(references[1].target.as_str(), "a.md");
}
