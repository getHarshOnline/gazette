//! Bounded prose-language syntax, independent of locale inference or translation identity.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A well-formed BCP 47 tag with normalized subtag casing.
///
/// Parsing reuses `OxiLangTag`, not a Gazette grammar. The 255-byte input ceiling
/// bounds this application value; it is not a BCP 47 maximum. This does not
/// consult the IANA registry, replace deprecated tags, detect prose language,
/// equate translations or select a publication default.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(try_from = "String")]
pub struct LanguageTag(String);

/// Malformed or over-limit language declaration; no inferred language is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidLanguageTag;

impl fmt::Display for InvalidLanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a well-formed BCP 47 language tag of at most 255 bytes")
    }
}

impl std::error::Error for InvalidLanguageTag {}

impl LanguageTag {
    /// Case-normalized spelling; never a display label or language-detection result.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for LanguageTag {
    type Err = InvalidLanguageTag;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() > 255 {
            return Err(InvalidLanguageTag);
        }
        oxilangtag::LanguageTag::parse_and_normalize(value)
            .map(|tag| Self(tag.into_inner()))
            .map_err(|_| InvalidLanguageTag)
    }
}

impl TryFrom<String> for LanguageTag {
    type Error = InvalidLanguageTag;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
