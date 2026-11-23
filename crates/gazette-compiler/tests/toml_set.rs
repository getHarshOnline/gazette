//! Complete syntax-set behavior through the real retained TOML parser.

use gazette_compiler::toml::{DocumentError, Error, Limits, ParsedSet, SetError, SetLimits};
use gazette_core::source::EntryPath;
use serde::Deserialize;

#[test]
fn independent_document_failures_are_complete_and_order_independent() {
    let a: EntryPath = "a.toml".parse().expect("path");
    let b: EntryPath = "b.toml".parse().expect("path");
    let c: EntryPath = "c.toml".parse().expect("path");
    let z: EntryPath = "z.toml".parse().expect("path");
    let inputs = [
        (&z, b"bad=[".as_slice()),
        (&c, b"valid=true".as_slice()),
        (&b, b"x='\xff'".as_slice()),
        (&a, b"bad={".as_slice()),
    ];
    let error = ParsedSet::parse(inputs, SetLimits::default()).expect_err("invalid complete set");
    let diagnostic = error.to_string();
    for name in ["a.toml", "b.toml", "z.toml"] {
        assert!(diagnostic.contains(name), "missing {name}: {diagnostic}");
    }
    assert!(
        !diagnostic.contains("c.toml"),
        "valid input is not a failure"
    );
    assert_eq!(
        ParsedSet::parse(inputs.into_iter().rev(), SetLimits::default())
            .expect_err("same failures")
            .to_string(),
        diagnostic
    );
    assert_eq!(error.code(), "toml_set_documents");
    let SetError::Documents(errors) = error else {
        panic!("expected source-local failures");
    };
    assert!(matches!(errors.as_slice(), [
        DocumentError::Toml { path: first, error: Error::Invalid { span: Some(_), .. } },
        DocumentError::Utf8 { path: middle, valid_up_to: 3 },
        DocumentError::Toml { path: last, error: Error::Invalid { span: Some(_), .. } },
    ] if first == &a && middle == &b && last == &z));
}

#[test]
fn resource_exhaustion_stops_without_claiming_complete_diagnostics() {
    let a: EntryPath = "a.toml".parse().expect("path");
    let b: EntryPath = "b.toml".parse().expect("path");
    let z: EntryPath = "z.toml".parse().expect("path");
    let sources = [
        (&a, b"bad=[".as_slice()),
        (&b, b"x=1".as_slice()),
        (&z, b"bad={".as_slice()),
    ];
    let error = ParsedSet::parse(
        sources,
        SetLimits {
            document: Limits {
                nodes: 1,
                ..Limits::default()
            },
            ..SetLimits::default()
        },
    )
    .expect_err("per-document resource stop");
    assert_eq!(error.code(), "toml_limit");
    assert!(matches!(error, SetError::Documents(errors)
        if matches!(errors.as_slice(), [DocumentError::Toml { path, error: Error::Limit("nodes") }] if path == &b)));
    assert!(matches!(
        ParsedSet::parse(
            sources,
            SetLimits {
                nodes: 1,
                ..SetLimits::default()
            }
        ),
        Err(SetError::Limit("nodes"))
    ));
    assert!(matches!(
        ParsedSet::parse(
            sources,
            SetLimits {
                documents: 2,
                ..SetLimits::default()
            }
        ),
        Err(SetError::Limit("documents"))
    ));
}

#[test]
fn retained_bytes_do_not_borrow_temporary_source_labels() {
    let parsed = {
        let path: EntryPath = "temporary-name.toml".parse().expect("label");
        ParsedSet::parse([(&path, b"x=1".as_slice())], SetLimits::default()).expect("parse")
    };
    assert_eq!(
        parsed.inspect().documents[0].path.as_str(),
        "temporary-name.toml"
    );
    assert_eq!(
        parsed
            .documents()
            .next()
            .expect("retained document")
            .1
            .source(),
        "x=1"
    );
}

#[test]
fn complete_set_is_order_independent_and_each_namespace_remains_typed_separately() {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fields {
        value: String,
    }
    let a: EntryPath = "a.toml".parse().expect("path");
    let b: EntryPath = "nested/.b".parse().expect("path");
    let inputs = [(&b, b"value='B'".as_slice()), (&a, b"value='A'".as_slice())];
    let parsed = ParsedSet::parse(inputs, SetLimits::default()).expect("complete set");
    let reversed =
        ParsedSet::parse(inputs.into_iter().rev(), SetLimits::default()).expect("reversed");
    assert_eq!(parsed.inspect(), reversed.inspect());
    let values: Vec<_> = parsed
        .documents()
        .map(|(path, doc)| {
            (
                path.as_str(),
                doc.deserialize::<Fields>()
                    .expect("explicit fixture schema")
                    .value,
            )
        })
        .collect();
    assert_eq!(values, [("a.toml", "A".into()), ("nested/.b", "B".into())]);
    let report = parsed.inspect();
    assert_eq!(
        report.source_bytes,
        inputs.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
    );
    assert_eq!(report.node_count, 4);
    assert_eq!(report.documents.len(), 2);
    assert!(!format!("{parsed:?}").contains("value='A'"));
}

#[test]
fn duplicates_late_errors_and_invalid_utf8_never_return_a_successful_subset() {
    let a: EntryPath = "a.toml".parse().expect("path");
    let z: EntryPath = "z.toml".parse().expect("path");
    let good = b"value='good'".as_slice();
    for bytes in [good, b"value='different'"] {
        assert!(
            matches!(ParsedSet::parse([(&a, good), (&a, bytes)], SetLimits::default()),
            Err(SetError::Duplicate(path)) if path == a)
        );
    }
    let prior = ParsedSet::parse([(&a, good)], SetLimits::default()).expect("prior evidence");
    let prior_report = prior.inspect();
    for inputs in [
        [(&a, good), (&z, b"bad=[".as_slice())],
        [(&z, b"bad=[".as_slice()), (&a, good)],
    ] {
        let error = ParsedSet::parse(inputs, SetLimits::default()).expect_err("invalid document");
        assert_eq!(error.code(), "toml_invalid");
        assert!(matches!(error, SetError::Documents(errors)
            if matches!(errors.as_slice(), [DocumentError::Toml { path, error: Error::Invalid { .. } }] if path == &z)));
    }
    assert!(
        matches!(ParsedSet::parse([(&a, good), (&z, b"a='\xff'")], SetLimits::default()),
        Err(SetError::Documents(errors))
            if matches!(errors.as_slice(), [DocumentError::Utf8 { path, valid_up_to: 3 }] if path == &z))
    );
    assert_eq!(
        prior.inspect(),
        prior_report,
        "new failure cannot alter prior evidence"
    );
}

#[test]
fn additions_removals_and_byte_changes_are_visible_without_merging_or_stale_fallback() {
    let a: EntryPath = "a.toml".parse().expect("path");
    let b: EntryPath = "b.toml".parse().expect("path");
    let input = [(&a, b"x=1".as_slice()), (&b, b"x=2".as_slice())];
    let full = ParsedSet::parse(input, SetLimits::default())
        .expect("full")
        .inspect();
    let removed = ParsedSet::parse([input[0]], SetLimits::default())
        .expect("removed")
        .inspect();
    assert_eq!(removed.documents.len(), 1);
    assert_eq!(removed.documents[0], full.documents[0]);
    let changed = ParsedSet::parse(
        [(&a, b"x=1 # byte change".as_slice()), input[1]],
        SetLimits::default(),
    )
    .expect("changed")
    .inspect();
    assert_ne!(
        changed.documents[0].syntax.source_sha256,
        full.documents[0].syntax.source_sha256
    );
    assert_eq!(changed.documents[1], full.documents[1]);
    let empty = ParsedSet::parse([], SetLimits::default())
        .expect("empty syntax set")
        .inspect();
    assert!(empty.documents.is_empty());
    assert_eq!((empty.source_bytes, empty.node_count), (0, 0));
}

#[test]
fn aggregate_and_per_document_limits_have_exact_boundaries() {
    let a: EntryPath = "a".parse().expect("path");
    let b: EntryPath = "b".parse().expect("path");
    let sources = [(&a, b"x=1".as_slice()), (&b, b"x=2".as_slice())];
    let exact = SetLimits {
        documents: 2,
        source_bytes: 6,
        path_bytes: 2,
        nodes: 4,
        document: Limits {
            source_bytes: 3,
            nodes: 2,
            depth: 1,
        },
    };
    assert!(ParsedSet::parse(sources, exact).is_ok());
    for (limits, expected) in [
        (
            SetLimits {
                documents: 1,
                ..exact
            },
            "documents",
        ),
        (
            SetLimits {
                source_bytes: 5,
                ..exact
            },
            "source bytes",
        ),
        (
            SetLimits {
                path_bytes: 1,
                ..exact
            },
            "path bytes",
        ),
        (SetLimits { nodes: 3, ..exact }, "nodes"),
    ] {
        assert!(
            matches!(ParsedSet::parse(sources, limits), Err(SetError::Limit(name)) if name == expected)
        );
    }
    for limits in [
        Limits {
            source_bytes: 2,
            ..exact.document
        },
        Limits {
            nodes: 1,
            ..exact.document
        },
        Limits {
            depth: 0,
            ..exact.document
        },
    ] {
        assert!(matches!(
            ParsedSet::parse(
                sources,
                SetLimits {
                    document: limits,
                    ..exact
                }
            ),
            Err(SetError::Documents(errors))
                if matches!(errors.as_slice(), [DocumentError::Toml { error: Error::Limit(_), .. }])
        ));
    }
}
