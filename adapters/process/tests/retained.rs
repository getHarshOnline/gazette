//! Exact retained-resource execution, independent from the source's former native location.

#![cfg(unix)]

use std::{
    collections::BTreeMap, ffi::OsString, fs, os::unix::ffi::OsStringExt,
    process::Command as NativeCommand, time::Duration,
};

use gazette_adapter_process::Runner;
use gazette_core::{
    process::{CaptureRequest, Cleanup, Command, Failure, Options, Request},
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use gazette_ports::process::RetainedCaptureRunner as _;

fn retained(script: &[u8]) -> CapturedSource {
    let directory = "provider".parse::<EntryPath>().expect("provider directory");
    let entrypoint = "provider/render.sh"
        .parse::<EntryPath>()
        .expect("provider entrypoint");
    let mut inventory = InventoryBuilder::new(Limits::default());
    inventory
        .insert(directory, Content::Directory)
        .expect("directory");
    inventory
        .insert(
            entrypoint.clone(),
            Content::File {
                bytes: script.len() as u64,
                sha256: Sha256Digest::of(script),
            },
        )
        .expect("entrypoint");
    CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([(entrypoint, script.to_vec())]),
    )
    .expect("exact retained source")
}

fn command(directory: &std::path::Path) -> Command {
    Command {
        executable: None,
        program: "/bin/sh".into(),
        arguments: Vec::new(),
        directory: directory.to_str().expect("UTF-8 fixture directory").into(),
        environment: BTreeMap::new(),
    }
}

fn capture(
    source: &CapturedSource,
    command: &Command,
) -> Result<Vec<u8>, gazette_core::process::Error> {
    Runner
        .capture_retained(
            CaptureRequest {
                request: Request {
                    command,
                    input: &[],
                    options: Options {
                        allow_execute: true,
                        max_input_bytes: 0,
                        timeout: Duration::from_secs(5),
                    },
                },
                max_output_bytes: 32,
                max_stderr_bytes: None,
            },
            source.as_input(),
            &"provider/render.sh".parse().expect("entrypoint"),
        )
        .map(|captured| captured.stdout)
}

#[test]
fn invocation_uses_only_retained_bytes_and_identity_ignores_former_native_location() {
    let first_native = tempfile::tempdir().expect("first former native root");
    let second_native = tempfile::tempdir().expect("second former native root");
    let first_path = first_native.path().join("render.sh");
    let second_path = second_native.path().join("renamed.sh");
    fs::write(&first_path, b"printf retained").expect("first native bytes");
    fs::write(&second_path, b"printf retained").expect("second native bytes");
    let first = retained(&fs::read(&first_path).expect("selected first bytes"));
    let second = retained(&fs::read(&second_path).expect("selected second bytes"));
    fs::write(&first_path, b"printf MUTATED").expect("mutate original after selection");

    assert_eq!(first.inventory().identity(), second.inventory().identity());
    assert_eq!(
        capture(&first, &command(first_native.path())).expect("retained first invocation"),
        b"retained"
    );
    assert_eq!(
        capture(&second, &command(second_native.path())).expect("retained second invocation"),
        b"retained"
    );
}

#[test]
fn provider_cannot_change_or_extend_the_exact_staged_tree_and_return_success() {
    for script in [
        b"printf changed > \"$0\"; printf output".as_slice(),
        b"printf '%01024d' 0 >> \"$0\"; printf output".as_slice(),
        b"printf extra > \"$(dirname \"$0\")/extra\"; printf output".as_slice(),
        b"/bin/rm \"$0\"; /bin/ln -s /dev/zero \"$0\"; printf output".as_slice(),
        b"provider=${0%/*}; root=${provider%/*}; /bin/rm \"$0\"; /bin/rmdir \"$provider\"; /bin/ln -s /dev \"$provider\"; printf output".as_slice(),
    ] {
        let directory = tempfile::tempdir().expect("explicit command working directory");
        let error = capture(&retained(script), &command(directory.path()))
            .expect_err("changed retained resources invalidate the invocation");
        assert_eq!(error.failure, Failure::Command);
        assert_eq!(error.cleanup, Cleanup::Reaped);
    }
}

#[test]
fn non_utf8_temporary_roots_fail_before_lossy_execution() {
    const CHILD: &str = "GAZETTE_TEST_NON_UTF8_TEMPORARY_ROOT";
    if std::env::var_os(CHILD).is_some() {
        let current = std::env::current_dir().expect("current fixture directory");
        let error = capture(&retained(b"printf SHOULD_NOT_RUN"), &command(&current))
            .expect_err("string-based command contracts reject non-UTF-8 native paths");
        assert_eq!(error.failure, Failure::Command);
        assert_eq!(error.cleanup, Cleanup::NotStarted);
        assert!(
            error
                .source
                .as_ref()
                .is_some_and(|source| source.to_string().contains("not UTF-8"))
        );
        return;
    }

    let owner = tempfile::tempdir().expect("owned temporary root");
    let native = owner
        .path()
        .join(OsString::from_vec(b"gazette-non-utf8-\xff".to_vec()));
    if let Err(error) = fs::create_dir(&native) {
        assert_eq!(
            error.raw_os_error(),
            Some(rustix::io::Errno::ILSEQ.raw_os_error()),
            "only a host-level invalid-byte refusal can discharge this fixture"
        );
        return;
    }
    let status = NativeCommand::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            "non_utf8_temporary_roots_fail_before_lossy_execution",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("TMPDIR", native)
        .status()
        .expect("isolated test child");
    assert!(status.success(), "non-UTF-8 fixture child failed: {status}");
}
