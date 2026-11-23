//! Publication selectors are not URLs; one grammar serves all resolved consumers.

use gazette_core::{
    publication::{Key, routing::RoutePath},
    source::Span,
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::Error;

const KEY_ESCAPE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b':')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// An exact retained content identity, independent of its publication address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityReference<'a> {
    /// Complete namespace and local identity; no namespace is inferred.
    pub content: Key,
    /// Original fragment suffix, checked against the target's native anchors.
    pub fragment: &'a str,
}

impl<'a> IdentityReference<'a> {
    /// Parse `id:<namespace:local>#fragment`, decoding the identity exactly once.
    ///
    /// # Errors
    /// Refuses missing namespaces, malformed escapes and invalid keys. Parsing
    /// establishes neither target existence nor publication permission.
    pub fn parse(destination: &'a str, span: Span) -> Result<Self, Error> {
        let failure = |reason| Error::LinkResolution { span, reason };
        let selector = destination
            .strip_prefix("id:")
            .ok_or_else(|| failure("expected an id: content selector"))?;
        let (identity, fragment) = selector
            .find('#')
            .map_or((selector, ""), |index| selector.split_at(index));
        let content = super::decode_reference(identity, span)?
            .parse()
            .map_err(|_| failure("identity selector requires an exact namespace:local key"))?;
        Ok(Self { content, fragment })
    }

    pub(crate) fn destination(content: &Key) -> String {
        format!("id:{}", utf8_percent_encode(content.as_str(), KEY_ESCAPE))
    }
}

/// Native wikilink destination selecting a configured domain and declared route.
/// Parsing establishes syntax only, never target existence or permission to publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcosystemReference<'a> {
    /// Normalized domain selector, independent of any local-preview address.
    pub domain: String,
    /// Explicit publication-relative route, without a deployment prefix.
    pub route: RoutePath,
    /// Optional original fragment suffix; the target's native anchor index validates it.
    pub fragment: &'a str,
}

impl<'a> EcosystemReference<'a> {
    /// Parse the established `site:domain/path/#fragment` destination.
    ///
    /// # Errors
    /// Refuses another wikilink language, credentials, ports, queries, unsafe
    /// route grammar and malformed selectors without fabricating an address.
    pub fn parse(destination: &'a str, span: Span) -> Result<Self, Error> {
        let failure = |reason| Error::LinkResolution { span, reason };
        let selector = destination
            .strip_prefix("site:")
            .ok_or_else(|| failure("expected a site: publication selector"))?;
        if selector
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
            || selector.contains(['\\', '?'])
        {
            return Err(failure("invalid symbolic publication selector"));
        }
        let (domain, path) = selector.split_once('/').ok_or_else(|| {
            failure("symbolic publication selector requires a domain and explicit route")
        })?;
        if domain.is_empty() || domain.contains(['@', ':', '%', '#', '[', ']']) {
            return Err(failure(
                "symbolic publication selector requires a domain without credentials or port",
            ));
        }
        let url::Host::Domain(domain) =
            url::Host::parse(domain).map_err(|_| failure("invalid symbolic publication domain"))?
        else {
            return Err(failure("symbolic publication selector requires a domain"));
        };
        let (path, fragment) = path
            .find('#')
            .map_or((path, ""), |index| path.split_at(index));
        let route = format!("/{path}")
            .parse()
            .map_err(|_| failure("invalid confined route in symbolic publication selector"))?;
        Ok(Self {
            domain,
            route,
            fragment,
        })
    }
}
