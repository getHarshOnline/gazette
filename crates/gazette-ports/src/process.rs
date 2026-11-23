//! Effectful native execution, outside deterministic compilation and policy meaning.

use gazette_core::process::{
    CaptureRequest, Captured, Error, Exit, IsolatedCaptureRequest, Request,
};
use gazette_core::source::{EntryPath, TreeInput};

/// Deliver exact stdin to an explicitly selected command and observe its exit only.
/// Implementations must clear inherited environment and discard stdout/stderr, not interpret
/// either as authority. This capability is not a sandbox, executable attestation or policy owner.
pub trait Runner {
    /// Validate permission/bounds before native reads or spawning. Honor an explicit executable
    /// pin before spawn and reverify it before returning success; never fall back to unpinned
    /// execution. Paired native observations require trusted namespace control, not a sandbox.
    /// Bound input delivery plus child execution.
    ///
    /// # Errors
    /// Refuses invalid requests or unsupported hosts, and preserves native failure/custody evidence.
    fn run(&self, request: Request<'_>) -> Result<Exit, Error>;
}

/// Execute the same explicit command contract while retaining bounded selected output streams.
pub trait CaptureRunner {
    /// Drain stdout and optionally stderr concurrently with input delivery, and require full input,
    /// every selected stream's EOF and reaped direct-child status within the shared deadline.
    /// A descendant retaining the pipe cannot turn an incomplete capture into success.
    /// The same selected executable pin as `Runner` applies; a failed postcheck returns no output.
    ///
    /// # Errors
    /// Refuses invalid requests, output overflow/read failure and native execution failures.
    /// No captured prefix escapes an error; cleanup describes only direct-child custody.
    fn capture(&self, request: CaptureRequest<'_>) -> Result<Captured, Error>;

    /// Deliver an explicitly selected finite segmented input under the same supervision.
    /// Existing embedders refuse until they implement complete ordered delivery and EOF.
    ///
    /// # Errors
    /// Refuses unsupported segmented transport; implementations preserve ordinary capture
    /// permission, executable-pin, deadline, stream-EOF and direct-child cleanup guarantees.
    fn capture_segmented(
        &self,
        request: gazette_core::process::SegmentedCaptureRequest<'_>,
    ) -> Result<Captured, Error> {
        request.validate()?;
        Err(Error::before_start(
            gazette_core::process::Failure::Unsupported,
        ))
    }
}

/// Execute a fixed native command from a newly created empty working directory.
pub trait IsolatedCaptureRunner {
    /// Create a private empty directory, bind it as the working directory, execute exactly once,
    /// verify that no entry was created there, and remove it before returning success.
    ///
    /// This is working-directory confinement, not a process sandbox. Callers must separately
    /// constrain the input language so it cannot name external resources or effects.
    ///
    /// # Errors
    /// Refuses invalid requests, any working-directory mutation, execution failure or cleanup
    /// uncertainty. No captured prefix escapes an error.
    fn capture_isolated(&self, request: IsolatedCaptureRequest<'_>) -> Result<Captured, Error>;
}

/// Execute an interpreter only against one exact retained resource tree.
pub trait RetainedCaptureRunner: CaptureRunner {
    /// Materialize the supplied immutable tree privately, prepend its exact entrypoint to the
    /// selected command arguments, execute once, verify the materialized tree did not change,
    /// and remove it before returning success.
    ///
    /// # Errors
    /// Refuses an absent entrypoint, staging/read-back mismatch, execution failure or cleanup
    /// uncertainty. The original native resource path is never reopened.
    fn capture_retained(
        &self,
        request: CaptureRequest<'_>,
        resources: TreeInput<'_>,
        entrypoint: &EntryPath,
    ) -> Result<Captured, Error>;
}

/// One adapter that can run both retained-resource and fresh-directory acquisition effects.
/// This is a composition convenience for workflows that can select either operation.
pub trait GenerationRunner: RetainedCaptureRunner + IsolatedCaptureRunner {}

impl<T> GenerationRunner for T where T: RetainedCaptureRunner + IsolatedCaptureRunner + ?Sized {}
