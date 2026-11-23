//! Recipe invariants keep source, executable and output-path kinds explicit.

use gazette_core::{
    documentation::{
        Format, Selection,
        generator::{Argument, Recipe},
    },
    process::ExecutablePin,
    source::Sha256Digest,
};
use std::collections::BTreeMap;

fn recipe() -> Recipe {
    Recipe {
        program: "/selected/rustdoc".into(),
        executable: ExecutablePin {
            bytes: 1,
            sha256: Sha256Digest::of(b"x"),
        },
        arguments: vec![
            Argument::Input {
                path: "lib.rs".parse().expect("fixture invariant"),
            },
            Argument::Literal { value: "-o".into() },
            Argument::OutputRoot,
        ],
        environment: BTreeMap::new(),
        version_arguments: vec!["--version".into()],
        expected_version_sha256: Sha256Digest::of(b"rustdoc fixture\n"),
        validation_arguments: None,
        output: Selection::File {
            path: "crate/index.html".parse().expect("fixture invariant"),
        },
        format: Format::RustdocHtml,
    }
}

#[test]
fn typed_file_and_root_arguments_round_trip_without_becoming_the_same_selector() {
    let mut value = recipe();
    value.validate().expect("fixture invariant");
    value.arguments.push(Argument::InputRoot);
    value.arguments.push(Argument::Output);
    let json = serde_json::to_value(&value).expect("fixture invariant");
    assert_eq!(json["arguments"][2]["kind"], "output-root");
    assert_eq!(json["arguments"][3]["kind"], "input-root");
    assert_eq!(json["arguments"][4]["kind"], "output");
    assert_eq!(
        serde_json::from_value::<Recipe>(json).expect("fixture invariant"),
        value
    );
}

#[test]
fn source_output_version_pin_and_native_command_bounds_are_required() {
    let base = recipe();
    let mut invalid = base.clone();
    invalid.arguments.remove(0);
    assert!(invalid.validate().is_err());
    let mut invalid = base.clone();
    invalid.arguments.pop();
    assert!(invalid.validate().is_err());
    let mut invalid = base.clone();
    invalid.version_arguments.clear();
    assert!(invalid.validate().is_err());
    let mut invalid = base.clone();
    invalid.executable.bytes = 0;
    assert!(invalid.validate().is_err());
    let mut invalid = base.clone();
    invalid.program = "rustdoc".into();
    assert!(invalid.validate().is_err());
    let mut invalid = base.clone();
    invalid.arguments.push(Argument::Literal {
        value: "x".repeat(65 << 10),
    });
    assert!(invalid.validate().is_err());
    let mut json = serde_json::to_value(base).expect("fixture invariant");
    json["output"]["path"] = "../outside".into();
    assert!(serde_json::from_value::<Recipe>(json).is_err());
}

#[test]
fn optional_validation_round_trips_and_requires_bounded_output_arguments() {
    let mut value = recipe();
    assert!(
        serde_json::to_value(&value)
            .expect("recipe")
            .get("validation_arguments")
            .is_none()
    );
    value.validation_arguments = Some(vec![
        Argument::Literal {
            value: "validate".into(),
        },
        Argument::Output,
    ]);
    value.validate().expect("selected validation");
    assert_eq!(
        serde_json::from_value::<Recipe>(serde_json::to_value(&value).expect("recipe"))
            .expect("round trip"),
        value
    );
    for arguments in [
        vec![],
        vec![Argument::Input {
            path: "lib.rs".parse().expect("entry"),
        }],
        vec![Argument::Output, Argument::Literal { value: "\0".into() }],
    ] {
        value.validation_arguments = Some(arguments);
        assert!(value.validate().is_err());
    }
}

#[test]
fn portable_text_validation_does_not_relax_native_execution_or_explicit_inputs() {
    let mut value = recipe();
    value.program = "machine:tools/bin/rustdoc".into();
    value.validate_text().expect("bounded portable spelling");
    assert!(
        value.validate().is_err(),
        "native execution still requires an absolute program"
    );
    value.arguments.remove(0);
    assert!(
        value.validate_text().is_err(),
        "source selection remains explicit"
    );
}

#[test]
fn explicit_tool_paths_require_binding_and_cannot_replace_captured_inputs() {
    let mut value = recipe();
    value.arguments.push(Argument::Path {
        path: "machine:tools/plugin.mjs".into(),
    });
    value.validate_text().unwrap();
    assert!(value.validate().is_err());
    *value.arguments.last_mut().unwrap() = Argument::Path {
        path: "/selected/plugin.mjs".into(),
    };
    value.validate().unwrap();
    value.validation_arguments = Some(vec![
        Argument::Output,
        Argument::Path {
            path: "relative/tool".into(),
        },
    ]);
    assert!(value.validate().is_err());
    value.validation_arguments = None;
    value.arguments.remove(0);
    assert!(
        value.validate().is_err(),
        "external tool paths do not select captured input"
    );
}
