//! Fixed Git advertisements plus bounded local namespace qualification, never fetch or release.
//!
//! The selected executable and its native helper installation must be trusted. The
//! child receives an isolated environment, but is not sandboxed; Git can launch its
//! own upload-pack/HTTPS helpers. Bounds cover captured output and post-spawn elapsed
//! time, not all provider allocations or descendants. Namespace checks require
//! trusted exclusive local custody and do not defeat hostile same-user replacement.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    os::unix::fs::MetadataExt,
    path::Path,
};

use gazette_core::{
    process,
    release::{
        GitCredentialSelection, GitSelection, RefAdvertisement, RemoteReadError, RemoteReadFailure,
        RemoteReadOptions, validate_git_destination,
    },
};
use gazette_ports::process::CaptureRunner;
use url::Url;

/// Read one advertised ref using an explicitly selected native Git installation.
pub struct RemoteReader<'a, R> {
    /// Shared bounded process mechanism; no shell command or arbitrary arguments are accepted here.
    pub runner: &'a R,
    /// Fixed native executable, isolation and optional HTTPS credential helper.
    pub selection: &'a GitSelection,
}

impl<R: CaptureRunner> gazette_ports::release::RemoteReader for RemoteReader<'_, R> {
    fn observe_remote(
        &self,
        remote: &str,
        branch: &str,
        options: RemoteReadOptions,
    ) -> Result<RefAdvertisement, RemoteReadError> {
        options.validate()?;
        if !self.selection.is_valid() {
            return Err(error(RemoteReadFailure::Selection));
        }
        validate_git_destination(remote, branch)
            .map_err(|_| error(RemoteReadFailure::Selection))?;
        if remote.len() > 16 * 1024 || branch.len() > 4096 {
            return Err(error(RemoteReadFailure::Limit));
        }
        let target = target(remote, options.allow_network)?;
        let program = self
            .selection
            .program
            .to_str()
            .filter(|_| self.selection.program.is_absolute())
            .ok_or_else(|| error(RemoteReadFailure::Selection))?;
        let isolation = Isolation::open(&self.selection.isolation)?;
        let directory = isolation
            .path
            .to_str()
            .filter(|p| !p.contains(':'))
            .ok_or_else(|| error(RemoteReadFailure::Isolation))?;
        let reference = format!("refs/heads/{branch}");
        let command = command(
            program,
            directory,
            &target,
            &reference,
            self.selection.credentials.as_ref(),
        )
        .map_err(|failure| error(failure.read()))?;
        command
            .validate()
            .map_err(|_| error(RemoteReadFailure::Selection))?;
        let captured = self.runner.capture(process::CaptureRequest {
            request: process::Request {
                command: &command,
                input: &[],
                options: process::Options {
                    allow_execute: true,
                    max_input_bytes: 0,
                    timeout: options.timeout,
                },
            },
            max_output_bytes: options.max_response_bytes,
            max_stderr_bytes: None,
        });
        let cleanup = Some(
            captured
                .as_ref()
                .map_or_else(|e| e.cleanup, |_| process::Cleanup::Reaped),
        );
        if isolation.recheck().is_err() {
            return Err(RemoteReadError {
                failure: RemoteReadFailure::Isolation,
                cleanup,
            });
        }
        let captured = captured.map_err(|e| RemoteReadError {
            failure: match e.failure {
                process::Failure::Permission => RemoteReadFailure::Permission,
                process::Failure::Command => RemoteReadFailure::Selection,
                process::Failure::Limit | process::Failure::OutputLimit => RemoteReadFailure::Limit,
                process::Failure::Timeout => RemoteReadFailure::Timeout,
                process::Failure::Unsupported => RemoteReadFailure::Unsupported,
                _ => RemoteReadFailure::Transport,
            },
            cleanup: Some(e.cleanup),
        })?;
        if captured.stdout.len() > options.max_response_bytes {
            return Err(RemoteReadError {
                failure: RemoteReadFailure::Limit,
                cleanup,
            });
        }
        let observed = advertisement(&captured, &reference)
            .map_err(|failure| RemoteReadError { failure, cleanup })?;
        // Silence alone remains indeterminate, including hidden refs and all network
        // providers. Only the existing trusted-local namespace verifier can strengthen it.
        if observed == RefAdvertisement::NotAdvertised
            && let Ok(url) = Url::parse(&target)
            && let Ok(root) = url.to_file_path()
            && crate::release::ref_absent(&root, &reference, options.max_response_bytes) == Ok(true)
        {
            return Ok(RefAdvertisement::Absent);
        }
        Ok(observed)
    }
}

fn error(failure: RemoteReadFailure) -> RemoteReadError {
    RemoteReadError::new(failure)
}

pub(super) fn target(remote: &str, allow_network: bool) -> Result<String, RemoteReadError> {
    if Path::new(remote).is_absolute() {
        return Url::from_file_path(remote)
            .map(String::from)
            .map_err(|()| error(RemoteReadFailure::Selection));
    }
    // Refuse URL parser repair (notably backslashes and omitted HTTPS authority).
    if remote.contains('\\') || !(remote.starts_with("file:///") || remote.starts_with("https://"))
    {
        return Err(error(RemoteReadFailure::Unsupported));
    }
    if remote
        .strip_prefix("https://")
        .is_some_and(|rest| rest.starts_with('/') || rest.is_empty())
    {
        return Err(error(RemoteReadFailure::Selection));
    }
    let url = Url::parse(remote).map_err(|_| error(RemoteReadFailure::Selection))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(error(RemoteReadFailure::Selection));
    }
    match url.scheme() {
        "file" if url.host_str().is_none() && url.port().is_none() => {
            let path = url
                .to_file_path()
                .map_err(|()| error(RemoteReadFailure::Selection))?;
            if !path.is_absolute()
                || path
                    .to_str()
                    .is_none_or(|path| path.chars().any(char::is_control))
            {
                return Err(error(RemoteReadFailure::Selection));
            }
        }
        "https" if url.host_str().is_some() => {
            if !allow_network {
                return Err(error(RemoteReadFailure::Permission));
            }
        }
        _ => return Err(error(RemoteReadFailure::Unsupported)),
    }
    // URL parsing is validation only: explicit URL bytes remain the selected Git
    // argument. Only absolute local paths are translated to unambiguous file URLs.
    Ok(remote.to_owned())
}

fn command(
    program: &str,
    directory: &str,
    remote: &str,
    reference: &str,
    credentials: Option<&GitCredentialSelection>,
) -> Result<process::Command, CommandConstructionError> {
    let mut command = git_command(program, directory, remote, credentials)?;
    command.arguments.extend(
        [
            "ls-remote",
            "--quiet",
            "--refs",
            "--exit-code",
            "--",
            remote,
            reference,
        ]
        .into_iter()
        .map(str::to_owned),
    );
    Ok(command)
}

#[derive(Clone, Copy)]
pub(super) enum CommandConstructionError {
    Permission,
    Selection,
}

impl CommandConstructionError {
    pub(super) const fn read(self) -> RemoteReadFailure {
        match self {
            Self::Permission => RemoteReadFailure::Permission,
            Self::Selection => RemoteReadFailure::Selection,
        }
    }
}

pub(super) fn git_command(
    program: &str,
    directory: &str,
    remote: &str,
    credentials: Option<&GitCredentialSelection>,
) -> Result<process::Command, CommandConstructionError> {
    let mut arguments = Vec::new();
    for setting in [
        "credential.helper=",
        "credential.interactive=false",
        "core.askPass=",
        "http.emptyAuth=false",
        "http.followRedirects=false",
        "http.sslVerify=true",
        "http.proxy=",
        "protocol.version=2",
    ] {
        arguments.extend(["-c".to_owned(), setting.to_owned()]);
    }
    if remote.starts_with("https://")
        && let Some(credentials) = credentials
    {
        if !credentials.allow_credentials {
            return Err(CommandConstructionError::Permission);
        }
        let helper = credentials
            .helper
            .to_str()
            .ok_or(CommandConstructionError::Selection)?;
        arguments.extend([
            "-c".to_owned(),
            format!("credential.helper={helper}"),
            "-c".to_owned(),
            "credential.useHttpPath=true".to_owned(),
        ]);
    }
    let environment = BTreeMap::from(
        [
            ("HOME", directory),
            ("XDG_CONFIG_HOME", directory),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_CEILING_DIRECTORIES", directory),
            (
                "GIT_ALLOW_PROTOCOL",
                if remote.starts_with("file://") {
                    "file"
                } else {
                    "https"
                },
            ),
            ("GIT_PROTOCOL_FROM_USER", "0"),
            ("GIT_NO_LAZY_FETCH", "1"),
            ("GIT_OPTIONAL_LOCKS", "0"),
            ("LC_ALL", "C"),
        ]
        .map(|(key, value)| (key.to_owned(), value.to_owned())),
    );
    Ok(process::Command {
        program: program.to_owned(),
        executable: None,
        arguments,
        directory: directory.to_owned(),
        environment,
    })
}

fn advertisement(
    captured: &process::Captured,
    reference: &str,
) -> Result<RefAdvertisement, RemoteReadFailure> {
    if captured.exit.code == Some(2) && captured.stdout.is_empty() {
        return Ok(RefAdvertisement::NotAdvertised);
    }
    if captured.exit.code != Some(0) {
        return Err(RemoteReadFailure::Transport);
    }
    let line = std::str::from_utf8(&captured.stdout)
        .ok()
        .and_then(|s| s.strip_suffix('\n'))
        .ok_or(RemoteReadFailure::Response)?;
    let (oid, actual) = line.split_once('\t').ok_or(RemoteReadFailure::Response)?;
    if actual != reference || oid.bytes().all(|byte| byte == b'0') {
        return Err(RemoteReadFailure::Response);
    }
    let oid = oid.parse().map_err(|_| RemoteReadFailure::Response)?;
    Ok(RefAdvertisement::Commit { oid })
}

pub(super) struct Isolation {
    pub(super) path: std::path::PathBuf,
    held: File,
    identity: (u64, u64),
}

impl Isolation {
    pub(super) fn open(path: &Path) -> Result<Self, RemoteReadError> {
        if !path.is_absolute() {
            return Err(error(RemoteReadFailure::Isolation));
        }
        let original =
            fs::symlink_metadata(path).map_err(|_| error(RemoteReadFailure::Isolation))?;
        if !original.is_dir() || original.mode() & 0o7777 != 0o700 {
            return Err(error(RemoteReadFailure::Isolation));
        }
        let path = fs::canonicalize(path).map_err(|_| error(RemoteReadFailure::Isolation))?;
        let held = File::open(&path).map_err(|_| error(RemoteReadFailure::Isolation))?;
        let selected = Self {
            path,
            held,
            identity: (original.dev(), original.ino()),
        };
        selected.recheck()?;
        Ok(selected)
    }

    pub(super) fn recheck(&self) -> Result<(), RemoteReadError> {
        for metadata in [self.held.metadata(), fs::symlink_metadata(&self.path)] {
            let metadata = metadata.map_err(|_| error(RemoteReadFailure::Isolation))?;
            if !metadata.is_dir()
                || metadata.mode() & 0o7777 != 0o700
                || (metadata.dev(), metadata.ino()) != self.identity
            {
                return Err(error(RemoteReadFailure::Isolation));
            }
        }
        let mut entries =
            fs::read_dir(&self.path).map_err(|_| error(RemoteReadFailure::Isolation))?;
        if entries.next().is_some() {
            return Err(error(RemoteReadFailure::Isolation));
        }
        Ok(())
    }
}
