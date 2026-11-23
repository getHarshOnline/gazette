//! Pure command validation; no executable, environment or filesystem is consulted.

use std::{collections::BTreeMap, error::Error as _, io, time::Duration};

use gazette_core::process::{Cleanup, Command, Error, Failure, Options, Request};

fn command() -> Command {
    Command {
        program: "/not-an-installed-checker".into(),
        executable: None,
        directory: "/not-a-required-existing-directory".into(),
        arguments: Vec::new(),
        environment: BTreeMap::new(),
    }
}

fn refused(result: Result<(), Error>, failure: Failure) {
    let error = result.expect_err("invalid selection must fail before starting a child");
    assert_eq!(error.failure, failure);
    assert_eq!(error.cleanup, Cleanup::NotStarted);
    assert!(error.source().is_none());
}

#[test]
fn literal_arguments_environment_and_borrowed_input_are_not_interpreted() {
    let mut selected = command();
    selected.arguments = vec![
        "".into(),
        "$HOME; $(touch forbidden) * 'quoted'".into(),
        "é\n".into(),
    ];
    selected.environment = BTreeMap::from([
        ("_NAME_2".into(), "$(ignored)=$OTHER\n".into()),
        ("A".into(), String::new()),
    ]);
    let original = selected.clone();
    selected
        .validate()
        .expect("literal shell syntax is ordinary argument data");
    let input = b"exact\0private\ninput";
    let request = Request {
        command: &selected,
        input,
        options: Options {
            allow_execute: true,
            max_input_bytes: input.len(),
            ..Options::default()
        },
    };
    request
        .options
        .validate(request.command, request.input)
        .expect("finite binary stdin is permitted");
    assert_eq!(selected, original);
    assert!(std::ptr::eq(request.command, &selected));
    assert_eq!(request.input, input);
    let decoded: Command = serde_json::from_str(r#"{"program":"/checker","directory":"/work"}"#)
        .expect("omitted collections have explicit empty defaults");
    assert!(decoded.arguments.is_empty());
    assert!(decoded.environment.is_empty());
}

#[test]
fn relative_coordinates_invalid_names_and_every_nul_bearing_field_are_refused() {
    for value in ["", "checker", "./checker", "~/checker", "C:\\checker"] {
        let mut selected = command();
        selected.program = value.into();
        refused(selected.validate(), Failure::Command);
        let mut selected = command();
        selected.directory = value.into();
        refused(selected.validate(), Failure::Command);
    }
    for name in ["", "1NAME", "A=B", "A-B", "A B", "é", "A\0B"] {
        let mut selected = command();
        selected.environment.insert(name.into(), "private".into());
        refused(selected.validate(), Failure::Command);
    }
    for field in 0..4 {
        let mut selected = command();
        match field {
            0 => selected.program.push('\0'),
            1 => selected.directory.push('\0'),
            2 => selected.arguments.push("private\0argument".into()),
            _ => {
                selected
                    .environment
                    .insert("NAME".into(), "private\0value".into());
            }
        }
        refused(selected.validate(), Failure::Command);
    }
}

#[test]
fn argument_environment_and_aggregate_utf8_byte_ceilings_are_exact() {
    let mut selected = command();
    selected.arguments = vec![String::new(); 256];
    selected.validate().expect("exact argument ceiling");
    selected.arguments.push(String::new());
    refused(selected.validate(), Failure::Limit);
    let mut selected = command();
    selected.environment = (0..64).map(|i| (format!("E{i}"), String::new())).collect();
    selected
        .validate()
        .expect("exact environment entry ceiling");
    selected.environment.insert("NEXT".into(), String::new());
    refused(selected.validate(), Failure::Limit);

    let mut selected = command();
    selected.environment.insert("A".into(), "é".into());
    let remaining = (64 << 10) - selected.program.len() - selected.directory.len() - 1 - "é".len();
    selected.arguments.push("x".repeat(remaining));
    selected
        .validate()
        .expect("all command fields consume exactly 64 KiB");
    selected
        .environment
        .get_mut("A")
        .expect("known fixture variable")
        .push('x');
    refused(selected.validate(), Failure::Limit);
}

#[test]
fn permission_complete_input_and_deadline_bounds_are_checked_before_execution() {
    let selected = command();
    refused(
        Options::default().validate(&selected, b""),
        Failure::Permission,
    );
    let mut invalid = selected.clone();
    invalid.program.clear();
    refused(
        Options::default().validate(&invalid, b""),
        Failure::Permission,
    );
    let options = Options {
        allow_execute: true,
        max_input_bytes: 3,
        timeout: Duration::from_millis(1),
    };
    options
        .validate(&selected, b"abc")
        .expect("exact input and minimum deadline");
    refused(options.validate(&selected, b"abcd"), Failure::Limit);
    Options {
        max_input_bytes: 0,
        ..options
    }
    .validate(&selected, b"")
    .expect("empty complete input");
    refused(
        Options {
            max_input_bytes: 0,
            ..options
        }
        .validate(&selected, b"a"),
        Failure::Limit,
    );
    Options {
        max_input_bytes: 64 << 20,
        timeout: Duration::from_secs(300),
        ..options
    }
    .validate(&selected, b"")
    .expect("maximum configured limits");
    for timeout in [
        Duration::ZERO,
        Duration::from_micros(999),
        Duration::from_secs(300) + Duration::from_nanos(1),
    ] {
        refused(
            Options { timeout, ..options }.validate(&selected, b""),
            Failure::Limit,
        );
    }
    refused(
        Options {
            max_input_bytes: (64 << 20) + 1,
            ..options
        }
        .validate(&selected, b""),
        Failure::Limit,
    );
}

#[test]
fn error_display_and_codes_do_not_disclose_native_details() {
    for (failure, code) in [
        (Failure::Permission, "process_permission"),
        (Failure::Command, "process_command"),
        (Failure::Limit, "process_limit"),
        (Failure::Unsupported, "process_unsupported"),
        (Failure::Executable, "process_executable"),
        (Failure::ExecutableChanged, "process_executable_changed"),
        (Failure::Spawn, "process_spawn"),
        (Failure::Input, "process_input"),
        (Failure::Wait, "process_wait"),
        (Failure::Timeout, "process_timeout"),
    ] {
        let error = Error {
            failure,
            cleanup: Cleanup::Uncertain,
            source: Some(io::Error::other("PRIVATE_PATH_SECRET")),
        };
        assert_eq!(error.code(), code);
        assert!(!error.to_string().contains("PRIVATE_PATH_SECRET"));
        assert!(error.to_string().contains("Uncertain"));
        assert!(
            error
                .source()
                .expect("native detail retained privately")
                .to_string()
                .contains("PRIVATE_PATH_SECRET")
        );
    }
}

#[test]
fn executable_pin_is_strict_optional_data_with_its_own_exact_byte_bound() {
    use gazette_core::{process::ExecutablePin, source::Sha256Digest};
    let mut selected = command();
    let plain = serde_json::to_value(&selected).expect("ordinary command");
    assert_eq!(plain["executable"], serde_json::Value::Null);
    let pin = ExecutablePin {
        bytes: 3,
        sha256: Sha256Digest::of(b"abc"),
    };
    selected.executable = Some(pin);
    selected
        .validate()
        .expect("pure validation never opens missing executable");
    let bytes = serde_json::to_vec(&selected).expect("pinned command");
    assert_eq!(
        serde_json::from_slice::<Command>(&bytes).expect("round trip"),
        selected
    );
    assert_ne!(
        plain,
        serde_json::to_value(&selected).expect("pin is part of identity")
    );
    for length in [0, (256 << 20) + 1, u64::MAX] {
        selected.executable = Some(ExecutablePin {
            bytes: length,
            ..pin
        });
        refused(selected.validate(), Failure::Limit);
    }
    selected.executable = Some(ExecutablePin {
        bytes: 256 << 20,
        ..pin
    });
    selected.validate().expect("exact executable bound");
    for invalid in [
        serde_json::json!({"bytes":3}),
        serde_json::json!({"bytes":3,"sha256":pin.sha256,"extra":true}),
        serde_json::json!({"bytes":3,"sha256":"not-a-digest"}),
        serde_json::json!({"bytes":-1,"sha256":pin.sha256}),
    ] {
        let mut object = plain.clone();
        object["executable"] = invalid;
        assert!(serde_json::from_value::<Command>(object).is_err());
    }
    let duplicate = format!(
        r#"{{"program":"/checker","directory":"/work","executable":{{"bytes":3,"bytes":4,"sha256":"{}"}}}}"#,
        pin.sha256
    );
    assert!(serde_json::from_str::<Command>(&duplicate).is_err());
}
