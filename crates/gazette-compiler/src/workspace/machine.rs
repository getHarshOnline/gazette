//! Explicit search ranking selection and graph-view destinations, not new content owners.

use gazette_core::publication::routing::RoutePath;
use serde::{Deserialize, Serialize};

use super::SearchProfile;

/// Search token processing and native ranking fields selected by a publication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum SearchConfiguration {
    /// Concise selection of the existing useful title/body/excerpt/project/keyword corpus.
    Profile(SearchProfile),
    /// Explicit token processing and ranking fields; identity/navigation metadata stays intact.
    Selection(#[serde(deserialize_with = "crate::tables::one")] SearchSelection),
}

/// Closed explicit ranking selection; no source path or parser-derived field is accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchSelection {
    /// Explicit token processing profile, not an inferred language tag.
    pub language: SearchProfile,
    /// Nonempty distinct native ranking fields, independent of retained result metadata.
    #[schemars(schema_with = "fields_schema")]
    pub fields: Vec<SearchField>,
}

fn fields_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "items": generator.subschema_for::<SearchField>(),
        "minItems": 1,
        "uniqueItems": true
    })
}

/// Plain-text values borrowed from the same retained page used by presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchField {
    /// Authored content title.
    Title,
    /// Explicit authored description, without inferred summary fallback.
    Description,
    /// Authored keyword phrases, not private tags or inherited classification.
    Keywords,
    /// Actual serving URL, preserving preview/base addressing.
    Route,
    /// Main body text projected from the retained admitted Markdown.
    Body,
    /// Subordinate article commentary text, never an additional search identity.
    Notes,
    /// Explicit excerpt text from the retained main body.
    Summary,
    /// Discovery-safe project references and names already resolved for the page.
    Projects,
}

impl SearchConfiguration {
    /// Selected token processing, shared by native output and browser presentation.
    pub fn profile(&self) -> SearchProfile {
        match self {
            Self::Profile(profile) => *profile,
            Self::Selection(selection) => selection.language,
        }
    }

    /// Selected ranking fields; result identity/navigation metadata is not filtered.
    pub fn fields(&self) -> &[SearchField] {
        match self {
            Self::Profile(_) => &[
                SearchField::Title,
                SearchField::Body,
                SearchField::Summary,
                SearchField::Projects,
                SearchField::Keywords,
            ],
            Self::Selection(selection) => &selection.fields,
        }
    }
}

/// Optional view of the existing discovery graph through the selected Theme entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphConfiguration {
    /// Explicit output selection; false never activates a graph view.
    pub enabled: bool,
    /// Native site-relative destination, not a new graph identity or template selector.
    pub path: RoutePath,
}

pub(super) fn validate(publication: &super::BrandPublication, errors: &mut Vec<super::Diagnostic>) {
    super::deployment::validate(publication, errors);
    if let Some(search) = &publication.machine.search {
        let fields = search.fields();
        if fields.is_empty()
            || fields
                .iter()
                .enumerate()
                .any(|(index, field)| fields[..index].contains(field))
        {
            errors.push(super::Diagnostic {
                code: "workspace_search_fields",
                field: "brands.publications.machine.search.fields".into(),
                key: Some(publication.site.clone()),
            });
        }
    }
}
