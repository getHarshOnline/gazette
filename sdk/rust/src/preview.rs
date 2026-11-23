//! Direct local preview over the shared application and selected HTTP transport.

use std::{collections::BTreeMap, net::SocketAddr};

pub use gazette_core::preview::{Error, Options, Poll};
pub use gazette_ports::preview::{PreviewReservation, PreviewSession};

use crate::{
    build::{self, AcquiredPublicationSet, RenderedPublicationSet},
    workspace::Key,
};

pub mod runner;
pub mod session;
pub mod watch;

/// Held loopback bindings awaiting one exact complete rendered site set.
///
/// This Gazette handle preserves listener custody without exposing the selected
/// HTTP adapter or granting publication, certification or background-runtime authority.
pub struct Reservation(gazette_adapter_http::Reservation);

/// Ready loopback preview borrowing already rendered immutable output.
///
/// Dropping or closing this handle releases its listeners; it never mutates sources.
pub struct Session<'a>(gazette_adapter_http::Session<'a>);

impl PreviewReservation for Reservation {
    type Session<'a> = Session<'a>;

    fn addresses(&self) -> &BTreeMap<Key, SocketAddr> {
        self.0.addresses()
    }

    fn urls(&self) -> &BTreeMap<Key, gazette_core::publication::routing::PublicBaseUrl> {
        self.0.urls()
    }

    fn attach<'a>(
        self,
        output: BTreeMap<Key, gazette_core::preview::Site<'a>>,
    ) -> Result<Self::Session<'a>, Error> {
        self.0.attach(output).map(Session)
    }
}

impl PreviewSession for Session<'_> {
    type Reservation = Reservation;

    fn addresses(&self) -> &BTreeMap<Key, SocketAddr> {
        self.0.addresses()
    }

    fn urls(&self) -> BTreeMap<Key, String> {
        self.0.urls()
    }

    fn poll(&mut self) -> Result<Poll, Error> {
        self.0.poll()
    }

    fn detach(self) -> Self::Reservation {
        Reservation(self.0.detach())
    }

    fn close(self) {
        self.0.close();
    }
}

/// Reserve every selected loopback socket before local-URL rendering.
///
/// # Errors
/// Refuses incomplete acquisition/bindings, unacknowledged listener effects or
/// native binding failure without returning a reduced set.
pub fn reserve(
    inputs: &AcquiredPublicationSet,
    addresses: BTreeMap<Key, SocketAddr>,
    options: Options,
) -> Result<Reservation, Error> {
    gazette_application::preview::reserve(&gazette_adapter_http::Http, inputs, addresses, options)
        .map(Reservation)
}

/// Render from retained inputs using exactly the held listener URL map. All
/// human/machine/resource projections use the shared model; no source is reread.
///
/// # Errors
/// Preserves complete-set, URL, semantic and render failures. No listener is
/// rebound, no output is served yet, and the caller retains reservation custody.
pub fn render<'a>(
    inputs: &'a AcquiredPublicationSet,
    reservation: &Reservation,
    mut options: build::PublicationSetRenderOptions<'_>,
    limits: build::PublicationSetRenderLimits,
) -> Result<RenderedPublicationSet<'a>, build::PublicationSetError> {
    if options.preview_urls.is_some() {
        return Err(build::PublicationSetError::Selection(
            "preview URLs come only from the held reservation",
        ));
    }
    options.preview_urls = Some(reservation.urls().clone());
    build::render_publications(inputs, options, limits)
}

/// Consume the original reservation and attach its exact complete rendered set.
///
/// # Errors
/// Refuses stale/mismatched local URLs, incomplete output and native attachment
/// failure, releasing every listener without altering captured sources.
pub fn attach<'a>(
    reservation: Reservation,
    rendered: &'a RenderedPublicationSet<'_>,
) -> Result<Session<'a>, Error> {
    gazette_application::preview::attach(reservation.0, rendered).map(Session)
}

/// Open an exact complete rendered site set on explicitly selected loopback sockets.
///
/// Retains borrowed output for the session, never rereads native source or rewrites URLs.
/// All listeners bind before success; drop/close releases them. No coordinator is required.
///
/// # Errors
/// Refuses incomplete/mismatched sets, unacknowledged effects, invalid bounds and
/// native bind failures without exposing a successfully reduced site set.
pub fn bind<'a>(
    rendered: &'a RenderedPublicationSet<'_>,
    addresses: BTreeMap<Key, SocketAddr>,
    options: Options,
) -> Result<Session<'a>, Error> {
    gazette_application::preview::bind(&gazette_adapter_http::Http, rendered, addresses, options)
        .map(Session)
}
