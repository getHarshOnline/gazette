//! Explicit subject kinds, independent of a project's continuity and availability.

use serde::{Deserialize, Serialize};

/// What the catalog identity denotes. Absence leaves the subject unspecified;
/// a repository, a page family or a lifecycle never supplies this declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectKind {
    /// A software application, whether planned, developing or available.
    SoftwareApplication,
    /// Programming source code, distinct from the application it implements.
    SoftwareSourceCode,
    /// An authored work whose more specific form is not declared.
    CreativeWork,
    /// A book, including a graphic novel while it is in development.
    Book,
    /// A collection of works or other items, distinct from its presenting page.
    Collection,
    /// An organizational undertaking directed toward an aim, not its output.
    Project,
    /// An organizational research undertaking, not a paper or software artifact.
    ResearchProject,
}
