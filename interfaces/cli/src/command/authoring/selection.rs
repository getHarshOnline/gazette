//! Exact supplied buffers for both native authoring entry points.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::authoring::session;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Source {
    /// Opaque client document identifier, such as a URI; never opened.
    id: String,
    /// Exact confined logical input path, not a filesystem locator.
    path: String,
    /// Complete selected baseline text.
    text: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PageInputSource {
    /// Opaque client identifier; never opened.
    id: String,
    /// Exact confined logical retained-input path.
    path: String,
    /// Complete retained bytes encoded once for JSON transport.
    bytes_base64: String,
}

impl PageInputSource {
    fn into_native(self) -> Result<session::PageInputSource, session::Error> {
        Ok(session::PageInputSource {
            id: self.id,
            path: self
                .path
                .parse()
                .map_err(|_| session::Error::Selection("invalid page input path"))?,
            bytes: STANDARD
                .decode(self.bytes_base64)
                .map_err(|_| session::Error::Selection("invalid page input base64"))?,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PackageSource {
    /// Exact typed Theme-package manifest baseline.
    Theme {
        package: gazette_sdk::workspace::ThemePackageId,
        id: String,
        path: String,
        text: String,
    },
    /// Exact typed plugin-package manifest baseline.
    Plugin {
        package: gazette_sdk::workspace::PluginPackageId,
        id: String,
        path: String,
        text: String,
    },
}

impl PackageSource {
    fn into_native(self) -> Result<session::PackageSource, session::Error> {
        let (package, id, path, text) = match self {
            Self::Theme {
                package,
                id,
                path,
                text,
            } => (
                gazette_sdk::authoring::PackageBaselineKey::Theme(package),
                id,
                path,
                text,
            ),
            Self::Plugin {
                package,
                id,
                path,
                text,
            } => (
                gazette_sdk::authoring::PackageBaselineKey::Plugin(package),
                id,
                path,
                text,
            ),
        };
        Ok(session::PackageSource {
            package,
            input: Source { id, path, text }.into_native()?,
        })
    }
}

impl Source {
    fn into_native(self) -> Result<session::Source, session::Error> {
        Ok(session::Source {
            id: self.id,
            path: self
                .path
                .parse()
                .map_err(|_| session::Error::Selection("invalid logical source path"))?,
            text: self.text,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Full selection of explicitly supplied buffers.
pub struct Selection {
    workspace: Source,
    /// Exact non-root modular workspace documents. Omit for the distinct
    /// single supplied-buffer authoring mode.
    workspace_documents: Option<Vec<Source>>,
    documents: Vec<Source>,
    /// Exact authored-manual baselines selected for Markdown editor analysis only.
    #[serde(default)]
    manuals: Vec<ManualSource>,
    /// Exact opaque retained page-package inputs; no native discovery occurs.
    #[serde(default)]
    page_inputs: Vec<PageInputSource>,
    /// Explicit typed package manifests. Empty keeps local pre-package authoring available.
    #[serde(default)]
    package_baselines: Vec<PackageSource>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ManualSource {
    /// Exact configured workspace source identity owning this capture-relative path.
    source: gazette_sdk::workspace::Key,
    id: String,
    path: String,
    text: String,
}

impl ManualSource {
    fn into_native(self) -> Result<session::ManualSource, session::Error> {
        Ok(session::ManualSource {
            source: self.source,
            input: Source {
                id: self.id,
                path: self.path,
                text: self.text,
            }
            .into_native()?,
        })
    }
}

impl Selection {
    pub(super) fn into_start(
        self,
        handle: String,
        limits: session::Limits,
    ) -> Result<super::Start, session::Error> {
        Ok(super::Start {
            handle,
            workspace: self.workspace.into_native()?,
            workspace_documents: self
                .workspace_documents
                .map(|sources| sources.into_iter().map(Source::into_native).collect())
                .transpose()?,
            documents: self
                .documents
                .into_iter()
                .map(Source::into_native)
                .collect::<Result<_, _>>()?,
            manuals: self
                .manuals
                .into_iter()
                .map(ManualSource::into_native)
                .collect::<Result<_, _>>()?,
            page_inputs: self
                .page_inputs
                .into_iter()
                .map(PageInputSource::into_native)
                .collect::<Result<_, _>>()?,
            package_baselines: self
                .package_baselines
                .into_iter()
                .map(PackageSource::into_native)
                .collect::<Result<_, _>>()?,
            limits,
        })
    }
}
