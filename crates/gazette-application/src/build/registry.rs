//! Prepare the complete private registry view from retained publication meaning.

use std::{collections::BTreeMap, fmt};

use gazette_core::{publication::registry, render, source::Sha256Digest};
use serde::Serialize;

use super::{ResolvedPublication, serialization};

mod persistence;
pub use persistence::{RegistryReconcileError, RegistryReconcileOptions};

/// Independent encoded projection bound, after the existing syntax/graph/capture limits.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RegistryLimits {
    /// Maximum bytes in the complete private projection's compact JSON encoding.
    /// Borrowed row-index allocation is additionally bounded by the input compiler limits.
    #[schemars(range(max = usize::MAX))]
    pub bytes: usize,
}

impl Default for RegistryLimits {
    fn default() -> Self {
        Self { bytes: 16 << 20 }
    }
}

/// Closed structural phase of one private registry projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum RegistryStatus {
    /// Structural resolution completed without publication admission.
    #[serde(rename = "resolved-unadmitted")]
    ResolvedUnadmitted,
}

/// A complete, bounded private projection; not a store write, admitted head or release.
/// Construction is only available on a source-bound resolved publication.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RegistryProjection<'a> {
    /// Current Gazette projection format, with no predecessor fallback.
    schema: &'static str,
    /// Fixed structural phase; persistence cannot promote this into public admission.
    status: RegistryStatus,
    /// Deterministic encoding identity of the complete structural model below.
    /// Excludes raw input ordering, native paths, source pins and operational coordinates.
    semantic_sha256: Sha256Digest,
    /// Exact original workspace bytes, separate from normalized structural identity.
    configuration_sha256: Sha256Digest,
    /// Original full-document byte identities, not normalized semantic content hashes.
    documents: BTreeMap<&'a gazette_core::publication::Key, Sha256Digest>,
    /// Complete typed registry model, borrowed from the same compiler result as rendering.
    model: registry::Projection<'a>,
}

impl RegistryProjection<'_> {
    /// Complete private rows and graph; no site or lifecycle filter was applied.
    pub fn model(&self) -> &registry::Projection<'_> {
        &self.model
    }

    /// Structural identity, not full build identity, exact input identity or authorization.
    pub fn semantic_sha256(&self) -> Sha256Digest {
        self.semantic_sha256
    }
}

/// Projection preparation failed before any writer acquisition or database effect.
#[derive(Debug)]
pub enum RegistryError {
    /// Unresolved external facts cannot enter a registry projection or reconciliation.
    Facts(gazette_core::publication::facts::Unavailable),
    /// A source association cannot be joined to retained content.
    Binding(gazette_core::publication::Key),
    /// Bounded deterministic serialization failed; no successful subset is returned.
    Encoding(render::Error),
}

impl RegistryError {
    /// Stable failure class; the source error retains encoding failure details.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Facts(error) => error.code(),
            Self::Binding(_) => "registry_source_binding",
            Self::Encoding(_) => "registry_encoding",
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Facts(error) => error.fmt(f),
            Self::Binding(key) => write!(
                f,
                "registry content {key} lacks retained source evidence; no complete projection prepared"
            ),
            Self::Encoding(error) => write!(
                f,
                "registry projection: {error}; no complete projection prepared"
            ),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Facts(error) => Some(error),
            Self::Binding(_) => None,
            Self::Encoding(error) => Some(error),
        }
    }
}

impl ResolvedPublication<'_, '_> {
    /// Prepare the entire configured publication universe as private registry rows.
    ///
    /// Uses existing catalog, graph, route and source-binding owners without native reads,
    /// renderer queries or another semantic resolution pass. The acquisition's consuming
    /// site does not filter the registry. Empty content remains a complete empty content
    /// set with all configured catalogs/sites, never a skip-reconciliation instruction.
    ///
    /// # Errors
    /// Refuses inconsistent retained associations or complete-encoding limits before a
    /// successful projection. This never acquires a writer, initializes or changes a store.
    pub fn registry(
        &self,
        limits: RegistryLimits,
    ) -> Result<RegistryProjection<'_>, RegistryError> {
        let authored = self.authored();
        let content = authored.content();
        let workspace = &content.compilation().workspace;
        workspace
            .require_resolved_facts()
            .map_err(RegistryError::Facts)?;
        let publications: BTreeMap<_, _> = workspace
            .brands()
            .flat_map(|brand| {
                brand
                    .publications
                    .iter()
                    .map(move |publication| (&publication.site, (&brand.id, publication)))
            })
            .collect();
        let mut routes = BTreeMap::new();
        let mut provenance = BTreeMap::new();
        let mut documents = BTreeMap::new();
        for node in content.graph().nodes() {
            let missing = || RegistryError::Binding(node.id.clone());
            let binding = authored.source_binding(&node.id).map_err(|_| missing())?;
            let path = authored.source_path(&node.id).ok_or_else(missing)?;
            provenance.insert(
                &node.id,
                registry::Provenance {
                    source: binding.source_key(),
                    entry: binding.entry(path).ok_or_else(missing)?,
                    project: binding.project,
                },
            );
            documents.insert(
                &node.id,
                authored.file(&node.id).ok_or_else(missing)?.source_sha256(),
            );
            if let Some(route) = content.route(&node.id) {
                routes.insert(
                    &node.id,
                    registry::Route {
                        url: &route.url,
                        canonical: node
                            .route
                            .as_ref()
                            .and_then(|route| route.canonical.as_ref()),
                        canonical_url: &route.canonical_url,
                        output: &route.output,
                        aliases: route
                            .aliases
                            .iter()
                            .map(|alias| registry::Alias {
                                url: &alias.url,
                                output: &alias.output,
                            })
                            .collect(),
                        mirrors: route
                            .mirrors
                            .iter()
                            .map(|mirror| registry::Mirror {
                                site: &mirror.site,
                                url: &mirror.url,
                                output: &mirror.output,
                            })
                            .collect(),
                    },
                );
            }
        }
        let model = registry::Projection {
            principals: workspace.principals().map(|row| (&row.id, row)).collect(),
            projects: content.projects().map(|row| (&row.id, row)).collect(),
            taxonomies: workspace.taxonomies().map(|row| (&row.id, row)).collect(),
            extensions: workspace.extensions().map(|row| (&row.id, row)).collect(),
            brands: workspace
                .brands()
                .map(|row| {
                    (
                        &row.id,
                        registry::Brand {
                            name: &row.name,
                            parent: row.parent.as_ref(),
                        },
                    )
                })
                .collect(),
            sites: workspace
                .sites()
                .map(|site| {
                    let publication = publications.get(&site.id);
                    (
                        &site.id,
                        registry::Site {
                            publisher: &site.publisher,
                            rights_holder: site.rights_holder.as_ref(),
                            default_author: site.default_author.as_ref(),
                            roles: &site.roles,
                            deployable: site.deployable,
                            brand: publication.map(|(brand, _)| *brand),
                            public_base_url: publication
                                .and_then(|(_, publication)| publication.public_base_url.as_ref()),
                            repository: publication
                                .and_then(|(_, publication)| publication.repository.as_deref()),
                            discussion: site.discussion.as_ref(),
                        },
                    )
                })
                .collect(),
            placements: workspace
                .placements()
                .iter()
                .map(|placement| registry::Placement {
                    site: &placement.site,
                    project: &placement.project,
                    role: placement.role,
                    render: placement.render,
                    discoverable: placement.discoverable,
                })
                .collect(),
            content: content.graph(),
            routes,
            provenance,
        };
        // Typed struct field order and BTree collections define this encoding. This
        // is not a claim of RFC 8785 canonicalization or a release signature format.
        let semantic_sha256 = model
            .semantic_sha256(limits.bytes)
            .map_err(RegistryError::Encoding)?;
        let projection = RegistryProjection {
            schema: registry::SCHEMA,
            status: RegistryStatus::ResolvedUnadmitted,
            semantic_sha256,
            configuration_sha256: content.compilation().source_sha256,
            documents,
            model,
        };
        // Bound the full report as well: exact-input evidence is not free metadata.
        serialization::encode(&projection, limits.bytes, "registry bytes")
            .map_err(RegistryError::Encoding)?;
        Ok(projection)
    }
}
