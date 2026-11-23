//! Logical placement paths, not proof of destination-filesystem confinement.

use std::fmt;
use std::str::FromStr;

use percent_encoding::percent_decode_str;
use serde::Serialize;

use super::Subdirectory;

/// A normalized nonempty placement beneath a site's future content root.
///
/// Input is a URL path, with at most one leading/trailing slash. Safe escapes
/// are decoded once per component. This value preserves case and Unicode;
/// native filesystem aliasing and confinement need a separate preflight.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct TargetPath(String);

/// A placement path has no unambiguous confined relative interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTargetPath;

impl fmt::Display for InvalidTargetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("select a non-root URL path without traversal, empty components, encoded separators, nested escapes, queries or fragments")
    }
}

impl std::error::Error for InvalidTargetPath {}

impl FromStr for TargetPath {
    type Err = InvalidTargetPath;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.contains(['?', '#']) {
            return Err(InvalidTargetPath);
        }
        let value = value.strip_prefix('/').unwrap_or(value);
        let value = value.strip_suffix('/').unwrap_or(value);
        let mut components = Vec::new();
        for raw in value.split('/') {
            let bytes = raw.as_bytes();
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'%'
                    && !bytes
                        .get(index + 1..index + 3)
                        .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
                {
                    return Err(InvalidTargetPath);
                }
            }
            let decoded = percent_decode_str(raw)
                .decode_utf8()
                .map_err(|_| InvalidTargetPath)?;
            if decoded.contains(['/', '%']) {
                return Err(InvalidTargetPath);
            }
            decoded
                .parse::<Subdirectory>()
                .map_err(|_| InvalidTargetPath)?;
            components.push(decoded.into_owned());
        }
        Ok(Self(components.join("/")))
    }
}

impl TargetPath {
    /// Decoded relative destination spelling, not a URL serialization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
