//! Installed aliases keep their declarations and complete native implementation data.

use super::*;

#[test]
fn installed_aliases_preserve_methods_examples_links_and_distinct_anchors() {
    let directory = tempfile::tempdir().unwrap();
    let program = std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into());
    generate_source(
        directory.path(),
        Path::new(&program),
        include_str!("../fixtures/rustdoc_aliases.rs"),
    );
    let native = directory.path().join("html");
    let input = capture(&native);
    let first =
        assemble(&input).expect("native aliases and their exact captured implementation data");
    for name in ["First", "Second"] {
        let output = observe_markdown(page(&first, &format!("fixture/rust/type.{name}.md")));
        for text in [
            "Type Alias",
            "pub type",
            "retained",
            "Borrow the original value.",
            "assert_eq!",
            "Implementations",
            "Trait Implementations",
        ] {
            assert!(output.text.contains(text), "{name}: missing {text}");
        }
        assert!(output.anchors.contains("method.retained"));
        assert!(
            output
                .links
                .contains(&format!("/fixture/rust/type.{name}#method.retained"))
        );
        assert!(!output.tags.contains("script"));
    }
    let root = observe_markdown(page(&first, "fixture/rust/index.md"));
    assert!(
        root.links
            .contains(&"/fixture/rust/type.First#method.retained".into())
    );
    assert_eq!(
        first.inventory().identity(),
        assemble(&input).unwrap().inventory().identity()
    );

    let data = native.join("type.impl/reference_fixture/struct.Record.js");
    let original = fs::read_to_string(&data).unwrap();
    fs::write(&data, format!("{original}\nwindow.extra=true;")).unwrap();
    assert!(assemble(&capture(&native)).is_err());
    fs::write(&data, &original).unwrap();
    let held = directory.path().join("held-type-data.js");
    fs::rename(&data, &held).unwrap();
    assert!(
        assemble(&capture(&native)).is_err(),
        "selected alias needs its captured data"
    );
    fs::rename(held, &data).unwrap();
    let alias = native.join("reference_fixture/type.First.html");
    let original_alias = fs::read_to_string(&alias).unwrap();
    fs::write(
        &alias,
        original_alias.replace("reference_fixture::First", "reference_fixture::Missing"),
    )
    .unwrap();
    assert!(
        assemble(&capture(&native)).is_err(),
        "a different alias is not a substitute"
    );
    fs::write(alias, original_alias).unwrap();
    assert_eq!(
        first.inventory().identity(),
        assemble(&capture(&native)).unwrap().inventory().identity()
    );
}

#[test]
fn installed_alias_preserves_external_methods_and_corrects_qualified_upstream_section() {
    let directory = tempfile::tempdir().unwrap();
    let program = std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into());
    generate_source(
        directory.path(),
        Path::new(&program),
        "//! Native external alias.\n/// An ordered lookup.\npub type Lookup = std::collections::BTreeMap<String, u8>;",
    );
    let native = directory.path().join("html");
    let data =
        fs::read_to_string(native.join("type.impl/alloc/collections/btree/map/struct.BTreeMap.js"))
            .unwrap();
    assert!(data.contains("index.html#insert-and-complex-keys"));
    assert!(!native.join("index.html").exists());
    let input = capture(&native);
    let target = assemble(&input).expect("qualified native standard-library section");
    let lookup = observe_markdown(page(&target, "fixture/rust/type.Lookup.md"));
    for text in [
        "BTreeMap",
        "insert",
        "get",
        "Examples",
        "IntoIterator",
        "The key is not updated",
    ] {
        assert!(lookup.text.contains(text), "missing {text}");
    }
    assert!(
        lookup
            .links
            .iter()
            .any(|link| link.starts_with("https://doc.rust-lang.org/")
                && link.ends_with("/std/collections/index.html#insert-and-complex-keys"))
    );
    assert!(
        !lookup
            .links
            .iter()
            .any(|link| link == "/fixture/rust/index#insert-and-complex-keys")
    );
    assert_eq!(
        target.inventory().identity(),
        assemble(&input).unwrap().inventory().identity()
    );
}

#[test]
fn installed_alias_links_use_the_nested_native_root_within_a_capture() {
    let directory = tempfile::tempdir().unwrap();
    let program = std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into());
    generate_source(
        directory.path(),
        Path::new(&program),
        include_str!("../fixtures/rustdoc_aliases.rs"),
    );
    let native = directory.path().join("html");
    let original = assemble(&capture(&native)).unwrap();
    let before = observe_markdown(page(&original, "fixture/rust/type.First.md"));
    for prefix in ["reference", "job/result/reference"] {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join(prefix);
        fs::create_dir_all(&nested).unwrap();
        materialize(&capture(&native), &nested);
        let input = capture(root.path());
        let selection = Selection::Directory {
            path: Some(format!("{prefix}/reference_fixture").parse().unwrap()),
        };
        let target = Mintlify::new(&gazette_adapter_markdown::Comrak)
            .assemble(
                Request {
                    name: "Nested Rust reference",
                    public_base_url: None,
                    target: &Target::Mintlify {
                        theme: "mint".into(),
                        primary_color: "#123abc".into(),
                        light_color: None,
                        dark_color: None,
                        telemetry: None,
                    },
                    projects: &[ProjectModule {
                        slug: "fixture",
                        title: "Fixture project",
                        manual: None,
                        navigation: vec![ProjectItem::Reference(Reference {
                            pages: None,
                            slug: "rust",
                            title: "Rust library",
                            artifact: Artifact {
                                format: Format::RustdocHtml,
                                tree: input.as_input(),
                                selection: &selection,
                            },
                        })],
                    }],
                    usage: None,
                },
                Limits::default(),
            )
            .expect("implementation links keep the native root under every capture prefix");
        let after = observe_markdown(page(&target, "fixture/rust/type.First.md"));
        assert_eq!(before.text, after.text);
        assert_eq!(before.anchors, after.anchors);
        assert!(
            after
                .links
                .contains(&"/fixture/rust/type.First#method.retained".into())
        );
        assert!(
            after
                .links
                .iter()
                .any(|link| link.starts_with("/fixture/rust/struct.Record"))
        );
    }
}
