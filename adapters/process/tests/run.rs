//! Real direct-child transport tests; the current test executable is the owned fixture.

#![cfg(unix)]

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    time::{Duration, Instant},
};

use gazette_adapter_process::Runner;
use gazette_core::process::{Cleanup, Command, Failure, Options, Request};
use gazette_ports::process::Runner as _;

const MODE: &str = "GAZETTE_PROCESS_FIXTURE";
const LITERAL: &str = "$(not-a-command); * 'literal'";

fn command(mode: &str) -> Command {
    let directory = std::env::current_dir().expect("current test directory exists");
    Command {
        executable: None,
        program: std::env::current_exe()
            .expect("owned test executable is available")
            .to_str()
            .expect("fixture executable path is UTF-8")
            .to_owned(),
        arguments: vec![
            "--exact".into(),
            "fixture".into(),
            "--nocapture".into(),
            "--skip".into(),
            LITERAL.into(),
        ],
        directory: directory
            .to_str()
            .expect("test directory path is UTF-8")
            .to_owned(),
        environment: BTreeMap::from([(MODE.into(), mode.into())]),
    }
}

fn options() -> Options {
    Options {
        allow_execute: true,
        max_input_bytes: 4 << 20,
        timeout: Duration::from_secs(5),
    }
}

fn bytes() -> Vec<u8> {
    (0..(1 << 20)).map(|n| (n % 251) as u8).collect()
}

#[test]
fn fixture() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    if mode == "reject" {
        std::process::exit(0);
    }
    if mode == "blocked" {
        std::thread::sleep(Duration::from_secs(30));
        panic!("deadline must terminate the blocked fixture");
    }
    if mode == "output" {
        let chunk = [b'x'; 64 << 10];
        for _ in 0..128 {
            std::io::stdout()
                .write_all(&chunk)
                .expect("stdout is discarded");
            std::io::stderr()
                .write_all(&chunk)
                .expect("stderr is discarded");
        }
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(4 << 20)
        .read_to_end(&mut input)
        .expect("fixture reads complete input and EOF");
    match mode.as_str() {
        "exact" => {
            assert_eq!(input, bytes(), "binary stdin must be exact and complete");
            assert!(std::env::args().any(|argument| argument == LITERAL));
            let directory =
                std::env::var("EXPECTED_DIRECTORY").expect("explicit directory binding");
            assert_eq!(
                std::env::current_dir().expect("fixture directory"),
                std::path::Path::new(&directory)
            );
            let mut keys: Vec<_> = std::env::vars().map(|(key, _)| key).collect();
            keys.sort();
            assert_eq!(keys, ["EXPECTED_DIRECTORY", MODE]);
        }
        "nonzero" => std::process::exit(23),
        "signal" => {
            rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::TERM)
                .expect("fixture can terminate itself");
            panic!("termination signal must not return");
        }
        "output" => assert_eq!(input, bytes()),
        "empty" => assert!(input.is_empty()),
        "wait" => {
            assert!(input.is_empty());
            std::thread::sleep(Duration::from_secs(30));
            panic!("deadline must terminate child after EOF");
        }
        _ => panic!("unknown owned fixture mode"),
    }
}

#[test]
fn exact_input_eof_literal_arguments_directory_and_cleared_environment() {
    let mut command = command("exact");
    command.directory = std::env::temp_dir()
        .canonicalize()
        .expect("existing temporary directory resolves")
        .to_str()
        .expect("temporary directory path is UTF-8")
        .to_owned();
    command
        .environment
        .insert("EXPECTED_DIRECTORY".into(), command.directory.clone());
    let report = Runner
        .run(Request {
            command: &command,
            input: &bytes(),
            options: options(),
        })
        .expect("complete binary input and exact native selection are delivered");
    assert_eq!(report.code, Some(0));
}

#[test]
fn empty_input_is_closed_and_native_nonzero_or_signal_is_not_reinterpreted() {
    for (mode, code) in [("empty", Some(0)), ("nonzero", Some(23)), ("signal", None)] {
        let report = Runner
            .run(Request {
                command: &command(mode),
                input: &[],
                options: options(),
            })
            .expect("complete empty input produces a native exit observation");
        assert_eq!(report.code, code, "mode {mode}");
    }
}

#[test]
fn complete_input_is_required_even_when_early_exit_is_zero() {
    let input = vec![b'x'; 4 << 20];
    let error = Runner
        .run(Request {
            command: &command("reject"),
            input: &input,
            options: options(),
        })
        .expect_err("zero exit cannot excuse undelivered input");
    assert_eq!(error.failure, Failure::Input);
    assert_eq!(error.cleanup, Cleanup::Reaped);
}

#[test]
fn deadline_covers_blocked_input_and_wait_after_eof() {
    let input = bytes();
    for (mode, input) in [("blocked", input.as_slice()), ("wait", &[][..])] {
        let start = Instant::now();
        let error = Runner
            .run(Request {
                command: &command(mode),
                input,
                options: Options {
                    timeout: Duration::from_millis(40),
                    ..options()
                },
            })
            .expect_err("one bounded deadline includes delivery and exit wait");
        assert_eq!(error.failure, Failure::Timeout);
        assert_eq!(error.cleanup, Cleanup::Reaped);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "deadline does not wait for fixture sleep"
        );
    }
}

#[test]
fn large_stdout_and_stderr_are_discarded_without_blocking_input() {
    let report = Runner
        .run(Request {
            command: &command("output"),
            input: &bytes(),
            options: options(),
        })
        .expect("discarded output cannot fill a captured-output pipe");
    assert_eq!(report.code, Some(0));
}

#[test]
fn permission_and_bounds_precede_native_lookup() {
    let mut command = command("empty");
    command.program.push_str(".missing");
    for (options, input, failure) in [
        (Options::default(), &[][..], Failure::Permission),
        (
            Options {
                max_input_bytes: 0,
                ..options()
            },
            &b"x"[..],
            Failure::Limit,
        ),
        (
            Options {
                timeout: Duration::ZERO,
                ..options()
            },
            &[][..],
            Failure::Limit,
        ),
        (options(), &[][..], Failure::Spawn),
    ] {
        let error = Runner
            .run(Request {
                command: &command,
                input,
                options,
            })
            .expect_err("refusal or missing executable cannot produce exit success");
        assert_eq!(error.failure, failure);
        assert_eq!(error.cleanup, Cleanup::NotStarted);
    }
}

#[test]
fn invalid_native_command_is_refused_without_starting() {
    for field in ["program", "directory", "environment", "argument"] {
        let mut command = command("empty");
        match field {
            "program" => command.program = "relative".into(),
            "directory" => command.directory = "relative".into(),
            "environment" => {
                command
                    .environment
                    .insert("INVALID=NAME".into(), "value".into());
            }
            "argument" => command.arguments.push("nul\0argument".into()),
            _ => unreachable!(),
        }
        let error = Runner
            .run(Request {
                command: &command,
                input: &[],
                options: options(),
            })
            .expect_err("invalid command must fail preflight");
        assert_eq!(error.failure, Failure::Command);
        assert_eq!(error.cleanup, Cleanup::NotStarted);
    }
}
