//! Gets the current evidence that a release's authority still holds, so a release can
//! be checked against fresh observations.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. The selected command owns how it observes current
//! independent authority, key and revocation frontiers, emergency stops and time. This
//! adapter captures each complete observation and delegates strict decoding through a
//! port; it cannot establish that a provider is trustworthy or current. Selection must
//! therefore be independent of the received authorization. No binding is cached. It
//! runs outside deterministic compilation and does not approve a release.

use gazette_core::{
    process::{CaptureRequest, Cleanup, Exit, Failure, Request},
    release::Binding,
};
use gazette_ports::{
    process::CaptureRunner,
    release::{AuthorityBindingDecoder, AuthorityError, AuthoritySource},
};

/// Redacted evidence from the most recent acquisition, not publisher approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Observation {
    /// No complete successful capture was obtained. Custody covers the direct child only.
    ExecutionFailed {
        /// Stable transport failure without command, environment or output disclosure.
        failure: Failure,
        /// Known direct-child custody; never proof of no external effects.
        cleanup: Cleanup,
    },
    /// Full capture completed, but the provider did not report successful acquisition.
    NonzeroExit {
        /// Reaped direct-child exit; a missing code is not success.
        exit: Exit,
        /// Complete captured stdout size, without its possibly private bytes.
        #[schemars(range(max = usize::MAX))]
        stdout_bytes: usize,
    },
    /// The provider exited successfully, but its complete output was not a strict binding.
    InvalidBinding {
        /// Existing strict release-parser diagnostic code.
        code: &'static str,
        /// Complete captured stdout size; output contents are not retained.
        #[schemars(range(max = usize::MAX))]
        stdout_bytes: usize,
    },
    /// Complete successful output decoded as the native binding shape, not admitted authority.
    Acquired {
        /// Complete captured stdout size; no binding or output is retained in the source.
        #[schemars(range(max = usize::MAX))]
        stdout_bytes: usize,
    },
}

/// Independently selected command/input/bounds invoked anew on every authority acquisition.
///
/// This is not a sandbox. Command permission allows its native effects; finite stdin is
/// only a selection/request and must not be used to manufacture authority from an untrusted
/// authorization. A native executable pin does not attest scripts or other provider inputs.
pub struct CommandSource<'a, R: ?Sized, D: ?Sized> {
    runner: &'a R,
    request: CaptureRequest<'a>,
    decoder: &'a D,
    last: Option<Observation>,
}

impl<'a, R: CaptureRunner + ?Sized, D: AuthorityBindingDecoder + ?Sized> CommandSource<'a, R, D> {
    /// Select a provider without executing it. Every acquisition separately checks permissions
    /// and bounds, invokes the runner, and decodes complete stdout with the shared parser.
    pub fn new(runner: &'a R, request: CaptureRequest<'a>, decoder: &'a D) -> Self {
        Self {
            runner,
            request,
            decoder,
            last: None,
        }
    }

    /// Most recent call's redacted transport/parse evidence, replaced even on refusal.
    /// None means this source has not attempted acquisition; no previous binding is available.
    pub fn last_observation(&self) -> Option<&Observation> {
        self.last.as_ref()
    }
}

impl<R: CaptureRunner + ?Sized, D: AuthorityBindingDecoder + ?Sized> AuthoritySource
    for CommandSource<'_, R, D>
{
    fn acquire_authority(&mut self) -> Result<Binding, AuthorityError> {
        self.last = None;
        let request = CaptureRequest {
            request: Request {
                command: self.request.request.command,
                input: self.request.request.input,
                options: self.request.request.options,
            },
            max_output_bytes: self.request.max_output_bytes,
            max_stderr_bytes: self.request.max_stderr_bytes,
        };
        // Refuse before even calling an injected runner; the adapter owns this permission edge.
        let captured = request
            .validate()
            .and_then(|()| self.runner.capture(request));
        let captured = match captured {
            Ok(captured) => captured,
            Err(error) => {
                self.last = Some(Observation::ExecutionFailed {
                    failure: error.failure,
                    cleanup: error.cleanup,
                });
                return Err(AuthorityError::Unavailable);
            }
        };
        let stdout_bytes = captured.stdout.len();
        if stdout_bytes > self.request.max_output_bytes {
            self.last = Some(Observation::ExecutionFailed {
                failure: Failure::OutputLimit,
                cleanup: Cleanup::Reaped,
            });
            return Err(AuthorityError::Unavailable);
        }
        if captured.exit.code != Some(0) {
            self.last = Some(Observation::NonzeroExit {
                exit: captured.exit,
                stdout_bytes,
            });
            return Err(AuthorityError::Unavailable);
        }
        match self.decoder.decode_binding(&captured.stdout) {
            Ok(binding) => {
                self.last = Some(Observation::Acquired { stdout_bytes });
                Ok(binding)
            }
            Err(code) => {
                self.last = Some(Observation::InvalidBinding { code, stdout_bytes });
                Err(AuthorityError::Invalid)
            }
        }
    }
}
