//! Authored citation values; links resolve later against the exact source owner.

use serde::{Deserialize, Serialize};

/// Optional citation text and downloads, independent of article classification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    /// Plain citation text, never trusted HTML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Additional authored formats in declaration order, without generated styles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formats: Vec<CitationFormat>,
    /// Source-relative retained BibTeX download or explicit public destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bibtex: Option<String>,
    /// Source-relative retained PDF download or explicit public destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<String>,
}

/// One explicitly supplied format, not a formatting algorithm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitationFormat {
    /// Authored reader-facing label.
    pub label: String,
    /// Plain text preserving authored punctuation and line breaks.
    pub text: String,
}

impl Citation {
    pub(super) fn valid(&self) -> bool {
        let plain = |s: &str| {
            !s.trim().is_empty()
                && !s
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        };
        let mut labels = std::collections::BTreeSet::new();
        (self.text.is_some()
            || !self.formats.is_empty()
            || self.bibtex.is_some()
            || self.pdf.is_some())
            && self.text.as_deref().is_none_or(plain)
            && self.formats.iter().all(|f| {
                plain(&f.label)
                    && plain(&f.text)
                    && f.label.trim() == f.label
                    && !f.label.chars().any(char::is_control)
                    && labels.insert(&f.label)
            })
            && [&self.bibtex, &self.pdf]
                .into_iter()
                .flatten()
                .all(|s| !s.is_empty() && s.trim() == s && !s.chars().any(char::is_control))
    }

    pub(super) fn texts(&self) -> impl Iterator<Item = &str> {
        self.text
            .iter()
            .map(String::as_str)
            .chain(
                self.formats
                    .iter()
                    .flat_map(|f| [f.label.as_str(), f.text.as_str()]),
            )
            .chain(self.bibtex.iter().chain(&self.pdf).map(String::as_str))
    }
}
