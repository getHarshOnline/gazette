//! Actual bounded stdout transports; shell use is explicitly selected by these fixtures only.

#![cfg(unix)]

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use gazette_adapter_process::Runner;
use gazette_core::process::{
    CaptureRequest, Captured, Cleanup, Command, Error, Failure, Options, Request,
};
use gazette_ports::process::CaptureRunner as _;

fn shell(script: &str) -> Command {
    Command {
        executable: None,
        program: "/bin/sh".into(),
        arguments: vec!["-c".into(), script.into()],
        directory: std::env::current_dir()
            .expect("fixture working directory exists")
            .to_str()
            .expect("fixture working directory is UTF-8")
            .into(),
        environment: BTreeMap::new(),
    }
}

fn options() -> Options {
    Options {
        allow_execute: true,
        max_input_bytes: 4 << 20,
        timeout: Duration::from_secs(5),
    }
}

fn capture(
    command: &Command,
    input: &[u8],
    maximum: usize,
    options: Options,
) -> Result<Captured, Error> {
    Runner.capture(CaptureRequest {
        request: Request {
            command,
            input,
            options,
        },
        max_output_bytes: maximum,
        max_stderr_bytes: None,
    })
}

#[test]
fn exact_binary_limit_empty_output_and_nonzero_status_are_complete_results() {
    let command = shell("printf '\\377\\000x'; exit 23");
    let result = capture(&command, &[], 3, options()).expect("exact binary output fits");
    assert_eq!(result.stdout, [255, 0, b'x']);
    assert_eq!(result.exit.code, Some(23));
    let empty = capture(&shell("exit 0"), &[], 0, options()).expect("zero permits empty stdout");
    assert!(empty.stdout.is_empty());
    assert_eq!(empty.exit.code, Some(0));
}

#[test]
fn overflow_returns_only_redacted_error_never_a_captured_prefix() {
    for (script, bound) in [("printf SECRET", 5), ("printf x", 0)] {
        let error = capture(&shell(script), &[], bound, options())
            .expect_err("complete stdout exceeds cap");
        assert_eq!(error.failure, Failure::OutputLimit);
        assert_eq!(error.code(), "process_output_limit");
        assert_eq!(error.cleanup, Cleanup::Reaped);
        assert!(!error.to_string().contains("SECRET"));
        assert!(error.source.is_none());
    }
}

#[test]
fn output_is_drained_before_child_reads_large_input_and_stderr_is_discarded() {
    let input: Vec<_> = (0..(1 << 20)).map(|n| (n % 251) as u8).collect();
    let command = shell(
        "/bin/dd if=/dev/zero bs=65536 count=4 2>/dev/null; /bin/dd if=/dev/zero bs=65536 count=4 1>&2 2>/dev/null; exec /bin/cat",
    );
    let result = capture(&command, &input, (256 << 10) + input.len(), options())
        .expect("concurrent stdout drain and stdin delivery avoid pipe deadlock");
    assert_eq!(result.exit.code, Some(0));
    assert_eq!(result.stdout.len(), (256 << 10) + input.len());
    assert!(result.stdout[..(256 << 10)].iter().all(|&byte| byte == 0));
    assert_eq!(&result.stdout[(256 << 10)..], input);
}

#[test]
fn capture_uses_only_explicit_environment_and_literal_arguments() {
    let mut command = shell("");
    command.program = "/usr/bin/env".into();
    command.arguments.clear();
    command.environment.insert(
        "GAZETTE_EXPLICIT".into(),
        "$(not-a-command); * literal".into(),
    );
    let expected = b"GAZETTE_EXPLICIT=$(not-a-command); * literal\n";
    let result =
        capture(&command, &[], expected.len(), options()).expect("explicit environment captured");
    assert_eq!(result.stdout, expected);
    assert_eq!(result.exit.code, Some(0));

    let mut command = shell("printf '%s' \"$1\"");
    command
        .arguments
        .extend(["fixture".into(), "$(not-a-command); * literal".into()]);
    let result = capture(&command, &[], 64, options()).expect("selected argument remains literal");
    assert_eq!(result.stdout, b"$(not-a-command); * literal");
}

#[test]
fn permissions_and_all_capture_bounds_fail_before_program_lookup() {
    let mut command = shell("");
    command.program = "/gazette-no-such-executable/capture".into();
    let denied = capture(
        &command,
        &[],
        1,
        Options {
            allow_execute: false,
            ..options()
        },
    )
    .expect_err("execution permission precedes lookup");
    assert_eq!(
        (denied.failure, denied.cleanup),
        (Failure::Permission, Cleanup::NotStarted)
    );
    let ceiling = capture(&command, &[], (64 << 20) + 1, options())
        .expect_err("capture ceiling checked before spawn");
    assert_eq!(
        (ceiling.failure, ceiling.cleanup),
        (Failure::OutputLimit, Cleanup::NotStarted)
    );
    let input = capture(
        &command,
        b"x",
        1,
        Options {
            max_input_bytes: 0,
            ..options()
        },
    )
    .expect_err("input ceiling checked before spawn");
    assert_eq!(
        (input.failure, input.cleanup),
        (Failure::Limit, Cleanup::NotStarted)
    );
    let deadline = capture(
        &command,
        &[],
        1,
        Options {
            timeout: Duration::ZERO,
            ..options()
        },
    )
    .expect_err("deadline checked before spawn");
    assert_eq!(
        (deadline.failure, deadline.cleanup),
        (Failure::Limit, Cleanup::NotStarted)
    );
}

#[test]
fn reaped_child_with_descendant_held_stdout_still_requires_eof_by_deadline() {
    // The short-lived descendant is deliberately not claimed to be killed by the runner.
    let command = shell("/bin/sleep 1 & exit 0");
    let started = Instant::now();
    let error = capture(
        &command,
        &[],
        0,
        Options {
            timeout: Duration::from_millis(100),
            ..options()
        },
    )
    .expect_err("a reaped shell alone does not complete an inherited pipe");
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Timeout, Cleanup::Reaped)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn stdout_eof_without_child_exit_and_early_exit_without_input_are_not_success() {
    let command = shell("exec 1>&-; exec /bin/sleep 1");
    let error = capture(
        &command,
        &[],
        0,
        Options {
            timeout: Duration::from_millis(100),
            ..options()
        },
    )
    .expect_err("stdout EOF still requires reaped exit");
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Timeout, Cleanup::Reaped)
    );
    let error = capture(
        &shell("printf short; exit 0"),
        &vec![b'x'; 4 << 20],
        5,
        options(),
    )
    .expect_err("early zero exit cannot excuse undelivered input");
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Input, Cleanup::Reaped)
    );
}

fn diagnostics(
    command: &Command,
    input: &[u8],
    output: usize,
    stderr: usize,
    options: Options,
) -> Result<Captured, Error> {
    Runner.capture(CaptureRequest {
        request: Request {
            command,
            input,
            options,
        },
        max_output_bytes: output,
        max_stderr_bytes: Some(stderr),
    })
}

#[test]
fn diagnostic_capture_preserves_exact_bytes_and_distinguishes_empty_from_discarded() {
    let command = shell("printf output; printf '\\377\\000warning' >&2; exit 23");
    let result = diagnostics(&command, &[], 6, 9, options()).expect("complete selected streams");
    assert_eq!(result.stdout, b"output");
    assert_eq!(result.stderr.as_deref(), Some(b"\xff\0warning".as_slice()));
    assert_eq!(result.exit.code, Some(23));
    let empty = diagnostics(&shell("exit 0"), &[], 0, 0, options()).expect("empty streams");
    assert_eq!(empty.stderr, Some(Vec::new()));
    let discarded = capture(&command, &[], 6, options()).expect("explicitly discarded diagnostics");
    assert_eq!(discarded.stderr, None);
}

#[test]
fn both_output_streams_drain_before_large_input_without_deadlock() {
    let input = vec![b'x'; 1 << 20];
    let command = shell(
        "/bin/dd if=/dev/zero bs=65536 count=4 2>/dev/null; /bin/dd if=/dev/zero bs=65536 count=4 1>&2 2>/dev/null; exec /bin/cat",
    );
    let result = diagnostics(
        &command,
        &input,
        (256 << 10) + input.len(),
        256 << 10,
        options(),
    )
    .expect("both full pipes drain alongside stdin delivery");
    assert_eq!(result.exit.code, Some(0));
    assert_eq!(&result.stdout[(256 << 10)..], input);
    assert!(result.stdout[..(256 << 10)].iter().all(|&byte| byte == 0));
    assert_eq!(result.stderr, Some(vec![0; 256 << 10]));
}

#[test]
fn diagnostic_overflow_discards_all_output_and_invalid_bound_precedes_spawn() {
    for bound in [0, 6] {
        let error = diagnostics(
            &shell("printf output; printf PRIVATE >&2"),
            &[],
            6,
            bound,
            options(),
        )
        .expect_err("stderr overflow is not truncated success");
        assert_eq!(
            (error.failure, error.cleanup),
            (Failure::OutputLimit, Cleanup::Reaped)
        );
        assert!(error.source.is_none());
        assert!(!error.to_string().contains("PRIVATE"));
    }
    let mut command = shell("");
    command.program = "/gazette-no-such-executable/diagnostics".into();
    let error = diagnostics(&command, &[], 0, (64 << 20) + 1, options())
        .expect_err("stderr ceiling checked before native lookup");
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::OutputLimit, Cleanup::NotStarted)
    );
}

#[test]
fn stderr_eof_and_child_reaping_are_both_required() {
    for script in [
        "exec 1>&-; /bin/sleep 1 & exit 0",
        "exec 1>&- 2>&-; exec /bin/sleep 1",
    ] {
        let error = diagnostics(
            &shell(script),
            &[],
            0,
            0,
            Options {
                timeout: Duration::from_millis(100),
                ..options()
            },
        )
        .expect_err("closed stdout cannot excuse retained stderr or live child");
        assert_eq!(
            (error.failure, error.cleanup),
            (Failure::Timeout, Cleanup::Reaped)
        );
    }
}

fn segmented(
    command: &Command,
    segments: &[&[u8]],
    maximum: usize,
    input_limit: usize,
    timeout: Duration,
) -> Result<Captured, Error> {
    Runner.capture_segmented(gazette_core::process::SegmentedCaptureRequest {
        command,
        segments,
        allow_execute: true,
        max_input_bytes: input_limit,
        timeout,
        max_output_bytes: Some(maximum),
        max_stderr_bytes: None,
    })
}

#[test]
fn segmented_binary_input_preserves_order_empty_parts_and_final_eof() {
    let command = shell("exec /bin/cat");
    let parts: &[&[u8]] = &[b"", &[0, 255, 9], b"", b"second", &[128, 0], b""];
    let expected = parts.concat();
    let actual = segmented(
        &command,
        parts,
        expected.len(),
        expected.len(),
        Duration::from_secs(5),
    )
    .expect("complete ordered segments");
    assert_eq!(actual.exit.code, Some(0));
    assert_eq!(actual.stdout, expected);
    assert_eq!(
        gazette_core::source::Sha256Digest::of_slices(parts.iter().copied()),
        gazette_core::source::Sha256Digest::of(&expected)
    );
}

#[test]
fn segmented_input_over_buffered_ceiling_uses_borrowed_parts_and_small_response() {
    let chunk = vec![7; 1 << 20];
    let parts = vec![chunk.as_slice(); 65];
    let command = shell("exec /usr/bin/wc -c");
    let actual = segmented(&command, &parts, 32, 65 << 20, Duration::from_secs(30))
        .expect("65 MiB delivered without a concatenated input buffer");
    assert_eq!(actual.exit.code, Some(0));
    assert_eq!(
        std::str::from_utf8(&actual.stdout).unwrap().trim(),
        (65 << 20).to_string()
    );
    let legacy = capture(
        &command,
        &[],
        32,
        Options {
            max_input_bytes: 65 << 20,
            ..options()
        },
    )
    .expect_err("ordinary buffered contract stays capped");
    assert_eq!(
        (legacy.failure, legacy.cleanup),
        (Failure::Limit, Cleanup::NotStarted)
    );
}

#[test]
fn segmented_bounds_and_permission_refuse_before_lookup() {
    let mut command = shell("");
    command.program = "/gazette-no-such-executable/segmented".into();
    let parts: &[&[u8]] = &[b"ab", b"cd"];
    let error = segmented(&command, parts, 0, 3, Duration::from_secs(1)).unwrap_err();
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Limit, Cleanup::NotStarted)
    );
    let count = vec![b"".as_slice(); 20_004];
    let error = segmented(&command, &count, 0, 0, Duration::from_secs(1)).unwrap_err();
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Limit, Cleanup::NotStarted)
    );
    let error = Runner
        .capture_segmented(gazette_core::process::SegmentedCaptureRequest {
            command: &command,
            segments: parts,
            allow_execute: false,
            max_input_bytes: 4,
            timeout: Duration::from_secs(1),
            max_output_bytes: Some(0),
            max_stderr_bytes: None,
        })
        .unwrap_err();
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Permission, Cleanup::NotStarted)
    );
}

#[test]
fn segmented_input_keeps_timeout_early_exit_and_capture_overflow_refusals() {
    let chunk = vec![1; 1 << 20];
    let parts = [chunk.as_slice(), chunk.as_slice()];
    let early =
        segmented(&shell("exit 0"), &parts, 0, 2 << 20, Duration::from_secs(5)).unwrap_err();
    assert_eq!(
        (early.failure, early.cleanup),
        (Failure::Input, Cleanup::Reaped)
    );
    let mut sleeper = shell("");
    sleeper.program = "/bin/sleep".into();
    sleeper.arguments = vec!["30".into()];
    let timeout = segmented(&sleeper, &parts, 0, 2 << 20, Duration::from_millis(100)).unwrap_err();
    assert_eq!(
        (timeout.failure, timeout.cleanup),
        (Failure::Timeout, Cleanup::Reaped)
    );
    let overflow = segmented(
        &shell("printf XX; exec /bin/cat"),
        &parts,
        1,
        2 << 20,
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert_eq!(
        (overflow.failure, overflow.cleanup),
        (Failure::OutputLimit, Cleanup::Reaped)
    );
}
