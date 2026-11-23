//! Strict named fields at the shared pattern boundary, without semantic admission.

use std::net::IpAddr;

use serde::{
    Deserialize, Deserializer,
    de::{Error, MapAccess, Visitor},
};

use super::Pattern;

fn string_field<'de, A: MapAccess<'de>>(
    map: &mut A,
    slot: &mut Option<String>,
    field: &'static str,
) -> Result<(), A::Error> {
    if slot.is_some() {
        return Err(A::Error::duplicate_field(field));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Kind,
            Value,
            Root,
            Name,
            Address,
            Prefix,
        }

        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Pattern;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("named disclosure pattern fields")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut kind, mut value, mut root, mut name, mut address, mut prefix) =
                    (None, None, None, None, None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Kind => string_field(&mut map, &mut kind, "kind")?,
                        Field::Value => string_field(&mut map, &mut value, "value")?,
                        Field::Root => string_field(&mut map, &mut root, "root")?,
                        Field::Name => string_field(&mut map, &mut name, "name")?,
                        Field::Address => {
                            if address.is_some() {
                                return Err(A::Error::duplicate_field("address"));
                            }
                            // Only human-readable native address strings, never octet tuples.
                            address = Some(map.next_value::<String>()?.parse::<IpAddr>().map_err(
                                |_| A::Error::custom("invalid disclosure network address"),
                            )?);
                        }
                        Field::Prefix => {
                            if prefix.is_some() {
                                return Err(A::Error::duplicate_field("prefix"));
                            }
                            prefix = Some(map.next_value::<u8>()?);
                        }
                    }
                }
                let kind = kind.ok_or_else(|| A::Error::missing_field("kind"))?;
                let extra = || A::Error::custom("fields do not match disclosure pattern kind");
                match kind.as_str() {
                    "literal"
                        if root.is_none()
                            && name.is_none()
                            && address.is_none()
                            && prefix.is_none() =>
                    {
                        Ok(Pattern::Literal {
                            value: value.ok_or_else(|| A::Error::missing_field("value"))?,
                        })
                    }
                    "path-tree"
                        if value.is_none()
                            && name.is_none()
                            && address.is_none()
                            && prefix.is_none() =>
                    {
                        Ok(Pattern::PathTree {
                            root: root.ok_or_else(|| A::Error::missing_field("root"))?,
                        })
                    }
                    "host"
                        if value.is_none()
                            && root.is_none()
                            && address.is_none()
                            && prefix.is_none() =>
                    {
                        Ok(Pattern::Host {
                            name: name.ok_or_else(|| A::Error::missing_field("name"))?,
                        })
                    }
                    "ip-network" if value.is_none() && root.is_none() && name.is_none() => {
                        Ok(Pattern::IpNetwork {
                            address: address.ok_or_else(|| A::Error::missing_field("address"))?,
                            prefix: prefix.ok_or_else(|| A::Error::missing_field("prefix"))?,
                        })
                    }
                    _ => Err(extra()),
                }
            }
        }
        decoder.deserialize_map(Fields)
    }
}
