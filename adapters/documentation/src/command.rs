//! Lower typed file/root arguments once; native process spelling has one existing owner.

use gazette_core::{
    documentation::generator::{Argument, Cause, Request},
    process::Command,
};
use std::path::Path;

pub(super) struct Commands {
    pub version: Command,
    pub generation: Command,
    pub validation: Option<Command>,
}

pub(super) fn commands(request: &Request<'_>) -> Result<Commands, Cause> {
    request.recipe.validate().map_err(Cause::Process)?;
    let directory = text(request.input.path())?;
    let output = request.recipe.output.path().map_or_else(
        || request.output.path().to_path_buf(),
        |path| request.output.path().join(path.as_str()),
    );
    let lower = |arguments: &[Argument]| {
        arguments
            .iter()
            .map(|argument| match argument {
                Argument::Literal { value } => Ok(value.clone()),
                Argument::Path { path } => text(Path::new(path)),
                Argument::Input { path } => text(&request.input.path().join(path.as_str())),
                Argument::InputRoot => Ok(".".to_owned()),
                Argument::Output => text(&output),
                Argument::OutputRoot => text(request.output.path()),
            })
            .collect::<Result<_, _>>()
    };
    let generation = Command {
        program: request.recipe.program.clone(),
        executable: Some(request.recipe.executable),
        arguments: lower(&request.recipe.arguments)?,
        directory,
        environment: request.recipe.environment.clone(),
    };
    let version = Command {
        arguments: request.recipe.version_arguments.clone(),
        ..generation.clone()
    };
    let validation = request
        .recipe
        .validation_arguments
        .as_ref()
        .map(|arguments| {
            Ok(Command {
                arguments: lower(arguments)?,
                ..generation.clone()
            })
        })
        .transpose()?;
    for command in [&version, &generation]
        .into_iter()
        .chain(validation.as_ref())
    {
        gazette_core::process::CaptureRequest {
            request: gazette_core::process::Request {
                command,
                input: &[],
                options: request.execution,
            },
            max_output_bytes: request.max_capture_bytes,
            max_stderr_bytes: None,
        }
        .validate()
        .map_err(Cause::Process)?;
    }
    Ok(Commands {
        version,
        generation,
        validation,
    })
}

fn text(path: &Path) -> Result<String, Cause> {
    if !path.is_absolute() {
        return Err(Cause::Invalid("documentation_generator_path"));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(Cause::Invalid("documentation_generator_path"))
}
