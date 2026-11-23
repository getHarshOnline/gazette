//! Retain selected configuration bytes independently of workspace admission.

use super::*;
use gazette_core::source::CapturedSource;

/// Captured configuration for editing, without claiming that it is a valid workspace.
///
/// A malformed root retains only that root: no include or binding path is guessed.
/// A readable selection retains its exact includes and separate machine binding,
/// even when their contents fail workspace validation. Native read failures still
/// return an error instead of an incomplete capture.
#[derive(Debug)]
pub struct CapturedWorkspace {
    native_root: PathBuf,
    workspace_path: EntryPath,
    captured: CapturedSource,
    selection: Result<workspace::RootSelection, Error>,
    syntax: toml::SetLimits,
}

impl CapturedWorkspace {
    /// Absolute directory selected by the caller, without source-root discovery.
    pub fn native_root(&self) -> &Path {
        &self.native_root
    }

    /// Exact selected root document, relative to the native directory.
    pub fn workspace_path(&self) -> &EntryPath {
        &self.workspace_path
    }

    /// Captured files and their necessary ancestor directories, including a selected binding.
    pub fn inventory(&self) -> &Inventory {
        self.captured.inventory()
    }

    /// Exact retained bytes, including invalid configuration or a selected machine binding.
    pub fn source(&self, path: &EntryPath) -> Option<&[u8]> {
        self.captured.file(path)
    }

    /// Root and known included documents in selection order; bindings remain separate.
    /// If root selection failed, only the root is returned for repair.
    pub fn documents(&self) -> impl Iterator<Item = (&EntryPath, &[u8])> {
        std::iter::once(&self.workspace_path)
            .chain(
                self.selection
                    .as_ref()
                    .ok()
                    .into_iter()
                    .flat_map(|s| &s.includes),
            )
            .filter_map(|path| self.captured.file(path).map(|bytes| (path, bytes)))
    }

    /// Why the root could not select its includes and binding, without discarding its bytes.
    /// Included-document and workspace errors are found by analysis or strict resolution.
    pub fn selection_error(&self) -> Option<&Error> {
        self.selection.as_ref().err()
    }

    /// Resolve these retained bytes using the original capture's syntax limits.
    /// No file is reopened, and invalid input never becomes build evidence.
    ///
    /// # Errors
    /// Returns root-selection, workspace-validation or machine-binding failures.
    pub fn resolve(self) -> Result<LoadedWorkspace, Error> {
        let Self {
            native_root,
            workspace_path,
            captured,
            selection,
            syntax,
        } = self;
        let selection = selection?;
        let document_sources = std::iter::once(&workspace_path)
            .chain(&selection.includes)
            .filter_map(|path| captured.file(path).map(|bytes| (path, bytes)));
        let documents = workspace::compile_documents(&workspace_path, document_sources, syntax)
            .map_err(Error::Workspace)?;
        let machine_roots = match selection.machine_binding {
            Some(path) => machine::roots(&native_root, &path, &captured, syntax.document)?,
            None => BTreeMap::new(),
        };
        let source_roots = documents
            .compilation
            .workspace
            .sources()
            .filter_map(|source| {
                resolve_root(Some(&native_root), &machine_roots, source.location.root())
                    .ok()
                    .map(|path| (source.id.clone(), path))
            })
            .collect();
        let (inventory, contents) = captured.into_parts();
        Ok(LoadedWorkspace {
            native_root,
            machine_roots,
            source_roots,
            inventory,
            contents,
            documents,
        })
    }
}

/// Capture the root and exactly the configuration paths it can declare.
///
/// Invalid root syntax or selection is retained for editing without opening
/// guessed paths. A valid selection is captured completely and its root bytes
/// must still match the first read. This operation neither compiles a workspace
/// nor opens its publication sources.
///
/// # Errors
/// Refuses relative roots, failed or incomplete native reads, changed selectors
/// and exhausted capture bounds. Syntax failures remain in the returned capture.
pub fn capture(
    reader: &impl SelectedSourceCapture,
    root: &Path,
    workspace_path: &EntryPath,
    limits: Limits,
) -> Result<CapturedWorkspace, Error> {
    if !root.is_absolute() {
        return Err(Error::RootNotAbsolute(root.to_owned()));
    }
    let initial = reader
        .capture_selected(root, std::slice::from_ref(workspace_path), limits.source)
        .map_err(Error::Source)?;
    let initial_root = required_file(&initial, workspace_path)?;
    let selection = std::str::from_utf8(initial_root)
        .map_err(|error| Error::Utf8 {
            valid_up_to: error.valid_up_to(),
        })
        .and_then(|text| {
            workspace::root_selection(text, limits.syntax.document).map_err(Error::Workspace)
        })
        .and_then(|selection| {
            if selection.machine_binding.as_ref() == Some(workspace_path) {
                return Err(Error::MachineBinding {
                    path: workspace_path.clone(),
                    error: toml::Error::Invalid {
                        message: "machine binding must be separate from workspace documents".into(),
                        span: None,
                    },
                });
            }
            Ok(selection)
        });
    let captured = if let Ok(selection) = &selection {
        let paths: Vec<_> = std::iter::once(workspace_path.clone())
            .chain(selection.includes.iter().cloned())
            .chain(selection.machine_binding.iter().cloned())
            .collect();
        let captured = reader
            .capture_selected(root, &paths, limits.source)
            .map_err(Error::Source)?;
        if required_file(&captured, workspace_path)? != initial_root {
            return Err(Error::Source(source::Error::Changed(PathBuf::from(
                workspace_path.as_str(),
            ))));
        }
        for path in &paths {
            required_file(&captured, path)?;
        }
        captured
    } else {
        initial
    };
    Ok(CapturedWorkspace {
        native_root: root.to_owned(),
        workspace_path: workspace_path.clone(),
        captured,
        selection,
        syntax: limits.syntax,
    })
}

fn required_file<'a>(captured: &'a CapturedSource, path: &EntryPath) -> Result<&'a [u8], Error> {
    captured.file(path).ok_or_else(|| {
        Error::Source(source::Error::InvalidInventory {
            path: PathBuf::from(path.as_str()),
            reason: "selected capture omitted a requested workspace document",
        })
    })
}
