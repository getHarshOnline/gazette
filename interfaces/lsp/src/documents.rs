//! LSP selection transport only; the SDK owns baselines, overlays and analysis.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::authoring::{self, session};
use lsp_types::{InitializeParams, Uri};
use serde::Deserialize;

use crate::{Error, Limits};

mod native;
mod state;
pub(super) use state::State;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    workspace: Source,
    /// Exact non-root modular workspace documents. Omit to select the distinct
    /// supplied-buffer workspace mode.
    workspace_documents: Option<Vec<Source>>,
    documents: Vec<Source>,
    /// Authored manuals receive shared Markdown editor analysis but are not
    /// publication content or documentation-target selections.
    #[serde(default)]
    manuals: Vec<ManualSource>,
    /// Exact opaque retained page-package inputs. These bytes are immutable and
    /// never become editor overlays or independently interpreted documents.
    #[serde(default)]
    page_inputs: Vec<PageInputSource>,
    #[serde(default)]
    package_baselines: Vec<PackageSource>,
}

pub(super) fn select_initialization(
    mut params: InitializeParams,
    limits: Limits,
) -> Result<State, String> {
    let options = params
        .initialization_options
        .take()
        .ok_or("explicit supplied buffers or native selection are required")?;
    if options.get("native_selection").is_some() {
        if options["native_selection"].get("discover").is_some() {
            return state::discover(&params, options, limits);
        }
        return native::select(&params, options, limits).map(State::supplied);
    }
    if !options
        .get("workspace")
        .is_some_and(serde_json::Value::is_object)
        || !options
            .get("documents")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|documents| documents.iter().all(serde_json::Value::is_object))
    {
        return Err("selected supplied sources must be named objects".into());
    }
    let selection = serde_json::from_value(options)
        .map_err(|_| "invalid selected-source initialization".to_owned())?;
    select(selection, limits)
        .map(State::supplied)
        .map_err(str::to_owned)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    uri: Uri,
    path: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualSource {
    source: gazette_sdk::workspace::Key,
    uri: Uri,
    path: String,
    text: String,
}

impl ManualSource {
    fn into_source(self) -> Result<session::ManualSource, &'static str> {
        Ok(session::ManualSource {
            source: self.source,
            input: Source {
                uri: self.uri,
                path: self.path,
                text: self.text,
            }
            .into_source()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageInputSource {
    uri: Uri,
    path: String,
    bytes_base64: String,
}

impl PageInputSource {
    fn into_source(self) -> Result<session::PageInputSource, &'static str> {
        Ok(session::PageInputSource {
            id: self.uri.as_str().to_owned(),
            path: self
                .path
                .parse()
                .map_err(|_| "invalid logical page input path")?,
            bytes: STANDARD
                .decode(self.bytes_base64)
                .map_err(|_| "invalid page input base64")?,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PackageSource {
    Theme {
        package: gazette_sdk::workspace::ThemePackageId,
        uri: Uri,
        path: String,
        text: String,
    },
    Plugin {
        package: gazette_sdk::workspace::PluginPackageId,
        uri: Uri,
        path: String,
        text: String,
    },
}

impl PackageSource {
    fn into_source(self) -> Result<session::PackageSource, &'static str> {
        let (package, uri, path, text) = match self {
            Self::Theme {
                package,
                uri,
                path,
                text,
            } => (
                authoring::PackageBaselineKey::Theme(package),
                uri,
                path,
                text,
            ),
            Self::Plugin {
                package,
                uri,
                path,
                text,
            } => (
                authoring::PackageBaselineKey::Plugin(package),
                uri,
                path,
                text,
            ),
        };
        Ok(session::PackageSource {
            package,
            input: Source { uri, path, text }.into_source()?,
        })
    }
}

impl Source {
    fn into_source(self) -> Result<session::Source, &'static str> {
        Ok(session::Source {
            id: self.uri.as_str().to_owned(),
            path: self
                .path
                .parse()
                .map_err(|_| "invalid logical source path")?,
            text: self.text,
        })
    }
}

pub(super) fn select(
    selection: Selection,
    limits: Limits,
) -> Result<session::Session, &'static str> {
    let workspace = selection.workspace.into_source()?;
    let workspace_documents = selection
        .workspace_documents
        .map(|documents| {
            documents
                .into_iter()
                .map(Source::into_source)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let documents = selection
        .documents
        .into_iter()
        .map(Source::into_source)
        .collect::<Result<Vec<_>, _>>()?;
    let manuals = selection
        .manuals
        .into_iter()
        .map(ManualSource::into_source)
        .collect::<Result<Vec<_>, _>>()?;
    let page_inputs = selection
        .page_inputs
        .into_iter()
        .map(PageInputSource::into_source)
        .collect::<Result<Vec<_>, _>>()?;
    let package_baselines = selection
        .package_baselines
        .into_iter()
        .map(PackageSource::into_source)
        .collect::<Result<Vec<_>, _>>()?;
    select_retained(
        workspace,
        workspace_documents,
        documents,
        manuals,
        page_inputs,
        package_baselines,
        limits,
    )
}

fn select_retained(
    workspace: session::Source,
    workspace_documents: Option<Vec<session::Source>>,
    documents: Vec<session::Source>,
    manuals: Vec<session::ManualSource>,
    page_inputs: Vec<session::PageInputSource>,
    package_baselines: Vec<session::PackageSource>,
    limits: Limits,
) -> Result<session::Session, &'static str> {
    let limits = session::Limits {
        documents: limits.documents,
        identifier_bytes: limits.uri_bytes,
        source_bytes: limits.source_bytes,
        analysis: authoring::Limits {
            workspace: limits.syntax,
            ..limits.authoring
        },
    };
    match workspace_documents {
        None => session::Session::new_with_manuals_page_inputs_and_package_baselines(
            workspace,
            documents,
            manuals,
            page_inputs,
            package_baselines,
            limits,
        ),
        Some(workspace_documents) => {
            session::Session::new_with_workspace_documents_manuals_page_inputs(
                workspace,
                workspace_documents,
                documents,
                manuals,
                page_inputs,
                package_baselines,
                limits,
            )
        }
    }
    .map_err(|error| error.reason())
}

pub(super) fn error(error: session::Error) -> Error {
    match error {
        session::Error::Selection(reason) => Error::Protocol(reason),
        session::Error::Limit(resource) => Error::Limit(resource),
    }
}
