//! Native editor discovery around the pure selected-buffer session.
//!
//! Configuration chooses source roles. Filesystem metadata chooses membership;
//! only authored text and exact package manifests are read. Asset observations
//! are availability evidence, not captured bytes or publication permission.

use std::path::{Path, PathBuf};

use gazette_core::source::{self, EntryPath, ListingLimits};

use super::*;

mod configuration;
pub mod handoff;
mod sources;

/// Convert a selected absolute native path to the client's stable identifier.
pub type Identifier = fn(&Path) -> Result<String, &'static str>;

/// An explicitly retained manual support file outside configured navigation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManualSelection {
    /// Configured manual source owner.
    pub source: Key,
    /// File relative to that configured source, not the workspace directory.
    pub path: EntryPath,
}

/// Read-only native discovery and editable selected buffers for one workspace.
pub struct Workspace {
    root: PathBuf,
    path: EntryPath,
    identifier: Identifier,
    manuals: Vec<ManualSelection>,
    limits: Limits,
    state: Session,
    paths: BTreeMap<String, PathBuf>,
    candidates: Vec<sources::Location>,
    unavailable: Option<&'static str>,
    watch_files: Vec<PathBuf>,
    configuration_baselines: BTreeMap<EntryPath, source::Sha256Digest>,
}

impl Workspace {
    /// Open an explicitly selected workspace, retaining invalid configuration for repair.
    /// No parent search, source writes, build or publication effect occurs.
    ///
    /// # Errors
    /// Refuses an unreadable root document, invalid identifiers or retained-state limits.
    pub fn open_workspace(
        root: PathBuf,
        path: EntryPath,
        manuals: Vec<ManualSelection>,
        identifier: Identifier,
        limits: Limits,
    ) -> Result<Self, Error> {
        if !root.is_absolute()
            || root.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(Error::Selection(
                "workspace root must be an absolute normalized path",
            ));
        }
        // Even a missing include or binding must leave the root editable.
        let captured = crate::configuration::capture_selected(
            &root,
            std::slice::from_ref(&path),
            source_limits(limits),
        )
        .map_err(|_| Error::Selection("workspace root document is unavailable"))?;
        let text = std::str::from_utf8(
            captured
                .file(&path)
                .ok_or(Error::Selection("workspace root document is absent"))?,
        )
        .map_err(|_| Error::Selection("workspace root must be UTF-8"))?
        .to_owned();
        let native = root.join(path.as_str());
        let id = identifier(&native).map_err(Error::Selection)?;
        let state = Session::new_with_workspace_documents(
            Source {
                id: id.clone(),
                path: path.clone(),
                text,
            },
            [],
            [],
            [],
            limits,
        )?;
        let mut workspace = Self {
            watch_files: vec![native.clone()],
            root,
            path,
            identifier,
            manuals,
            limits,
            state,
            paths: BTreeMap::from([(id, native)]),
            candidates: Vec::new(),
            unavailable: None,
            configuration_baselines: BTreeMap::new(),
        };
        workspace.refresh()?;
        Ok(workspace)
    }

    /// Current buffers and analysis, including any current discovery failure.
    pub fn session(&self) -> &Session {
        &self.state
    }

    /// Exact native configuration files to watch, including the separate machine binding.
    pub fn configuration_paths(&self) -> impl Iterator<Item = &Path> {
        self.watch_files.iter().map(PathBuf::as_path)
    }

    /// Re-read selected configuration and refresh discovered membership.
    /// Open overlays survive; native failure never exposes an older successful analysis.
    ///
    /// # Errors
    /// Refuses exhausted retained-state or generation limits.
    pub fn refresh(&mut self) -> Result<(), Error> {
        let captured = configuration::capture(self);
        if let Ok(configuration) = &captured {
            self.watch_files.clone_from(&configuration.watch_files);
            self.configuration_baselines
                .clone_from(&configuration.baselines);
        }
        let result = captured
            .as_ref()
            .map_err(|error| *error)
            .and_then(|configuration| sources::select(self, configuration));
        match result {
            Ok(selection) => match self.state.reselect(selection.state) {
                Ok(()) => {
                    self.paths = selection.paths;
                    self.candidates = selection.candidates;
                    self.unavailable = (!selection.resolved
                        && self
                            .state
                            .analysis
                            .as_ref()
                            .is_ok_and(|analysis| analysis.structurally_valid()))
                    .then_some("selected workspace configuration could not be resolved");
                }
                Err(error) => self.unavailable = Some(error.reason()),
            },
            Err(error) => {
                if let Ok(configuration) = &captured
                    && let Ok(selection) = sources::retain_invalid(self, &configuration.documents)
                    && self.state.reselect(selection.state).is_ok()
                {
                    self.paths = selection.paths;
                }
                self.unavailable = Some(error.reason());
            }
        }
        self.mark_unavailable()?;
        Ok(())
    }

    /// Open current text, discovering a newly created selected page on first open.
    ///
    /// # Errors
    /// Refuses retained-state or generation limits.
    pub fn open(
        &mut self,
        id: &str,
        path: &Path,
        version: i32,
        text: String,
    ) -> Result<bool, Error> {
        if !self.select_path(path)? {
            return Ok(false);
        }
        let changed = self.state.open(id, version, text)?;
        self.after_edit(id, changed)?;
        Ok(changed)
    }

    /// Discover whether a native file belongs to this editor before accepting its text.
    /// Unrelated paths cause neither a source scan nor a generation change.
    ///
    /// # Errors
    /// Refuses invalid identifiers or exhausted analysis generations.
    pub fn select_path(&mut self, path: &Path) -> Result<bool, Error> {
        let id = (self.identifier)(path).map_err(Error::Selection)?;
        if self.state.asset_observations.contains_key(&id) {
            return Ok(false);
        }
        if !self.state.files.contains_key(&id) {
            if !self.watch_files.iter().any(|selected| selected == path)
                && !self
                    .candidates
                    .iter()
                    .any(|location| location.contains(path))
            {
                return Ok(false);
            }
            self.refresh()?;
        }
        Ok(self.state.files.contains_key(&id))
    }

    /// Replace a selected open buffer with a strictly newer full-text version.
    ///
    /// # Errors
    /// Refuses retained-state or generation limits.
    pub fn change(&mut self, id: &str, version: i32, text: String) -> Result<bool, Error> {
        let changed = self.state.change(id, version, text)?;
        self.after_edit(id, changed)?;
        Ok(changed)
    }

    /// Acknowledge exact saved text and refresh native membership and assets.
    ///
    /// # Errors
    /// Refuses mismatched saved text or retained-state and generation limits.
    pub fn save(&mut self, id: &str, text: &str) -> Result<bool, Error> {
        let changed = self.state.save(id, text)?;
        if changed {
            self.refresh()?;
        }
        Ok(changed)
    }

    /// Close an overlay and return to its native baseline and configuration selection.
    ///
    /// # Errors
    /// Refuses retained-state or generation limits.
    pub fn close(&mut self, id: &str) -> Result<bool, Error> {
        let changed = self.state.close(id)?;
        self.after_edit(id, changed)?;
        Ok(changed)
    }

    fn after_edit(&mut self, id: &str, changed: bool) -> Result<(), Error> {
        if !changed {
            return Ok(());
        }
        let state = self.session();
        if id == state.workspace
            || state
                .workspace_documents
                .as_ref()
                .is_some_and(|ids| ids.contains(id))
        {
            self.refresh()
        } else {
            self.mark_unavailable()
        }
    }

    fn mark_unavailable(&mut self) -> Result<(), Error> {
        if let Some(reason) = self.unavailable {
            let state = &mut self.state;
            if !matches!(state.analysis, Err(authoring::Error::UnavailableSources(_))) {
                state.unavailable(reason)?;
            }
        }
        Ok(())
    }

    fn source_limits(&self) -> source::Limits {
        source_limits(self.limits)
    }

    fn listing_limits(&self) -> ListingLimits {
        ListingLimits {
            entries: self.source_limits().entries,
            path_bytes: self.limits.identifier_bytes,
            ..Default::default()
        }
    }
}

fn source_limits(limits: Limits) -> source::Limits {
    source::Limits {
        entries: limits
            .documents
            .saturating_mul(source::Limits::default().depth + 1),
        file_bytes: limits.source_bytes as u64,
        total_bytes: limits.source_bytes as u64,
        ..Default::default()
    }
}
