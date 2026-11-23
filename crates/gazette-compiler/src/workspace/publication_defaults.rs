//! Resolve declared publication defaults before applying the publication schema.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Declaration, Diagnostic, Error, Key};
use crate::toml::ParsedToml;

/// Settings shared by the workspace's publications.
#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Defaults {
    /// Publication fields inherited by sites unless they override them.
    #[serde(default)]
    #[schemars(schema_with = "publication_schema")]
    publication: Map<String, Value>,
}

/// Publication settings shared by sites with this declared publisher.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PublisherDefaults {
    /// Existing principal holding the publisher role on the receiving sites.
    pub(super) id: Key,
    /// Publisher-specific publication fields, overriding workspace defaults.
    #[serde(default)]
    #[schemars(schema_with = "publication_schema")]
    publication: Map<String, Value>,
}

#[derive(Default)]
pub(super) struct Inheritance {
    defaults: Map<String, Value>,
    publishers: BTreeMap<Key, Map<String, Value>>,
    sites: BTreeMap<Key, Key>,
    active: bool,
}

fn invalid(field: &str) -> Error {
    Error::Invalid(vec![Diagnostic {
        code: "workspace_publication_defaults",
        field: field.into(),
        key: None,
    }])
}

pub(super) fn validate_accounts(
    defaults: Option<&Defaults>,
    publishers: &[PublisherDefaults],
    accounts: &BTreeMap<Key, super::OnlineAccount>,
    errors: &mut Vec<Diagnostic>,
) -> Result<(), Error> {
    let fields = defaults
        .map(|value| ("defaults.publication".to_owned(), &value.publication))
        .into_iter()
        .chain(publishers.iter().map(|publisher| {
            (
                format!("publishers[{:?}].publication", publisher.id.as_str()),
                &publisher.publication,
            )
        }));
    for (field, fields) in fields {
        if let Some(selection) = fields.get("accounts") {
            let selection: Vec<Key> = decode(selection.clone())?;
            let inline = fields
                .get("social")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|social| social.keys().map(String::as_str));
            super::accounts::validate_selection(accounts, &selection, inline, &field, errors);
        }
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|error| {
        Error::Toml(crate::toml::Error::Invalid {
            message: error.to_string(),
            span: None,
        })
    })
}

impl Inheritance {
    pub(super) fn collect<'a, 's: 'a>(
        documents: impl IntoIterator<Item = &'a ParsedToml<'s>>,
    ) -> Result<Self, Error> {
        let mut result = Self::default();
        let mut has_defaults = false;
        let mut site_publishers = Vec::new();
        for document in documents {
            let value: Value = document.deserialize().map_err(Error::Toml)?;
            if let Some(defaults) = value.get("defaults") {
                if has_defaults {
                    return Err(invalid("defaults: declared in more than one document"));
                }
                has_defaults = true;
                let defaults: Defaults = decode(defaults.clone())?;
                validate_fields(&defaults.publication)?;
                result.defaults = defaults.publication;
                result.active = true;
            }
            if let Some(publishers) = value.get("publishers") {
                for publisher in decode::<Vec<PublisherDefaults>>(publishers.clone())? {
                    validate_fields(&publisher.publication)?;
                    if result
                        .publishers
                        .insert(publisher.id, publisher.publication)
                        .is_some()
                    {
                        return Err(invalid("publishers: duplicate publisher defaults"));
                    }
                }
                result.active = true;
            }
            if let Some(sites) = value.get("sites").and_then(Value::as_array) {
                for site in sites {
                    result.active |= site.get("name").is_some();
                    let (Some(id), Some(publisher)) = (site.get("id"), site.get("publisher"))
                    else {
                        continue;
                    };
                    site_publishers.push((id.clone(), publisher.clone()));
                }
            }
        }
        if result.active {
            for (id, publisher) in site_publishers {
                result.sites.insert(decode(id)?, decode(publisher)?);
            }
        }
        Ok(result)
    }

    pub(super) fn read(&self, document: &ParsedToml<'_>) -> Result<Declaration, Error> {
        if !self.active {
            return document.deserialize().map_err(Error::Toml);
        }
        let mut value: Value = document.deserialize().map_err(Error::Toml)?;
        if let Some(sites) = value.get_mut("sites").and_then(Value::as_array_mut) {
            for site in sites.iter_mut().filter(|site| site.get("name").is_some()) {
                let id = site.get("id").cloned().map(decode::<Key>).transpose()?;
                let mut effective = self.effective(id.as_ref());
                merge(&mut effective, site.take());
                *site = effective;
            }
        }
        if let Some(brands) = value.get_mut("brands").and_then(Value::as_array_mut) {
            for brand in brands {
                if let Some(publications) =
                    brand.get_mut("publications").and_then(Value::as_array_mut)
                {
                    for publication in publications {
                        let site = publication
                            .get("site")
                            .cloned()
                            .map(decode::<Key>)
                            .transpose()?;
                        let mut effective = self.effective(site.as_ref());
                        merge(&mut effective, publication.take());
                        *publication = effective;
                    }
                }
            }
        }
        decode(value)
    }

    fn effective(&self, site: Option<&Key>) -> Value {
        let mut effective = Value::Object(self.defaults.clone());
        if let Some(settings) = site
            .and_then(|site| self.sites.get(site))
            .and_then(|publisher| self.publishers.get(publisher))
        {
            merge(&mut effective, Value::Object(settings.clone()));
        }
        effective
    }
}

// Identity, source selection and ordering remain explicit on each publication.
// Maps inherit field by field; scalars and lists replace the inherited value.
fn validate_fields(fields: &Map<String, Value>) -> Result<(), Error> {
    static VALIDATOR: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    let validator = VALIDATOR.get_or_init(|| {
        jsonschema::draft202012::options()
            .with_retriever(crate::schema::DenyExternal)
            .build(publication_schema(&mut schemars::SchemaGenerator::default()).as_value())
            .expect("publication defaults use the compiler's closed schema")
    });
    if let Err(error) = validator.validate(&Value::Object(fields.clone())) {
        return Err(invalid(&format!(
            "defaults.publication{}",
            error.instance_path()
        )));
    }
    Ok(())
}

fn publication_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<super::BrandPublication>();
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.retain(|field, _| {
            matches!(
                field.as_str(),
                "language" | "contacts" | "social" | "accounts" | "machine" | "processing"
            )
        });
    }
    // Defaults may provide only part of a nested table. The merged publication
    // still passes the complete schema, including every required field.
    fn optional(value: &mut Value) {
        match value {
            Value::Object(object) => {
                object.remove("required");
                for value in object.values_mut() {
                    optional(value);
                }
            }
            Value::Array(array) => array.iter_mut().for_each(optional),
            _ => {}
        }
    }
    let object = schema.ensure_object();
    object.remove("required");
    object.values_mut().for_each(optional);
    schema
}

fn merge(base: &mut Value, overrides: Value) {
    match (base, overrides) {
        (Value::Object(base), Value::Object(overrides)) => {
            for (key, value) in overrides {
                match base.get_mut(&key) {
                    Some(previous) => merge(previous, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overrides) => *base = overrides,
    }
}

pub(super) fn read(document: &ParsedToml<'_>) -> Result<Declaration, Error> {
    Inheritance::collect([document])?.read(document)
}
