//! Reads back what GitHub Pages reports about a build, so a release can be compared with
//! GitHub's own account of it.
//!
//! This integration builds on `gazette-core` and the capability traits in
//! `gazette-ports`; `gazette-sdk` builds on it. It lives outside compilation, Git ref
//! state and release settlement. It reads an explicitly selected Pages build. It does
//! not request a build, infer the latest build, change Pages configuration or probe a
//! served site.

mod hosting;
mod pages;
pub use hosting::HostFiles;
mod transport;

pub use gazette_core::http::Options;
pub use pages::{Build, BuildState, HttpOutcome, PagesBuild, PagesObservation};
pub use transport::observe_pages_build;

/// Stable refusal without provider body, local path or credentials in diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    /// Every native read, execution and network permission is required.
    Permission,
    /// Invalid or unsupported destination, exact build identity or executable selection.
    Selection,
    /// A requested or observed bound was exceeded.
    Limit,
    /// Native acquisition failed; no partial body is interpreted.
    Transport {
        /// Existing process failure class.
        failure: gazette_core::process::Failure,
        /// Direct-child custody, not absence of external effects.
        cleanup: gazette_core::process::Cleanup,
    },
    /// The selected transport exited unsuccessfully or omitted complete HTTP evidence.
    Response,
    /// Successful HTTP response was malformed, ambiguous or belonged to another build.
    Binding,
}

impl Error {
    /// Stable code shared by all application interfaces.
    pub fn code(self) -> &'static str {
        match self {
            Self::Permission => "github_permission",
            Self::Selection => "github_selection",
            Self::Limit => "github_limit",
            Self::Transport { .. } => "github_transport",
            Self::Response => "github_response",
            Self::Binding => "github_binding",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Error {}

impl From<gazette_core::http::Error> for Error {
    fn from(error: gazette_core::http::Error) -> Self {
        use gazette_core::http::Error as Http;
        match error {
            Http::Permission => Self::Permission,
            Http::Selection => Self::Selection,
            Http::Limit => Self::Limit,
            Http::Transport { failure, cleanup } => Self::Transport { failure, cleanup },
            Http::Response => Self::Response,
        }
    }
}
