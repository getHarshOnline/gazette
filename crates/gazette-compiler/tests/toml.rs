//! Real TOML parsing and typed consumption, without a publication schema invented here.

use std::collections::BTreeMap;

use gazette_compiler::toml::{Error, Limits, ParsedToml};
use gazette_core::source::Sha256Digest;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Fixture {
    title: String,
    number: i64,
    enabled: bool,
    scores: Vec<f64>,
    extra: BTreeMap<String, String>,
}

#[test]
fn retained_toml_projects_into_an_explicit_schema_without_source_rewriting() {
    let source = "# source comment\ntitle = 'literal +++ value'\nnumber = 9223372036854775807\nenabled = true\nscores = [1.25, -2.5]\n[extra]\n'quoted.key' = 'preserve me'\n";
    let parsed = ParsedToml::parse(source, Limits::default()).expect("TOML");
    assert_eq!(parsed.source(), source);
    let report = parsed.inspect();
    assert_eq!(report.source_sha256, Sha256Digest::of(source.as_bytes()));
    assert_eq!(
        report.keys,
        ["title", "number", "enabled", "scores", "extra"]
    );
    let value: Fixture = parsed.deserialize().expect("typed fields");
    assert_eq!(value.title, "literal +++ value");
    assert_eq!(
        value.number,
        i64::MAX,
        "no intermediate JSON number truncation"
    );
    assert!(value.enabled);
    assert_eq!(value.scores, [1.25, -2.5]);
    assert_eq!(value.extra["quoted.key"], "preserve me");
    assert_eq!(
        parsed
            .deserialize::<Fixture>()
            .expect("repeat typed projection"),
        value
    );
    assert_eq!(parsed.source(), source);
    assert!(!format!("{parsed:?}").contains("preserve me"));
}

#[test]
fn malformed_duplicate_and_late_bad_input_never_produce_parsed_metadata() {
    for source in [
        "a = 1\na = 2\n",
        "title = 'valid'\nlast = [",
        "a.b = 1\na = 2\n",
        "x = nope",
        "a = { b = 1, b = 2 }",
        "[a]\n[a]\n",
    ] {
        let error = ParsedToml::parse(source, Limits::default()).expect_err(source);
        assert!(matches!(error, Error::Invalid { .. }), "{error:?}");
    }
}

#[test]
fn unknown_fields_remain_available_and_only_the_explicit_consumer_rejects_them() {
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        title: String,
    }
    let parsed = ParsedToml::parse(
        "title = 'title'\nfuture = { mode = 'pending' }\n",
        Limits::default(),
    )
    .expect("unknown syntax preserved");
    assert_eq!(parsed.inspect().keys, ["title", "future"]);
    assert!(parsed.source().contains("future"));
    assert!(matches!(
        parsed.deserialize::<Strict>(),
        Err(Error::Invalid { .. })
    ));
    let strict: Strict = ParsedToml::parse("title = 'title'", Limits::default())
        .expect("syntax")
        .deserialize()
        .expect("explicit schema");
    assert_eq!(strict.title, "title");
}

#[test]
fn node_and_depth_limits_cover_arrays_tables_inline_tables_and_implicit_parents() {
    for source in [
        "values = [1, 2, 3]",
        "a.b.c = true",
        "a = { b = { c = 1 } }",
        "[[a]]\nb = 1\n[[a]]\nb = 2\n",
    ] {
        let parsed = ParsedToml::parse(source, Limits::default()).expect("baseline");
        let nodes = parsed.inspect().node_count;
        assert!(nodes > 1);
        assert!(
            ParsedToml::parse(
                source,
                Limits {
                    source_bytes: source.len(),
                    nodes,
                    ..Limits::default()
                }
            )
            .is_ok()
        );
        assert!(matches!(
            ParsedToml::parse(
                source,
                Limits {
                    nodes: nodes - 1,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("nodes"))
        ));
        assert!(matches!(
            ParsedToml::parse(
                source,
                Limits {
                    depth: 0,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("depth"))
        ));
        assert!(matches!(
            ParsedToml::parse(
                source,
                Limits {
                    source_bytes: source.len() - 1,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit("source bytes"))
        ));
    }
    let empty = ParsedToml::parse(
        "",
        Limits {
            source_bytes: 0,
            nodes: 1,
            depth: 0,
        },
    )
    .expect("empty root");
    assert_eq!(empty.inspect().node_count, 1);
    assert!(empty.inspect().keys.is_empty());
    assert!(matches!(
        ParsedToml::parse(
            "",
            Limits {
                nodes: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("nodes"))
    ));
}
