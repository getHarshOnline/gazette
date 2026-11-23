//! Supplied-byte page-package inspection through the Rust SDK authoring session.

use gazette_sdk::{
    authoring::{self, session},
    source::EntryPath,
};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;

use crate::{encode, integer};

/// One exact logical source and its complete retained bytes.
#[napi(object)]
pub struct PagePackageSource {
    /// Root-relative compiler path; never opened as a native path.
    pub path: String,
    /// Complete selected bytes. Publication documents must be UTF-8; companions may be binary.
    pub bytes: Uint8Array,
}

/// Complete supplied selection for one page-package ownership query.
#[napi(object)]
pub struct PagePackageRequest {
    /// Complete workspace source selected by the caller.
    pub workspace: PagePackageSource,
    /// Complete Theme-rendered publication document set.
    pub documents: Vec<PagePackageSource>,
    /// Complete opaque companion set retained for page-package resolution.
    pub companions: Vec<PagePackageSource>,
    /// Exact selected path whose compiler-owned package role is requested.
    pub query: String,
}

/// Host/session ceilings; omitted values retain the Rust SDK defaults.
#[napi(object)]
pub struct PagePackageOptions {
    /// Maximum selected entries, including the workspace.
    pub selection_entries: Option<f64>,
    /// Maximum aggregate logical identifier bytes.
    pub identifier_bytes: Option<f64>,
    /// Maximum aggregate retained source bytes.
    pub source_bytes: Option<f64>,
    /// Maximum exact serialized result bytes; defaults to 1 MiB.
    pub result_bytes: Option<f64>,
}

fn classified(code: &'static str, maximum: usize) -> Error {
    match encode(&serde_json::json!({"code": code}), maximum) {
        Ok(value) => Error::new(Status::InvalidArg, value),
        Err(error) => error,
    }
}

fn path(value: &str, maximum: usize) -> Result<EntryPath> {
    value
        .parse::<EntryPath>()
        .map_err(|error| classified(error.code(), maximum))
}

fn document(value: PagePackageSource, maximum: usize) -> Result<session::Source> {
    let path = path(&value.path, maximum)?;
    let text = String::from_utf8(value.bytes.to_vec())
        .map_err(|_| classified("authoring_source_utf8", maximum))?;
    Ok(session::Source {
        id: value.path,
        path,
        text,
    })
}

fn companion(value: PagePackageSource, maximum: usize) -> Result<session::PageInputSource> {
    Ok(session::PageInputSource {
        path: path(&value.path, maximum)?,
        id: value.path,
        bytes: value.bytes.to_vec(),
    })
}

/// Inspect one exact supplied input's compiler-owned page-package role.
///
/// The Rust SDK authoring session owns selection and retained-byte binding, and the
/// application resolver owns package membership. This projection performs no native
/// reads, path discovery, companion inference or compatibility fallback. Standalone
/// pages return JSON `null`.
///
/// # Errors
/// Rejects invalid/ambiguous selections, non-UTF-8 publication documents, exhausted
/// bounds, unavailable native analysis or a query outside the exact selected set.
#[napi]
pub fn inspect_page_package(
    request: PagePackageRequest,
    options: Option<PagePackageOptions>,
) -> Result<String> {
    let defaults = session::Limits::default();
    let options = options.unwrap_or(PagePackageOptions {
        selection_entries: None,
        identifier_bytes: None,
        source_bytes: None,
        result_bytes: None,
    });
    let maximum = integer(options.result_bytes, 1 << 20)?;
    let query = path(&request.query, maximum)?;
    let workspace = document(request.workspace, maximum)?;
    let documents = request
        .documents
        .into_iter()
        .map(|source| document(source, maximum))
        .collect::<Result<Vec<_>>>()?;
    let companions = request
        .companions
        .into_iter()
        .map(|source| companion(source, maximum))
        .collect::<Result<Vec<_>>>()?;
    let session = session::Session::new_with_manuals_page_inputs_and_package_baselines(
        workspace,
        documents,
        [],
        companions,
        [],
        session::Limits {
            documents: integer(options.selection_entries, defaults.documents)?,
            identifier_bytes: integer(options.identifier_bytes, defaults.identifier_bytes)?,
            source_bytes: integer(options.source_bytes, defaults.source_bytes)?,
            analysis: defaults.analysis,
        },
    )
    .map_err(|error| classified(error.code(), maximum))?;
    if let Err(error) = session.analysis() {
        return Err(classified(error.code(), maximum));
    }
    let ownership = session.page_package(query.as_str()).map_err(|error| {
        classified(
            match error {
                authoring::NavigationError::Limit => "authoring_navigation_limit",
                authoring::NavigationError::InvalidSource => "authoring_source",
                authoring::NavigationError::Unavailable => "authoring_unavailable",
                authoring::NavigationError::Ambiguous => "authoring_ambiguous",
            },
            maximum,
        )
    })?;
    encode(&ownership, maximum)
}
