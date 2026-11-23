//! Bounded HTTP acquisition and complete-set loopback preview over immutable captures.

pub mod client;
pub use client::{Client, Curl};

use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    net::{SocketAddr, TcpListener},
};

use gazette_core::{
    preview::{Binding, Error, Options, Poll, Site as Output},
    publication::{
        Key,
        routing::{PublicBaseUrl, RoutePath},
    },
    render::MediaType,
    source::{CapturedSource, EntryPath},
};
use gazette_ports::preview::{PreviewReservation, PreviewServer, PreviewSession};

mod encoding;
mod request;
mod response;

/// Native loopback transport; no global runtime or background listener ownership.
pub struct Http;

struct Listener {
    listener: TcpListener,
    address: SocketAddr,
    mount: RoutePath,
}

struct Site<'a> {
    binding: Listener,
    output: &'a CapturedSource,
    media_types: &'a BTreeMap<EntryPath, MediaType>,
    missing_page: Vec<u8>,
}

/// Continuously held exact listener set; cannot serve until complete output is attached.
pub struct Reservation {
    listeners: BTreeMap<Key, Listener>,
    addresses: BTreeMap<Key, SocketAddr>,
    urls: BTreeMap<Key, PublicBaseUrl>,
    next: usize,
    options: Options,
}

/// Exact ready listener set borrowing the already rendered immutable output.
/// Drop releases listeners; it never cleans up captured output or native sources.
pub struct Session<'a> {
    sites: Vec<Site<'a>>,
    addresses: BTreeMap<Key, SocketAddr>,
    urls: BTreeMap<Key, PublicBaseUrl>,
    next: usize,
    options: Options,
}

impl PreviewServer for Http {
    type Reservation = Reservation;

    fn reserve(
        &self,
        bindings: BTreeMap<Key, Binding<'_>>,
        options: Options,
    ) -> Result<Reservation, Error> {
        options.validate()?;
        if bindings.is_empty() || bindings.len() > options.max_sites {
            return Err(Error::Refused(
                "preview requires one nonempty exact bounded site set",
            ));
        }
        let mut fixed = BTreeSet::new();
        for binding in bindings.values() {
            let address = binding.address;
            if !address.ip().is_loopback() || (address.port() != 0 && !fixed.insert(address)) {
                return Err(Error::Refused(
                    "preview requires distinct explicit loopback bindings",
                ));
            }
        }
        let mut listeners = BTreeMap::new();
        let mut bound = BTreeMap::new();
        let mut urls = BTreeMap::new();
        for (key, binding) in bindings {
            let mount = mount(binding.base_url)?;
            let listener = TcpListener::bind(binding.address).map_err(Error::Io)?;
            listener.set_nonblocking(true).map_err(Error::Io)?;
            let address = listener.local_addr().map_err(Error::Io)?;
            let local = format!("http://{address}{}", mount.as_str())
                .parse()
                .map_err(|_| Error::Refused("invalid local preview base"))?;
            urls.insert(key.clone(), local);
            bound.insert(key.clone(), address);
            listeners.insert(
                key,
                Listener {
                    listener,
                    address,
                    mount,
                },
            );
        }
        Ok(Reservation {
            listeners,
            addresses: bound,
            urls,
            next: 0,
            options,
        })
    }
}

impl PreviewReservation for Reservation {
    type Session<'a> = Session<'a>;

    fn addresses(&self) -> &BTreeMap<Key, SocketAddr> {
        &self.addresses
    }

    fn urls(&self) -> &BTreeMap<Key, PublicBaseUrl> {
        &self.urls
    }

    fn attach<'a>(self, output: BTreeMap<Key, Output<'a>>) -> Result<Session<'a>, Error> {
        if self.listeners.is_empty() || !self.listeners.keys().eq(output.keys()) {
            return Err(Error::Refused(
                "preview attachment requires the exact reserved site set",
            ));
        }
        let mut sites = Vec::with_capacity(output.len());
        for ((_, binding), (_, output)) in self.listeners.into_iter().zip(output) {
            if binding.mount != mount(output.base_url)? {
                return Err(Error::Refused(
                    "preview attachment mount differs from its reservation",
                ));
            }
            let missing_page =
                response::missing_page(&binding.mount, output.output, output.search_page);
            sites.push(Site {
                binding,
                output: output.output,
                media_types: output.media_types,
                missing_page,
            });
        }
        Ok(Session {
            sites,
            addresses: self.addresses,
            urls: self.urls,
            next: self.next,
            options: self.options,
        })
    }
}

fn mount(base: &PublicBaseUrl) -> Result<RoutePath, Error> {
    url::Url::parse(base.as_str())
        .map_err(|_| Error::Refused("invalid preview mount"))?
        .path()
        .parse()
        .map_err(|_| Error::Refused("invalid preview mount"))
}

impl PreviewSession for Session<'_> {
    type Reservation = Reservation;

    fn addresses(&self) -> &BTreeMap<Key, SocketAddr> {
        &self.addresses
    }

    fn urls(&self) -> BTreeMap<Key, String> {
        self.urls
            .iter()
            .map(|(key, url)| (key.clone(), url.as_str().to_owned()))
            .collect()
    }

    fn poll(&mut self) -> Result<Poll, Error> {
        if self.sites.is_empty() {
            return Err(Error::Refused("preview session has terminated"));
        }
        for _ in 0..self.sites.len() {
            let index = self.next;
            self.next = (index + 1) % self.sites.len();
            let site = &self.sites[index];
            match site.binding.listener.accept() {
                Ok((mut stream, peer)) => {
                    if !peer.ip().is_loopback() || stream.set_nonblocking(false).is_err() {
                        return Ok(Poll::Disconnected);
                    }
                    return Ok(
                        if request::serve(
                            &mut stream,
                            site.binding.address,
                            site.output,
                            &site.binding.mount,
                            site.media_types,
                            &site.missing_page,
                            self.options,
                        )
                        .is_ok()
                        {
                            Poll::Responded
                        } else {
                            Poll::Disconnected
                        },
                    );
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                Err(error) => {
                    self.sites.clear();
                    self.addresses.clear();
                    self.urls.clear();
                    return Err(Error::Io(error));
                }
            }
        }
        Ok(Poll::Idle)
    }

    fn detach(self) -> Reservation {
        // Attachment creates sites in the same key order as the retained address
        // map; terminal listener failure clears both together.
        let listeners = self
            .addresses
            .keys()
            .cloned()
            .zip(self.sites.into_iter().map(|site| site.binding))
            .collect();
        Reservation {
            listeners,
            addresses: self.addresses,
            urls: self.urls,
            next: self.next,
            options: self.options,
        }
    }

    fn close(self) {}
}
