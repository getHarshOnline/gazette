//! Large declarations and private-module reexports from the installed Rustdoc.

use super::*;

#[test]
fn native_reexports_and_collapsed_declarations_preserve_their_complete_api() {
    let directory = tempfile::tempdir().unwrap();
    let fields = (0..13)
        .map(|i| format!("pub field_{i}: u32,\n"))
        .collect::<String>();
    let variants = (0..13)
        .map(|i| format!("Variant{i},\n"))
        .collect::<String>();
    let source = format!(
        "//! [Original location](private/struct.Large.html#structfield.field_12).\nmod private {{\n/// All thirteen fields remain available.\npub struct Large {{ {fields} }}\n/// All thirteen variants remain available.\npub enum Choice {{ {variants} }}\n}}\npub use private::{{Large, Choice}};\n"
    );
    let program = std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into());
    generate_source(directory.path(), Path::new(&program), &source);
    let native = directory.path().join("html");
    let redirect = native.join("reference_fixture/private/struct.Large.html");
    let original = fs::read_to_string(&redirect).unwrap();
    assert!(original.contains("location.replace("));
    let large_path = native.join("reference_fixture/struct.Large.html");
    let large_source = fs::read_to_string(&large_path).unwrap();
    assert!(large_source.contains("Show 13 fields"));
    let choice_path = native.join("reference_fixture/enum.Choice.html");
    assert!(
        fs::read_to_string(&choice_path)
            .unwrap()
            .contains("Show 13 variants")
    );
    let first = assemble(&capture(&native)).expect("actual native reexports and controls");
    let large = observe_markdown(page(&first, "fixture/rust/struct.Large.md"));
    let choice = observe_markdown(page(&first, "fixture/rust/enum.Choice.md"));
    for i in 0..13 {
        assert!(large.text.contains(&format!("field_{i}")));
        assert!(choice.text.contains(&format!("Variant{i}")));
    }
    assert!(large.text.contains("All thirteen fields remain available."));
    let index = observe_markdown(page(&first, "fixture/rust/index.md"));
    assert!(
        index
            .links
            .contains(&"/fixture/rust/struct.Large#structfield.field_12".into())
    );
    assert!(
        first
            .file(&"fixture/rust/private/struct.Large.md".parse().unwrap())
            .is_none()
    );
    assert_eq!(
        first.inventory().identity(),
        assemble(&capture(&native)).unwrap().inventory().identity()
    );

    for changed in [
        original.replace(
            "<title>Redirection</title>",
            "<title>New API meaning</title>",
        ),
        format!("{original}<p>Extra meaning</p>"),
        original.replace(
            "../../reference_fixture/struct.Large.html",
            "../../outside/struct.Large.html",
        ),
        original.replace(
            "../../reference_fixture/struct.Large.html",
            "../../../../escape.html",
        ),
    ] {
        assert_ne!(changed, original);
        fs::write(&redirect, changed).unwrap();
        assert!(
            assemble(&capture(&native)).is_err(),
            "changed or unselected redirect refused"
        );
    }
    fs::write(&redirect, &original).unwrap();
    let source_copy = native.join("reference_fixture/source.html");
    fs::copy(
        native.join("src/reference_fixture/library.rs.html"),
        &source_copy,
    )
    .unwrap();
    let source_redirect = native.join("reference_fixture/private/struct.SourceAlias.html");
    fs::write(
        &source_redirect,
        original.replace(
            "../../reference_fixture/struct.Large.html",
            "../source.html",
        ),
    )
    .unwrap();
    let error = assemble(&capture(&native)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Rustdoc redirect has no selected API target")
    );
    fs::remove_file(source_redirect).unwrap();
    fs::remove_file(source_copy).unwrap();

    for name in ["", "bad-name"] {
        let target_name = format!("macro.{name}.html");
        let target = native.join("reference_fixture").join(&target_name);
        let alias = native
            .join("reference_fixture")
            .join(format!("macro.{name}!.html"));
        fs::write(&target, &large_source).unwrap();
        fs::write(
            &alias,
            original.replace("../../reference_fixture/struct.Large.html", &target_name),
        )
        .unwrap();
        let error = assemble(&capture(&native)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported Rustdoc macro redirect name")
        );
        fs::remove_file(alias).unwrap();
        fs::remove_file(target).unwrap();
    }
    for label in [
        "Show 0 fields",
        "Show thirteen fields",
        "Additional API meaning",
    ] {
        fs::write(&large_path, large_source.replace("Show 13 fields", label)).unwrap();
        let error = assemble(&capture(&native)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unrecognized Rustdoc control content")
        );
    }
    fs::write(&large_path, large_source).unwrap();
    assert_eq!(
        first.inventory().identity(),
        assemble(&capture(&native)).unwrap().inventory().identity()
    );
}
