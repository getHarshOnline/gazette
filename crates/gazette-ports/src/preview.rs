//! Loopback transport over borrowed immutable output; no source discovery or rendering.

use std::{collections::BTreeMap, net::SocketAddr};

use gazette_core::{
    preview::{Binding, Error, Options, Poll, Site},
    publication::{Key, routing::PublicBaseUrl},
};

/// Bind the entire selected set before exposing readiness; failed startup releases all sockets.
pub trait PreviewServer {
    /// Complete native listener ownership, without borrowing future output.
    type Reservation: PreviewReservation;

    /// Reserve the exact socket set before rendering; dropping it releases all sockets.
    ///
    /// # Errors
    /// Refuses permission, bounds, non-loopback/duplicate bindings or any native
    /// failure. No successfully reduced set escapes.
    fn reserve(
        &self,
        bindings: BTreeMap<Key, Binding<'_>>,
        options: Options,
    ) -> Result<Self::Reservation, Error>;

    /// Retain exact output and open only explicitly selected loopback listeners.
    ///
    /// # Errors
    /// Refuses permission, bounds, non-loopback/duplicate bindings, incomplete sets
    /// or native failures. No successful subset or background worker escapes.
    fn bind<'a>(
        &self,
        output: BTreeMap<Key, Site<'a>>,
        addresses: BTreeMap<Key, SocketAddr>,
        options: Options,
    ) -> Result<<Self::Reservation as PreviewReservation>::Session<'a>, Error> {
        if !output.keys().eq(addresses.keys()) {
            return Err(Error::Refused(
                "preview requires the exact output and binding set",
            ));
        }
        let bindings = output
            .iter()
            .zip(addresses)
            .map(|((key, site), (_, address))| {
                (
                    key.clone(),
                    Binding {
                        address,
                        base_url: site.base_url,
                    },
                )
            })
            .collect();
        self.reserve(bindings, options)?.attach(output)
    }
}

/// Reserved exact sockets and local URL observations, with no serving API yet.
pub trait PreviewReservation: Sized {
    /// Output is borrowed only after consuming the reservation.
    type Session<'a>: PreviewSession<Reservation = Self>;

    /// Exact socket addresses retained continuously through attachment.
    fn addresses(&self) -> &BTreeMap<Key, SocketAddr>;

    /// Typed local bases derived from actual sockets and configured mount paths.
    fn urls(&self) -> &BTreeMap<Key, PublicBaseUrl>;

    /// Attach the complete output set to these same sockets without reopening them.
    ///
    /// # Errors
    /// Rejects empty/terminated reservations, mismatched site keys or mount paths;
    /// failure drops the entire reservation.
    fn attach<'a>(self, output: BTreeMap<Key, Site<'a>>) -> Result<Self::Session<'a>, Error>;
}

/// A ready exact set. Drop or close releases every listener without deleting source/state.
pub trait PreviewSession {
    /// Listener ownership after releasing the current output borrow.
    type Reservation: PreviewReservation;

    /// Actual bound addresses, including native ephemeral ports, in exact site-key order.
    fn addresses(&self) -> &BTreeMap<Key, SocketAddr>;

    /// Local entry URLs projected from the actual sockets and configured mount paths.
    /// These observations never replace the publication's canonical URLs.
    fn urls(&self) -> BTreeMap<Key, String>;

    /// Accept at most one client and perform bounded HTTP work without native file reads.
    ///
    /// # Errors
    /// Listener failure terminates the session and releases all listeners; a client
    /// timeout/disconnect is reported separately and cannot claim successful delivery.
    fn poll(&mut self) -> Result<Poll, Error>;

    /// Release borrowed output while preserving the exact sockets, URLs and limits.
    /// A terminated session yields an empty reservation that cannot be attached.
    fn detach(self) -> Self::Reservation;

    /// Release this session's sockets. No persistent URL override or publication state exists.
    fn close(self);
}
