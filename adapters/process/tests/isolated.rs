//! Fresh-working-directory capture through the native process adapter.

#![cfg(unix)]

use std::{collections::BTreeMap, time::Duration};

use gazette_adapter_process::Runner;
use gazette_core::{
    process::{
        Cleanup, ExecutablePin, Failure, IsolatedCaptureRequest, IsolatedCommand, IsolatedRequest,
        Options, PinnedExecutable,
    },
    source::Sha256Digest,
};
use gazette_ports::process::IsolatedCaptureRunner as _;

fn shell(script: &str) -> IsolatedCommand {
    let path = std::fs::canonicalize("/bin/sh").expect("exact native shell path");
    let bytes = std::fs::read(&path).expect("native shell bytes");
    IsolatedCommand {
        executable: PinnedExecutable {
            program: path.to_str().expect("native shell path").into(),
            executable: ExecutablePin {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(&bytes),
            },
        },
        arguments: vec!["-c".into(), script.into()],
        environment: BTreeMap::new(),
    }
}

fn capture(command: &IsolatedCommand) -> Result<Vec<u8>, gazette_core::process::Error> {
    Runner
        .capture_isolated(IsolatedCaptureRequest {
            request: IsolatedRequest {
                command,
                input: &[],
                options: Options {
                    allow_execute: true,
                    max_input_bytes: 0,
                    timeout: Duration::from_secs(5),
                },
            },
            max_output_bytes: 64,
            max_stderr_bytes: Some(64),
        })
        .map(|captured| captured.stdout)
}

#[test]
fn isolated_capture_starts_empty_with_no_inherited_environment() {
    let output = capture(&shell(
        "test -z \"$(ls -A)\" && test -z \"${GAZETTE_AMBIENT+x}\" && printf isolated",
    ))
    .expect("fresh isolated execution");
    assert_eq!(output, b"isolated");
}

#[test]
fn isolated_capture_refuses_working_directory_mutation() {
    let error = capture(&shell("printf residue > created; printf output"))
        .expect_err("working directory mutation invalidates captured output");
    assert_eq!(error.failure, Failure::Command);
    assert_eq!(error.cleanup, Cleanup::Reaped);
}
