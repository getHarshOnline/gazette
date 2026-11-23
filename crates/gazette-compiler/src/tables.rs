//! Shared named-table boundary for workspace and authored declarations.

use std::{fmt, marker::PhantomData};

use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};

// Derived structs also accept positional arrays. This deserialization-only
// boundary is shared by the table, optional table and table-list consumers.
struct Named<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Named<T> {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Table<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Table<T> {
            type Value = Named<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("named configuration fields")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Named)
            }
        }
        decoder.deserialize_map(Table(PhantomData))
    }
}

pub(super) fn one<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<T, D::Error> {
    Named::deserialize(decoder).map(|Named(value)| value)
}

pub(super) fn optional<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<Named<T>>::deserialize(decoder).map(|value| value.map(|Named(value)| value))
}

pub(super) fn many<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Vec<T>, D::Error> {
    Vec::<Named<T>>::deserialize(decoder)
        .map(|values| values.into_iter().map(|Named(value)| value).collect())
}
