//! Portable documentation coordinates retain the shared native source-root grammar.

use gazette_compiler::{toml, workspace};

fn configuration(path: &str) -> String {
    format!(
        "[[principals]]\nid='person:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='person:one'\ndeployable=false\n[[brands]]\nid='brand:one'\nname='Documentation'\n[[brands.publications]]\nsite='site:one'\ndocumentation_repository={{name='independent-docs',path={path:?}}}\n"
    )
}

#[test]
fn absolute_workspace_and_confined_relative_paths_preserve_exact_declarations() {
    for path in [
        "/explicit/domain-doc",
        ".",
        "domain-doc",
        "generated/é-reference",
        "machine:builds/domain-doc",
    ] {
        let parsed = workspace::compile(&configuration(path), toml::Limits::default())
            .expect("strict portable native path contract");
        assert_eq!(
            parsed
                .workspace
                .documentation_repository(&"site:one".parse().expect("site"))
                .expect("sole brand-owned target")
                .path,
            path
        );
        assert!(path.parse::<workspace::SourceRoot>().is_ok());
    }
}

#[test]
fn escapes_ambiguous_components_controls_and_exact_duplicate_doc_roots_fail_semantically() {
    for path in [
        "../outside",
        "a/../b",
        "./docs",
        "a//b",
        "/a/../b",
        "a\\b",
        "a\nb",
        "",
    ] {
        let workspace::Error::Invalid(errors) =
            workspace::compile(&configuration(path), Default::default())
                .expect_err("native root grammar rejects ambiguous coordinates")
        else {
            panic!("typed semantic path failure")
        };
        assert!(
            errors
                .iter()
                .any(|error| error.code == "workspace_repository_path")
        );
    }
    let second = "[[sites]]\nid='site:two'\npublisher='person:one'\ndeployable=false\n[[brands]]\nid='brand:two'\nname='Second docs'\n[[brands.publications]]\nsite='site:two'\ndocumentation_repository={name='second',path='domain-doc'}\n";
    let workspace::Error::Invalid(errors) = workspace::compile(
        &format!("{}{second}", configuration("domain-doc")),
        Default::default(),
    )
    .expect_err("two doc owners cannot select one exact coordinate") else {
        panic!("typed duplicate failure")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.code == "workspace_duplicate"
                && error.field == "brands.publications.documentation_repository.path")
    );
}
