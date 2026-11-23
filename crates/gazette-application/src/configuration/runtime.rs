//! Bind command coordinates once from the same captured configuration as source roots.

use std::{collections::BTreeMap, fmt, path::Path};

use gazette_compiler::workspace::{RuntimeArgument, RuntimeCommand, Workspace};
use gazette_core::process::Command;

use super::{SourceRootUnavailable, resolve_root};

/// A selected command cannot be turned into one complete native command.
#[derive(Clone, Debug)]
pub enum RuntimeBindingError {
    /// A selected path needs a workspace or machine root that was not captured.
    Root {
        /// Command part that needs the binding.
        field: &'static str,
        /// Missing explicit root.
        source: SourceRootUnavailable,
    },
    /// A command path is malformed or its native value is not UTF-8.
    Path(&'static str),
    /// The expanded command failed native path, pin, environment or size validation.
    Command(&'static str),
}

impl RuntimeBindingError {
    /// Stable refusal before command comparison, capture or execution.
    pub fn code(&self) -> &'static str {
        "runtime_command_binding"
    }
}

impl fmt::Display for RuntimeBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root { field, source } => write!(f, "runtime command {field}: {source}"),
            Self::Path(field) => write!(f, "runtime command {field} requires a valid UTF-8 path"),
            Self::Command(code) => write!(f, "bound runtime command failed validation: {code}"),
        }
    }
}

impl std::error::Error for RuntimeBindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Root { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) struct RuntimeCommands {
    pub math: Option<Result<Command, RuntimeBindingError>>,
    pub policy: Option<Result<Command, RuntimeBindingError>>,
}

impl RuntimeCommands {
    pub(super) fn bind(
        workspace: &Workspace,
        native_root: Option<&Path>,
        roots: &BTreeMap<String, std::path::PathBuf>,
    ) -> Self {
        let bind = |selection: &RuntimeCommand| {
            let path = |value: &str, field| bind_path(native_root, roots, value, field);
            let command = Command {
                program: path(&selection.program, "program")?,
                executable: selection.executable,
                arguments: selection
                    .arguments
                    .iter()
                    .map(|argument| match argument {
                        RuntimeArgument::Literal(value) => Ok(value.clone()),
                        RuntimeArgument::Path { path: value } => path(value, "argument"),
                    })
                    .collect::<Result<_, _>>()?,
                directory: path(&selection.directory, "directory")?,
                environment: selection.environment.clone(),
            };
            command
                .validate()
                .map_err(|error| RuntimeBindingError::Command(error.code()))?;
            Ok(command)
        };
        Self {
            math: workspace
                .math_typesetter()
                .map(|typesetter| bind(&typesetter.command)),
            policy: workspace.policy_check().map(bind),
        }
    }
}

/// Resolve explicit runtime paths without reading native files or the process environment.
pub(super) fn bind_path(
    native_root: Option<&Path>,
    roots: &BTreeMap<String, std::path::PathBuf>,
    value: &str,
    field: &'static str,
) -> Result<String, RuntimeBindingError> {
    let coordinate =
        RuntimeCommand::coordinate(value).map_err(|_| RuntimeBindingError::Path(field))?;
    resolve_root(native_root, roots, &coordinate)
        .map_err(|source| RuntimeBindingError::Root { field, source })?
        .into_os_string()
        .into_string()
        .map_err(|_| RuntimeBindingError::Path(field))
}
