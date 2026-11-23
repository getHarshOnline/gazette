//! Explicit delivery coordinates, without provider inference or authority acquisition.

use serde::{Deserialize, Serialize};

/// Selected publication delivery. Absence at the workspace boundary means local-only.
/// Deserialization establishes shape; validation establishes only literal coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum PublicationDestination {
    /// A literal Git remote and branch, not reachability or permission to push.
    Git {
        /// Exact selected remote bytes, never normalized or opened by validation.
        remote: String,
        /// Literal branch beneath `refs/heads/`, not a refspec or mutable source selector.
        branch: String,
    },
}

impl<'de> Deserialize<'de> for PublicationDestination {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Kind,
            Remote,
            Branch,
        }

        struct Destination;
        impl<'de> Visitor<'de> for Destination {
            type Value = PublicationDestination;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("named publication destination fields")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut kind, mut remote, mut branch) = (None, None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    let (slot, name) = match field {
                        Field::Kind => (&mut kind, "kind"),
                        Field::Remote => (&mut remote, "remote"),
                        Field::Branch => (&mut branch, "branch"),
                    };
                    if slot.is_some() {
                        return Err(A::Error::duplicate_field(name));
                    }
                    *slot = Some(map.next_value::<String>()?);
                }
                if kind.ok_or_else(|| A::Error::missing_field("kind"))? != "git" {
                    return Err(A::Error::custom("unsupported publication destination kind"));
                }
                Ok(PublicationDestination::Git {
                    remote: remote.ok_or_else(|| A::Error::missing_field("remote"))?,
                    branch: branch.ok_or_else(|| A::Error::missing_field("branch"))?,
                })
            }
        }
        // Derived enum decoding also accepts positional arrays. This contract
        // requires named fields at every entrypoint, not only at TOML/SQL wrappers.
        decoder.deserialize_map(Destination)
    }
}

impl PublicationDestination {
    /// Validate the selected coordinates without allocation, I/O or normalization.
    ///
    /// # Errors
    /// Refuses empty/control-bearing or padded remotes and non-literal branch syntax.
    pub fn validate(&self) -> Result<(), DestinationError> {
        match self {
            Self::Git { remote, branch } => validate_git_destination(remote, branch),
        }
    }
}

/// Invalid delivery coordinates; private remote or branch bytes are never included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DestinationError;

impl std::fmt::Display for DestinationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid publication destination")
    }
}

impl std::error::Error for DestinationError {}

/// The shared literal Git destination grammar used by configuration and authorization.
/// This is not URL validation, provider selection, native resolution or effect permission.
///
/// # Errors
/// Refuses malformed coordinates without disclosing them. Accepted bytes are unchanged.
pub fn validate_git_destination(remote: &str, branch: &str) -> Result<(), DestinationError> {
    let text = |value: &str| {
        !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
    };
    let valid_branch = text(branch)
        && !branch.starts_with('-')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch != "@"
        && !branch
            .chars()
            .any(|ch| ch.is_whitespace() || "~^:?*[\\".contains(ch))
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"));
    if !text(remote) || !valid_branch {
        return Err(DestinationError);
    }
    Ok(())
}
