//! Exact local or explicitly selected HTTPS receive-pack dispatch.
//!
//! Normal publication uses an exact expected-old lease with an ordinary non-force
//! refspec. The prepared raw single-parent commit proves the intended update is a
//! fast-forward from that exact reviewed state. A concurrent destination change is
//! therefore rejected rather than overwritten. Neither a native return nor an
//! already-present ref establishes release success; independent read-back remains
//! authoritative evidence after dispatch.

use gazette_core::{
    process::{self, Cleanup},
    release::{
        DispatchEffect, DispatchError, DispatchFailure, DispatchObservation, DispatchOptions,
        DispatchRequest, DispatchStatus, RemoteReadFailure, RemoteState,
    },
    source::Sha256Digest,
};
use gazette_ports::{process::CaptureRunner, release::ReleaseWorktrees};

use crate::{Git, remote};

mod local;

/// Exact one-target Git dispatch, called only after a durable attempt start.
/// The application supplies current authority and complete filesystem-output checks.
pub struct Dispatcher<'a, R> {
    /// Existing bounded process mechanism; no arbitrary caller argument vector.
    pub runner: &'a R,
    /// Fixed native executable, isolation and optional HTTPS credential helper.
    pub selection: &'a gazette_core::release::GitSelection,
}

impl<R: CaptureRunner> gazette_ports::release::ReleaseDispatcher for Dispatcher<'_, R> {
    fn dispatch(
        &self,
        request: DispatchRequest<'_>,
        options: DispatchOptions,
    ) -> Result<DispatchObservation, DispatchError> {
        options.validate()?;
        request.validate()?;
        if options.profile == gazette_core::release::DispatchProfile::LocalTransaction {
            return local::dispatch(request, options);
        }
        if !self.selection.is_valid() {
            return Err(DispatchError::refused(DispatchFailure::Selection));
        }
        if request.target.remote.len() > 16 * 1024 || request.target.branch.len() > 4096 {
            return Err(DispatchError::refused(DispatchFailure::Limit));
        }
        let target =
            remote::target(&request.target.remote, options.allow_network).map_err(|error| {
                DispatchError::refused(match error.failure {
                    RemoteReadFailure::Permission => DispatchFailure::Permission,
                    RemoteReadFailure::Unsupported => DispatchFailure::Unsupported,
                    _ => DispatchFailure::Selection,
                })
            })?;
        // The shared destination gate admits only local files or explicitly permitted
        // HTTPS. Both use the same exact expected-old lease, ordinary non-force
        // refspec and independently verified commit; TLS, redirect and credential
        // policy belongs to git_command.
        let program = self
            .selection
            .program
            .to_str()
            .filter(|_| self.selection.program.is_absolute())
            .ok_or_else(|| DispatchError::refused(DispatchFailure::Selection))?;
        let isolation = remote::Isolation::open(&self.selection.isolation)
            .map_err(|_| DispatchError::refused(DispatchFailure::Isolation))?;
        let directory = isolation
            .path
            .to_str()
            .filter(|path| !path.contains(':'))
            .ok_or_else(|| DispatchError::refused(DispatchFailure::Isolation))?;
        Git.inspect_worktree(
            request.repository,
            &request.target.local_commit_oid,
            request.output,
            request.metadata,
            options.worktree,
        )
        .map_err(|_| DispatchError::refused(DispatchFailure::LocalState))?;
        let command = command(
            program,
            directory,
            &target,
            self.selection.credentials.as_ref(),
            request,
        )?;
        command
            .validate()
            .map_err(|_| DispatchError::refused(DispatchFailure::Selection))?;
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
        let cleanup = captured
            .as_ref()
            .map_or_else(|error| error.cleanup, |_| Cleanup::Reaped);
        if isolation.recheck().is_err() {
            return Err(uncertain(DispatchFailure::Isolation, cleanup));
        }
        let captured = captured.map_err(|error| {
            let failure = match error.failure {
                process::Failure::Timeout => DispatchFailure::Timeout,
                process::Failure::Limit | process::Failure::OutputLimit => DispatchFailure::Limit,
                _ => DispatchFailure::Transport,
            };
            if error.cleanup == Cleanup::NotStarted {
                DispatchError::refused(failure)
            } else {
                uncertain(failure, error.cleanup)
            }
        })?;
        if captured.stdout.len() > options.max_response_bytes {
            return Err(uncertain(DispatchFailure::Limit, cleanup));
        }
        let status = response(&captured, &target, &request.target.refspec)
            .map_err(|failure| uncertain(failure, cleanup))?;
        Ok(DispatchObservation {
            status,
            exit_code: captured.exit.code,
            response_sha256: Sha256Digest::of(&captured.stdout),
            cleanup,
        })
    }
}

fn command(
    program: &str,
    directory: &str,
    remote: &str,
    credentials: Option<&gazette_core::release::GitCredentialSelection>,
    request: DispatchRequest<'_>,
) -> Result<process::Command, DispatchError> {
    let git_directory = request.repository.join(".git");
    let git_directory = git_directory
        .to_str()
        .ok_or_else(|| DispatchError::refused(DispatchFailure::Selection))?;
    let mut command = remote::git_command(program, directory, remote, credentials).map_err(
        |error| match error {
            remote::CommandConstructionError::Permission => {
                DispatchError::refused(DispatchFailure::Permission)
            }
            remote::CommandConstructionError::Selection => {
                DispatchError::refused(DispatchFailure::Selection)
            }
        },
    )?;
    for setting in [
        "core.hooksPath=/dev/null",
        "push.followTags=false",
        "push.gpgSign=false",
        "push.recurseSubmodules=no",
        "gc.auto=0",
        "maintenance.auto=false",
    ] {
        command.arguments.extend(["-c".into(), setting.into()]);
    }
    command
        .environment
        .insert("GIT_NO_REPLACE_OBJECTS".into(), "1".into());
    let remote_ref = format!("refs/heads/{}", request.target.branch);
    let expected = match &request.target.expected_remote_oid_or_unborn {
        RemoteState::Commit { oid } => oid.as_str(),
        RemoteState::Unborn => "",
    };
    command.arguments.extend([
        format!("--git-dir={git_directory}"),
        "push".into(),
        "--porcelain".into(),
        "--no-progress".into(),
        "--no-verify".into(),
        "--no-follow-tags".into(),
        "--signed=false".into(),
        "--recurse-submodules=no".into(),
        format!("--force-with-lease={remote_ref}:{expected}"),
        "--".into(),
        remote.into(),
        request.target.refspec.clone(),
    ]);
    Ok(command)
}

fn response(
    captured: &process::Captured,
    remote: &str,
    refspec: &str,
) -> Result<DispatchStatus, DispatchFailure> {
    let text = std::str::from_utf8(&captured.stdout).map_err(|_| DispatchFailure::Response)?;
    let mut lines = text
        .strip_suffix('\n')
        .ok_or(DispatchFailure::Response)?
        .split('\n');
    if lines.next() != Some(format!("To {remote}").as_str()) {
        return Err(DispatchFailure::Response);
    }
    let mut fields = lines.next().ok_or(DispatchFailure::Response)?.split('\t');
    let flag = fields.next().ok_or(DispatchFailure::Response)?;
    if fields.next() != Some(refspec)
        || fields
            .next()
            .is_none_or(|summary| summary.is_empty() || summary.chars().any(char::is_control))
        || fields.next().is_some()
        || lines.next() != Some("Done")
        || lines.next().is_some()
    {
        return Err(DispatchFailure::Response);
    }
    match (flag, captured.exit.code) {
        (" " | "*", Some(0)) => Ok(DispatchStatus::Updated),
        ("=", Some(0)) => Ok(DispatchStatus::AlreadyPresent),
        ("!", Some(1)) => Ok(DispatchStatus::Rejected),
        _ => Err(DispatchFailure::Response),
    }
}

fn uncertain(failure: DispatchFailure, cleanup: Cleanup) -> DispatchError {
    DispatchError {
        failure,
        effect: DispatchEffect::Unverified,
        cleanup,
    }
}
