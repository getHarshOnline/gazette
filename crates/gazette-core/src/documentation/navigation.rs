//! Explicit generated-reference page order; source identity and page bytes stay separate.

use super::Error;
use crate::source::EntryPath;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};

/// One page, directory selection or named group within a complete generated Markdown directory.
/// Coordinates are extensionless routes relative to that selected directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReferencePage {
    /// One exact selected page; its source body supplies its title.
    Page {
        /// Confined extensionless page route relative to the selected artifact directory.
        path: EntryPath,
    },
    /// All pages beneath a directory, grouped by their relative directory names.
    Directory {
        /// Confined directory prefix relative to the selected artifact directory.
        path: EntryPath,
    },
    /// An explicitly named navigation group; no identity is inferred from its label.
    Group {
        /// Nonempty navigation label.
        title: String,
        /// Complete ordered children.
        pages: Vec<ReferencePage>,
    },
}

impl<'de> Deserialize<'de> for ReferencePage {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        use serde::de::{Error as _, MapAccess, Visitor};

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Kind,
            Path,
            Title,
            Pages,
        }
        struct Named;
        impl<'de> Visitor<'de> for Named {
            type Value = ReferencePage;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("named reference page, directory or group fields")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut kind, mut path, mut title, mut pages) = (None, None, None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Kind => {
                            if kind.is_some() {
                                return Err(A::Error::duplicate_field("kind"));
                            }
                            kind = Some(map.next_value::<String>()?);
                        }
                        Field::Path => {
                            if path.is_some() {
                                return Err(A::Error::duplicate_field("path"));
                            }
                            path = Some(map.next_value::<EntryPath>()?);
                        }
                        Field::Title => {
                            if title.is_some() {
                                return Err(A::Error::duplicate_field("title"));
                            }
                            title = Some(map.next_value::<String>()?);
                        }
                        Field::Pages => {
                            if pages.is_some() {
                                return Err(A::Error::duplicate_field("pages"));
                            }
                            pages = Some(map.next_value::<Vec<ReferencePage>>()?);
                        }
                    }
                }
                match kind
                    .ok_or_else(|| A::Error::missing_field("kind"))?
                    .as_str()
                {
                    kind @ ("page" | "directory") => {
                        if title.is_some() || pages.is_some() {
                            return Err(A::Error::custom(
                                "page and directory accept only kind and path",
                            ));
                        }
                        let path = path.ok_or_else(|| A::Error::missing_field("path"))?;
                        Ok(if kind == "page" {
                            ReferencePage::Page { path }
                        } else {
                            ReferencePage::Directory { path }
                        })
                    }
                    "group" => {
                        if path.is_some() {
                            return Err(A::Error::custom(
                                "group accepts only kind, title and pages",
                            ));
                        }
                        Ok(ReferencePage::Group {
                            title: title.ok_or_else(|| A::Error::missing_field("title"))?,
                            pages: pages.ok_or_else(|| A::Error::missing_field("pages"))?,
                        })
                    }
                    value => Err(A::Error::unknown_variant(
                        value,
                        &["page", "directory", "group"],
                    )),
                }
            }
        }
        // The canonical node enforces named maps for direct SDK/JSON and every
        // recursive child; compiler-owned named-table wrappers are unnecessary.
        decoder.deserialize_map(Named)
    }
}

pub(super) fn validate(pages: &[ReferencePage], context: &str) -> Result<(), Error> {
    fn walk<'a>(
        pages: &'a [ReferencePage],
        depth: usize,
        seen: &mut BTreeSet<(bool, &'a str)>,
        context: &str,
    ) -> Result<(), Error> {
        let invalid = |reason| Error::Invalid {
            context: context.to_owned(),
            reason,
        };
        if pages.is_empty() {
            return Err(invalid("explicit reference navigation or group is empty"));
        }
        for page in pages {
            match page {
                ReferencePage::Page { path } | ReferencePage::Directory { path } => {
                    if !path.as_str().split('/').all(|part| {
                        !part.is_empty()
                            && part
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                    }) {
                        return Err(invalid(
                            "expected an extensionless Markdown-relative page or directory path",
                        ));
                    }
                    if !seen.insert((
                        matches!(page, ReferencePage::Directory { .. }),
                        path.as_str(),
                    )) {
                        return Err(invalid(
                            "duplicate explicit reference page or directory selection",
                        ));
                    }
                }
                ReferencePage::Group { title, pages } => {
                    if title.trim().is_empty() || title.chars().any(char::is_control) {
                        return Err(invalid("empty or control-containing reference group label"));
                    }
                    // Engine traversal profile for direct SDK values, not a provider guarantee.
                    if depth >= 64 {
                        return Err(invalid("reference navigation exceeds 64 nested groups"));
                    }
                    walk(pages, depth + 1, seen, context)?;
                }
            }
        }
        Ok(())
    }
    walk(pages, 0, &mut BTreeSet::new(), context)
}
