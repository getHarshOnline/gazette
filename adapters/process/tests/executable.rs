//! Real pinned executable-file observations, not interpreter or process-image attestation.

#![cfg(unix)]

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use gazette_adapter_process::Runner;
use gazette_core::{
    process::{CaptureRequest, Cleanup, Command, ExecutablePin, Failure, Options, Request},
    source::Sha256Digest,
};
use gazette_ports::process::{CaptureRunner as _, Runner as _};

// This explicitly selected script is the pinned file. /bin/sh and the native utilities are
// fixture dependencies, deliberately not claimed to be covered by that one file's digest.
const SCRIPT: &str = "#!/bin/sh\ncase \"$1\" in qualify) exit 0;; esac\n/bin/cat >/dev/null\nprintf witness > \"$2\"\nprintf PRIVATE_OUTPUT\ncase \"$1\" in\nchanged) printf '\\n# changed bytes\\n' >> \"$0\";;\nreplaced) /bin/cp \"$0\" \"$0.next\" && /bin/mv \"$0.next\" \"$0\";;\ndeleted) /bin/rm \"$0\";;\nfailed) /bin/rm \"$0\"; while :; do :; done;;\nnonzero) exit 23;;\nesac\nexit 0\n";

struct Fixture {
    root: PathBuf,
    program: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir()
            .canonicalize()
            .expect("native temporary root exists");
        let root = (0..128)
            .find_map(|_| {
                let path = parent.join(format!(
                    "gazette-executable-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => Some(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("private fixture creation: {error}"),
                }
            })
            .expect("fresh unique disposable fixture");
        let program = root.join("selected.sh");
        fs::write(&program, SCRIPT).expect("exact fixture program bytes");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700))
            .expect("explicit fixture execute permission");
        let marker = root.join("executed");
        Self {
            root,
            program,
            marker,
        }
    }

    fn command(&self, mode: &str) -> Command {
        Command {
            program: self
                .program
                .to_str()
                .expect("UTF-8 fixture executable")
                .into(),
            executable: Some(ExecutablePin {
                bytes: SCRIPT.len() as u64,
                sha256: Sha256Digest::of(SCRIPT.as_bytes()),
            }),
            arguments: vec![
                mode.into(),
                self.marker.to_str().expect("UTF-8 witness path").into(),
            ],
            directory: self.root.to_str().expect("UTF-8 fixture directory").into(),
            environment: BTreeMap::new(),
        }
    }

    fn qualify_launch(&self) {
        // A fresh shebang launch can spend seconds in native interpreter initialization.
        // Qualify that launch separately, without witness or mutation effects. The later
        // transport/mutation cases retain their tight deadlines; they do not promise cold
        // interpreter startup. Pre-start refusal cases deliberately never call this helper.
        let command = self.command("qualify");
        let exit = Runner
            .run(Request {
                command: &command,
                input: &[],
                options: Options {
                    timeout: Duration::from_secs(30),
                    ..options()
                },
            })
            .expect("exact pinned fixture must complete native launch qualification");
        assert_eq!(exit.code, Some(0));
        assert!(!self.marker.exists(), "qualification has no witness effect");
        assert_eq!(
            fs::read(&self.program).expect("qualified selected program"),
            SCRIPT.as_bytes(),
            "qualification must preserve the exact selected bytes"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This exact directory was exclusively created by this fixture, never adopted.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn options() -> Options {
    Options {
        allow_execute: true,
        max_input_bytes: 4096,
        timeout: Duration::from_secs(5),
    }
}

#[test]
fn exact_pin_is_checked_by_both_native_traits_without_reinterpreting_exit() {
    for capture in [false, true] {
        for (mode, expected) in [("unchanged", 0), ("nonzero", 23)] {
            let fixture = Fixture::new();
            fixture.qualify_launch();
            let command = fixture.command(mode);
            let request = Request {
                command: &command,
                input: b"complete retained stdin",
                options: options(),
            };
            let exit = if capture {
                let result = Runner
                    .capture(CaptureRequest {
                        request,
                        max_output_bytes: 14,
                        max_stderr_bytes: None,
                    })
                    .expect("exact pin and complete native capture");
                assert_eq!(result.stdout, b"PRIVATE_OUTPUT");
                result.exit
            } else {
                Runner.run(request).expect("exact pin and discarded output")
            };
            assert_eq!(exit.code, Some(expected));
            assert_eq!(
                fs::read(&fixture.marker).expect("real executable witness"),
                b"witness"
            );
            assert_eq!(
                fs::read(&fixture.program).expect("unchanged selected program"),
                SCRIPT.as_bytes()
            );
        }
    }
}

#[test]
fn mismatched_pin_refuses_before_any_child_effect_and_permission_precedes_lookup() {
    let fixture = Fixture::new();
    for bad_length in [false, true] {
        let mut command = fixture.command("unchanged");
        let pin = command.executable.as_mut().expect("explicit fixture pin");
        if bad_length {
            pin.bytes += 1;
        } else {
            pin.sha256 = Sha256Digest::of(b"wrong reviewed executable");
        }
        let error = Runner
            .run(Request {
                command: &command,
                input: &[],
                options: options(),
            })
            .expect_err("mismatched selected bytes cannot execute");
        assert_eq!(
            (error.failure, error.cleanup),
            (Failure::Executable, Cleanup::NotStarted)
        );
        assert!(!fixture.marker.exists());
    }
    let mut command = fixture.command("unchanged");
    command.program.push_str(".missing");
    let error = Runner
        .run(Request {
            command: &command,
            input: &[],
            options: Options::default(),
        })
        .expect_err("permission is checked before pinned program lookup");
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Permission, Cleanup::NotStarted)
    );
    assert!(!fixture.marker.exists());
}

#[test]
fn changed_replaced_and_deleted_programs_cannot_return_a_successful_exit_or_stdout() {
    for capture in [false, true] {
        for mode in ["changed", "replaced", "deleted"] {
            let fixture = Fixture::new();
            fixture.qualify_launch();
            let command = fixture.command(mode);
            let request = Request {
                command: &command,
                input: &[],
                options: options(),
            };
            let error = if capture {
                Runner
                    .capture(CaptureRequest {
                        request,
                        max_output_bytes: 14,
                        max_stderr_bytes: None,
                    })
                    .expect_err("changed pinned program must discard complete captured output")
            } else {
                Runner
                    .run(request)
                    .expect_err("changed pinned program cannot yield an exit report")
            };
            assert_eq!(
                (error.failure, error.cleanup),
                (Failure::ExecutableChanged, Cleanup::Reaped),
                "{mode}"
            );
            assert_eq!(
                fs::read(&fixture.marker).expect("actual child reached mutation"),
                b"witness"
            );
            assert!(!error.to_string().contains("PRIVATE_OUTPUT"));
            match mode {
                "changed" => assert_ne!(
                    fs::read(&fixture.program).expect("changed program"),
                    SCRIPT.as_bytes()
                ),
                "replaced" => assert_eq!(
                    fs::read(&fixture.program).expect("same bytes, different native file"),
                    SCRIPT.as_bytes()
                ),
                "deleted" => assert!(!fixture.program.exists()),
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn symlink_directory_and_socket_are_refused_without_blocking_or_starting() {
    let fixture = Fixture::new();
    let link = fixture.root.join("linked.sh");
    symlink(&fixture.program, &link).expect("disposable symlink");
    // Keep the pathname short enough for both Linux and macOS Unix-domain
    // socket limits even when the host supplies a long temporary-directory root.
    let socket = tempfile::Builder::new()
        .prefix("gzs")
        .tempfile_in(std::env::temp_dir())
        .expect("short disposable socket pathname")
        .into_temp_path();
    fs::remove_file(&socket).expect("vacate reserved socket pathname");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)
        .expect("native disposable Unix socket creation");
    for path in [link.as_path(), fixture.root.as_path(), socket.as_ref()] {
        let mut command = fixture.command("unchanged");
        command.program = path.to_str().expect("fixture path").into();
        let started = Instant::now();
        let error = Runner
            .run(Request {
                command: &command,
                input: &[],
                options: options(),
            })
            .expect_err("pinned program must be a real regular file");
        assert_eq!(
            (error.failure, error.cleanup),
            (Failure::Executable, Cleanup::NotStarted)
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "special-file lookup must not wait for a peer"
        );
        assert!(!fixture.marker.exists());
    }
}

#[test]
fn pin_bounds_precede_lookup_and_transport_failures_keep_their_original_custody() {
    let fixture = Fixture::new();
    for bytes in [0, (256 << 20) + 1] {
        let mut command = fixture.command("unchanged");
        command.program.push_str(".missing");
        command.executable.as_mut().expect("explicit pin").bytes = bytes;
        let error = Runner
            .run(Request {
                command: &command,
                input: &[],
                options: options(),
            })
            .expect_err("invalid pin ceiling precedes native file lookup");
        assert_eq!(error.failure, Failure::Limit);
        assert_eq!(error.cleanup, Cleanup::NotStarted);
        assert!(
            error.source.is_none(),
            "pure pin bounds precede missing-path IO"
        );
        assert!(!fixture.marker.exists());
    }
    fixture.qualify_launch();
    let command = fixture.command("failed");
    let error = Runner
        .run(Request {
            command: &command,
            input: &[],
            options: Options {
                timeout: Duration::from_secs(1),
                ..options()
            },
        })
        .expect_err("native timeout must survive later executable disappearance");
    assert_eq!(
        (error.failure, error.cleanup),
        (Failure::Timeout, Cleanup::Reaped)
    );
    assert!(
        fixture.marker.exists(),
        "actual child executed and changed its selected path"
    );
    assert!(!fixture.program.exists());
}
