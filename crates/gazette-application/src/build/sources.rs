//! Workspace-driven source acquisition through the existing confined reader path.

use std::fmt;

use gazette_compiler::{
    toml,
    workspace::{self, Compilation, Key},
};
use gazette_core::source::TargetPath;
use gazette_ports::source::{RevisionReader, SourceCapture};
use serde::Serialize;

use crate::source::{
    self, AcquiredInput, DevelopmentSource, PlacementRequest, PreparationError, PreparationLimits,
    PreparedSourceSet, VerifiedSource, VerifiedSubmodule,
};

/// One explicitly selected site's retained bytes and complete compiled configuration.
#[derive(Debug)]
pub struct AcquiredSite {
    configuration: crate::configuration::ResolvedWorkspace,
    site: Key,
    sources: PreparedSourceSet,
}

impl AcquiredSite {
    /// Complete configuration, including unselected sites, bound to its exact source bytes.
    pub fn compilation(&self) -> &Compilation {
        self.configuration.compilation()
    }

    /// Exact configuration capture, including the separately selected machine binding.
    pub fn configuration_evidence(&self) -> &crate::configuration::Evidence {
        self.configuration.evidence()
    }

    /// Explicit site selection; never inferred from the current directory.
    pub fn site(&self) -> &Key {
        &self.site
    }

    /// Retained bytes and kind-specific native evidence, ordered by logical target.
    pub fn sources(&self) -> &PreparedSourceSet {
        &self.sources
    }

    /// Project verification without serializing retained content or inventing a build receipt.
    pub fn inspect(&self) -> Inspection<'_> {
        Inspection {
            compilation: self.compilation(),
            configuration: self.configuration_evidence(),
            site: &self.site,
            placements: self
                .sources
                .placements()
                .iter()
                .map(|placement| PlacementInspection {
                    target: &placement.request().target,
                    evidence: SourceEvidence::from(placement.source()),
                })
                .collect(),
        }
    }
}

/// Source verification projection shared by SDK and CLI; not publication admission.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Inspection<'a> {
    /// Whole configuration and exact configuration-source digest.
    pub compilation: &'a Compilation,
    /// Exact supplied or native configuration evidence, separate from logical identity.
    pub configuration: &'a crate::configuration::Evidence,
    /// Explicit site selection.
    pub site: &'a Key,
    /// Every rendered placement, with no successfully acquired subset on failure.
    pub placements: Vec<PlacementInspection<'a>>,
}

/// One configured destination with exact native source evidence.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PlacementInspection<'a> {
    /// Normalized logical target, not a checked native output path.
    pub target: &'a TargetPath,
    /// Filesystem observation or repository/parent-gitlink evidence; never interchangeable.
    pub evidence: SourceEvidence<'a>,
}

/// Exact native verification or explicit retained observation, without payload copies.
#[derive(schemars::JsonSchema)]
#[schemars(with = "SourceEvidenceWire<'a>")]
pub enum SourceEvidence<'a> {
    /// Complete retained local observation without repository lineage.
    Filesystem(&'a gazette_core::source::Inventory),
    /// Direct repository pin and content inventory.
    Repository(&'a VerifiedSource),
    /// Parent pin, child gitlink and child content inventory.
    Submodule(&'a VerifiedSubmodule),
    /// Explicit working-byte observation and independently checked Git baseline.
    Development(&'a DevelopmentSource),
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SourceEvidenceWire<'a> {
    Filesystem {
        observation: &'a gazette_core::source::Inventory,
    },
    Repository {
        verification: &'a VerifiedSource,
    },
    Submodule {
        verification: &'a VerifiedSubmodule,
    },
    Development {
        observation: &'a DevelopmentSource,
    },
}

impl Serialize for SourceEvidence<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Filesystem(observation) => SourceEvidenceWire::Filesystem { observation },
            Self::Repository(verification) => SourceEvidenceWire::Repository { verification },
            Self::Submodule(verification) => SourceEvidenceWire::Submodule { verification },
            Self::Development(observation) => SourceEvidenceWire::Development { observation },
        }
        .serialize(serializer)
    }
}

impl<'a> From<&'a AcquiredInput> for SourceEvidence<'a> {
    fn from(input: &'a AcquiredInput) -> Self {
        match input {
            AcquiredInput::Filesystem(source) => Self::Filesystem(source.inventory()),
            AcquiredInput::Repository(source) => Self::Repository(source.verification()),
            AcquiredInput::Submodule(source) => Self::Submodule(source.verification()),
            AcquiredInput::Development(source) => Self::Development(source),
        }
    }
}

/// Whole configuration or selected-site source preparation failed; no partial result survives.
#[derive(Debug)]
pub enum Error {
    /// Explicit external publication facts are unresolved before source acquisition.
    Facts(gazette_core::publication::facts::Unavailable),
    /// Syntax/reference failure before native reads.
    Configuration(workspace::Error),
    /// The exact requested site is absent.
    UnknownSite(Key),
    /// An expected resolved source binding is missing; never select a fallback.
    MissingBinding(Key),
    /// A portable root lacks its selected workspace or machine binding.
    SourceRoot {
        /// Exact source binding whose root could not be resolved.
        binding: Key,
        /// Missing native workspace or named machine root.
        error: crate::configuration::SourceRootUnavailable,
    },
    /// Existing source preparation's pin, overlap, confinement or resource failure.
    Source(PreparationError),
}

impl Error {
    /// Stable failure class shared across public interfaces.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Facts(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::UnknownSite(_) => "workspace_site",
            Self::MissingBinding(_) => "workspace_source_binding",
            Self::SourceRoot { .. } => "workspace_source_root_unavailable",
            Self::Source(_) => "workspace_source",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Facts(error) => error.fmt(f),
            Self::Configuration(error) => error.fmt(f),
            Self::UnknownSite(key) => write!(
                f,
                "site {key:?} is not declared; select an exact configured site"
            ),
            Self::MissingBinding(key) => write!(
                f,
                "source binding for {key:?} is missing; recompile the complete declaration"
            ),
            Self::SourceRoot { binding, error } => {
                write!(f, "source binding for {binding:?}: {error}")
            }
            Self::Source(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Facts(error) => Some(error),
            Self::Configuration(error) => Some(error),
            Self::SourceRoot { error, .. } => Some(error),
            Self::Source(error) => Some(error),
            _ => None,
        }
    }
}

/// Compile the complete declaration, then acquire one selected site's rendered documentation.
///
/// All references resolve before native reads. Selected-site target/aggregate checks
/// reuse [`source::prepare`]. Disabled placements are not read; every selected input
/// must succeed. Website optional-degradation policy is not interpreted here.
/// No staging, database, network, source write or certification occurs.
///
/// # Errors
/// Rejects configuration, unknown site, missing binding, target overlap, pin,
/// confinement and resource failures without a partial acquired site.
pub fn acquire_site(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    source: &str,
    site: &Key,
    syntax: toml::Limits,
    limits: PreparationLimits,
) -> Result<AcquiredSite, Error> {
    let compilation = workspace::compile(source, syntax).map_err(Error::Configuration)?;
    acquire_resolved_site(
        reader,
        revisions,
        crate::configuration::ResolvedWorkspace::supplied(compilation),
        site,
        limits,
    )
}

/// Acquire one site's documentation from an already captured workspace and machine binding.
///
/// # Errors
/// Rejects unresolved facts, unknown sites, unbound roots and failed source preparation.
pub fn acquire_resolved_site(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    site: &Key,
    limits: PreparationLimits,
) -> Result<AcquiredSite, Error> {
    let workspace = &configuration.compilation().workspace;
    workspace.require_resolved_facts().map_err(Error::Facts)?;
    if !workspace.sites().any(|declared| &declared.id == site) {
        return Err(Error::UnknownSite(site.clone()));
    }
    let mut requests = Vec::new();
    for placement in workspace
        .placements()
        .iter()
        .filter(|placement| &placement.site == site && placement.render)
    {
        let documents = placement
            .documents
            .as_ref()
            .ok_or_else(|| Error::MissingBinding(placement.project.clone()))?;
        let binding = workspace
            .source(&documents.source)
            .ok_or_else(|| Error::MissingBinding(documents.source.clone()))?;
        let source =
            super::binding::selection(&configuration, &binding.location).map_err(|error| {
                Error::SourceRoot {
                    binding: binding.id.clone(),
                    error,
                }
            })?;
        requests.push(PlacementRequest {
            target: documents.target.clone(),
            source,
        });
    }
    for content in workspace
        .content_bindings()
        .filter(|content| content.site == site && content.documentation_mount.is_some())
    {
        let documents = content
            .tree_binding()
            .ok_or_else(|| Error::MissingBinding(content.source_key().clone()))?;
        let binding = workspace
            .source(content.source_key())
            .ok_or_else(|| Error::MissingBinding(content.source_key().clone()))?;
        let selected =
            super::binding::selection(&configuration, &binding.location).map_err(|error| {
                Error::SourceRoot {
                    binding: binding.id.clone(),
                    error,
                }
            })?;
        requests.push(PlacementRequest {
            target: documents.target.clone(),
            source: selected,
        });
    }
    let sources = source::prepare(reader, revisions, requests, limits).map_err(Error::Source)?;
    Ok(AcquiredSite {
        configuration,
        site: site.clone(),
        sources,
    })
}
