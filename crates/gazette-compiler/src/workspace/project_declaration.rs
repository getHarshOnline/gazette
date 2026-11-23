//! Select one owner of project facts without manufacturing incomplete project records.

use gazette_core::publication::{Key, Project};
use serde::{Deserialize, Serialize};

/// A project is declared here in full or selected by its canonical authored landing.
/// The strict alternatives reject mixed declarations, including otherwise equal facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "ProjectSourceDeclaration")]
pub enum ProjectDeclaration {
    /// Facts are read from this exact content identity in the captured corpus.
    Landing(ProjectLandingSource),
    /// A complete workspace declaration, retained for publishers without a landing.
    Inline(Box<Project>),
}

/// An exact authored owner, not a filename, route or request to read another source tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectLandingSource {
    /// Configured project namespace.
    pub id: Key,
    /// Content identity whose singular `project` table owns these facts.
    pub landing: Key,
}

impl ProjectDeclaration {
    /// Configured identity, available before any publication content is acquired.
    pub fn id(&self) -> &Key {
        match self {
            Self::Landing(source) => &source.id,
            Self::Inline(project) => &project.id,
        }
    }

    /// Facts available from configuration alone; a landing selection has none yet.
    pub fn inline(&self) -> Option<&Project> {
        match self {
            Self::Inline(project) => Some(project),
            Self::Landing(_) => None,
        }
    }
}
