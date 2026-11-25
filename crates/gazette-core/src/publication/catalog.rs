//! Shared publication catalogs; declarations do not establish identity or admission.

use serde::{Deserialize, Serialize};

use super::{Availability, Key, LanguageTag, ProjectLicense, ProjectLifecycle, Visibility};

/// Declared identity form; a site, brand or project cannot substitute for a publisher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalKind {
    /// A person reference.
    Person,
    /// An organization reference, not proof of legal status.
    Organization,
}

impl PrincipalKind {
    /// Fragment used for this principal at its configured public website.
    pub fn public_fragment(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Organization => "organization",
        }
    }
}

/// A reference to an independently owned identity, not an identity record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    /// Stable native-owner key.
    pub id: Key,
    /// Explicit form, never inferred from a domain or repository.
    pub kind: PrincipalKind,
    /// Explicit display name, not proof of legal identity or publication approval.
    /// Omission never falls back to the key, an email address or another owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Registered name, when the owner explicitly declares it for publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legal_name: Option<String>,
    /// Explicit website destination, independent from publisher roles and contacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<PrincipalWebsite>,
    /// Public identity references declared directly or through explicitly owned accounts.
    /// Displaying a social link on a publication does not establish ownership.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub same_as: Vec<String>,
}

/// A principal's selected website; a site reference follows native address selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PrincipalWebsite {
    /// Exact publication root, retaining its public and preview address distinction.
    Site {
        /// Explicit workspace site key, not inferred from a principal's other roles.
        site: Key,
    },
    /// A website outside the configured publications; no remote lookup occurs.
    External {
        /// Explicit absolute HTTP(S) URL, validated during workspace resolution.
        url: String,
    },
}

/// Independently declared site responsibilities, not content attribution or permission.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationRole {
    /// Canon authority for the selected site; declaration is not canon approval.
    CanonAuthority,
    /// Controller for the selected site's data processing, not a runtime controller.
    DataController,
    /// Site operation, independent of publishing and data processing.
    SiteOperator,
    /// Data processing for the selected site, not controller authority.
    DataProcessor,
}

/// One exact principal's declared site role. Multiple roles require separate bindings.
/// Declaration does not establish legal status, consent, evidence or public admission.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PublicationRoleBinding {
    /// Stable principal key; no name, email, account or domain matching.
    pub principal: Key,
    /// Exact responsibility, never derived from the site's publisher.
    pub role: PublicationRole,
}

/// A project with three independently declared state axes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Stable native project key, not inferred from a remote URL.
    pub id: Key,
    /// Explicit subject form, independent from lifecycle, visibility and availability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<super::ProjectKind>,
    /// Public HTTP(S) entity IRI, distinct from the page describing it. When absent,
    /// a resolved canonical landing supplies the public entity address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Explicit display name, independent of identity, repository naming and content titles.
    /// Omission has no inferred fallback and declaration is not public-field admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Explicit publication-facing HTTP(S) repository address, validated by workspace resolution.
    /// Not an acquisition remote, reachability observation, ownership claim or lineage inference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Explicit reading, article-index and code-documentation destinations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<super::ProjectResources>,
    /// Exact owner-evidenced SPDX expression for this project's software.
    /// Omission remains unknown/held; this is never a content licence or permission proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<ProjectLicense>,
    /// The research programme this subject belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub programme: Option<Key>,
    /// Published credit and reference files for this project's code repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ProjectSource>,
    /// Public category and operating-system requirements of the application.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software: Option<ProjectSoftware>,
    /// Published papers produced by this research, with their own identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub papers: Vec<ResearchPaper>,
    /// Project-owned discussion settings, never borrowed from a site or placement.
    /// Omission selects no project discussion; it is not a site fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discussion: Option<super::Discussion>,
    /// Explicit project origin, independent of repository address and every state axis.
    /// Omission is unspecified, never original work or an ownership assertion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ProjectOrigin>,
    /// Continuity of the project.
    pub lifecycle: ProjectLifecycle,
    /// Owner-declared visibility.
    pub visibility: Visibility,
    /// Independently declared availability.
    pub availability: Availability,
    /// Explicit route through another publication, independent of lifecycle or deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_through: Option<ProjectAvailability>,
}

/// Search and discovery facts declared by a software application's owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSoftware {
    /// The category that describes what the application does.
    pub application_category: String,
    /// Supported operating systems as a public display value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operating_system: Option<String>,
}

/// Explicit publication facts for a code repository, separate from application credit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSource {
    /// Public repository document explaining the applicable software licence.
    pub license_url: String,
    /// Public repository notices that accompany the licence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice_url: Option<String>,
    /// Public citation file for the source code.
    pub citation_url: String,
    /// Author credited by that file.
    pub author: Key,
    /// Rights holder of this project's own source code.
    pub rights_holder: Key,
}

/// One published research paper at an explicitly selected address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResearchPaper {
    /// Title on the paper.
    pub name: String,
    /// Public address that identifies this edition.
    pub url: String,
}

/// The configured publication and page through which a gated project can be obtained.
/// This relationship grants neither a licence nor authority to promise a deployment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectAvailability {
    /// Exact site, preserving its publisher and public/preview address selection.
    pub site: Key,
    /// Exact enquiry or acquisition page on that site.
    pub content: Key,
}

/// One project's role at one publication site.
///
/// This edge describes where project documentation is canonically published. It is
/// independent from content syndication, route aliases or redirects, and repository
/// origin/fork lineage.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectPlacementRole {
    /// The project's sole canonical publication placement.
    Primary,
    /// An additional publication placement whose canonical home remains the primary.
    Mirror,
}

impl ProjectPlacementRole {
    /// Whether this edge is the project's canonical publication placement.
    pub fn is_primary(self) -> bool {
        matches!(self, Self::Primary)
    }
}

/// Declared origin, not intellectual-property ownership, licensing or client status.
/// Derivative variants name one exact catalog project, never an inferred remote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProjectOrigin {
    /// Explicitly declared original project; does not establish exclusive rights.
    // A unit variant would silently accept extra fields despite deny_unknown_fields.
    Original {},
    /// Independently maintained successor derived from the named upstream project.
    SuccessorFork {
        /// Exact upstream project identity; no inherited roles or permission.
        upstream: Key,
    },
    /// Convenience mirror, not an independently maintained successor product.
    ConvenienceMirror {
        /// Exact mirrored project identity, independent of the local remote spelling.
        upstream: Key,
    },
}

impl ProjectOrigin {
    /// Stable declaration spelling shared by native persistence and linked-data projections.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Original {} => "original",
            Self::SuccessorFork { .. } => "successor-fork",
            Self::ConvenienceMirror { .. } => "convenience-mirror",
        }
    }

    /// Borrow the declared lineage endpoint without inspecting repository state.
    pub fn upstream(&self) -> Option<&Key> {
        match self {
            Self::Original {} => None,
            Self::SuccessorFork { upstream } | Self::ConvenienceMirror { upstream } => {
                Some(upstream)
            }
        }
    }
}

/// Workspace-owned taxonomy identity and display metadata, independent from collections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Taxonomy {
    /// Exact catalog identity; labels never supply identity or joins.
    pub id: Key,
    /// Explicit nonblank display label, preserved rather than slugified.
    pub label: String,
    /// Optional content-language scope; memberships require this exact normalized tag.
    /// Absence is language-neutral, not a default inferred for content or labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
    /// Terms scoped to this taxonomy, sorted and duplicate-checked by workspace resolution.
    #[serde(default)]
    pub terms: Vec<TaxonomyTerm>,
}

/// A term's identity is the composite `(taxonomy, term)`, not a global owner key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyTerm {
    /// Exact term key within its containing taxonomy.
    pub id: Key,
    /// Explicit nonblank label; equal labels do not merge terms.
    pub label: String,
}

impl Taxonomy {
    /// Look up an exact term in a workspace-resolved, sorted taxonomy catalog.
    /// No case, label, language or sibling-taxonomy fallback is performed.
    pub fn term(&self, key: &Key) -> Option<&TaxonomyTerm> {
        self.terms
            .binary_search_by(|term| term.id.cmp(key))
            .ok()
            .map(|index| &self.terms[index])
    }
}
