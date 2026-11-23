//! Native artifact scope is explicit; selection never guesses a marker or sibling.

use gazette_core::{
    documentation::{Artifact, Format, Selection},
    source::{CapturedSource, Content, InventoryBuilder, Limits, Sha256Digest},
};
use std::collections::BTreeMap;

fn capture() -> CapturedSource {
    let mut inventory = InventoryBuilder::new(Limits::default());
    for path in ["docs", "empty"] {
        inventory
            .insert(path.parse().expect("path"), Content::Directory)
            .expect("directory");
    }
    inventory
        .insert(
            "docs/index.json".parse().expect("path"),
            Content::File {
                bytes: 2,
                sha256: Sha256Digest::of(b"{}"),
            },
        )
        .expect("file");
    CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([("docs/index.json".parse().expect("path"), b"{}".to_vec())]),
    )
    .expect("capture")
}

#[test]
fn exact_file_and_directory_metadata_share_the_retained_tree() {
    let capture = capture();
    let tree = capture.as_input();
    for path in [None, Some("docs"), Some("empty")] {
        let selection = Selection::Directory {
            path: path.map(|path| path.parse().expect("path")),
        };
        assert_eq!(
            selection.content(&tree).expect("directory"),
            Content::Directory
        );
        let artifact = Artifact {
            format: Format::TypeDocJson,
            tree: capture.as_input(),
            selection: &selection,
        };
        assert!(artifact.bytes().is_err(), "no implicit index.json fallback");
    }
    let selection = Selection::File {
        path: "docs/index.json".parse().expect("path"),
    };
    assert_eq!(
        selection.content(&tree).expect("file"),
        Content::File {
            bytes: 2,
            sha256: Sha256Digest::of(b"{}"),
        }
    );
    let artifact = Artifact {
        format: Format::TypeDocJson,
        tree: capture.as_input(),
        selection: &selection,
    };
    assert!(std::ptr::eq(
        artifact.bytes().expect("bytes"),
        capture
            .file(&"docs/index.json".parse().expect("path"))
            .expect("original")
    ));
}

#[test]
fn missing_or_wrong_kind_never_selects_another_artifact() {
    let capture = capture();
    for selection in [
        Selection::File {
            path: "docs".parse().expect("path"),
        },
        Selection::File {
            path: "missing".parse().expect("path"),
        },
        Selection::Directory {
            path: Some("docs/index.json".parse().expect("path")),
        },
        Selection::Directory {
            path: Some("missing".parse().expect("path")),
        },
    ] {
        assert!(selection.content(&capture.as_input()).is_err());
    }
    let empty = CapturedSource::from_parts(
        InventoryBuilder::new(Limits::default()).finish(),
        BTreeMap::new(),
    )
    .expect("empty capture");
    assert_eq!(
        Selection::Directory { path: None }
            .content(&empty.as_input())
            .expect("root"),
        Content::Directory
    );
}

#[test]
fn tagged_selection_round_trips_without_legacy_or_implicit_paths() {
    for value in [
        serde_json::json!({"kind":"file","path":"docs/index.json"}),
        serde_json::json!({"kind":"directory","path":"docs"}),
        serde_json::json!({"kind":"directory"}),
    ] {
        let selection: Selection = serde_json::from_value(value.clone()).expect("selection");
        assert_eq!(serde_json::to_value(selection).expect("encode"), value);
    }
    for value in [
        serde_json::json!("docs/index.json"),
        serde_json::json!(["file", "docs/index.json"]),
        serde_json::json!({"kind":"file"}),
        serde_json::json!({"kind":"file","path":null}),
        serde_json::json!({"kind":{"file":null},"path":"docs/index.json"}),
        serde_json::json!({"kind":["directory"]}),
        serde_json::json!({"kind":0}),
        serde_json::json!({"kind":"directory","path":"."}),
        serde_json::json!({"kind":"file","path":"../outside"}),
        serde_json::json!({"kind":"directory","entry":"docs"}),
        serde_json::json!({"kind":"directory","path":"/absolute"}),
    ] {
        assert!(
            serde_json::from_value::<Selection>(value.clone()).is_err(),
            "{value}"
        );
    }
    for source in [
        r#"{"kind":"file","kind":"directory","path":"docs"}"#,
        r#"{"kind":"directory","path":null,"path":"docs"}"#,
    ] {
        assert!(serde_json::from_str::<Selection>(source).is_err());
    }
}
