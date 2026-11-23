//! Workspace Graphviz selection owns only an exact pinned executable.

use gazette_compiler::{toml, workspace};
use gazette_core::{process::PinnedExecutable, source::Sha256Digest};

const TOPOLOGY: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";

fn compile(source: &str) -> Result<workspace::Compilation, workspace::Error> {
    workspace::compile(source, toml::Limits::default())
}

#[test]
fn pinned_graphviz_provider_is_retained_without_execution_policy() {
    let digest = Sha256Digest::of(b"selected graphviz bytes");
    let source = format!(
        "dot_renderer={{program='/opt/gazette/bin/dot',executable={{bytes=23,sha256='{digest}'}}}}\n{TOPOLOGY}"
    );
    let compiled = compile(&source).expect("pure provider selection");
    assert_eq!(
        compiled
            .workspace
            .dot_renderer()
            .map(|provider| &provider.executable),
        Some(&PinnedExecutable {
            program: "/opt/gazette/bin/dot".into(),
            executable: gazette_core::process::ExecutablePin {
                bytes: 23,
                sha256: digest,
            },
        })
    );
}

#[test]
fn graphviz_configuration_cannot_inject_arguments_environment_or_directory() {
    let digest = Sha256Digest::of(b"selected graphviz bytes");
    for field in [
        "arguments=['-o','/tmp/output']",
        "environment={HOME='/private'}",
        "directory='/source'",
    ] {
        let source = format!(
            "[dot_renderer]\nprogram='/opt/gazette/bin/dot'\nexecutable={{bytes=23,sha256='{digest}'}}\n{field}\n{TOPOLOGY}"
        );
        assert!(
            matches!(
                compile(&source),
                Err(workspace::Error::Toml(toml::Error::Invalid { .. }))
            ),
            "unexpected Graphviz command surface: {field}"
        );
    }
}

#[test]
fn graphviz_provider_requires_an_absolute_pinned_regular_file_shape() {
    let digest = Sha256Digest::of(b"selected graphviz bytes");
    for source in [
        format!(
            "dot_renderer={{program='dot',executable={{bytes=23,sha256='{digest}'}}}}\n{TOPOLOGY}"
        ),
        format!(
            "dot_renderer={{program='/opt/gazette/bin/dot',executable={{bytes=0,sha256='{digest}'}}}}\n{TOPOLOGY}"
        ),
    ] {
        let workspace::Error::Invalid(errors) =
            compile(&source).expect_err("invalid provider selection")
        else {
            panic!("expected semantic provider error");
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "workspace_dot_command");
        assert_eq!(errors[0].field, "dot_renderer");
    }
}
