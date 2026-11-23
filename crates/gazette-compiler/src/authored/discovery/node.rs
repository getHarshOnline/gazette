//! Borrowed index metadata; structural edges are projected separately, never rewritten.

use std::borrow::Cow;

use gazette_core::publication::content::{CollectionPurpose, ContentKind, ContentNode, PageFamily};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Serialize, Serializer, ser::SerializeMap};

use crate::{authored::SitePresentation, workspace::ResolvedRoute};

/// An eligible content record in an automatic listing, not another canonical node.
/// Container endpoints are deliberately absent; relationship projections carry them.
#[derive(Clone, Debug)]
pub struct DiscoveryNode<'a> {
    node: &'a ContentNode,
    presentation: Option<SitePresentation>,
    resolved_route: Option<&'a ResolvedRoute>,
    artwork: Option<&'a crate::authored::ListingArtwork>,
    documentation: bool,
    listing: bool,
}

impl<'a> DiscoveryNode<'a> {
    pub(super) fn new(
        node: &'a ContentNode,
        presentation: Option<SitePresentation>,
        resolved_route: Option<&'a ResolvedRoute>,
        documentation: bool,
        listing: bool,
    ) -> Self {
        Self {
            node,
            presentation,
            resolved_route,
            artwork: None,
            documentation,
            listing,
        }
    }

    pub(super) fn with_artwork(
        mut self,
        artwork: Option<&'a crate::authored::ListingArtwork>,
    ) -> Self {
        self.artwork = artwork;
        self
    }

    /// Kind and page family only; discovery relationships are separate projections.
    pub fn structure(&self) -> impl Serialize + '_ {
        Structure::from(&self.node.structure)
    }
}

impl Serialize for DiscoveryNode<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let node = self.node;
        let mut record = serializer.serialize_map(None)?;
        record.serialize_entry("id", &node.id)?;
        record.serialize_entry("site", &node.site)?;
        if self.listing {
            record.serialize_entry("listing", &true)?;
        }
        if self.documentation {
            record.serialize_entry("documentation", &true)?;
        }
        if let Some(presentation) = &self.presentation {
            record.serialize_entry("presentation", presentation)?;
        }
        if let Some(route) = self.resolved_route {
            record.serialize_entry("resolved_route", route)?;
        }
        if let Some(artwork) = self.artwork {
            record.serialize_entry("artwork", artwork)?;
        }
        record.serialize_entry("title", &node.title)?;
        if let Some(value) = &node.description {
            record.serialize_entry("description", value)?;
        }
        if !node.keywords.is_empty() {
            record.serialize_entry("keywords", &node.keywords)?;
        }
        if let Some(value) = &node.schema_type {
            record.serialize_entry("schema_type", value)?;
        }
        if let Some(value) = &node.subject {
            record.serialize_entry("subject", value)?;
        }
        if let Some(value) = &node.license {
            record.serialize_entry("license", value)?;
        }
        if let Some(value) = &node.citation {
            record.serialize_entry("citation", value)?;
        }
        if let Some(value) = &node.published {
            record.serialize_entry("published", value)?;
        }
        if let Some(value) = &node.modified {
            record.serialize_entry("modified", value)?;
        }
        record.serialize_entry("lifecycle", &node.lifecycle)?;
        if let Some(value) = &node.language {
            record.serialize_entry("language", value)?;
        }
        if !node.taxonomies.is_empty() {
            record.serialize_entry("taxonomies", &node.taxonomies)?;
        }
        if !node.attributions.is_empty() {
            record.serialize_entry("attributions", &node.attributions)?;
        }
        if let Some(value) = &node.route {
            record.serialize_entry("route", value)?;
        }
        record.serialize_entry("structure", &Structure::from(&node.structure))?;
        record.end()
    }
}

impl JsonSchema for DiscoveryNode<'_> {
    fn schema_name() -> Cow<'static, str> {
        "DiscoveryNode".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        // Discovery retains native scalar metadata, but never its structural edges.
        // Reuse that strict owning schema and derive the replacement from the exact
        // shallow transport used above; do not relax ContentNode admission itself.
        let mut schema = ContentNode::<gazette_core::publication::Key>::json_schema(generator);
        let properties = schema
            .ensure_object()
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
            .expect("the native ContentNode derives an object metadata schema");
        properties.insert(
            "documentation".into(),
            generator.subschema_for::<bool>().to_value(),
        );
        properties.insert(
            "listing".into(),
            generator.subschema_for::<bool>().to_value(),
        );
        properties.insert(
            "structure".into(),
            generator.subschema_for::<Structure<'_>>().to_value(),
        );
        properties.insert(
            "presentation".into(),
            generator.subschema_for::<SitePresentation>().to_value(),
        );
        properties.insert(
            "resolved_route".into(),
            generator.subschema_for::<ResolvedRoute>().to_value(),
        );
        properties.insert(
            "artwork".into(),
            generator
                .subschema_for::<crate::authored::ListingArtwork>()
                .to_value(),
        );
        schema
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[schemars(rename = "DiscoveryStructure")]
enum Structure<'a> {
    Page {
        family: &'a PageFamily,
    },
    Collection {
        #[serde(skip_serializing_if = "Option::is_none")]
        purpose: Option<&'a CollectionPurpose>,
    },
    Journey,
}

impl<'a> From<&'a ContentKind> for Structure<'a> {
    fn from(value: &'a ContentKind) -> Self {
        match value {
            ContentKind::Page { family } => Self::Page { family },
            ContentKind::Collection { purpose, .. } => Self::Collection {
                purpose: purpose.as_ref(),
            },
            ContentKind::Journey { .. } => Self::Journey,
        }
    }
}
