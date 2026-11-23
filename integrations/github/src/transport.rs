use gazette_core::http::{Options, Request};
use gazette_ports::http::Reader;

use crate::{Error, PagesBuild, PagesObservation, pages};

/// Read one exact public Pages build through an independently composed HTTP reader.
/// No build, credential acquisition, retry or publication effect is requested.
///
/// # Errors
/// Refuses permissions, selection, bounds, transport failure or unbound provider JSON.
pub fn observe_pages_build(
    reader: &impl Reader,
    selection: &PagesBuild,
    options: Options,
) -> Result<PagesObservation, Error> {
    options.validate()?;
    let endpoint = selection.endpoint()?;
    let response = reader.get(Request {
        url: &endpoint,
        headers: &[
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2026-03-10"),
            ("Cache-Control", "no-cache"),
        ],
        options,
    })?;
    if response.body.len() > options.response_bytes {
        return Err(Error::Limit);
    }
    if !(100..=599).contains(&response.status) {
        return Err(Error::Response);
    }
    pages::decode(selection, endpoint, response.status, &response.body)
}
