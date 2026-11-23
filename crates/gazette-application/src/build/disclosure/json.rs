//! Stream JSON keys and values without dropping repeated object members.

use super::{DisclosureProbeError, DisclosureRepresentation, Scan};
use gazette_core::source::EntryPath;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::fmt;

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<(), DisclosureProbeError> {
    let mut state = Strings {
        scan,
        path,
        failure: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let result = Value(&mut state)
        .deserialize(&mut deserializer)
        .and_then(|()| deserializer.end());
    if let Some(error) = state.failure {
        return Err(error);
    }
    result.map_err(|error| DisclosureProbeError::Json {
        path: path.clone(),
        line: error.line(),
        column: error.column(),
    })
}

struct Strings<'a, 'b> {
    scan: &'a mut Scan<'b>,
    path: &'a EntryPath,
    failure: Option<DisclosureProbeError>,
}
struct Value<'a, 'b, 'c>(&'a mut Strings<'b, 'c>);

impl<'de> DeserializeSeed<'de> for Value<'_, '_, '_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Value<'_, '_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON value")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        self.0
            .scan
            .text(
                self.0.path,
                value.as_bytes(),
                DisclosureRepresentation::JsonString,
            )
            .map_err(|error| {
                self.0.failure = Some(error);
                E::custom("disclosure probe stopped")
            })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        while sequence.next_element_seed(Value(self.0))?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_key_seed(Value(self.0))?.is_some() {
            map.next_value_seed(Value(self.0))?;
        }
        Ok(())
    }
}
