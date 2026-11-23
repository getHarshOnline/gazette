//! Shared supplied/native workspace selection for read-only inspection phases.

use super::{AcquiredInputs, CompositionError, WorkspaceInput};

/// Backward-compatible detached TOML or an explicitly selected native workspace.
/// A string is always source text; it is never interpreted as a filename.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum WorkspaceSource {
    /// Complete detached workspace TOML, preserving the original request shape.
    Supplied(String),
    /// The same explicit source selection accepted by local publication recipes.
    Selected(WorkspaceInput),
}

impl From<String> for WorkspaceSource {
    fn from(source: String) -> Self {
        Self::Supplied(source)
    }
}

impl From<&str> for WorkspaceSource {
    fn from(source: &str) -> Self {
        Self::Supplied(source.into())
    }
}

impl WorkspaceSource {
    fn input(&self) -> WorkspaceInput {
        match self {
            Self::Supplied(source) => WorkspaceInput::Supplied {
                source: source.clone(),
            },
            Self::Selected(input) => input.clone(),
        }
    }
}

/// A selected workspace or its complete source acquisition was refused.
#[derive(Debug)]
pub enum SelectedAcquisitionError {
    /// Native selection or detached workspace resolution failed before acquisition.
    Configuration(crate::configuration::WorkspaceLoadError),
    /// A required composition/publication source or capability could not be acquired.
    Composition(CompositionError),
    /// Selected project-documentation acquisition failed.
    Site(super::Error),
}

impl SelectedAcquisitionError {
    /// Preserve the native failure classification.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Configuration(error) => error.code(),
            Self::Composition(error) => error.code(),
            Self::Site(error) => error.code(),
        }
    }
}

impl std::fmt::Display for SelectedAcquisitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(error) => error.fmt(formatter),
            Self::Composition(error) => error.fmt(formatter),
            Self::Site(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SelectedAcquisitionError {}

/// Acquire one site's documentation using the same explicit workspace selection as builds.
/// The caller must grant native reads before invoking this operation.
///
/// # Errors
/// Preserves workspace capture, binding and selected-source acquisition failures.
pub fn acquire_selected_site(
    source: &WorkspaceSource,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<super::AcquiredSite, SelectedAcquisitionError> {
    let configuration = source
        .input()
        .resolve(syntax)
        .map_err(SelectedAcquisitionError::Configuration)?;
    gazette_application::build::acquire_resolved_site(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        configuration,
        site,
        limits,
    )
    .map_err(SelectedAcquisitionError::Site)
}

/// Resolve one exact workspace and inspect its selected composition sources.
/// The caller must grant native reads before invoking this operation.
///
/// # Errors
/// Preserves complete workspace capture and source-acquisition failures.
pub fn acquire_selected_composition(
    source: &WorkspaceSource,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredInputs, SelectedAcquisitionError> {
    let configuration = source
        .input()
        .resolve(syntax)
        .map_err(SelectedAcquisitionError::Configuration)?;
    gazette_application::build::acquire_resolved_composition(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        configuration,
        site,
        limits,
    )
    .map_err(SelectedAcquisitionError::Composition)
}

/// Resolve one exact workspace and acquire its complete publication universe.
/// The caller must grant native reads before invoking this operation.
///
/// # Errors
/// Preserves workspace, complete source-set and local capability failures.
pub fn acquire_selected_publication(
    source: &WorkspaceSource,
    site: &crate::workspace::Key,
    syntax: crate::toml::Limits,
    limits: crate::source::PreparationLimits,
) -> Result<AcquiredInputs, SelectedAcquisitionError> {
    let configuration = source
        .input()
        .resolve(syntax)
        .map_err(SelectedAcquisitionError::Configuration)?;
    let mut inputs = gazette_application::build::acquire_resolved_publication(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        configuration,
        site,
        limits,
    )
    .map_err(SelectedAcquisitionError::Composition)?;
    inputs
        .acquire_direct_local_capabilities(syntax, Default::default())
        .map_err(|error| {
            SelectedAcquisitionError::Composition(CompositionError::Capability(Box::new(error)))
        })?;
    Ok(inputs)
}
