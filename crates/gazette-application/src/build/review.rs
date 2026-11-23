//! Exact private review material from the compiler's existing projections.

mod components;
mod response;
mod uses;
pub use response::{
    MatchedReviewResponse, ReviewDecision, ReviewOutcome, ReviewPolicy, ReviewResponseBinding,
    ReviewResponseError, ReviewResponseLimits, ReviewResponseStatus, ReviewSubmission,
};
pub(crate) use response::{match_captured_review_response, match_review_response};
pub use uses::{ReviewUse, ReviewUses};

use std::{collections::BTreeMap, fmt};

use gazette_compiler::authored;
use gazette_core::{
    publication::{Key, content},
    render,
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use serde::Serialize;
use serde_json::Value;

use super::{ResolvedPublication, serialization::encode};

/// Bounds for a complete private request; exhaustion never returns a subset.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewLimits {
    /// Maximum request JSON, selection JSON and aggregate JSON-pointer bytes, independently.
    #[schemars(range(max = usize::MAX))]
    pub bytes: usize,
    /// Maximum value occurrences, including containers and repeated nested references.
    #[schemars(range(max = usize::MAX))]
    pub uses: usize,
    /// Bounds on the existing site-graph projection before transport encoding.
    pub graph: content::Limits,
}

impl Default for ReviewLimits {
    fn default() -> Self {
        Self {
            bytes: 16 << 20,
            uses: 100_000,
            graph: content::Limits::default(),
        }
    }
}

/// A use context, not a declaration of public permission or discoverability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReviewTarget {
    /// Complete local page context, including its nested relationships and body fragment.
    Page {
        /// Exact selected home-site content identity.
        content: Key,
    },
    /// Complete local graph, including foreign endpoints and relationship evidence.
    SiteGraph,
    /// Byte-bound static transport, including assets not referenced by any page.
    Asset {
        /// Private logical corpus path; the payload separately records the public output.
        path: EntryPath,
    },
    /// Effective Theme/capability selections, not compiled presentation or resource output.
    Composition,
    /// Opaque inputs requiring a later capability compiler or explicit disposition.
    Uncompiled,
}

/// One immutable shared projection; values are serialized from its native owner once.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReviewSurface {
    target: ReviewTarget,
    value: Value,
}

impl ReviewSurface {
    /// Exact surface and subject; the same value in another surface is a different use.
    pub fn target(&self) -> &ReviewTarget {
        &self.target
    }

    /// Complete projection, never a flattened or repaired business schema.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Enumerate every value occurrence with its RFC 6901 pointer, including containers.
    /// Object keys and array order remain meaningful; absence is not an invented null field.
    pub fn uses(&self) -> ReviewUses<'_> {
        ReviewUses::new(&self.value)
    }
}

/// Engine-owned private request encoding, not the Website policy or release ABI.
/// No deserialization or mutable access can manufacture retained-input provenance.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReviewRequest {
    format: &'static str,
    site: Key,
    configuration_sha256: Sha256Digest,
    sources: BTreeMap<Key, TreeIdentity>,
    selection_sha256: Option<Sha256Digest>,
    surfaces: Vec<ReviewSurface>,
}

impl ReviewRequest {
    /// Exact selected site; destinations within projections come from its compiled routes.
    pub fn site(&self) -> &Key {
        &self.site
    }

    /// Digest of the complete candidate selection, including exclusions, encoded as compact
    /// serde JSON. None identifies the unselected private review, even when its surfaces
    /// happen to equal an all-included candidate. Neither state establishes permission.
    pub fn selection_sha256(&self) -> Option<Sha256Digest> {
        self.selection_sha256
    }

    /// Complete ordered material, including uncompiled obligations and private selections.
    pub fn surfaces(&self) -> &[ReviewSurface] {
        &self.surfaces
    }
}

/// Read-only review inventory, not approval, an admitted model or an artifact certificate.
///
/// Unselected reviews include drafts; both phases retain private values and source
/// coordinates. Do not publish this report.
/// Its identity binds exact configuration bytes (including source pins) and captured trees,
/// but not wall-clock observations. It is deliberately not a portable artifact identity:
/// configuration paths/comments are bound too. It excludes rendered Theme/resource bytes;
/// those still require later output review and disclosure validation.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PublicationReview {
    sha256: Sha256Digest,
    request: ReviewRequest,
}

impl PublicationReview {
    /// SHA-256 of compact serde JSON for this exact immutable request and format.
    /// An equal digest establishes equal request bytes, never owner authority or approval.
    pub fn sha256(&self) -> Sha256Digest {
        self.sha256
    }

    /// The sole retained request; no live source reads or policy calls are performed.
    pub fn request(&self) -> &ReviewRequest {
        &self.request
    }
}

impl ResolvedPublication<'_, '_> {
    /// Inventory all currently projectable uses for the selected publication from retained input.
    ///
    /// Uses the same page and graph producers as local rendering. No page-level flag masks
    /// nested journey members, roles, evidence, assets or foreign references. Markdown body
    /// projection runs under its existing policy and explicit raster decoder;
    /// Theme and resource programs do not run. Stored image bytes remain review
    /// inputs, not disclosure-approved or sanitized representations.
    /// This does not filter public output, invoke policy, certify bytes or authorize release.
    ///
    /// # Errors
    /// Rejects projection failures, unsupported JSON depth and complete-request resource
    /// limits without a successful partial inventory. Existing checking is not weakened.
    pub fn review(
        &self,
        limits: ReviewLimits,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PublicationReview, ReviewError> {
        self.review_with_selection(None, limits, raster, markdown)
    }

    pub(super) fn review_with_selection(
        &self,
        selection: Option<&authored::Selection>,
        limits: ReviewLimits,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PublicationReview, ReviewError> {
        self.review_site(
            self.inputs().site(),
            self.inputs(),
            selection,
            limits,
            raster,
            markdown,
        )
    }

    pub(super) fn review_site(
        &self,
        site: &Key,
        presentation: &super::AcquiredInputs,
        selection: Option<&authored::Selection>,
        limits: ReviewLimits,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PublicationReview, ReviewError> {
        let selection_sha256 = selection
            .map(|selection| {
                encode(selection, limits.bytes, "review selection bytes")
                    .map(|bytes| Sha256Digest::of(&bytes))
                    .map_err(ReviewError::Encoding)
            })
            .transpose()?;
        let inputs = self.inputs();
        let metadata =
            components::metadata(presentation, site, limits).map_err(ReviewError::Metadata)?;
        let definitions = metadata
            .as_ref()
            .map(|metadata| metadata.effective_theme().components());
        let component_presentation =
            definitions.map(
                |definitions| gazette_compiler::markdown::ComponentPresentation {
                    definitions,
                    renderer: &components::SemanticChildren,
                },
            );
        let mut surfaces = Vec::new();
        let mut budget = Budget {
            bytes: limits.bytes,
            pointers: limits.bytes,
            uses: limits.uses,
        };
        budget.push(
            &mut surfaces,
            ReviewTarget::Composition,
            &metadata
                .as_ref()
                .map(|metadata| metadata.composition())
                .or_else(|| inputs.compilation().workspace.composition_for(site)),
        )?;
        for node in self
            .authored()
            .content()
            .graph()
            .nodes()
            .filter(|node| &node.site == site)
        {
            let page = self
                .authored()
                .project_page_with(&node.id, raster, markdown, None, component_presentation)
                .map_err(ReviewError::Page)?;
            budget.push(
                &mut surfaces,
                ReviewTarget::Page {
                    content: node.id.clone(),
                },
                &page,
            )?;
        }
        let graph = self
            .authored()
            .project_site_graph(site, limits.graph)
            .map_err(ReviewError::Graph)?;
        budget.push(&mut surfaces, ReviewTarget::SiteGraph, &graph)?;
        let assets = self
            .authored()
            .assets_for(site)
            .map_err(ReviewError::Asset)?;
        for (path, asset) in assets.entries() {
            budget.push(
                &mut surfaces,
                ReviewTarget::Asset { path: path.clone() },
                asset,
            )?;
        }
        budget.push(
            &mut surfaces,
            ReviewTarget::Uncompiled,
            &assets.uncompiled(),
        )?;
        let request = ReviewRequest {
            format: "gazette-publication-review/1",
            site: site.clone(),
            configuration_sha256: inputs.compilation().source_sha256,
            sources: inputs
                .sources()
                .iter()
                .map(|(key, input)| (key.clone(), input.inventory().identity()))
                .collect(),
            selection_sha256,
            surfaces,
        };
        let bytes = encode(&request, limits.bytes, "review request bytes")
            .map_err(ReviewError::Encoding)?;
        Ok(PublicationReview {
            sha256: Sha256Digest::of(&bytes),
            request,
        })
    }
}

struct Budget {
    bytes: usize,
    pointers: usize,
    uses: usize,
}

impl Budget {
    fn push(
        &mut self,
        surfaces: &mut Vec<ReviewSurface>,
        target: ReviewTarget,
        value: &impl Serialize,
    ) -> Result<(), ReviewError> {
        let bytes =
            encode(value, self.bytes, "review projection bytes").map_err(ReviewError::Encoding)?;
        self.bytes -= bytes.len();
        let surface = ReviewSurface {
            target,
            value: serde_json::from_slice(&bytes).map_err(ReviewError::Json)?,
        };
        for usage in surface.uses() {
            self.uses = self.uses.checked_sub(1).ok_or(ReviewError::Limit("uses"))?;
            self.pointers = self
                .pointers
                .checked_sub(usage.pointer.len())
                .ok_or(ReviewError::Limit("JSON-pointer bytes"))?;
        }
        surfaces.push(surface);
        Ok(())
    }
}

/// Review stopped without an inventory; no failure is converted into implicit exclusion.
#[derive(Debug)]
pub enum ReviewError {
    /// Selected component definitions could not be checked from retained metadata.
    Metadata(super::MetadataError),
    /// The existing page producer rejected a required value or body reference.
    Page(authored::ProjectionError),
    /// The existing graph producer rejected its complete site view.
    Graph(authored::SiteGraphError),
    /// Static transport could not account for its inputs.
    Asset(authored::AssetError),
    /// Bounded native serialization failed.
    Encoding(render::Error),
    /// Serialized native data exceeds supported JSON structure.
    Json(serde_json::Error),
    /// No truncated review is returned at a logical resource ceiling.
    Limit(&'static str),
}

impl ReviewError {
    /// Stable class; native projection failures retain their original classification.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Metadata(error) => error.code(),
            Self::Page(error) => error.code(),
            Self::Graph(error) => error.code(),
            Self::Asset(error) => error.code(),
            Self::Encoding(render::Error::Limit(_)) | Self::Limit(_) => "publication_review_limit",
            Self::Encoding(_) | Self::Json(_) => "publication_review_encoding",
        }
    }
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => error.fmt(f),
            Self::Page(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
            Self::Asset(error) => error.fmt(f),
            Self::Encoding(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Limit(resource) => write!(
                f,
                "publication review {resource} ceiling exceeded; no complete request produced"
            ),
        }
    }
}

impl std::error::Error for ReviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            Self::Page(error) => Some(error),
            Self::Graph(error) => Some(error),
            Self::Asset(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Limit(_) => None,
        }
    }
}
