//! Byte identity independent of a particular hashing implementation.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

/// Exactly 32 SHA-256 bytes, projected as lowercase hexadecimal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sha256Digest([u8; 32]);

impl schemars::JsonSchema for Sha256Digest {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Sha256Digest".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 64,
            "maxLength": 64,
            "pattern": "^[0-9a-f]{64}$"
        })
    }
}

impl Sha256Digest {
    /// Fixed-width digest bytes, independent of their text projection.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Compute the identity of ordered retained slices without copying their concatenation.
    /// Callers establish aggregate bounds before hashing; slice boundaries add no bytes.
    pub fn of_slices<'a>(slices: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut hash = Sha256::new();
        for slice in slices {
            hash.update(slice);
        }
        Self(hash.finalize().into())
    }

    /// Compute the identity of an already bounded byte slice.
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

impl From<[u8; 32]> for Sha256Digest {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A digest is not exactly 64 lowercase hexadecimal characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSha256Digest;

impl fmt::Display for InvalidSha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("use an exact 64-character lowercase SHA-256 digest")
    }
}

impl std::error::Error for InvalidSha256Digest {}

impl FromStr for Sha256Digest {
    type Err = InvalidSha256Digest;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(InvalidSha256Digest);
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            // The spelling check above establishes ASCII byte boundaries.
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| InvalidSha256Digest)?;
        }
        Ok(Self(bytes))
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
