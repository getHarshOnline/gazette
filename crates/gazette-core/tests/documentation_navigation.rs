//! Explicit reference navigation has one named ingress and bounded effect-free semantics.

use gazette_core::{
    documentation::{
        Error, Format, Plan, PlannedProject, PlannedProjectItem, PlannedReference, ReferencePage,
        Selection, Target,
    },
    source::EntryPath,
};

fn page(path: &str) -> ReferencePage {
    ReferencePage::Page {
        path: path.parse().unwrap(),
    }
}

fn group(title: &str, pages: Vec<ReferencePage>) -> ReferencePage {
    ReferencePage::Group {
        title: title.into(),
        pages,
    }
}

fn directory(path: &str) -> ReferencePage {
    ReferencePage::Directory {
        path: path.parse().unwrap(),
    }
}

fn validate(pages: &[ReferencePage], format: Format, selection: &Selection) -> Result<(), Error> {
    let target = Target::Mintlify {
        theme: "mint".into(),
        primary_color: "#123abc".into(),
        light_color: None,
        dark_color: None,
        telemetry: None,
    };
    Plan {
        name: "Library",
        target: &target,
        projects: vec![PlannedProject {
            slug: "library",
            title: "Library",
            manual_assets: vec![],
            navigation: vec![PlannedProjectItem::Reference(PlannedReference {
                slug: "reference",
                title: "API",
                format,
                selection,
                pages: Some(pages),
            })],
        }],
    }
    .validate()
}

#[test]
fn canonical_nodes_require_named_maps_at_every_recursive_ingress() {
    let accepted = r#"{"kind":"group","title":"API","pages":[{"kind":"page","path":"index"},{"kind":"group","title":"Types","pages":[{"kind":"page","path":"types/Value"}]}]}"#;
    let node: ReferencePage = serde_json::from_str(accepted).unwrap();
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        serde_json::from_str::<serde_json::Value>(accepted).unwrap()
    );
    for invalid in [
        "null",
        "true",
        "1",
        "[]",
        r#"["page","index"]"#,
        "{}",
        r#"{"kind":"unknown","path":"index"}"#,
        r#"{"kind":"page"}"#,
        r#"{"kind":"page","path":null}"#,
        r#"{"kind":"page","path":"index","path":"other"}"#,
        r#"{"kind":"page","kind":"group","path":"index"}"#,
        r#"{"kind":"page","path":"index","title":"Other"}"#,
        r#"{"kind":"page","path":"index","pages":[]}"#,
        r#"{"kind":"directory"}"#,
        r#"{"kind":"directory","path":"types","title":"Types"}"#,
        r#"{"kind":"directory","path":"types","pages":[]}"#,
        r#"{"kind":"directory","path":"types","extra":true}"#,
        r#"{"kind":"directory","path":"types","path":"other"}"#,
        r#"{"kind":"group","pages":[]}"#,
        r#"{"kind":"group","title":"API"}"#,
        r#"{"kind":"group","title":"API","pages":null}"#,
        r#"{"kind":"group","title":"API","pages":[],"path":"index"}"#,
        r#"{"kind":"group","title":"API","pages":[],"extra":true}"#,
        r#"{"kind":"group","title":"API","pages":[["page","index"]]}"#,
        r#"{"kind":"group","title":"API","pages":[{"kind":"page","path":"index"},["page","other"]]}"#,
        r#"{"kind":"group","title":"API","pages":[{"kind":"group","title":"Nested","pages":[false]}]}"#,
        r#"{"kind":"group","title":"API","pages":[{"kind":"page","path":"index","extra":0}]}"#,
    ] {
        assert!(
            serde_json::from_str::<ReferencePage>(invalid).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn directories_round_trip_as_named_selections_without_resolving_source_bytes() {
    let source =
        r#"{"kind":"group","title":"Types","pages":[{"kind":"directory","path":"types"}]}"#;
    let node: ReferencePage = serde_json::from_str(source).unwrap();
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        serde_json::from_str::<serde_json::Value>(source).unwrap()
    );
    let selection = Selection::Directory { path: None };
    validate(
        &[page("types"), directory("types")],
        Format::Markdown,
        &selection,
    )
    .unwrap();
    for pages in [
        vec![directory("types"), directory("types")],
        vec![directory("types.md")],
    ] {
        assert!(validate(&pages, Format::Markdown, &selection).is_err());
    }
}

#[test]
fn shape_validation_is_byte_free_and_refuses_unsupported_artifact_modes() {
    let directory = Selection::Directory { path: None };
    let file = Selection::File {
        path: "reference.md".parse().unwrap(),
    };
    let pages = [page("index"), group("Members", vec![page("types/Value")])];
    validate(&pages, Format::Markdown, &directory).unwrap();
    assert!(validate(&pages, Format::Markdown, &file).is_err());
    for format in [
        Format::TypeDocJson,
        Format::OpenApiJson,
        Format::SphinxJson,
        Format::RustdocHtml,
    ] {
        assert!(validate(&pages, format, &directory).is_err());
    }
}

#[test]
fn direct_sdk_values_cannot_bypass_navigation_semantics() {
    let directory = Selection::Directory { path: None };
    for pages in [
        vec![],
        vec![group("Empty", vec![])],
        vec![group(" ", vec![page("index")])],
        vec![group("Bad\nlabel", vec![page("index")])],
        vec![page("index"), group("Duplicate", vec![page("index")])],
        vec![page("index.md")],
        vec![page("type.with-dot")],
    ] {
        assert!(validate(&pages, Format::Markdown, &directory).is_err());
    }
    // This coordinate is reference-relative, not the global artifacts transport namespace.
    validate(&[page("artifacts/index")], Format::Markdown, &directory).unwrap();
    assert!("../index".parse::<EntryPath>().is_err());
}

#[test]
fn engine_group_depth_has_an_exact_direct_sdk_boundary() {
    let directory = Selection::Directory { path: None };
    let mut node = page("index");
    for _ in 0..64 {
        node = group("Nested", vec![node]);
    }
    validate(std::slice::from_ref(&node), Format::Markdown, &directory).unwrap();
    assert!(
        validate(
            &[group("Too deep", vec![node])],
            Format::Markdown,
            &directory
        )
        .is_err()
    );
}
