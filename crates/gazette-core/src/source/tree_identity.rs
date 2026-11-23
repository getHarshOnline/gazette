//! Versioned identity of a complete canonical inventory, not a release artifact.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use sha2::{Digest, Sha256};

use super::{Content, Entry, Sha256Digest};

/// Identity of the structural tree represented by an inventory.
///
/// This binds literal relative paths, entry roles, file lengths and file digests.
/// It excludes native root locations, times, permissions, Git revisions, owners,
/// policy and renderer identity. Those require separate bindings. An observed
/// inventory and a proposed inventory use the same structural encoding; neither
/// becomes native-byte proof, durable ownership or certification by hashing it.
/// Deserialization accepts only this exact encoding and a well-formed digest;
/// it transports a claimed pin, not proof that any native tree matches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeIdentity(Sha256Digest);

impl schemars::JsonSchema for TreeIdentity {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "TreeIdentity".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "format": {"const": Self::FORMAT},
                "sha256": generator.subschema_for::<Sha256Digest>()
            },
            "required": ["format", "sha256"],
            "additionalProperties": false
        })
    }
}

impl TreeIdentity {
    /// Identifier of this Engine-owned canonical encoding, not a publication ABI.
    pub const FORMAT: &str = "gazette.inventory.v1";

    /// SHA-256 over the versioned, length-framed canonical tree encoding.
    pub fn sha256(&self) -> Sha256Digest {
        self.0
    }

    pub(super) fn from_entries(entries: &[Entry]) -> Self {
        let mut hash = Sha256::new();
        hash.update(Self::FORMAT.as_bytes());
        hash.update([0]);
        hash.update((entries.len() as u64).to_be_bytes());
        for entry in entries {
            let path = entry.path.as_str().as_bytes();
            hash.update([match entry.content {
                Content::Directory => 0,
                Content::File { .. } => 1,
            }]);
            hash.update((path.len() as u64).to_be_bytes());
            hash.update(path);
            if let Content::File { bytes, sha256 } = entry.content {
                hash.update(bytes.to_be_bytes());
                hash.update(sha256.as_bytes());
            }
        }
        Self(Sha256Digest::from(<[u8; 32]>::from(hash.finalize())))
    }
}

impl Serialize for TreeIdentity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("TreeIdentity", 2)?;
        state.serialize_field("format", Self::FORMAT)?;
        state.serialize_field("sha256", &self.0)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TreeIdentity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Identity {
            format: String,
            sha256: Sha256Digest,
        }
        let identity = Identity::deserialize(deserializer)?;
        if identity.format != Self::FORMAT {
            return Err(D::Error::custom("unsupported tree identity format"));
        }
        Ok(Self(identity.sha256))
    }
}
