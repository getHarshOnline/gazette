//! Complete-document framing and provenance through the real TOML/Markdown parsers.

use gazette_compiler::{
    document::{Error, Limits, ParsedDocument},
    markdown, toml,
};
use gazette_core::source::{Precision, Sha256Digest};
use serde::Deserialize;

#[path = "support/html_text.rs"]
mod html_text;

#[test]
fn full_metadata_and_body_identities_remain_distinct_and_ranges_are_exact() {
    let source = "+++\r\ntitle = 'Metadata only'\r\n[extra]\r\nmode = 'unknown'\r\n+++\r\n\r\n# Body é\r\n\r\n[ref](/docs)\r\n";
    let document =
        ParsedDocument::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("whole document");
    let report = document.inspect().expect("inspection");
    assert_eq!(
        &source[report.metadata_span.start..report.metadata_span.end],
        document.metadata().source()
    );
    let body = &source[report.body_span.start..report.body_span.end];
    assert!(body.starts_with("\r\n# Body é"), "blank lines preserved");
    assert_eq!(report.source_sha256, Sha256Digest::of(source.as_bytes()));
    assert_eq!(
        report.markdown.source_sha256,
        Sha256Digest::of(body.as_bytes())
    );
    assert_ne!(report.source_sha256, report.markdown.source_sha256);
    assert_eq!(report.metadata.keys, ["title", "extra"]);
    assert_eq!(report.markdown.headings.len(), 1);
    assert_eq!(report.markdown.headings[0].text, "Body é");
    let span = report.markdown.headings[0].span;
    assert_eq!(
        &source[report.body_span.start + span.start..report.body_span.start + span.end],
        "# Body é"
    );
    assert_eq!(report.markdown.headings[0].precision, Precision::Construct);
    let body_parser = markdown::ParsedMarkdown::parse(
        body,
        markdown::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("independent body");
    assert_eq!(report.markdown, body_parser.inspect().expect("body report"));
    assert_eq!(
        document.render_html().expect("body HTML"),
        body_parser.render_html().expect("same body HTML")
    );
    assert!(
        !document
            .render_html()
            .expect("HTML")
            .contains("Metadata only")
    );
}

#[test]
fn delimiter_like_values_and_body_delimiters_are_never_substring_split() {
    #[derive(Deserialize)]
    struct Metadata {
        title: String,
        description: String,
    }
    let source = "+++\ntitle = 'a +++ value'\ndescription = \"\"\"line one\nnot +++ a fence\nline three\"\"\"\n+++\n# Body\n\n```toml\n+++\ntitle = 'body code'\n+++\n```\n";
    let doc = ParsedDocument::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("explicit framing");
    let meta: Metadata = doc.metadata().deserialize().expect("typed metadata");
    assert_eq!(meta.title, "a +++ value");
    assert!(meta.description.contains("not +++ a fence"));
    let report = doc.inspect().expect("report");
    assert_eq!(report.markdown.headings.len(), 1);
    assert!(html_text::text(&doc.render_html().expect("body")).contains("title = 'body code'"));
}

#[test]
fn malformed_framing_or_metadata_never_falls_back_to_body_or_a_later_fence() {
    for source in [
        "# plain body",
        "\n+++\ntitle='x'\n+++\n",
        "+++",
        "+++\ntitle = 'x'",
        "+++\ntitle='x'\n+++ \n",
        "---\ntitle: x\n---\n",
        "+++\ntitle = 'x'\n+++\r",
    ] {
        assert!(
            matches!(
                ParsedDocument::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak),
                Err(Error::Framing(_))
            ),
            "{source:?}"
        );
    }
    for source in [
        "+++\ntitle = 'first'\ntitle = 'second'\n+++\n# not a fallback\n",
        "+++\ntitle = \"\"\"open\n+++\nclosed\"\"\"\n+++\n# body\n",
    ] {
        assert!(
            matches!(
                ParsedDocument::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak),
                Err(Error::Metadata { .. })
            ),
            "reserved standalone fence: {source:?}"
        );
    }
    assert!(
        ParsedDocument::parse(
            "+++\n+++",
            Limits::default(),
            &gazette_adapter_markdown::Comrak
        )
        .is_ok(),
        "empty metadata/body with EOF closing fence"
    );
}

#[test]
fn independent_whole_metadata_and_body_limits_fail_without_partial_results() {
    let source = "+++\ntitle = 'test'\n+++\n**body**\n";
    assert!(matches!(
        ParsedDocument::parse(
            source,
            Limits {
                source_bytes: source.len() - 1,
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak
        ),
        Err(Error::SourceLimit)
    ));
    assert!(matches!(
        ParsedDocument::parse(
            source,
            Limits {
                metadata: toml::Limits {
                    source_bytes: 1,
                    ..toml::Limits::default()
                },
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak
        ),
        Err(Error::Metadata {
            error: toml::Error::Limit("source bytes"),
            ..
        })
    ));
    assert!(matches!(
        ParsedDocument::parse(
            source,
            Limits {
                metadata: toml::Limits {
                    nodes: 1,
                    ..toml::Limits::default()
                },
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak
        ),
        Err(Error::Metadata {
            error: toml::Error::Limit("nodes"),
            ..
        })
    ));
    assert!(matches!(
        ParsedDocument::parse(
            source,
            Limits {
                markdown: markdown::Limits {
                    depth: 1,
                    ..markdown::Limits::default()
                },
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak
        ),
        Err(Error::Markdown {
            error: markdown::Error::Limit("syntax depth"),
            ..
        })
    ));
    let doc = ParsedDocument::parse(
        source,
        Limits {
            markdown: markdown::Limits {
                output_bytes: 1,
                ..markdown::Limits::default()
            },
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax parsed");
    assert!(doc.inspect().is_err());
    assert!(doc.render_html().is_err());
}

#[test]
fn metadata_cannot_grant_raw_html_trust_and_errors_keep_the_original_offset() {
    let source = "+++\nraw_html = true\n+++\n<script>bad()</script>\n";
    let doc = ParsedDocument::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("syntax");
    let offset = doc.inspect().expect("report").body_span.start;
    let Error::Markdown {
        error: markdown::Error::RawHtml(span),
        offset: observed,
    } = doc.render_html().expect_err("no trust inference")
    else {
        panic!("wrong error")
    };
    assert_eq!(offset, observed);
    assert!(source[offset + span.start..offset + span.end].starts_with("<script>"));
}
