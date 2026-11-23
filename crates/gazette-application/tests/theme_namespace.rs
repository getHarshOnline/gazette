//! Resolved Theme graphs assemble into one exact captured Tera input namespace.

use std::collections::BTreeMap;

use gazette_application::build::theme_namespace::{Error, ThemeNamespace};
use gazette_compiler::{
    theme_graph::Graph,
    theme_package::{ParsedManifest, ThemePackageId},
    toml,
};
use gazette_core::source::{
    self, CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest,
};

const FOUNDATION: &str =
    "package='theme:foundation'\nnamespace='foundation'\ntemplates='templates'\n";
const ROOT: &str = "package='theme:root'\nnamespace='publication'\ntemplates='view'\n[[dependencies]]\npackage='theme:foundation'\n";

fn manifest(source: &str) -> ParsedManifest<'_> {
    ParsedManifest::parse(source, toml::Limits::default()).expect("valid package manifest")
}

fn id(value: &str) -> ThemePackageId {
    value.parse().expect("valid package identity")
}

fn capture(entries: &[(&str, Option<&[u8]>)]) -> CapturedSource {
    let mut builder = InventoryBuilder::new(source::Limits::default());
    let mut files = BTreeMap::new();
    for (path, bytes) in entries {
        let path: EntryPath = path.parse().expect("valid entry path");
        let content = if let Some(bytes) = bytes {
            files.insert(path.clone(), bytes.to_vec());
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            }
        } else {
            Content::Directory
        };
        builder
            .insert(path, content)
            .expect("ordered valid capture");
    }
    CapturedSource::from_parts(builder.finish(), files).expect("complete captured source")
}

#[test]
fn mounts_every_regular_template_file_under_its_package_namespace() {
    let foundation_manifest = manifest(FOUNDATION);
    let root_manifest = manifest(ROOT);
    let graph = Graph::resolve(
        &id("theme:root"),
        [root_manifest.manifest(), foundation_manifest.manifest()],
    )
    .expect("resolved graph");
    let foundation = capture(&[
        ("notes.txt", Some(b"not selected")),
        ("templates", None),
        ("templates/base.html", Some(b"foundation")),
        ("templates/parts", None),
        ("templates/parts/macro.html", Some(b"macro")),
    ]);
    let root = capture(&[
        ("view", None),
        ("view/page.html", Some(b"page")),
        ("view/partials", None),
        ("view/partials/card.html", Some(b"card")),
    ]);
    let foundation_tree = foundation.inventory().identity();
    let root_tree = root.inventory().identity();
    let captures = BTreeMap::from([
        (id("theme:root"), root),
        (id("theme:foundation"), foundation),
    ]);

    let namespace = ThemeNamespace::assemble(&graph, &captures, source::Limits::default())
        .expect("complete virtual namespace");
    let paths = namespace
        .inventory()
        .entries()
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "foundation",
            "foundation/base.html",
            "foundation/parts",
            "foundation/parts/macro.html",
            "publication",
            "publication/page.html",
            "publication/partials",
            "publication/partials/card.html",
        ]
    );
    let input = namespace.as_input().expect("verified Tera input");
    assert_eq!(
        input.file(&"foundation/base.html".parse().unwrap()),
        Some(b"foundation".as_slice())
    );
    assert_eq!(
        input.file(&"publication/partials/card.html".parse().unwrap()),
        Some(b"card".as_slice())
    );
    assert!(input.file(&"notes.txt".parse().unwrap()).is_none());

    assert_eq!(
        namespace.packages()[0].package().as_str(),
        "theme:foundation"
    );
    assert_eq!(namespace.packages()[0].namespace().as_str(), "foundation");
    assert_eq!(namespace.packages()[0].templates().to_string(), "templates");
    assert_eq!(namespace.packages()[0].source_tree(), foundation_tree);
    assert_eq!(namespace.packages()[1].package().as_str(), "theme:root");
    assert_eq!(namespace.packages()[1].source_tree(), root_tree);
    assert_eq!(
        namespace.packages()[1]
            .files()
            .iter()
            .map(EntryPath::as_str)
            .collect::<Vec<_>>(),
        ["publication/page.html", "publication/partials/card.html"]
    );
}

#[test]
fn missing_capture_fails_before_returning_a_partial_namespace() {
    let foundation = manifest(FOUNDATION);
    let root = manifest(ROOT);
    let graph =
        Graph::resolve(&id("theme:root"), [root.manifest(), foundation.manifest()]).unwrap();
    let captures = BTreeMap::from([(
        id("theme:foundation"),
        capture(&[("templates", None), ("templates/base.html", Some(b"base"))]),
    )]);

    let error = ThemeNamespace::assemble(&graph, &captures, source::Limits::default())
        .expect_err("root capture is missing");
    assert_eq!(error.code(), "theme_namespace_capture");
    assert!(matches!(
        error,
        Error::MissingCapture { package } if package == id("theme:root")
    ));
}

#[test]
fn template_root_must_be_an_existing_nonempty_directory() {
    let root = manifest("package='theme:root'\nnamespace='root'\ntemplates='templates'\n");
    let graph = Graph::resolve(&id("theme:root"), [root.manifest()]).unwrap();
    for (capture, reason) in [
        (capture(&[("other.txt", Some(b"other"))]), "missing"),
        (capture(&[("templates", Some(b"file"))]), "not a directory"),
        (capture(&[("templates", None)]), "contains no regular files"),
    ] {
        let captures = BTreeMap::from([(id("theme:root"), capture)]);
        let error = ThemeNamespace::assemble(&graph, &captures, source::Limits::default())
            .expect_err(reason);
        assert_eq!(error.code(), "theme_namespace_root");
        assert!(error.to_string().contains(reason));
    }
}

#[test]
fn equal_namespaces_from_distinct_packages_are_an_exact_collision() {
    let dependency =
        manifest("package='theme:dependency'\nnamespace='shared'\ntemplates='templates'\n");
    let root = manifest(
        "package='theme:root'\nnamespace='shared'\ntemplates='templates'\n[[dependencies]]\npackage='theme:dependency'\n",
    );
    let graph =
        Graph::resolve(&id("theme:root"), [root.manifest(), dependency.manifest()]).unwrap();
    let captures = BTreeMap::from([
        (
            id("theme:dependency"),
            capture(&[("templates", None), ("templates/base.html", Some(b"base"))]),
        ),
        (
            id("theme:root"),
            capture(&[("templates", None), ("templates/page.html", Some(b"page"))]),
        ),
    ]);

    let error = ThemeNamespace::assemble(&graph, &captures, source::Limits::default())
        .expect_err("namespace collision");
    assert_eq!(error.code(), "theme_namespace_collision");
    assert!(matches!(
        error,
        Error::Collision { path, first, second }
            if path.as_str() == "shared"
                && first == id("theme:dependency")
                && second == id("theme:root")
    ));
}

#[test]
fn namespace_limits_apply_to_the_complete_combined_tree() {
    let root = manifest("package='theme:root'\nnamespace='root'\ntemplates='templates'\n");
    let graph = Graph::resolve(&id("theme:root"), [root.manifest()]).unwrap();
    let captures = BTreeMap::from([(
        id("theme:root"),
        capture(&[("templates", None), ("templates/page.html", Some(b"page"))]),
    )]);

    let error = ThemeNamespace::assemble(
        &graph,
        &captures,
        source::Limits {
            entries: 1,
            ..source::Limits::default()
        },
    )
    .expect_err("combined tree exceeds caller entry ceiling");
    assert_eq!(error.code(), "source_limit");
}

#[test]
fn captures_outside_the_resolved_graph_do_not_enter_the_namespace() {
    let root = manifest("package='theme:root'\nnamespace='root'\ntemplates='templates'\n");
    let graph = Graph::resolve(&id("theme:root"), [root.manifest()]).unwrap();
    let captures = BTreeMap::from([
        (
            id("theme:root"),
            capture(&[("templates", None), ("templates/page.html", Some(b"page"))]),
        ),
        (
            id("theme:unused"),
            capture(&[
                ("templates", None),
                ("templates/unused.html", Some(b"unused")),
            ]),
        ),
    ]);

    let namespace = ThemeNamespace::assemble(&graph, &captures, source::Limits::default())
        .expect("selected namespace");
    assert_eq!(namespace.packages().len(), 1);
    assert!(
        namespace
            .as_input()
            .unwrap()
            .file(&"unused/unused.html".parse().unwrap())
            .is_none()
    );
}
