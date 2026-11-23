//! Duplicate-safe, number-preserving JSON decoding through Serde's native grammar.

use std::fmt;

use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, value::RawValue};

/// Decode complete JSON without duplicate decoded keys or numeric rounding.
///
/// Callers bound source bytes before parsing. Borrowed raw slices preserve number
/// spelling until Serde's arbitrary-precision number decoder consumes it. Objects
/// never pass through Value's private-token map handling: those keys remain data.
/// Retained nesting is capped at 128 values. Raw scanning uses source-bounded
/// scratch space; repeated subtree scans cost at most depth times input bytes.
///
/// # Errors
/// Invalid JSON, duplicate members or exhausted nesting. Nested diagnostics refer
/// to the current value slice, not necessarily the complete source coordinates.
pub fn parse(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    parse_with_number_validation(bytes, &|_| true)
}

/// Apply a caller's numeric-literal profile before Serde can normalize spelling.
///
/// The native JSON grammar still validates every token. The predicate receives
/// only complete raw number literals, never quoted strings or object keys. It
/// cannot rewrite a value. This is useful for signature profiles that distinguish
/// `-0` from `0`; ordinary [`parse`] retains Serde's value-level interpretation.
/// Predicates must not treat partial visits as a successfully decoded document.
///
/// # Errors
/// The same failures as [`parse`], plus any numeric literal refused by the predicate.
pub fn parse_with_number_validation(
    bytes: &[u8],
    validate_number: &dyn Fn(&str) -> bool,
) -> Result<Value, serde_json::Error> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Node {
        depth: 0,
        validate_number,
    }
    .deserialize(&mut decoder)?;
    decoder.end()?;
    Ok(value)
}

#[derive(Clone, Copy)]
struct Node<'a> {
    depth: usize,
    validate_number: &'a dyn Fn(&str) -> bool,
}

impl<'de> DeserializeSeed<'de> for Node<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        if self.depth >= 128 {
            return Err(de::Error::custom("JSON nesting ceiling exceeded"));
        }
        let raw = <&RawValue>::deserialize(deserializer)?;
        if matches!(raw.get().as_bytes().first(), Some(b'-' | b'0'..=b'9'))
            && !(self.validate_number)(raw.get())
        {
            return Err(de::Error::custom(
                "numeric literal refused by caller profile",
            ));
        }
        let mut decoder = serde_json::Deserializer::from_str(raw.get());
        let value = match raw.get().as_bytes().first() {
            Some(b'{') => decoder.deserialize_map(self),
            Some(b'[') => decoder.deserialize_seq(self),
            // RawValue already established a complete valid scalar. No user map
            // can impersonate Serde's internal arbitrary-precision number token.
            _ => serde_json::from_str(raw.get()),
        }
        .map_err(de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Visitor<'de> for Node<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON with unique object members")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(Node {
            depth: self.depth + 1,
            validate_number: self.validate_number,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            // Equality follows escape decoding, before reading a duplicate value.
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate JSON object member"));
            }
            values.insert(
                key,
                map.next_value_seed(Node {
                    depth: self.depth + 1,
                    validate_number: self.validate_number,
                })?,
            );
        }
        Ok(Value::Object(values))
    }
}
