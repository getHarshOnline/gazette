//! Complete-set local preview; serving locations never mutate publication configuration.

use std::{collections::BTreeMap, net::SocketAddr};

use gazette_core::{
    preview::{Binding, Error, Options, Site},
    publication::Key,
};
use gazette_ports::preview::{PreviewReservation, PreviewServer};

use crate::build::{AcquiredPublicationSet, RenderedPublicationSet};

pub mod watch;

/// Hold the exact site's loopback listeners before developer rendering. The
/// acquisition remains immutable; socket-assigned URLs are ephemeral evidence.
///
/// # Errors
/// Refuses incomplete source acquisition, missing configured bases, set mismatch
/// and native binding failures. No partial reservation is returned.
pub fn reserve<P: PreviewServer>(
    server: &P,
    inputs: &AcquiredPublicationSet,
    addresses: BTreeMap<Key, SocketAddr>,
    options: Options,
) -> Result<P::Reservation, Error> {
    options.validate()?;
    if !inputs.sites().keys().eq(addresses.keys()) || inputs.sites().values().any(Result::is_err) {
        return Err(Error::Refused(
            "preview requires the exact complete acquired site set",
        ));
    }
    let bindings = addresses
        .into_iter()
        .map(|(key, address)| {
            let base_url = inputs
                .compilation()
                .workspace
                .public_base_url(&key)
                .ok_or(Error::Refused("preview site has no configured public base"))?;
            Ok((key, Binding { address, base_url }))
        })
        .collect::<Result<_, Error>>()?;
    server.reserve(bindings, options)
}

/// Attach one complete render to the same listeners held during compilation.
/// Local-mode output must name exactly their socket-derived serving URLs.
///
/// # Errors
/// Drops every reserved listener on incomplete output, mismatched URL intent or
/// attachment failure. This operation never retries rendering or binds again.
pub fn attach<'a, R: PreviewReservation>(
    reservation: R,
    rendered: &'a RenderedPublicationSet<'_>,
) -> Result<R::Session<'a>, Error> {
    if !rendered.is_complete()
        || !rendered.sites().keys().eq(reservation.addresses().keys())
        || rendered
            .preview_urls()
            .is_some_and(|urls| urls != reservation.urls())
    {
        return Err(Error::Refused(
            "preview output does not match the exact held listener set",
        ));
    }
    reservation.attach(output(rendered)?)
}

/// Bind every successful requested site to its explicit loopback address.
///
/// The rendered set supplies immutable bytes, not filesystem paths. Existing
/// canonical and cross-site URLs remain unchanged; this is not local-URL
/// rewriting, watch/rebuild, public admission or artifact certification.
///
/// # Errors
/// Refuses an incomplete set or mismatched bindings before any listener effect,
/// then preserves native all-or-nothing startup and bounded transport failures.
pub fn bind<'a, P: PreviewServer>(
    server: &'a P,
    rendered: &'a RenderedPublicationSet<'_>,
    addresses: BTreeMap<Key, SocketAddr>,
    options: Options,
) -> Result<<P::Reservation as PreviewReservation>::Session<'a>, Error> {
    options.validate()?;
    if !rendered.is_complete() || !rendered.sites().keys().eq(addresses.keys()) {
        return Err(Error::Refused(
            "preview requires the exact complete rendered site set",
        ));
    }
    if rendered.preview_urls().is_some() {
        return Err(Error::Refused(
            "developer-local output requires its original reservation",
        ));
    }
    server.bind(output(rendered)?, addresses, options)
}

fn output<'a>(rendered: &'a RenderedPublicationSet<'_>) -> Result<BTreeMap<Key, Site<'a>>, Error> {
    rendered
        .sites()
        .iter()
        .map(|(key, result)| {
            let rendered = result
                .as_ref()
                .map_err(|_| Error::Refused("preview site has no complete output"))?;
            let base_url = rendered
                .preview_urls()
                .and_then(|urls| urls.get(key))
                .or_else(|| {
                    rendered
                        .inputs()
                        .compilation()
                        .workspace
                        .public_base_url(key)
                })
                .ok_or(Error::Refused("preview site has no configured public base"))?;
            Ok((
                key.clone(),
                Site {
                    output: rendered.site().output(),
                    media_types: rendered.site().media_types(),
                    search_page: rendered.site().search_page(),
                    base_url,
                },
            ))
        })
        .collect()
}
