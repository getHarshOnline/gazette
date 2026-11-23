//! Explicit licence reference, without licence inference, fetching or approval.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use url::Url;

/// Exact ASCII HTTP(S) URI naming the declared content licence.
/// Parsing checks an address, not the referenced terms or permission to publish.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String")]
pub struct ContentLicense(String);

/// Missing absolute address or an unsafe/ambiguous URI spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentLicenseError;

impl fmt::Display for ContentLicenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected an explicit ASCII HTTP(S) content-licence URI without credentials, whitespace or malformed escapes")
    }
}
impl std::error::Error for ContentLicenseError {}

impl FromStr for ContentLicense {
    type Err = ContentLicenseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.iter().enumerate().any(|(index, byte)| {
            !byte.is_ascii_graphic()
                || b"\\\"<>`{}|^".contains(byte)
                || (*byte == b'%'
                    && !bytes
                        .get(index + 1..index + 3)
                        .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit)))
        }) {
            return Err(ContentLicenseError);
        }
        let url = Url::parse(value).map_err(|_| ContentLicenseError)?;
        if !matches!(url.scheme(), "http" | "https")
            || !value.split_once("://").is_some_and(|(_, rest)| {
                rest.split(['/', '?', '#'])
                    .next()
                    .is_some_and(|authority| !authority.is_empty() && !authority.contains('@'))
            })
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ContentLicenseError);
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for ContentLicense {
    type Error = ContentLicenseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ContentLicense> for String {
    fn from(value: ContentLicense) -> Self {
        value.0
    }
}

impl ContentLicense {
    /// Original explicit URI, never a normalized or acquired replacement.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentLicense {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
