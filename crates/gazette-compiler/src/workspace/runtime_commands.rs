//! Portable command declarations, resolved by native configuration acquisition.

use std::{collections::BTreeMap, path::Path};

use gazette_core::process::{self, Command, ExecutablePin};
use serde::{Deserialize, Serialize};

use super::SourceRoot;

/// One argument is either literal text or an explicitly selected native path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum RuntimeArgument {
    /// Passed verbatim, including machine-looking strings and shell metacharacters.
    Literal(String),
    /// Uses the same workspace and machine coordinates as program and directory.
    Path {
        /// Absolute, workspace-relative or `machine:name/path` coordinate.
        path: String,
    },
}

impl From<String> for RuntimeArgument {
    fn from(value: String) -> Self {
        Self::Literal(value)
    }
}

impl From<&str> for RuntimeArgument {
    fn from(value: &str) -> Self {
        Self::Literal(value.to_owned())
    }
}

/// A command selected by the workspace, before its machine coordinates are bound.
/// Native execution always receives a separate validated absolute `process::Command`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommand {
    /// Absolute, workspace-relative or machine-root executable coordinate; no PATH search.
    pub program: String,
    /// Optional executable bytes; interpreted scripts and imports remain separate inputs.
    #[serde(default)]
    pub executable: Option<ExecutablePin>,
    /// Ordered literal text and explicitly typed path arguments.
    #[serde(default)]
    pub arguments: Vec<RuntimeArgument>,
    /// Explicit working directory, using the same coordinates as the program.
    pub directory: String,
    /// Complete literal environment; values are never interpolated or inherited.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl RuntimeCommand {
    /// Validate portable coordinates and the shared native command ceilings without I/O.
    ///
    /// # Errors
    /// Rejects malformed coordinates, invalid pins, text or environment and excessive input.
    pub fn validate(&self) -> Result<(), process::Error> {
        Self::coordinate(&self.program)?;
        Self::coordinate(&self.directory)?;
        let arguments = self
            .arguments
            .iter()
            .map(|argument| match argument {
                RuntimeArgument::Literal(value) => Ok(value.clone()),
                RuntimeArgument::Path { path } => {
                    Self::coordinate(path)?;
                    Ok(path.clone())
                }
            })
            .collect::<Result<Vec<_>, process::Error>>()?;
        Command {
            program: self.program.clone(),
            executable: self.executable,
            arguments,
            directory: self.directory.clone(),
            environment: self.environment.clone(),
        }
        .validate_text()
    }

    /// Interpret a path coordinate without opening it or consulting process state.
    /// Existing absolute native spellings, including `/`, pass through unchanged.
    ///
    /// # Errors
    /// Rejects NULs and malformed or escaping relative/machine coordinates.
    pub fn coordinate(value: &str) -> Result<SourceRoot, process::Error> {
        if Path::new(value).is_absolute() && !value.contains('\0') {
            return Ok(SourceRoot::External(value.to_owned()));
        }
        value
            .parse()
            .map_err(|_| process::Error::before_start(process::Failure::Command))
    }
}

/// Workspace-selected output framing; candidate checks always keep their JSON protocol.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyOutputTransport {
    /// The existing complete Base64 JSON envelope, bounded by native input limits.
    #[default]
    InlineV1,
    /// Bounded JSON metadata followed by every retained file in inventory order.
    FramedV2,
}
