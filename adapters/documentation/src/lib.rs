//! Runs native documentation generators over retained input and keeps their output as
//! explicit artifacts.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it, and `gazette-adapter-mintlify` assembles what it
//! generates. It generates; it is not a hosting converter. Work directories remain
//! retained on every result. Cleared environment, executable pins and observed input
//! equality are not a sandbox or proof that a tool ignored other native inputs.

mod command;

use gazette_core::{
    documentation::generator::{
        Argument, Cause, Error, Evidence, Generated, Invocation, Phase, Request, Validation,
    },
    process::{self, CaptureRequest, Captured, Command},
    source::{CapturedSource, Sha256Digest},
};
use gazette_ports::{
    destination::RootSeparation, documentation::Generator, process::CaptureRunner,
    source::SourceCapture,
};

/// Thin native adapter; application composition supplies retained work directories and providers.
pub struct NativeGenerator<'a, F: ?Sized, R: ?Sized> {
    files: &'a F,
    runner: &'a R,
}

impl<'a, F: SourceCapture + RootSeparation + ?Sized, R: CaptureRunner + ?Sized>
    NativeGenerator<'a, F, R>
{
    /// Bind existing providers without inspecting files or executing commands.
    pub fn new(files: &'a F, runner: &'a R) -> Self {
        Self { files, runner }
    }

    fn input(&self, request: &Request<'_>) -> Result<CapturedSource, Cause> {
        let input = self
            .files
            .capture(request.input.path(), None, request.input_limits)
            .map_err(|error| Cause::Source(Box::new(error)))?;
        if input.inventory() != request.input.inventory() {
            return Err(Cause::Invalid("documentation_generator_input_changed"));
        }
        Ok(input)
    }

    fn output_empty(&self, request: &Request<'_>) -> Result<(), Cause> {
        let output = self
            .files
            .capture(request.output.path(), None, request.output_limits)
            .map_err(|error| Cause::Source(Box::new(error)))?;
        if !request.output.inventory().entries().is_empty()
            || output.inventory() != request.output.inventory()
        {
            return Err(Cause::Invalid("documentation_generator_output_not_empty"));
        }
        Ok(())
    }

    fn invoke(
        &self,
        request: &Request<'_>,
        command: &Command,
        record: &mut Option<Invocation>,
    ) -> Result<Captured, Cause> {
        let command_sha256 =
            Sha256Digest::of(&serde_json::to_vec(command).map_err(Cause::Encoding)?);
        *record = Some(Invocation {
            command_sha256,
            exit: None,
            stdout_sha256: None,
            stdout_bytes: None,
        });
        let captured = self
            .runner
            .capture(CaptureRequest {
                request: process::Request {
                    command,
                    input: &[],
                    options: request.execution,
                },
                max_output_bytes: request.max_capture_bytes,
                max_stderr_bytes: None,
            })
            .map_err(Cause::Process)?;
        // An injected provider cannot bypass the consumer's complete-output bound.
        if captured.stdout.len() > request.max_capture_bytes {
            return Err(Cause::Invalid("documentation_generator_capture_limit"));
        }
        *record = Some(Invocation {
            command_sha256,
            exit: Some(captured.exit),
            stdout_sha256: Some(Sha256Digest::of(&captured.stdout)),
            stdout_bytes: Some(captured.stdout.len()),
        });
        if captured.exit.code != Some(0) {
            return Err(Cause::Invalid("documentation_generator_exit"));
        }
        Ok(captured)
    }
}

impl<F: SourceCapture + RootSeparation + ?Sized, R: CaptureRunner + ?Sized> Generator
    for NativeGenerator<'_, F, R>
{
    fn generate(&self, request: Request<'_>) -> Result<Generated, Error> {
        let mut evidence = Evidence {
            input: request.input.inventory().identity(),
            version: None,
            expected_version_sha256: request.recipe.expected_version_sha256,
            version_text: None,
            generation: None,
            validation: None,
        };
        let mut phase = Phase::Preflight;
        let result = (|| {
            // Native permission and command bounds precede any injected filesystem/provider call.
            let commands = command::commands(&request)?;
            self.files
                .check_root_separation(request.output.path(), &[request.input.path().to_path_buf()])
                .map_err(|error| Cause::Separation(Box::new(error)))?;
            let input = self.input(&request)?;
            self.output_empty(&request)?;
            for argument in request
                .recipe
                .arguments
                .iter()
                .chain(request.recipe.validation_arguments.iter().flatten())
            {
                if let Argument::Input { path } = argument
                    && !input
                        .inventory()
                        .entries()
                        .iter()
                        .any(|entry| &entry.path == path)
                {
                    return Err(Cause::Invalid("documentation_generator_input_missing"));
                }
            }
            phase = Phase::Version;
            let observed = self.invoke(&request, &commands.version, &mut evidence.version)?;
            let text = String::from_utf8(observed.stdout)
                .map_err(|_| Cause::Invalid("documentation_generator_version"))?;
            if text.trim().is_empty() {
                return Err(Cause::Invalid("documentation_generator_version"));
            }
            evidence.version_text = Some(text);
            if evidence
                .version
                .as_ref()
                .and_then(|invocation| invocation.stdout_sha256)
                != Some(request.recipe.expected_version_sha256)
            {
                return Err(Cause::Invalid("documentation_generator_version_identity"));
            }
            self.input(&request)?;
            self.output_empty(&request)?;
            phase = Phase::Generation;
            self.invoke(&request, &commands.generation, &mut evidence.generation)?;
            phase = Phase::Capture;
            self.input(&request)?;
            let output = self
                .files
                .capture(request.output.path(), None, request.output_limits)
                .map_err(|error| Cause::Source(Box::new(error)))?;
            request
                .recipe
                .output
                .content(&output.as_input())
                .map_err(|_| Cause::Invalid("documentation_generator_output_selection"))?;
            // Input replacement during output capture also invalidates this job.
            self.input(&request)?;
            if let Some(command) = &commands.validation {
                phase = Phase::Validation;
                let mut invocation = None;
                let result = self.invoke(&request, command, &mut invocation);
                evidence.validation = invocation.map(|invocation| Validation {
                    tree: output.inventory().identity(),
                    invocation,
                });
                result?;
                self.input(&request)?;
                let checked = self
                    .files
                    .capture(request.output.path(), None, request.output_limits)
                    .map_err(|error| Cause::Source(Box::new(error)))?;
                if checked.inventory() != output.inventory() {
                    return Err(Cause::Invalid("documentation_generator_output_changed"));
                }
                self.input(&request)?;
            }
            Ok(output)
        })();
        match result {
            Ok(output) => Ok(Generated {
                output,
                selection: request.recipe.output.clone(),
                format: request.recipe.format,
                evidence,
            }),
            Err(cause) => Err(Error {
                phase,
                cause,
                input: request.input.path().to_path_buf(),
                output: request.output.path().to_path_buf(),
                evidence: Box::new(evidence),
            }),
        }
    }
}
