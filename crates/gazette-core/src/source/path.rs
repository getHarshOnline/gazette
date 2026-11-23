//! Literal source selection, deliberately separate from URL normalization.

use serde::Serialize;
use std::fmt;
use std::str::FromStr;

/// A non-empty, unambiguous subdirectory relative to an admitted source root.
///
/// Parsing preserves bytes: no case folding, Unicode normalization, percent
/// decoding or dot-component cleanup is performed. A percent escape that could
/// conceal traversal, a separator, a control or nested encoding is rejected.
/// This is a filesystem selector, not a publication route or URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct Subdirectory(String);

/// A source selector cannot be resolved safely without changing its meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSubdirectory(&'static str);

impl fmt::Display for InvalidSubdirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; select a literal relative subdirectory without traversal or escapes",
            self.0
        )
    }
}

impl std::error::Error for InvalidSubdirectory {}

impl FromStr for Subdirectory {
    type Err = InvalidSubdirectory;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        for component in value.split('/') {
            validate_component(component)?;
            if component.contains('%') {
                validate_escapes(component)?;
            }
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> serde::Deserialize<'de> for Subdirectory {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <String as serde::Deserialize>::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Subdirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Subdirectory {
    /// Append another validated relative selection without changing either name.
    pub fn join(&self, child: &Self) -> Self {
        Self(format!("{self}/{child}"))
    }

    /// Iterate the validated literal components without normalization or decoding.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

fn validate_component(component: &str) -> Result<(), InvalidSubdirectory> {
    if matches!(component, "" | "." | "..") {
        return Err(InvalidSubdirectory("empty or dot path component"));
    }
    if component
        .chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err(InvalidSubdirectory("separator, platform prefix or control"));
    }
    Ok(())
}

fn validate_escapes(component: &str) -> Result<(), InvalidSubdirectory> {
    let mut decoded = Vec::with_capacity(component.len());
    let mut bytes = component.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let mut ahead = bytes.clone();
            let pair = ahead.next().and_then(hex).zip(ahead.next().and_then(hex));
            if let Some((high, low)) = pair {
                let value = high * 16 + low;
                if value == b'%' {
                    return Err(InvalidSubdirectory("nested percent encoding"));
                }
                decoded.push(value);
                bytes = ahead;
                continue;
            }
        }
        // An incomplete/non-hex percent sequence is a literal filesystem name.
        // Nothing in the source reader treats it as an escape.
        decoded.push(byte);
    }
    let decoded = std::str::from_utf8(&decoded)
        .map_err(|_| InvalidSubdirectory("non-UTF-8 percent encoding"))?;
    validate_component(decoded)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/internal/source/path.rs"]
mod tests;
