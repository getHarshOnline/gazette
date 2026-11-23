//! Explicit provider-native observations, not Git state, compilation or release settlement.

pub use crate::http::Curl;
pub use gazette_integration_github::{
    Build, BuildState, Error, HttpOutcome, Options, PagesBuild, PagesObservation,
};

/// Read the exact selected public Pages build through the native bounded transport.
/// No build, deployment, credential lookup, retry or journal mutation occurs.
///
/// # Errors
/// Refuses permissions, selection, bounds, transport failure or mismatched response binding.
pub fn observe_pages_build(
    curl: &Curl,
    selection: &PagesBuild,
    options: Options,
) -> Result<PagesObservation, Error> {
    let curl = curl.native();
    gazette_integration_github::observe_pages_build(
        &gazette_adapter_http::Client {
            runner: &gazette_adapter_process::Runner,
            curl: &curl,
        },
        selection,
        options,
    )
}
