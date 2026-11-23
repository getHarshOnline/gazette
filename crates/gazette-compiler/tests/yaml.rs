//! Landing-source prerequisites: bounded parsing and honest source locations.

use gazette_compiler::{
    toml::PathSegment,
    yaml::{Limits, ParsedYaml},
};
use gazette_core::source::Sha256Digest;
use serde::Deserialize;

#[test]
fn preserves_original_identity_and_unicode_construct_locations() {
    let source = "title: दृष्टि\r\nsections:\r\n  - body: |\r\n      A question changes.\r\n      Another line.\r\n  - body: 'A second passage'\r\n";
    let parsed = ParsedYaml::parse(source, Limits::default()).unwrap();
    assert_eq!(parsed.source(), source);
    assert_eq!(parsed.source_sha256(), Sha256Digest::of(source.as_bytes()));
    let span = parsed
        .span_at(&[
            PathSegment::Key("sections".into()),
            PathSegment::Index(0),
            PathSegment::Key("body".into()),
        ])
        .unwrap();
    let construct = &source[span.start..span.end];
    assert!(
        construct.starts_with("A question changes."),
        "{construct:?}"
    );
    assert!(construct.contains("A question changes."));
    assert!(!construct.contains("A second passage"));
    assert_eq!(
        parsed.value()["sections"][0]["body"],
        "A question changes.\nAnother line.\n"
    );
    assert!(
        parsed
            .span_at(&[PathSegment::Key("absent".into())])
            .is_none()
    );
}

#[test]
fn refuses_ambiguous_or_expanding_yaml() {
    for source in [
        "a: 1\na: 2\n",
        "a: &shared value\nb: *shared\n",
        "a: !custom value\n",
        "a: {<<: {b: 2}}\n",
        "? [a, b]\n: value\n",
        "a: 1\n---\nb: 2\n",
        "- item\n",
    ] {
        assert!(
            ParsedYaml::parse(source, Limits::default()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn plain_scalars_do_not_invent_yaml_11_booleans_or_dates() {
    let parsed = ParsedYaml::parse(
        "yes: yes\non: on\ndate: 2026-10-19\nflag: true\nnumber: 12\nquoted: '12'\n",
        Limits::default(),
    )
    .unwrap();
    assert_eq!(parsed.value()["yes"], "yes");
    assert_eq!(parsed.value()["on"], "on");
    assert_eq!(parsed.value()["date"], "2026-10-19");
    assert_eq!(parsed.value()["flag"], true);
    assert_eq!(parsed.value()["number"], 12);
    assert_eq!(parsed.value()["quoted"], "12");
}

#[test]
fn consumer_schema_rejects_unknown_fields_without_losing_the_source() {
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Declaration {
        title: String,
    }
    let parsed = ParsedYaml::parse("title: A question\n", Limits::default()).unwrap();
    assert_eq!(
        parsed.deserialize::<Declaration>().unwrap().title,
        "A question"
    );
    let parsed = ParsedYaml::parse("title: A question\nextra: true\n", Limits::default()).unwrap();
    assert_eq!(
        parsed.deserialize::<Declaration>().unwrap_err().code,
        "yaml_schema"
    );
}

#[test]
fn every_ceiling_is_enforced() {
    let cases = [
        Limits {
            source_bytes: 3,
            ..Limits::default()
        },
        Limits {
            nodes: 2,
            ..Limits::default()
        },
        Limits {
            depth: 0,
            ..Limits::default()
        },
        Limits {
            decoded_bytes: 2,
            ..Limits::default()
        },
    ];
    for limits in cases {
        assert_eq!(
            ParsedYaml::parse("a: [long value]\n", limits)
                .unwrap_err()
                .code,
            "yaml_limit"
        );
    }
}

#[test]
fn duplicate_diagnostic_is_a_valid_utf8_range_after_non_ascii_text() {
    let source = "title: दृष्टि\na: first\na: second\n";
    let error = ParsedYaml::parse(source, Limits::default()).unwrap_err();
    let span = error.span.unwrap();
    assert_eq!(&source[span.start..], "a: second\n");
}

#[test]
fn unicode_inside_block_scalars_does_not_shift_following_fields() {
    for style in ["|", ">"] {
        let source =
            format!("body: {style}\r\n  An account of दृष्टि and 雪.\r\nnext: 'The next field'\r\n");
        let parsed = ParsedYaml::parse(&source, Limits::default()).unwrap();
        let span = parsed.span_at(&[PathSegment::Key("next".into())]).unwrap();
        assert!(source[span.start..span.end].starts_with("'The next field'"));
        let duplicate = format!("{source}next: repeated\r\n");
        let error = ParsedYaml::parse(&duplicate, Limits::default()).unwrap_err();
        assert_eq!(
            &duplicate[error.span.unwrap().start..],
            "next: repeated\r\n"
        );
    }
}
