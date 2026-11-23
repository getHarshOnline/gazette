//! Resolve current configuration overlays while retaining native baselines separately.

use std::cell::RefCell;

use gazette_adapter_fs::FileSystem;
use gazette_core::source::{self, CapturedSource, Content, InventoryBuilder, Sha256Digest};
use gazette_ports::source::SelectedSourceCapture;

use super::*;

pub(super) struct Configuration {
    pub documents: Vec<Source>,
    pub loaded: Option<crate::configuration::LoadedWorkspace>,
    pub watch_files: Vec<PathBuf>,
    pub failure: Option<&'static str>,
    pub baselines: BTreeMap<EntryPath, Sha256Digest>,
}

pub(super) fn capture(workspace: &Workspace) -> Result<Configuration, Error> {
    let reader = CurrentConfiguration {
        workspace,
        baselines: RefCell::new(BTreeMap::new()),
    };
    let limits = crate::configuration::WorkspaceLoadLimits {
        source: workspace.source_limits(),
        syntax: workspace.limits.analysis.workspace_documents,
    };
    let captured = gazette_application::configuration::capture(
        &reader,
        &workspace.root,
        &workspace.path,
        limits,
    );
    let captured = match captured {
        Ok(captured) => captured,
        Err(_) => return reader.root_for_repair(limits.syntax.document),
    };
    let baselines = reader.baselines.borrow();
    let documents = captured
        .documents()
        .map(|(path, _)| {
            let text = baselines
                .get(path)
                .ok_or(Error::Selection("configuration baseline is absent"))?;
            Ok(Source {
                id: (workspace.identifier)(&workspace.root.join(path.as_str()))
                    .map_err(Error::Selection)?,
                path: path.clone(),
                text: text.clone(),
            })
        })
        .collect::<Result<_, Error>>()?;
    let watch_files = captured
        .inventory()
        .entries()
        .iter()
        .filter(|entry| matches!(entry.content, Content::File { .. }))
        .map(|entry| workspace.root.join(entry.path.as_str()))
        .collect();
    Ok(Configuration {
        documents,
        loaded: captured.resolve().ok(),
        watch_files,
        failure: None,
        baselines: baselines
            .iter()
            .map(|(path, text)| (path.clone(), Sha256Digest::of(text.as_bytes())))
            .collect(),
    })
}

struct CurrentConfiguration<'a> {
    workspace: &'a Workspace,
    baselines: RefCell<BTreeMap<EntryPath, String>>,
}

impl CurrentConfiguration<'_> {
    fn root_for_repair(&self, limits: crate::toml::Limits) -> Result<Configuration, Error> {
        let baselines = self.baselines.borrow();
        let workspace = self.workspace;
        let text = baselines
            .get(&workspace.path)
            .ok_or(Error::Selection("selected workspace root is unavailable"))?;
        let native = workspace.root.join(workspace.path.as_str());
        let id = (workspace.identifier)(&native).map_err(Error::Selection)?;
        let current = workspace
            .state
            .files
            .get(&id)
            .and_then(|document| document.overlay.as_ref())
            .map_or(text.as_str(), |overlay| overlay.text.as_str());
        let mut watch_files = vec![native];
        if let Ok(selection) = gazette_compiler::workspace::root_selection(current, limits) {
            // Missing selected files remain watch targets, never fabricated inputs.
            watch_files.extend(
                selection
                    .includes
                    .iter()
                    .chain(selection.machine_binding.iter())
                    .map(|path| workspace.root.join(path.as_str())),
            );
        }
        Ok(Configuration {
            documents: vec![Source {
                id,
                path: workspace.path.clone(),
                text: text.clone(),
            }],
            loaded: None,
            watch_files,
            failure: Some("selected workspace includes or machine binding are unavailable"),
            baselines: baselines
                .iter()
                .map(|(path, text)| (path.clone(), Sha256Digest::of(text.as_bytes())))
                .collect(),
        })
    }
}

impl SelectedSourceCapture for CurrentConfiguration<'_> {
    fn capture_selected(
        &self,
        root: &Path,
        paths: &[EntryPath],
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        // Native presence and confinement are established even for open files.
        // This private composition is only for editor analysis, never build input.
        let (inventory, contents) = FileSystem
            .capture_selected(root, paths, limits)?
            .into_parts();
        let mut files = contents.into_files();
        for (path, bytes) in &mut files {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| source::Error::InvalidName(root.join(path.as_str())))?;
            self.baselines
                .borrow_mut()
                .insert(path.clone(), text.to_owned());
            let state = &self.workspace.state;
            let id = (self.workspace.identifier)(&root.join(path.as_str()))
                .map_err(|_| source::Error::InvalidName(root.join(path.as_str())))?;
            let is_configuration = id == state.workspace
                || state
                    .workspace_documents
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&id));
            if is_configuration
                && let Some(overlay) = state.files.get(&id).and_then(|file| file.overlay.as_ref())
            {
                *bytes = overlay.text.as_bytes().to_vec();
            }
        }
        let mut builder = InventoryBuilder::new(limits);
        for entry in inventory.entries() {
            let content = match entry.content {
                Content::Directory => Content::Directory,
                Content::File { .. } => {
                    let bytes = &files[&entry.path];
                    Content::File {
                        bytes: bytes.len() as u64,
                        sha256: Sha256Digest::of(bytes),
                    }
                }
            };
            builder.insert(entry.path.clone(), content)?;
        }
        CapturedSource::from_parts(builder.finish(), files)
    }
}
