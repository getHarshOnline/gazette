//! Portable command syntax remains distinct from a native execution request.

use gazette_compiler::workspace::{RuntimeArgument, RuntimeCommand};
use gazette_core::process::Command;
use serde_json::json;

fn selection(value: serde_json::Value) -> RuntimeCommand {
    serde_json::from_value(value).unwrap()
}

#[test]
fn explicit_path_arguments_keep_literal_values_and_closed_shapes() {
    let value = json!({"program":"machine:runtime/bin/node","directory":".","arguments":["machine:literal/file","$HOME; $(ignored)",{"path":"machine:checker/check.py"}],"environment":{"PATH":"$PATH"}});
    let command = selection(value.clone());
    command.validate().unwrap();
    assert!(
        matches!(&command.arguments[0], RuntimeArgument::Literal(value) if value=="machine:literal/file")
    );
    assert_eq!(
        serde_json::to_value(command).unwrap()["arguments"],
        value["arguments"]
    );
    for argument in [
        json!({"path":"x","extra":true}),
        json!({"path":12}),
        json!({"literal":"x"}),
        json!(["x"]),
    ] {
        let mut invalid = value.clone();
        invalid["arguments"] = json!([argument]);
        assert!(serde_json::from_value::<RuntimeCommand>(invalid).is_err());
    }
}

#[test]
fn portable_coordinates_and_native_execution_have_separate_validation() {
    for path in [
        "machine:runtime/bin/node",
        ".",
        "tools/check.py",
        "/",
        "/native/../spelling",
    ] {
        let command =
            selection(json!({"program":path,"directory":path,"arguments":[{"path":path}]}));
        command.validate().unwrap();
    }
    for path in [
        "",
        "../escape",
        "machine:",
        "machine:9invalid/file",
        "machine:root/../escape",
        "machine:root//file",
        "machine:root/a\u{0000}b",
    ] {
        for field in ["program", "directory", "argument"] {
            let mut value = json!({"program":"/checker","directory":"/"});
            if field == "argument" {
                value["arguments"] = json!([{"path":path}]);
            } else {
                value[field] = json!(path);
            }
            assert!(selection(value).validate().is_err(), "{field}: {path:?}");
        }
    }
    let native = Command {
        program: "machine:runtime/node".into(),
        executable: None,
        arguments: vec![],
        directory: "/".into(),
        environment: Default::default(),
    };
    assert!(
        native.validate().is_err(),
        "portable spelling must never reach native execution"
    );
}

#[test]
fn existing_absolute_command_json_and_limits_remain_compatible() {
    let native = Command {
        program: "/checker".into(),
        executable: None,
        arguments: vec!["literal".into()],
        directory: "/".into(),
        environment: Default::default(),
    };
    let value = serde_json::to_value(&native).unwrap();
    let portable = selection(value.clone());
    portable.validate().unwrap();
    assert_eq!(serde_json::to_value(portable).unwrap(), value);
    for arguments in [
        vec![json!(""); 257],
        vec![json!({"path":format!("machine:runtime/{}", "x".repeat(64<<10))})],
    ] {
        assert!(
            selection(json!({"program":"/checker","directory":"/","arguments":arguments}))
                .validate()
                .is_err()
        );
    }
}
