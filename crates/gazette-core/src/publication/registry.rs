//! Private relational projection vocabulary, not source truth or public admission.
//!
//! Catalogs and content borrow the same resolved values used by publication outputs.
//! The application supplies complete workspace scope; a site's filtered output is
//! not a valid replacement set. Storage must not infer relationships from these rows.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{
    Key, Principal, Project, ProjectPlacementRole, Taxonomy, content::Graph, routing::PublicBaseUrl,
};
use crate::source::EntryPath;

/// Current private structural encoding; no predecessor interpretation is accepted.
pub const SCHEMA: &str = "gazette.registry/v1";

/// Complete structural registry view. This includes private, draft and archived data.
/// It is an inward storage contract, not a deserializable publication permission.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Projection<'a> {
    /// Exact existing principal records, including unused declarations.
    pub principals: BTreeMap<&'a Key, &'a Principal>,
    /// Existing project records with all three independent state axes.
    pub projects: BTreeMap<&'a Key, &'a Project>,
    /// Shared taxonomy catalogs and their normalized terms, not copied labels per page.
    pub taxonomies: BTreeMap<&'a Key, &'a Taxonomy>,
    /// Exact selected domain contracts; stored once, not reconstructed from page values.
    pub extensions: BTreeMap<&'a Key, &'a super::ExtensionSchema>,
    /// Brand display values; a brand key never becomes a principal or publisher.
    pub brands: BTreeMap<&'a Key, Brand<'a>>,
    /// Complete workspace site scope, including sites without any content.
    pub sites: BTreeMap<&'a Key, Site<'a>>,
    /// Exact ordered-by-key placement set; disabled placements remain represented.
    pub placements: Vec<Placement<'a>>,
    /// The existing complete resolved graph, including declaration evidence.
    pub content: &'a Graph,
    /// Already compiled addresses keyed by content identity; unrouted nodes are absent.
    pub routes: BTreeMap<&'a Key, Route<'a>>,
    /// One native source association per content identity, independent of project subjects.
    pub provenance: BTreeMap<&'a Key, Provenance<'a>>,
}

impl Projection<'_> {
    /// Hash this complete structural view with deterministic typed JSON framing.
    /// This excludes bodies, assets and operational state; it is not a release ABI.
    ///
    /// # Errors
    /// Refuses complete-encoding overflow or serialization failure without a partial digest.
    pub fn semantic_sha256(
        &self,
        maximum_bytes: usize,
    ) -> Result<crate::source::Sha256Digest, crate::render::Error> {
        let mut output = crate::render::LimitedWriter::new(Vec::new(), maximum_bytes);
        let result = serde_json::to_writer(&mut output, &(SCHEMA, self));
        if output.exceeded() {
            return Err(crate::render::Error::Limit("registry bytes"));
        }
        result.map_err(|error| crate::render::Error::Context(Box::new(error)))?;
        Ok(crate::source::Sha256Digest::of(&output.into_inner()))
    }
}

/// Brand projection; publication choices stay in their workspace configuration owner.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Brand<'a> {
    /// Explicit display name, never a natural key.
    pub name: &'a str,
    /// Exact configured brand parent; no principal, publisher or role inheritance.
    pub parent: Option<&'a Key>,
}

/// Site relationships and publication address, without copying composition machinery.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Site<'a> {
    /// Exact configured publisher principal; no author or brand fallback.
    pub publisher: &'a Key,
    /// Exact site rights selection, independent from page attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights_holder: Option<&'a Key>,
    /// Exact selected person for scoped policy; does not create content authorship.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_author: Option<&'a Key>,
    /// Normalized explicit site responsibilities; absence never infers publisher roles.
    pub roles: &'a [super::PublicationRoleBinding],
    /// Configured release eligibility, not authorization or readiness.
    pub deployable: bool,
    /// Explicit brand association; an unbound site does not invent a brand.
    pub brand: Option<&'a Key>,
    /// Sole configured public base; none is not a guessed domain or preview address.
    pub public_base_url: Option<&'a PublicBaseUrl>,
    /// Explicit public repository from the owning brand publication, not its private remote.
    pub repository: Option<&'a str>,
    /// Complete site-owned discussion atom; never inherited from another owner.
    pub discussion: Option<&'a super::Discussion>,
}

/// Project placement is independent from whether any documentation was admitted.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Placement<'a> {
    /// Exact site endpoint.
    pub site: &'a Key,
    /// Exact project endpoint.
    pub project: &'a Key,
    /// Exact canonical publication role; no route or lineage relationship is inferred.
    pub role: ProjectPlacementRole,
    /// Explicit materialization selection.
    pub render: bool,
    /// Independent discovery selection.
    pub discoverable: bool,
}

/// Resolved address projection; it borrows rather than recomputes compiler URLs.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Route<'a> {
    /// This content's serving address, distinct from a selected original's address.
    pub url: &'a str,
    /// Explicit original content endpoint; none means this route is its own canonical home.
    pub canonical: Option<&'a Key>,
    /// Absolute canonical address from the existing route compiler.
    pub canonical_url: &'a str,
    /// Exact site-relative generated file path.
    pub output: &'a EntryPath,
    /// Every compiled alias, retaining the compiler's normalized order.
    pub aliases: Vec<Alias<'a>>,
    /// Explicit cross-site redirects to this content's one canonical home.
    pub mirrors: Vec<Mirror<'a>>,
}

/// A redirect presentation, not another content object, source or attribution.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Mirror<'a> {
    /// Exact destination site of the redirect file, not the canonical content home.
    pub site: &'a Key,
    /// Absolute address already resolved by the route compiler.
    pub url: &'a str,
    /// Exact output within the destination site.
    pub output: &'a EntryPath,
}

/// An alias points to the containing record's canonical URL, never another chain.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Alias<'a> {
    /// Absolute compiled alias address.
    pub url: &'a str,
    /// Exact site-relative output file.
    pub output: &'a EntryPath,
}

/// Source provenance does not confer authorship, about, ownership or permission.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Provenance<'a> {
    /// Exact workspace source binding, not an ambient repository path.
    pub source: &'a Key,
    /// Literal capture-relative source entry, not a route or logical mount prefix.
    pub entry: &'a str,
    /// Native project source, if this is project documentation rather than authored copy.
    pub project: Option<&'a Key>,
}
