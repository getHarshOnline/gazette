//! Explicit native acquisition for one strict, modular Gazette workspace.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use gazette_compiler::{toml, workspace};
use gazette_core::source::{self, EntryPath, FileContents, Inventory};
use gazette_ports::source::SelectedSourceCapture;

mod capture;
mod discovery;
mod machine;
mod runtime;
pub use capture::{CapturedWorkspace, capture};
pub use discovery::{
    DiscoveredSource, DiscoveryError, discover_authoring_sources, discover_sources,
};
pub use runtime::RuntimeBindingError;

/// Independent native acquisition and complete syntax-set ceilings.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Exact selected-file capture bounds.
    pub source: source::Limits,
    /// Aggregate and per-document TOML bounds.
    pub syntax: toml::SetLimits,
}

impl Default for Limits {
    fn default() -> Self {
        let syntax = toml::SetLimits::default();
        Self {
            source: source::Limits {
                file_bytes: syntax.document.source_bytes as u64,
                total_bytes: syntax.source_bytes as u64,
                ..source::Limits::default()
            },
            syntax,
        }
    }
}

/// Exact captured configuration tree and its one resolved workspace.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct LoadedWorkspace {
    native_root: PathBuf,
    #[serde(skip)]
    #[schemars(skip)]
    machine_roots: BTreeMap<String, PathBuf>,
    source_roots: BTreeMap<workspace::Key, PathBuf>,
    inventory: Inventory,
    #[serde(skip)]
    #[schemars(skip)]
    contents: FileContents,
    documents: workspace::DocumentCompilation,
}

/// Exact configuration acquisition evidence retained beside every resolved build graph.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// One exact caller-supplied UTF-8 document; no native configuration path was opened.
    Supplied {
        /// Digest of the exact supplied document bytes.
        source_sha256: gazette_core::source::Sha256Digest,
    },
    /// Exact native document set selected by the root document.
    Documents {
        /// Absolute selected workspace root that gives relative binding values their meaning.
        native_root: PathBuf,
        /// Selected configuration and machine-binding files plus necessary ancestor directories.
        inventory: Inventory,
        /// Root and included documents with their individual byte digests.
        documents: Vec<workspace::DocumentEvidence>,
    },
}

/// One immutable workspace compilation and the exact configuration evidence that produced it.
#[derive(Clone, Debug)]
pub struct ResolvedWorkspace {
    compilation: Arc<workspace::Compilation>,
    evidence: Arc<Evidence>,
    native_root: Option<Arc<PathBuf>>,
    machine_roots: Arc<BTreeMap<String, PathBuf>>,
    commands: Arc<runtime::RuntimeCommands>,
}

/// A portable coordinate lacks its explicitly selected native binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceRootUnavailable {
    /// Detached configuration has no selected native workspace root.
    Workspace,
    /// The selected machine binding does not supply this root name.
    Machine(String),
}

impl fmt::Display for SourceRootUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workspace => formatter.write_str(
                "workspace-relative source root requires a selected native workspace; load the workspace from an explicit root instead of supplying detached TOML bytes",
            ),
            Self::Machine(name) => write!(
                formatter,
                "machine root {name:?} is not bound; load a workspace with an explicit machine_binding and a matching roots entry",
            ),
        }
    }
}

impl std::error::Error for SourceRootUnavailable {}

impl ResolvedWorkspace {
    pub(crate) fn supplied(compilation: workspace::Compilation) -> Self {
        let source_sha256 = compilation.source_sha256;
        let commands =
            runtime::RuntimeCommands::bind(&compilation.workspace, None, &BTreeMap::new());
        Self {
            compilation: Arc::new(compilation),
            evidence: Arc::new(Evidence::Supplied { source_sha256 }),
            native_root: None,
            machine_roots: Arc::new(BTreeMap::new()),
            commands: Arc::new(commands),
        }
    }

    /// The single resolved topology consumed by acquisition, rendering and release preparation.
    pub fn compilation(&self) -> &workspace::Compilation {
        &self.compilation
    }

    /// Exact supplied or selected-document evidence behind the compilation identity.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// Native math command bound from this exact captured configuration.
    /// Unused command failures do not prevent inspecting the portable workspace.
    ///
    /// # Errors
    /// Refuses missing roots, non-UTF-8 paths or expanded commands above native limits.
    pub fn math_command(
        &self,
    ) -> Result<Option<&gazette_core::process::Command>, RuntimeBindingError> {
        self.commands
            .math
            .as_ref()
            .map(|command| command.as_ref().map_err(Clone::clone))
            .transpose()
    }

    /// Native policy command used for both independent digest checks and execution.
    /// No paths are reopened and no environment or current-directory lookup occurs.
    ///
    /// # Errors
    /// Refuses missing roots, non-UTF-8 paths or expanded commands above native limits.
    pub fn policy_command(
        &self,
    ) -> Result<Option<&gazette_core::process::Command>, RuntimeBindingError> {
        self.commands
            .policy
            .as_ref()
            .map(|command| command.as_ref().map_err(Clone::clone))
            .transpose()
    }

    /// Bind a generator's executable, argument and environment paths using captured roots.
    /// Literal values and retained-input arguments remain unchanged.
    ///
    /// # Errors
    /// Refuses missing roots, malformed paths and expanded native recipes above their limits.
    pub fn documentation_recipe(
        &self,
        recipe: &gazette_core::documentation::generator::Recipe<workspace::RuntimeArgument>,
    ) -> Result<gazette_core::documentation::generator::Recipe, RuntimeBindingError> {
        let path = |value: &str, field| {
            runtime::bind_path(
                self.native_root.as_deref().map(PathBuf::as_path),
                &self.machine_roots,
                value,
                field,
            )
        };
        let program = path(&recipe.program, "program")?;
        let mut bound = recipe.try_map_runtime_values(|value, field| match value {
            workspace::RuntimeArgument::Literal(value) => Ok(value.clone()),
            workspace::RuntimeArgument::Path { path: value } => path(value, field),
        })?;
        bound.program = program;
        for argument in bound
            .arguments
            .iter_mut()
            .chain(bound.validation_arguments.iter_mut().flatten())
        {
            if let gazette_core::documentation::generator::Argument::Path { path: value } = argument
            {
                *value = path(value, "argument")?;
            }
        }
        bound
            .validate()
            .map_err(|error| RuntimeBindingError::Command(error.code()))?;
        Ok(bound)
    }

    /// Resolve one portable coordinate against this workspace and its machine binding.
    ///
    /// Absolute external roots pass through unchanged. Relative roots never consult
    /// the process current directory and fail for detached supplied-buffer workspaces.
    ///
    /// # Errors
    /// Refuses unbound machine roots and relative roots in detached supplied configuration.
    pub fn source_root(
        &self,
        root: &workspace::SourceRoot,
    ) -> Result<PathBuf, SourceRootUnavailable> {
        resolve_root(
            self.native_root.as_deref().map(PathBuf::as_path),
            &self.machine_roots,
            root,
        )
    }
}

fn resolve_root(
    native_root: Option<&Path>,
    machine_roots: &BTreeMap<String, PathBuf>,
    root: &workspace::SourceRoot,
) -> Result<PathBuf, SourceRootUnavailable> {
    use workspace::SourceRoot;
    let (base, path) = match root {
        SourceRoot::External(path) => return Ok(PathBuf::from(path)),
        SourceRoot::Machine { name, path } => (
            machine_roots
                .get(name)
                .ok_or_else(|| SourceRootUnavailable::Machine(name.clone()))?
                .as_path(),
            path.as_ref(),
        ),
        SourceRoot::Workspace | SourceRoot::WorkspaceRelative(_) => (
            native_root.ok_or(SourceRootUnavailable::Workspace)?,
            root.workspace_relative().flatten(),
        ),
    };
    Ok(match path {
        None => base.to_owned(),
        Some(path) => path
            .components()
            .fold(base.to_owned(), |root, component| root.join(component)),
    })
}

impl LoadedWorkspace {
    /// Native roots keyed by configured source identity, before subdirectory selection.
    ///
    /// These private coordinates use the captured binding; they do not establish source
    /// existence, capture contents or verify repository pins. Unbound roots are omitted.
    pub fn source_roots(&self) -> &BTreeMap<workspace::Key, PathBuf> {
        &self.source_roots
    }

    /// Exact selected files and necessary ancestor directories only.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Exact retained bytes for a selected workspace document or machine binding.
    ///
    /// Consumers never reopen the native path after the workspace loader has
    /// established its complete document set and compilation identity.
    pub fn source(&self, path: &EntryPath) -> Option<&[u8]> {
        self.contents.get(path)
    }

    /// Resolved workspace plus every contributing document digest.
    pub fn documents(&self) -> &workspace::DocumentCompilation {
        &self.documents
    }

    /// Move the independently useful structural and semantic results.
    pub fn into_parts(self) -> (Inventory, workspace::DocumentCompilation) {
        (self.inventory, self.documents)
    }

    /// Bind the loaded document set to the one immutable compilation used by build phases.
    pub fn into_resolved(self) -> ResolvedWorkspace {
        let Self {
            native_root,
            machine_roots,
            source_roots: _,
            inventory,
            contents: _,
            documents,
        } = self;
        let workspace::DocumentCompilation {
            compilation,
            documents,
        } = documents;
        let commands = runtime::RuntimeCommands::bind(
            &compilation.workspace,
            Some(&native_root),
            &machine_roots,
        );
        ResolvedWorkspace {
            compilation: Arc::new(compilation),
            evidence: Arc::new(Evidence::Documents {
                native_root: native_root.clone(),
                inventory,
                documents,
            }),
            native_root: Some(Arc::new(native_root)),
            machine_roots: Arc::new(machine_roots),
            commands: Arc::new(commands),
        }
    }
}

/// Native capture, UTF-8/schema, or workspace-resolution failure.
#[derive(Debug)]
pub enum Error {
    /// The selected native root was relative, so its meaning depended on process state.
    RootNotAbsolute(PathBuf),
    /// Confined exact-file acquisition failed.
    Source(source::Error),
    /// The root document is not UTF-8.
    Utf8 {
        /// Length of the valid UTF-8 prefix in the selected root document.
        valid_up_to: usize,
    },
    /// Strict syntax, module-set, or semantic resolution failed.
    Workspace(workspace::Error),
    /// The selected machine binding could not supply bounded, typed root values.
    MachineBinding {
        /// Exact selected binding path, relative to the workspace.
        path: EntryPath,
        /// Syntax, size or root-value failure.
        error: toml::Error,
    },
}

impl Error {
    /// Stable underlying failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::RootNotAbsolute(_) => "workspace_root_not_absolute",
            Self::Source(error) => error.code(),
            Self::Utf8 { .. } => "workspace_root_utf8",
            Self::Workspace(error) => error.code(),
            Self::MachineBinding { .. } => "workspace_machine_binding",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootNotAbsolute(root) => write!(
                formatter,
                "workspace root {} is not absolute; select one explicit native root",
                root.display()
            ),
            Self::Source(error) => error.fmt(formatter),
            Self::Utf8 { valid_up_to } => {
                write!(
                    formatter,
                    "workspace root is not UTF-8 at byte {valid_up_to}"
                )
            }
            Self::Workspace(error) => error.fmt(formatter),
            Self::MachineBinding { path, error } => {
                write!(formatter, "machine binding {path}: {error}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::MachineBinding { error, .. } => Some(error),
            Self::RootNotAbsolute(_) | Self::Utf8 { .. } => None,
        }
    }
}

/// Load one explicitly selected root and exactly its declared non-recursive includes.
///
/// The root is captured once to decode its includes and machine binding, then again
/// with the complete exact set. Changed root bytes fail closed. No sibling discovery,
/// extension filtering, legacy schema, registry access or source fallback occurs.
///
/// # Errors
///
/// Rejects unsafe or changing native inputs, invalid UTF-8/syntax, incomplete
/// document sets, exhausted limits and unresolved workspace meaning.
pub fn load(
    reader: &impl SelectedSourceCapture,
    root: &Path,
    workspace_path: &EntryPath,
    limits: Limits,
) -> Result<LoadedWorkspace, Error> {
    capture(reader, root, workspace_path, limits)?.resolve()
}

/// Resolve one exact supplied document without opening native configuration paths.
///
/// # Errors
///
/// Rejects invalid syntax, exhausted limits and unresolved workspace meaning.
pub fn resolve(source: &str, limits: toml::Limits) -> Result<ResolvedWorkspace, Error> {
    let compilation = workspace::compile(source, limits).map_err(Error::Workspace)?;
    Ok(ResolvedWorkspace::supplied(compilation))
}
