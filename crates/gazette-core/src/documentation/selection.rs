//! Explicit native artifact scope, independent of language, generator and hosting target.

use super::Error;
use crate::source::{Content, EntryPath, TreeInput};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Select a file or directory within one retained capture, without filename inference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Selection {
    /// One exact regular file; companions remain available in the enclosing capture.
    File {
        /// Literal capture-relative path.
        path: EntryPath,
    },
    /// One exact directory, including its complete descendant set.
    Directory {
        /// Literal capture-relative directory; absence explicitly selects the capture root.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<EntryPath>,
    },
}

impl<'de> Deserialize<'de> for Selection {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        use serde::de::{Error as _, MapAccess, Visitor};

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Kind,
            Path,
        }
        struct Named;
        impl<'de> Visitor<'de> for Named {
            type Value = Selection;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("named documentation selection fields")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Selection, A::Error> {
                let (mut kind, mut path) = (None, None);
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
                            path = Some(map.next_value::<Option<EntryPath>>()?);
                        }
                    }
                }
                match kind
                    .ok_or_else(|| A::Error::missing_field("kind"))?
                    .as_str()
                {
                    "file" => Ok(Selection::File {
                        path: path
                            .flatten()
                            .ok_or_else(|| A::Error::missing_field("path"))?,
                    }),
                    "directory" => Ok(Selection::Directory {
                        path: path.flatten(),
                    }),
                    value => Err(A::Error::unknown_variant(value, &["file", "directory"])),
                }
            }
        }
        // Match the named-object schema at every consumer, including nested recipes.
        decoder.deserialize_map(Named)
    }
}

impl Selection {
    /// Selected relative path; only an explicit capture-root directory has no path.
    pub fn path(&self) -> Option<&EntryPath> {
        match self {
            Self::File { path } => Some(path),
            Self::Directory { path } => path.as_ref(),
        }
    }

    /// Validate the declared kind against the retained tree and borrow its native metadata.
    /// Directory content is bound by this selection and the enclosing capture identity;
    /// no file digest or generator compatibility is invented for it.
    ///
    /// # Errors
    /// Rejects an absent path or a file/directory mismatch, without selecting another entry.
    pub fn content(&self, tree: &TreeInput<'_>) -> Result<Content, Error> {
        let Some(path) = self.path() else {
            return Ok(Content::Directory);
        };
        let entries = tree.inventory().entries();
        let content = entries
            .binary_search_by(|entry| entry.path.cmp(path))
            .ok()
            .map(|index| entries[index].content);
        match (self, content) {
            (Self::File { .. }, Some(content @ Content::File { .. }))
            | (Self::Directory { .. }, Some(content @ Content::Directory)) => Ok(content),
            _ => Err(Error::Invalid {
                context: self.to_string(),
                reason: "selected documentation path is absent or has the wrong file/directory kind",
            }),
        }
    }
}

impl fmt::Display for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { path } => write!(f, "file {path}"),
            Self::Directory { path: Some(path) } => write!(f, "directory {path}"),
            Self::Directory { path: None } => f.write_str("capture-root directory"),
        }
    }
}
