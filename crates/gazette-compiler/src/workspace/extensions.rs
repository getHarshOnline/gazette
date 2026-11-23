//! Compile workspace-owned data contracts once, without acquiring external schemas.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::publication::{ExtensionSchema, Key};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

struct Compiled {
    definition: ExtensionSchema,
    validator: jsonschema::Validator,
}

/// Immutable declarations and their derived validators; only declarations are identity.
#[derive(Default)]
pub(super) struct Extensions {
    entries: BTreeMap<Key, Compiled>,
}

impl Extensions {
    pub(super) fn compile(
        definitions: Vec<ExtensionSchema>,
    ) -> Result<Self, Vec<super::Diagnostic>> {
        let mut entries = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut errors = Vec::new();
        for (index, definition) in definitions.into_iter().enumerate() {
            if !seen.insert(definition.id.clone()) {
                errors.push(super::Diagnostic {
                    code: "workspace_extension_duplicate",
                    field: format!("extensions[{index}].id"),
                    key: Some(definition.id),
                });
                continue;
            }
            let failure = || super::Diagnostic {
                code: "workspace_extension_schema",
                field: format!("extensions[{index}].schema"),
                key: Some(definition.id.clone()),
            };
            if definition.schema.get("$schema").is_some_and(|schema| {
                schema.as_str() != Some("https://json-schema.org/draft/2020-12/schema")
            }) {
                errors.push(failure());
                continue;
            }
            match jsonschema::draft202012::options()
                .with_retriever(crate::schema::DenyExternal)
                .build(&definition.schema)
                .map_err(|_| failure())
            {
                Ok(validator) => {
                    entries.insert(
                        definition.id.clone(),
                        Compiled {
                            definition,
                            validator,
                        },
                    );
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(Self { entries })
        } else {
            errors.sort();
            Err(errors)
        }
    }

    pub(super) fn validate(&self, id: &Key, data: &Value) -> Result<(), &'static str> {
        let entry = self.entries.get(id).ok_or("extension_unknown")?;
        if entry.validator.is_valid(data) {
            Ok(())
        } else {
            Err("extension_data")
        }
    }

    pub(super) fn definition(&self, id: &Key) -> Option<&ExtensionSchema> {
        self.entries.get(id).map(|entry| &entry.definition)
    }

    pub(super) fn definitions(&self) -> impl Iterator<Item = (&Key, &ExtensionSchema)> {
        self.entries
            .iter()
            .map(|(id, entry)| (id, &entry.definition))
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.definitions()).finish()
    }
}

impl PartialEq for Extensions {
    fn eq(&self, other: &Self) -> bool {
        self.definitions().eq(other.definitions())
    }
}

impl Eq for Extensions {}

impl Serialize for Extensions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (id, definition) in self.definitions() {
            map.serialize_entry(id, definition)?;
        }
        map.end()
    }
}

impl schemars::JsonSchema for Extensions {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExtensionSchemas".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <BTreeMap<Key, ExtensionSchema> as schemars::JsonSchema>::json_schema(generator)
    }
}
