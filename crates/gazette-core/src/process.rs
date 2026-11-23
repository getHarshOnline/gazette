//! Explicit native command selection and observations, never a sandbox or policy verdict.

use std::{collections::BTreeMap, fmt, io, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

mod segmented;
pub use segmented::{SEGMENTED_INPUT_BYTES, SegmentedCaptureRequest};

/// Independently selected executable bytes, not an interpreter/import or authority attestation.
/// Native runners verify the regular file before spawn and again before returning success.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutablePin {
    /// Exact nonzero file length, at most 256 MiB; independent of stdin/output limits.
    #[schemars(range(min = 1, max = 268435456))]
    pub bytes: u64,
    /// SHA-256 of the complete selected executable file.
    pub sha256: crate::source::Sha256Digest,
}

/// One absolute native executable whose complete bytes are mandatory before and after use.
///
/// This selects a program, not its arguments, environment, working directory, libraries,
/// plugins or other ambient runtime inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PinnedExecutable {
    /// Absolute executable path. No `PATH` lookup or shell expansion is permitted.
    pub program: String,
    /// Exact selected executable bytes.
    pub executable: ExecutablePin,
}

impl PinnedExecutable {
    /// Validate the path spelling and executable pin without native access.
    ///
    /// # Errors
    /// Rejects relative/NUL-bearing paths and invalid executable byte bounds.
    pub fn validate(&self) -> Result<(), Error> {
        validate_program(&self.program, Some(self.executable))
    }
}

/// A selected executable, without shell expansion, implicit PATH lookup or inherited environment.
/// Paths are native operational coordinates, not portable artifact identity or executable attestation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Absolute executable path. Gazette does not interpret command strings.
    pub program: String,
    /// Optional explicit byte pin; omission selects ordinary unpinned native execution.
    /// Pinning an interpreter does not pin scripts, imports, libraries or external inputs.
    #[serde(default)]
    pub executable: Option<ExecutablePin>,
    /// Literal ordered argument vector; shell metacharacters remain ordinary bytes.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Explicit absolute working directory; this does not confine subprocess effects.
    pub directory: String,
    /// Complete non-secret environment. Omission means empty, not process inheritance.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl Command {
    /// Validate native spelling and fixed command ceilings without accessing the filesystem.
    /// At most 256 arguments, 64 environment entries and 64 KiB of command text are supported.
    ///
    /// # Errors
    /// Rejects relative paths, NULs, invalid environment names and excessive command text.
    pub fn validate(&self) -> Result<(), Error> {
        validate_program(&self.program, self.executable)?;
        if !Path::new(&self.directory).is_absolute() {
            return Err(Error::before_start(Failure::Command));
        }
        self.validate_text()
    }

    /// Check pins, argument/environment shape and text ceilings before paths are bound.
    /// This does not establish native path validity; execution must use `validate`.
    ///
    /// # Errors
    /// Rejects invalid pins, NULs, environment names and excessive command text.
    pub fn validate_text(&self) -> Result<(), Error> {
        if self
            .executable
            .is_some_and(|pin| pin.bytes == 0 || pin.bytes > 256 << 20)
        {
            return Err(Error::before_start(Failure::Limit));
        }
        if self.arguments.len() > 256 || self.environment.len() > 64 {
            return Err(Error::before_start(Failure::Limit));
        }
        for name in self.environment.keys() {
            let mut bytes = name.bytes();
            if !bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::before_start(Failure::Command));
            }
        }
        let mut total = 0usize;
        for value in [&self.program, &self.directory]
            .into_iter()
            .chain(&self.arguments)
            .chain(
                self.environment
                    .iter()
                    .flat_map(|(key, value)| [key, value]),
            )
        {
            if value.contains('\0') {
                return Err(Error::before_start(Failure::Command));
            }
            total = total
                .checked_add(value.len())
                .filter(|&bytes| bytes <= 64 << 10)
                .ok_or_else(|| Error::before_start(Failure::Limit))?;
        }
        Ok(())
    }
}

fn validate_program(program: &str, executable: Option<ExecutablePin>) -> Result<(), Error> {
    if executable.is_some_and(|pin| pin.bytes == 0 || pin.bytes > 256 << 20) {
        return Err(Error::before_start(Failure::Limit));
    }
    if !Path::new(program).is_absolute() || program.contains('\0') {
        return Err(Error::before_start(Failure::Command));
    }
    Ok(())
}

/// A native command whose caller fixes arguments and environment while the adapter owns a fresh
/// disposable working directory. The directory is absent from artifact identity by construction.
///
/// This narrows working-state access but is not an operating-system sandbox: the selected program
/// still has its process authority and ambient dynamic-library/runtime dependencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IsolatedCommand {
    /// Exact executable selection.
    #[serde(flatten)]
    pub executable: PinnedExecutable,
    /// Literal caller-owned argument vector.
    pub arguments: Vec<String>,
    /// Complete caller-owned environment. Empty means empty, not inherited.
    pub environment: BTreeMap<String, String>,
}

impl IsolatedCommand {
    /// Validate the selected program and fixed command ceilings without native access.
    ///
    /// # Errors
    /// Rejects invalid pins, paths, environment names, NULs or excessive command text.
    pub fn validate(&self) -> Result<(), Error> {
        self.as_command("/").validate()
    }

    /// Bind this exact command to the adapter-owned absolute disposable directory.
    pub fn as_command(&self, directory: &str) -> Command {
        Command {
            program: self.executable.program.clone(),
            executable: Some(self.executable.executable),
            arguments: self.arguments.clone(),
            directory: directory.to_owned(),
            environment: self.environment.clone(),
        }
    }
}

/// Effect acknowledgement and bounds for one invocation, not executable trust or containment.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Explicit consent to execute the selected command with its native authority.
    pub allow_execute: bool,
    /// Complete stdin ceiling, checked before spawning; no prefix is accepted as complete input.
    pub max_input_bytes: usize,
    /// One deadline shared by input delivery, selected output capture and child execution after spawning.
    /// Native process creation itself is not promised to be interruptible.
    pub timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            allow_execute: false,
            max_input_bytes: 16 << 20,
            timeout: Duration::from_secs(30),
        }
    }
}

impl Options {
    /// Refuse unacknowledged or unbounded execution before a process exists.
    ///
    /// # Errors
    /// Rejects permission, input overflow, ceilings above 64 MiB or deadlines outside 1 ms–5 min.
    pub fn validate(self, command: &Command, input: &[u8]) -> Result<(), Error> {
        if !self.allow_execute {
            return Err(Error::before_start(Failure::Permission));
        }
        command.validate()?;
        if self.max_input_bytes > 64 << 20
            || input.len() > self.max_input_bytes
            || self.timeout < Duration::from_millis(1)
            || self.timeout > Duration::from_secs(300)
        {
            return Err(Error::before_start(Failure::Limit));
        }
        Ok(())
    }
}

/// Borrowed complete input; a provider must not reopen native publication sources.
pub struct Request<'a> {
    /// Workspace-selected command; no provider substitution is allowed.
    pub command: &'a Command,
    /// Exact finite bytes to deliver on stdin, followed by EOF.
    pub input: &'a [u8],
    /// Per-call effects and bounds.
    pub options: Options,
}

/// Explicit bounded output capture; no truncated prefix is a complete result.
pub struct CaptureRequest<'a> {
    /// Existing exact command/input/deadline contract.
    pub request: Request<'a>,
    /// Maximum complete stdout bytes, at most 64 MiB. Zero permits only empty output.
    pub max_output_bytes: usize,
    /// Optional complete stderr ceiling, at most 64 MiB. None discards stderr; zero requires EOF
    /// without diagnostics. Capturing diagnostics does not interpret them as acceptance.
    pub max_stderr_bytes: Option<usize>,
}

impl CaptureRequest<'_> {
    /// Validate all execution and capture bounds before native process creation.
    ///
    /// # Errors
    /// Refuses the underlying request or an output ceiling above 64 MiB.
    pub fn validate(&self) -> Result<(), Error> {
        self.request
            .options
            .validate(self.request.command, self.request.input)?;
        if self.max_output_bytes > 64 << 20
            || self.max_stderr_bytes.is_some_and(|bytes| bytes > 64 << 20)
        {
            return Err(Error::before_start(Failure::OutputLimit));
        }
        Ok(())
    }
}

/// Borrowed complete input for a command executed from an adapter-owned fresh directory.
pub struct IsolatedRequest<'a> {
    /// Exact pinned command without a caller-selected working directory.
    pub command: &'a IsolatedCommand,
    /// Exact finite bytes delivered on stdin.
    pub input: &'a [u8],
    /// Per-call effects and bounds.
    pub options: Options,
}

/// Explicit bounded capture whose working directory is fresh, verified empty and removed.
pub struct IsolatedCaptureRequest<'a> {
    /// Existing effect, input and command contract.
    pub request: IsolatedRequest<'a>,
    /// Maximum complete stdout bytes, at most 64 MiB.
    pub max_output_bytes: usize,
    /// Optional complete stderr ceiling, at most 64 MiB.
    pub max_stderr_bytes: Option<usize>,
}

impl IsolatedCaptureRequest<'_> {
    /// Validate all bounds before the adapter creates a disposable directory.
    ///
    /// # Errors
    /// Refuses permission, malformed command/input bounds or oversized capture ceilings.
    pub fn validate(&self) -> Result<(), Error> {
        if !self.request.options.allow_execute {
            return Err(Error::before_start(Failure::Permission));
        }
        self.request.command.validate()?;
        if self.request.options.max_input_bytes > 64 << 20
            || self.request.input.len() > self.request.options.max_input_bytes
            || self.request.options.timeout < Duration::from_millis(1)
            || self.request.options.timeout > Duration::from_secs(300)
        {
            return Err(Error::before_start(Failure::Limit));
        }
        if self.max_output_bytes > 64 << 20
            || self.max_stderr_bytes.is_some_and(|bytes| bytes > 64 << 20)
        {
            return Err(Error::before_start(Failure::OutputLimit));
        }
        Ok(())
    }
}

/// Complete selected streams and reaped direct-child exit, not interpretation or effect acceptance.
#[derive(Debug)]
pub struct Captured {
    /// Native status after full input delivery and stdout EOF.
    pub exit: Exit,
    /// Exact bounded stdout bytes, including non-UTF-8 bytes; never a truncated prefix.
    pub stdout: Vec<u8>,
    /// Complete diagnostics when requested, including an empty stream; None means discarded.
    pub stderr: Option<Vec<u8>>,
}

/// Native exit observation after complete stdin delivery; not proof of input interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Exit {
    /// Native exit code, or None when terminated without a normal code.
    pub code: Option<i32>,
}

/// Direct-child custody on a failed invocation. None of these establishes absence of side effects.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Cleanup {
    /// Spawning was not attempted or did not produce a child.
    NotStarted,
    /// The direct child was reaped; descendants and other external effects are not certified.
    Reaped,
    /// Direct-child termination/reaping could not be established.
    Uncertain,
}

/// Stable redacted failure classification; subprocess output is never a policy result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Failure {
    /// Execution was not explicitly permitted.
    Permission,
    /// Invalid command spelling or environment.
    Command,
    /// Unsupported work or input bound.
    Limit,
    /// The host cannot provide the required bounded transport.
    Unsupported,
    /// Selected executable could not be verified before spawn; no child was started.
    Executable,
    /// Executable binding changed or could not be reverified after direct-child completion.
    ExecutableChanged,
    /// Native process creation failed.
    Spawn,
    /// Complete input could not be delivered.
    Input,
    /// A selected output stream could not be completely read.
    Output,
    /// A selected output stream or its configured ceiling exceeds the allowed bound.
    OutputLimit,
    /// Child status could not be inspected.
    Wait,
    /// The shared post-spawn deadline expired.
    Timeout,
}

/// Failed invocation with explicit direct-child custody; native details remain private.
#[derive(Debug)]
pub struct Error {
    /// Stable phase/failure classification.
    pub failure: Failure,
    /// Known direct-child custody, never automatic permission to retry.
    pub cleanup: Cleanup,
    /// Original native error, if one exists.
    pub source: Option<io::Error>,
}

impl Error {
    /// Stable diagnostic class without command, environment or subprocess output disclosure.
    pub fn code(&self) -> &'static str {
        match self.failure {
            Failure::Permission => "process_permission",
            Failure::Command => "process_command",
            Failure::Limit => "process_limit",
            Failure::Unsupported => "process_unsupported",
            Failure::Executable => "process_executable",
            Failure::ExecutableChanged => "process_executable_changed",
            Failure::Spawn => "process_spawn",
            Failure::Input => "process_input",
            Failure::Output => "process_output",
            Failure::OutputLimit => "process_output_limit",
            Failure::Wait => "process_wait",
            Failure::Timeout => "process_timeout",
        }
    }

    /// Construct a refusal before a native child exists.
    pub fn before_start(failure: Failure) -> Self {
        Self {
            failure,
            cleanup: Cleanup::NotStarted,
            source: None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "command {:?}; direct-child custody {:?}",
            self.failure, self.cleanup
        )
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|error| error as _)
    }
}
