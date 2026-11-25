//! Selected manual links admit companions without changing chapter selection.

use super::*;

#[test]
fn linked_companions_are_retained_once_without_publishing_unlinked_files() {
    let tree = manual_source();
    let mut module = project(&tree);
    module.manual.as_mut().unwrap().assets.clear();
    let output = assemble(&[module]).unwrap();
    let path = entry("gazette/assets/docs/media/diagram.png");
    assert_eq!(output.file(&path), Some(b"retained-image".as_slice()));
    let page = std::str::from_utf8(output.file(&entry("gazette/start.md")).unwrap()).unwrap();
    assert!(page.contains("/gazette/assets/docs/media/diagram.png"));
    assert_eq!(
        tree.file(&entry("docs/media/diagram.png")),
        Some(b"retained-image".as_slice())
    );
}

#[test]
fn missing_companions_and_unselected_manual_pages_still_fail() {
    for target in ["media/missing.png", "unselected.md", "../../../outside.png"] {
        let source = format!("# Start\n\n[Linked]({target})\n");
        let tree = retained(&[
            ("docs/start.md", source.as_bytes()),
            ("docs/guide.md", b"# Guide\n"),
            ("docs/unselected.md", b"# Unselected\n"),
            ("docs/media/diagram.png", b"retained-image"),
        ]);
        let mut module = project(&tree);
        module.manual.as_mut().unwrap().assets.clear();
        assert!(assemble(&[module]).is_err(), "{target}");
    }
}

#[test]
fn shared_captures_choose_one_companion_path_independently_of_module_order() {
    let tree = retained(&[
        (
            "docs/start.md",
            b"# Start\n\n[Guide](guide.md#guide) ![Diagram](media/diagram.png)\n",
        ),
        (
            "docs/guide.md",
            b"# Guide\n\n[Start](start.md#start) ![Diagram](media/diagram.png)\n",
        ),
        ("docs/media/diagram.png", b"retained-image"),
    ]);
    for reverse in [false, true] {
        let mut first = project(&tree);
        first.slug = "zeta";
        first.navigation.truncate(1);
        first.manual.as_mut().unwrap().assets.clear();
        let mut second = project(&tree);
        second.slug = "alpha";
        second.navigation.remove(0);
        second.manual.as_mut().unwrap().assets.clear();
        let modules = if reverse {
            vec![second, first]
        } else {
            vec![first, second]
        };
        let output = assemble(&modules).unwrap();
        assert!(
            output
                .file(&entry("alpha/assets/docs/media/diagram.png"))
                .is_some()
        );
        assert!(
            output
                .file(&entry("zeta/assets/docs/media/diagram.png"))
                .is_none()
        );
        for page in ["gazette/start.md", "gazette/guide.md"] {
            assert!(
                std::str::from_utf8(output.file(&entry(page)).unwrap())
                    .unwrap()
                    .contains("/alpha/assets/docs/media/diagram.png")
            );
        }
    }
}

#[test]
fn discovered_companions_cannot_enter_a_reference_namespace() {
    let tree = manual_source();
    let generated = Input::new(b"# Reference\n");
    let mut module = project(&tree);
    module.manual.as_mut().unwrap().assets.clear();
    module.navigation.push(ProjectItem::Reference(Reference {
        slug: "assets",
        title: "Reference",
        pages: None,
        artifact: generated.artifact(Format::Markdown),
    }));
    assert!(
        assemble(&[module])
            .unwrap_err()
            .to_string()
            .contains("overlaps")
    );
}

#[test]
fn discovered_companions_count_toward_the_complete_output_budget() {
    let tree = manual_source();
    let mut module = project(&tree);
    module.manual.as_mut().unwrap().assets.clear();
    let output = assemble(std::slice::from_ref(&module)).unwrap();
    let limits = Limits {
        total_bytes: output.inventory().total_bytes() - 1,
        ..Limits::default()
    };
    assert!(
        Mintlify::new(&gazette_adapter_markdown::Comrak)
            .assemble(
                Request {
                    name: "Product documentation",
                    public_base_url: None,
                    target: &Target::Mintlify {
                        theme: "mint".into(),
                        primary_color: "#123abc".into(),
                        light_color: None,
                        dark_color: None,
                        telemetry: None,
                    },
                    projects: &[module],
                    usage: None,
                    metadata: Default::default(),
                },
                limits,
            )
            .is_err()
    );
}
