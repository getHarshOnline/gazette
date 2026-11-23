//! Explicit public addresses, separate from object identity and native source paths.

use std::{fmt, str::FromStr};

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::{Deserialize, Serialize, Serializer};
use url::Url;

use crate::source::EntryPath;

mod preview_links;
mod slug;
pub use preview_links::{PreviewLinkBases, is_loopback_link};
pub use slug::RouteSlug;

const SEGMENT: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Site-relative address with an explicit leading slash. A trailing slash means
/// a directory index; otherwise the exact final component is a file, even without
/// an extension. Unicode and safe escapes normalize to one percent-encoded URL.
/// Neither Unicode normalization nor filesystem case folding is performed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String")]
#[schemars(with = "String")]
pub struct RoutePath {
    url: String,
    output: EntryPath,
}

/// A public address is ambiguous, unsupported or unsafe to map to an output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidRoute;

impl fmt::Display for InvalidRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("use an explicit confined route or HTTP(S) base URL; exclude credentials, traversal, repeated separators, queries, fragments, controls and nested escapes")
    }
}
impl std::error::Error for InvalidRoute {}

impl FromStr for RoutePath {
    type Err = InvalidRoute;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let relative = value.strip_prefix('/').ok_or(InvalidRoute)?;
        let directory = value.ends_with('/');
        let relative = relative.strip_suffix('/').unwrap_or(relative);
        let mut encoded = Vec::new();
        let mut decoded = Vec::new();
        if value != "/" {
            for raw in relative.split('/') {
                let bytes = raw.as_bytes();
                for (index, byte) in bytes.iter().enumerate() {
                    if *byte == b'%'
                        && !bytes
                            .get(index + 1..index + 3)
                            .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
                    {
                        return Err(InvalidRoute);
                    }
                }
                let part = percent_decode_str(raw)
                    .decode_utf8()
                    .map_err(|_| InvalidRoute)?;
                if matches!(part.as_ref(), "" | "." | "..")
                    || part
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '/' | '\\' | '%' | '?' | '#'))
                {
                    return Err(InvalidRoute);
                }
                encoded.push(utf8_percent_encode(&part, SEGMENT).to_string());
                decoded.push(part.into_owned());
            }
        }
        let mut url = format!("/{}", encoded.join("/"));
        if directory && url != "/" {
            url.push('/');
        }
        if directory {
            decoded.push("index.html".into());
        }
        let output = decoded.join("/").parse().map_err(|_| InvalidRoute)?;
        Ok(Self { url, output })
    }
}
impl TryFrom<String> for RoutePath {
    type Error = InvalidRoute;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}
impl Serialize for RoutePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl RoutePath {
    /// Normalized site-relative URL spelling, not a native path.
    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// Literal decoded file beneath the site's output root. The deployment base
    /// prefix is not repeated here. Native writes still require confinement checks.
    pub fn output(&self) -> &EntryPath {
        &self.output
    }

    /// Append literal entry components beneath a directory route, keeping a trailing slash.
    /// An absent entry preserves this route. Entry names are encoded before route parsing;
    /// percent sequences in native names never become URL escapes or change their identity.
    ///
    /// # Errors
    /// Rejects a file route or entry names forbidden by the existing route grammar,
    /// including literal percent signs, queries, fragments and control characters.
    pub fn join_directory(&self, entry: Option<&EntryPath>) -> Result<Self, InvalidRoute> {
        if !self.url.ends_with('/') {
            return Err(InvalidRoute);
        }
        let Some(entry) = entry else {
            return Ok(self.clone());
        };
        let mut url = self.url.clone();
        for component in entry.as_str().split('/') {
            url.extend(utf8_percent_encode(component, SEGMENT));
            url.push('/');
        }
        url.parse()
    }

    /// Place a literal retained file beneath this directory address. The same
    /// codec owns its encoded public URL and decoded output path; no index is added.
    ///
    /// # Errors
    /// Rejects a file base or entry components forbidden by the route grammar.
    pub fn join_file(&self, entry: &EntryPath) -> Result<Self, InvalidRoute> {
        let directory = self.join_directory(Some(entry))?;
        directory.as_str().trim_end_matches('/').parse()
    }
}

/// Explicit HTTP(S) publication base including any deployment prefix, ending in
/// `/`. URL parsing normalizes host/scheme/default port; path rules use `RoutePath`.
/// This is configuration, never proof of reachability, ownership or permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String")]
pub struct PublicBaseUrl(String);

impl FromStr for PublicBaseUrl {
    type Err = InvalidRoute;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.chars().any(|c| c.is_control() || c.is_whitespace())
            || value.contains(['\\', '?', '#'])
        {
            return Err(InvalidRoute);
        }
        let (_, rest) = value.split_once("://").ok_or(InvalidRoute)?;
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, "/".to_owned()), |(host, path)| {
                (host, format!("/{path}"))
            });
        if authority.is_empty() || authority.contains('@') || !path.ends_with('/') {
            return Err(InvalidRoute);
        }
        let path: RoutePath = path.parse()?;
        let mut parsed = Url::parse(value).map_err(|_| InvalidRoute)?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(InvalidRoute);
        }
        parsed.set_path(path.as_str());
        Ok(Self(parsed.into()))
    }
}
impl TryFrom<String> for PublicBaseUrl {
    type Error = InvalidRoute;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}
impl PublicBaseUrl {
    /// DNS host of this address; IP literals have no domain binding.
    pub fn domain(&self) -> Option<String> {
        Url::parse(&self.0).ok()?.domain().map(str::to_owned)
    }

    /// Normalized public base, including its trailing slash and optional prefix.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve beneath the configured base without dropping its deployment prefix.
    /// This pure operation neither checks a remote nor grants publication authority.
    pub fn resolve(&self, path: &RoutePath) -> String {
        format!("{}{}", self.0.trim_end_matches('/'), path.as_str())
    }

    /// Public spelling of the actual output file, for collision checks across
    /// independently configured site mounts. This is not a canonical redirect.
    pub fn resolve_output(&self, path: &RoutePath) -> String {
        let mut value = self.resolve(path);
        if path.as_str().ends_with('/') {
            value.push_str("index.html");
        }
        value
    }

    /// Resolve a resource URL from its emitted file into this site's exact output namespace.
    /// URL joining uses browser-style path resolution; no native path or remote is read.
    ///
    /// # Errors
    /// Rejects credentials, queries/fragments, malformed paths, other origins and references
    /// outside the configured deployment prefix. Existence remains an inventory check.
    pub fn resource_target(
        &self,
        origin: &RoutePath,
        reference: &str,
    ) -> Result<RoutePath, InvalidRoute> {
        if reference.is_empty()
            || reference
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || reference.contains(['\\', '?', '#'])
        {
            return Err(InvalidRoute);
        }
        let base = Url::parse(&self.0).map_err(|_| InvalidRoute)?;
        let origin = Url::parse(&self.resolve_output(origin)).map_err(|_| InvalidRoute)?;
        let target = origin.join(reference).map_err(|_| InvalidRoute)?;
        if target.origin() != base.origin()
            || !target.username().is_empty()
            || target.password().is_some()
            || target.query().is_some()
            || target.fragment().is_some()
        {
            return Err(InvalidRoute);
        }
        let relative = target
            .path()
            .strip_prefix(base.path())
            .ok_or(InvalidRoute)?;
        if relative.is_empty() || relative.ends_with('/') {
            return Err(InvalidRoute);
        }
        format!("/{relative}").parse()
    }
}

/// Authored route intent. Aliases are same-site redirect addresses, not symbolic
/// lookup names, external canonical overrides or historical DB imports. Foreign
/// redirect presentations name their serving site explicitly and keep one home.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// Exact primary address; never inferred from title, identity or source path.
    pub path: RoutePath,
    /// Original content object when this address serves a syndicated copy.
    /// The referenced object must have its own route and no canonical override.
    /// This declaration confers no rights, attribution or publication permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<super::Key>,
    /// Redirect addresses targeting this object's resolved primary URL directly.
    #[serde(default)]
    pub aliases: Vec<RoutePath>,
    /// Explicit redirect presentations on other sites. They retain this object's
    /// one canonical home; they neither duplicate its body nor change provenance.
    #[serde(default)]
    pub mirrors: Vec<Mirror>,
}

/// A cross-site redirect address for the enclosing canonical object, not a
/// repository-lineage classification or an independently authored treatment.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    /// Exact workspace site receiving the redirect; its publisher remains independent.
    pub site: super::Key,
    /// Exact address beneath that site's public base, without inferred mount remapping.
    pub path: RoutePath,
}
