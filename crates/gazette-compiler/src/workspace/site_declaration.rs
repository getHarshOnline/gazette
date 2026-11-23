//! A site may name its own brand and publication without three repeated objects.

use super::{Brand, BrandPublication, Key, Site};
use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::{Map, Value};
use std::{
    borrow::Cow,
    collections::BTreeSet,
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

pub(super) struct SiteDeclaration {
    pub(super) site: Site,
    pub(super) brand: Option<Brand>,
    pub(super) explicit_brand: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct BrandIdentity {
    id: Key,
    #[serde(default)]
    parent: Option<Key>,
}

/// A short site's brand name is beside its identity table, not inside it.
pub(super) fn field_origin_path(
    declaration: &[crate::toml::PathSegment],
    field: &[crate::toml::PathSegment],
) -> Vec<crate::toml::PathSegment> {
    use crate::toml::PathSegment;
    let mut path = declaration.to_vec();
    if matches!(declaration, [PathSegment::Key(sites), PathSegment::Index(_), PathSegment::Key(brand)]
        if sites == "sites" && brand == "brand")
        && matches!(field.first(), Some(PathSegment::Key(name)) if name == "name")
    {
        path.pop();
    }
    path.extend_from_slice(field);
    path
}

/// Follow the short theme spelling back to the authored field.
pub(super) fn theme_origin_path(
    syntax: &crate::toml::ParsedToml<'_>,
    path: &mut Vec<crate::toml::PathSegment>,
) {
    use crate::toml::PathSegment;
    if !matches!(path.as_slice(), [PathSegment::Key(sites), PathSegment::Index(_), PathSegment::Key(composition), PathSegment::Key(theme), ..]
        if sites == "sites" && composition == "composition" && theme == "theme")
    {
        return;
    }
    let mut short = path[..2].to_vec();
    short.push(PathSegment::Key("theme".into()));
    if syntax.span_at(&short).is_none() {
        return;
    }
    let scalar = syntax.string_at(&short).is_some();
    let suffix = &path[4..];
    if !(scalar && matches!(suffix, [PathSegment::Key(key)] if key == "package")) {
        short.extend_from_slice(suffix);
    }
    *path = short;
}

impl Deref for SiteDeclaration {
    type Target = Site;
    fn deref(&self) -> &Site {
        &self.site
    }
}

impl DerefMut for SiteDeclaration {
    fn deref_mut(&mut self) -> &mut Site {
        &mut self.site
    }
}

pub(super) fn publication_fields() -> &'static BTreeSet<String> {
    static FIELDS: OnceLock<BTreeSet<String>> = OnceLock::new();
    FIELDS.get_or_init(|| {
        schemars::schema_for!(BrandPublication)
            .get("properties")
            .and_then(Value::as_object)
            .expect("publication object schema")
            .keys()
            .filter(|name| name.as_str() != "site")
            .cloned()
            .collect()
    })
}

impl<'de> Deserialize<'de> for SiteDeclaration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
        let name = fields.remove("name");
        let identity = fields
            .remove("brand")
            .map(serde_json::from_value::<BrandIdentity>)
            .transpose()
            .map_err(D::Error::custom)?;
        let explicit_brand = identity.is_some();
        let mut publication = Map::new();
        for field in publication_fields() {
            if let Some(value) = fields.remove(field) {
                publication.insert(field.clone(), value);
            }
        }
        for (short, full) in [("domain", "public_base_url"), ("port", "local_port")] {
            if let Some(mut value) = fields.remove(short) {
                if publication.contains_key(full) {
                    return Err(D::Error::custom(format!("use {short} or {full}, not both")));
                }
                if short == "domain" {
                    let domain = value
                        .as_str()
                        .ok_or_else(|| D::Error::custom("domain must be a hostname"))?;
                    if domain.is_empty() || domain.contains(['/', ':', '?', '#', '@']) {
                        return Err(D::Error::custom(
                            "domain must be a hostname; use public_base_url for a full address",
                        ));
                    }
                    value = Value::String(format!("https://{domain}/"));
                }
                publication.insert(full.into(), value);
            }
        }
        if let Some(theme) = fields.remove("theme") {
            let composition = fields
                .entry("composition")
                .or_insert_with(|| Value::Object(Map::new()));
            let composition = composition
                .as_object_mut()
                .ok_or_else(|| D::Error::custom("composition must be a table"))?;
            if composition.contains_key("theme") {
                return Err(D::Error::custom("theme is declared twice"));
            }
            let theme = if theme.is_string() {
                serde_json::json!({"package": theme})
            } else {
                theme
            };
            composition.insert("theme".into(), theme);
        }
        let site: Site = serde_json::from_value(Value::Object(fields)).map_err(D::Error::custom)?;
        let brand = match name {
            Some(name) => {
                publication.insert(
                    "site".into(),
                    serde_json::to_value(&site.id).map_err(D::Error::custom)?,
                );
                let publication =
                    serde_json::from_value(Value::Object(publication)).map_err(D::Error::custom)?;
                let identity = match identity {
                    Some(identity) => identity,
                    None => BrandIdentity {
                        id: format!("brand:{}", site.id)
                            .parse()
                            .map_err(D::Error::custom)?,
                        parent: None,
                    },
                };
                Some(Brand {
                    id: identity.id,
                    name: serde_json::from_value(name).map_err(D::Error::custom)?,
                    parent: identity.parent,
                    publications: vec![publication],
                })
            }
            None if publication.is_empty() && !explicit_brand => None,
            None => {
                return Err(D::Error::custom(
                    "a site with brand or publication fields needs its own name, or a separate brand publication",
                ));
            }
        };
        Ok(Self {
            site,
            brand,
            explicit_brand,
        })
    }
}

impl schemars::JsonSchema for SiteDeclaration {
    fn schema_name() -> Cow<'static, str> {
        "SiteDeclaration".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let settings = || {
            schemars::generate::SchemaSettings::draft2020_12()
                .with(|settings| settings.inline_subschemas = true)
                .into_generator()
        };
        let mut schema = settings().into_root_schema_for::<Site>();
        let publication = settings().into_root_schema_for::<BrandPublication>();
        let properties = schema
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .expect("site object schema");
        for (name, field) in publication
            .get("properties")
            .and_then(Value::as_object)
            .expect("publication object schema")
        {
            if name != "site" {
                properties.insert(name.clone(), field.clone());
            }
        }
        properties.insert("name".into(), serde_json::json!({"type":"string", "description":"Display name when this site is its own brand."}));
        properties.insert(
            "brand".into(),
            serde_json::to_value(settings().into_root_schema_for::<BrandIdentity>())
                .expect("brand identity schema"),
        );
        properties.insert("domain".into(), serde_json::json!({"type":"string", "description":"Public hostname; use public_base_url instead for a deployment prefix."}));
        properties.insert("port".into(), properties["local_port"].clone());
        properties.insert("theme".into(), serde_json::json!({"anyOf":[{"type":"string"},settings().into_root_schema_for::<super::ThemeSelection>()]}));
        schema.insert(
            "dependentRequired".into(),
            serde_json::json!({"brand": ["name"]}),
        );
        schema
    }
}
