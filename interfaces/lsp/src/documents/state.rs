//! Dispatch mutations through native discovery without exposing mutable session bypasses.

use std::{ops::Deref, path::Path};

use super::*;
use session::native::{ManualSelection, Workspace};

pub(crate) enum State {
    Supplied(Box<session::Session>),
    Native(Box<Workspace>),
}

impl Deref for State {
    type Target = session::Session;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Supplied(state) => state,
            Self::Native(workspace) => workspace.session(),
        }
    }
}

impl State {
    pub(crate) fn handoff_selection(&self) -> Option<session::native::handoff::Selection> {
        match self {
            Self::Native(workspace) => Some(workspace.handoff_selection()),
            Self::Supplied(_) => None,
        }
    }

    pub(crate) fn configuration_uris(&self) -> Result<Vec<String>, session::Error> {
        match self {
            Self::Supplied(_) => Ok(vec![]),
            Self::Native(workspace) => workspace
                .configuration_paths()
                .map(|path| identifier(path).map_err(session::Error::Selection))
                .collect(),
        }
    }
    pub(crate) fn select_uri(&mut self, uri: &Uri) -> Result<bool, session::Error> {
        match self {
            Self::Supplied(state) => Ok(state.files().contains_key(uri.as_str())),
            Self::Native(workspace) => {
                let path = native::file_path(uri).map_err(session::Error::Selection)?;
                workspace.select_path(&path)
            }
        }
    }
    pub(crate) fn supplied(state: session::Session) -> Self {
        Self::Supplied(Box::new(state))
    }

    pub(crate) fn open(
        &mut self,
        id: &str,
        version: i32,
        text: String,
    ) -> Result<bool, session::Error> {
        match self {
            Self::Supplied(state) => state.open(id, version, text),
            Self::Native(workspace) => {
                let uri = id
                    .parse()
                    .map_err(|_| session::Error::Selection("invalid file URI"))?;
                let path = native::file_path(&uri).map_err(session::Error::Selection)?;
                workspace.open(id, &path, version, text)
            }
        }
    }

    pub(crate) fn change(
        &mut self,
        id: &str,
        version: i32,
        text: String,
    ) -> Result<bool, session::Error> {
        match self {
            Self::Supplied(state) => state.change(id, version, text),
            Self::Native(workspace) => workspace.change(id, version, text),
        }
    }

    pub(crate) fn close(&mut self, id: &str) -> Result<bool, session::Error> {
        match self {
            Self::Supplied(state) => state.close(id),
            Self::Native(workspace) => workspace.close(id),
        }
    }

    pub(crate) fn save(&mut self, id: &str, text: &str) -> Result<bool, session::Error> {
        match self {
            Self::Supplied(state) => state.save(id, text),
            Self::Native(workspace) => workspace.save(id, text),
        }
    }

    pub(crate) fn refresh(&mut self) -> Result<bool, session::Error> {
        match self {
            Self::Supplied(_) => Ok(false),
            Self::Native(workspace) => {
                let generation = workspace.session().generation();
                workspace.refresh()?;
                Ok(generation != workspace.session().generation())
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Initialization {
    native_selection: Discovery,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discovery {
    allow_read: bool,
    discover: bool,
    workspace: gazette_sdk::configuration::EntryPath,
    #[serde(default)]
    manuals: Vec<Manual>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manual {
    source: gazette_sdk::workspace::Key,
    path: gazette_sdk::configuration::EntryPath,
}

pub(super) fn discover(
    params: &InitializeParams,
    options: serde_json::Value,
    limits: Limits,
) -> Result<State, String> {
    let selection: Initialization =
        serde_json::from_value(options).map_err(|_| "invalid native discovery initialization")?;
    let selection = selection.native_selection;
    if !selection.allow_read || !selection.discover {
        return Err(
            "native discovery requires explicit read permission and discovery selection".into(),
        );
    }
    let root = native::native_root(params)?;
    let limits = session::Limits {
        documents: limits.documents,
        identifier_bytes: limits.uri_bytes,
        source_bytes: limits.source_bytes,
        analysis: authoring::Limits {
            workspace: limits.syntax,
            ..limits.authoring
        },
    };
    Workspace::open_workspace(
        root,
        selection.workspace,
        selection
            .manuals
            .into_iter()
            .map(|manual| ManualSelection {
                source: manual.source,
                path: manual.path,
            })
            .collect(),
        identifier,
        limits,
    )
    .map(|workspace| State::Native(Box::new(workspace)))
    .map_err(|error| error.to_string())
}

fn identifier(path: &Path) -> Result<String, &'static str> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| "selected native path is not a file URI")
}
