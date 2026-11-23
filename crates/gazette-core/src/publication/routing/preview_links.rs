//! Explicit local projection of ordinary public hyperlinks, never a source locator.

use std::collections::BTreeMap;

use super::{InvalidRoute, PublicBaseUrl, RoutePath};

/// Whether an HTTP(S) hyperlink addresses this machine's loopback interface.
pub fn is_loopback_link(destination: &str) -> bool {
    let parsed = if destination.starts_with("//") {
        url::Url::parse(&format!("http:{destination}"))
    } else {
        url::Url::parse(destination)
    };
    parsed.is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && match url.host() {
                Some(url::Host::Ipv4(host)) => host.is_loopback(),
                Some(url::Host::Ipv6(host)) => host.is_loopback(),
                Some(url::Host::Domain(host)) => {
                    let host = host.trim_end_matches('.');
                    host == "localhost" || host.ends_with(".localhost")
                }
                None => false,
            }
    })
}

impl PublicBaseUrl {
    /// Project a hyperlink under this base to another base, preserving its suffix.
    /// Unrelated destinations return `None`; no target existence is inferred.
    ///
    /// # Errors
    /// Refuses unsafe matched paths or a changed mount prefix.
    pub fn project_link(
        &self,
        destination: &str,
        target: &Self,
    ) -> Result<Option<String>, InvalidRoute> {
        let from = url::Url::parse(self.as_str()).map_err(|_| InvalidRoute)?;
        let to = url::Url::parse(target.as_str()).map_err(|_| InvalidRoute)?;
        if from.path() != to.path() {
            return Err(InvalidRoute);
        }
        let rest = destination.strip_prefix(self.as_str()).or_else(|| {
            if from.path() != "/" {
                return None;
            }
            let rest = destination.strip_prefix(self.as_str().strip_suffix('/')?)?;
            (rest.is_empty() || rest.starts_with(['?', '#'])).then_some(rest)
        });
        let Some(rest) = rest else { return Ok(None) };
        let end = rest.find(['?', '#']).unwrap_or(rest.len());
        let _: RoutePath = format!("/{}", &rest[..end]).parse()?;
        if destination.chars().any(char::is_control) {
            return Err(InvalidRoute);
        }
        Ok(Some(format!("{}{rest}", target.as_str())))
    }
}

/// Preview-only public-base to literal-loopback-base bindings.
///
/// Paths and their mount prefixes stay unchanged. This supplies no listener,
/// target existence, native acquisition or permission to publish the result.
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(try_from = "BTreeMap<String, PublicBaseUrl>")]
pub struct PreviewLinkBases(BTreeMap<String, PublicBaseUrl>);

impl TryFrom<BTreeMap<String, PublicBaseUrl>> for PreviewLinkBases {
    type Error = InvalidRoute;

    fn try_from(bindings: BTreeMap<String, PublicBaseUrl>) -> Result<Self, Self::Error> {
        if bindings.is_empty() || bindings.len() > 128 {
            return Err(InvalidRoute);
        }
        let mut bytes = 0usize;
        for (source, target) in &bindings {
            let public: PublicBaseUrl = source.parse()?;
            let from = url::Url::parse(source).map_err(|_| InvalidRoute)?;
            let to = url::Url::parse(target.as_str()).map_err(|_| InvalidRoute)?;
            bytes = bytes
                .checked_add(source.len() + target.as_str().len())
                .ok_or(InvalidRoute)?;
            if public.as_str() != source
                || bytes > 65536
                || to.scheme() != "http"
                || to.path() != from.path()
                || to.port_or_known_default() == Some(0)
                || !match to.host() {
                    Some(url::Host::Ipv4(host)) => host.is_loopback(),
                    Some(url::Host::Ipv6(host)) => host.is_loopback(),
                    _ => false,
                }
            {
                return Err(InvalidRoute);
            }
        }
        Ok(Self(bindings))
    }
}

impl PreviewLinkBases {
    /// Project a matching hyperlink, preserving its literal query and fragment.
    /// Unmatched addresses and non-URL text remain untouched by the caller.
    ///
    /// # Errors
    /// Rejects unsafe path spelling under a matched base instead of normalizing
    /// traversal, encoded separators or malformed escapes into another target.
    pub fn project(&self, destination: &str) -> Result<Option<String>, InvalidRoute> {
        let mut selected: Option<(usize, String)> = None;
        for (source, target) in &self.0 {
            let public: PublicBaseUrl = source.parse()?;
            if let Some(projected) = public.project_link(destination, target)?
                && selected
                    .as_ref()
                    .is_none_or(|(length, _)| source.len() > *length)
            {
                selected = Some((source.len(), projected));
            }
        }
        Ok(selected.map(|(_, projected)| projected))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/preview_links.rs"]
mod tests;
