//! Write-free logical placement preflight and whole-set source acquisition.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use gazette_core::source::{
    CapturedSource, CommitId, EntryPath, Error, Inventory, RevisionLimits, Subdirectory,
    TargetPath, TreeInput,
};
use gazette_ports::source::{RevisionReader, SourceCapture};

use super::{
    AcquiredSource, Batch, BatchError, DevelopmentSource, VerifiedSource, VerifiedSubmodule,
};

/// An explicit source choice; absence or failure never selects another variant.
#[derive(Clone, Debug)]
pub enum SourceSelection {
    /// One bounded local tree with no claimed repository lineage.
    Filesystem {
        /// Explicit native root.
        root: PathBuf,
        /// Optional confined selection beneath that root.
        subdirectory: Option<Subdirectory>,
    },
    /// A local tree with explicitly excluded subtrees, without implicit ignores.
    FilteredFilesystem {
        /// Explicit native root.
        root: PathBuf,
        /// Literal capture-relative paths excluded before reading.
        exclude: Vec<EntryPath>,
    },
    /// Only the named local files or directory trees, retaining root-relative paths.
    IncludedFilesystem {
        /// Explicit native root.
        root: PathBuf,
        /// Required literal selections; unrelated siblings are not captured.
        include: Vec<EntryPath>,
        /// Optional branch checked before and after capture, without changing Git state.
        expected_branch: Option<String>,
    },
    /// Tracked and nonignored working files beneath one exact repository root.
    WorkingRepository {
        /// Exact native repository root.
        root: PathBuf,
        /// Optional folder; absence selects the exact repository root.
        subdirectory: Option<Subdirectory>,
        /// Optional branch checked before and after capture.
        expected_branch: Option<String>,
    },
    /// Documentation in one exact checked-out repository.
    Repository {
        /// Explicit native repository root.
        root: PathBuf,
        /// Literal documentation selection beneath that root.
        subdirectory: Subdirectory,
        /// Exact checked-out commit.
        commit: CommitId,
    },
    /// Documentation in a child pinned by the selected parent's gitlink.
    Submodule {
        /// Explicit native parent repository root.
        parent: PathBuf,
        /// Exact checked-out parent commit.
        parent_commit: CommitId,
        /// Literal child location beneath the parent.
        submodule_path: Subdirectory,
        /// Literal documentation selection beneath the child.
        subdirectory: Subdirectory,
    },
}

/// One source placement within a single site's future content tree.
#[derive(Clone, Debug)]
pub struct PlacementRequest {
    /// Nonempty normalized logical target.
    pub target: TargetPath,
    /// Explicit source to acquire; pinned repository selections retain mandatory pins.
    pub source: SourceSelection,
}

/// Bounds for the complete set, in addition to each source's native-read limits.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparationLimits {
    /// Maximum number of requested placements.
    #[schemars(range(max = usize::MAX))]
    pub sources: usize,
    /// Maximum retained source entries across all placements.
    #[schemars(range(max = usize::MAX))]
    pub entries: usize,
    /// Maximum retained payload bytes across all placements.
    #[schemars(range(max = u64::MAX))]
    pub bytes: u64,
    /// Per-source file/depth/entry/byte limits and Git metadata allowance.
    pub per_source: RevisionLimits,
}

/// Acquired payloads retain distinct filesystem, repository or parent-gitlink evidence.
#[derive(Debug)]
pub enum AcquiredInput {
    /// One complete retained local observation without revision lineage.
    Filesystem(CapturedSource),
    /// Direct repository proof and payloads.
    Repository(AcquiredSource<VerifiedSource>),
    /// Parent-gitlink proof and child payloads.
    Submodule(AcquiredSource<VerifiedSubmodule>),
    /// Working bytes with a separately checked Git baseline; never verified materialization.
    Development(DevelopmentSource),
}

impl AcquiredInput {
    /// Explicit development acquisition remains marked even when working bytes are clean.
    pub fn is_development(&self) -> bool {
        matches!(self, Self::Development(_))
    }
    /// The common structural observation, without discarding provenance.
    pub fn inventory(&self) -> &Inventory {
        match self {
            Self::Filesystem(source) => source.inventory(),
            Self::Repository(source) => source.verification().inventory(),
            Self::Submodule(source) => source.verification().source().inventory(),
            Self::Development(source) => source.inventory(),
        }
    }

    /// Read retained bytes without reopening either source kind.
    pub fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        match self {
            Self::Filesystem(source) => source.file(path),
            Self::Repository(source) => source.file(path),
            Self::Submodule(source) => source.file(path),
            Self::Development(source) => source.file(path),
        }
    }

    /// Project the retained tree for staging or adapter consumption without native reads.
    /// Revision and development evidence remain on this acquired source.
    ///
    /// # Errors
    /// Rejects any mismatch between the complete inventory and retained payloads.
    pub fn as_input(&self) -> Result<TreeInput<'_>, Error> {
        TreeInput::new(
            self.inventory(),
            self.inventory()
                .entries()
                .iter()
                .filter_map(|entry| {
                    self.file(&entry.path)
                        .map(|bytes| (entry.path.clone(), bytes))
                })
                .collect(),
        )
    }
}

/// One acquired placement; neither source nor target can be silently substituted.
#[derive(Debug)]
pub struct PreparedPlacement {
    request: PlacementRequest,
    source: AcquiredInput,
}

impl PreparedPlacement {
    /// Original explicit selection and normalized logical destination.
    pub fn request(&self) -> &PlacementRequest {
        &self.request
    }

    /// Immutable payloads and typed verification evidence.
    pub fn source(&self) -> &AcquiredInput {
        &self.source
    }
}

/// A complete acquired set with unique, non-overlapping logical targets.
///
/// This is not destination-filesystem admission, a generated-tree manifest,
/// publication-policy approval or certification. No partial set is returned.
#[derive(Debug)]
pub struct PreparedSourceSet {
    placements: Vec<PreparedPlacement>,
}

impl PreparedSourceSet {
    /// All requested placements in deterministic target order.
    pub fn placements(&self) -> &[PreparedPlacement] {
        &self.placements
    }
}

/// Why the complete logical source set could not be prepared.
#[derive(Debug)]
pub enum PreparationError {
    /// The request exceeds the selected source-count bound.
    TooManySources {
        /// Caller-selected maximum.
        limit: usize,
    },
    /// A reader returned data exceeding the remaining complete-set allowance.
    AggregateLimit {
        /// Placement whose observation exceeded the allowance.
        target: TargetPath,
        /// Resource exhausted across the complete set.
        resource: &'static str,
    },
    /// Two normalized targets are equal or one contains the other.
    Overlap {
        /// Existing target or ancestor.
        first: TargetPath,
        /// Conflicting target.
        second: TargetPath,
    },
    /// A selected source could not be acquired under its remaining allowance.
    Source {
        /// Placement that failed.
        target: TargetPath,
        /// Exact source failure.
        error: Error,
    },
}

impl fmt::Display for PreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManySources { limit } => write!(
                f,
                "source count exceeds {limit}; narrow the set or explicitly raise its bound"
            ),
            Self::AggregateLimit { target, resource } => write!(
                f,
                "{target:?} exceeds the source set's {resource}; inspect the reader and selected limits"
            ),
            Self::Overlap { first, second } => write!(
                f,
                "overlapping source targets {first:?} and {second:?}; select disjoint placements"
            ),
            Self::Source { target, error } => write!(
                f,
                "cannot prepare {target:?}: {error}; no source set was returned"
            ),
        }
    }
}

impl std::error::Error for PreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Validate the complete logical target set, then acquire every requested source.
///
/// No reads occur before target/count preflight. Acquisition is deterministic
/// and all-or-nothing in memory. This does not inspect or mutate destinations;
/// filesystem aliasing, source/output collision and writer admission remain
/// mandatory before any staged write. No Website required/optional policy is inferred.
///
/// # Errors
///
/// Rejects target overlap, excessive source count, or any source/aggregate limit
/// failure without returning earlier successfully acquired placements.
pub fn prepare(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    mut requests: Vec<PlacementRequest>,
    limits: PreparationLimits,
) -> Result<PreparedSourceSet, PreparationError> {
    if requests.len() > limits.sources {
        return Err(PreparationError::TooManySources {
            limit: limits.sources,
        });
    }
    requests.sort_by(|a, b| a.target.cmp(&b.target));
    let mut targets = BTreeMap::new();
    for request in &requests {
        let target = &request.target;
        if targets.insert(target.as_str(), target).is_some() {
            return Err(PreparationError::Overlap {
                first: target.clone(),
                second: target.clone(),
            });
        }
        for (index, _) in target.as_str().match_indices('/') {
            let ancestor = &target.as_str()[..index];
            if let Some(first) = targets.get(ancestor) {
                return Err(PreparationError::Overlap {
                    first: (**first).clone(),
                    second: target.clone(),
                });
            }
        }
    }
    let mut placements = Vec::with_capacity(requests.len());
    let mut batch = Batch::new(limits);
    for request in requests {
        let source =
            batch
                .acquire(reader, revisions, &request.source)
                .map_err(|error| match error {
                    BatchError::Native(error) => PreparationError::Source {
                        target: request.target.clone(),
                        error,
                    },
                    BatchError::Aggregate(resource) => PreparationError::AggregateLimit {
                        target: request.target.clone(),
                        resource,
                    },
                })?;
        placements.push(PreparedPlacement { request, source });
    }
    Ok(PreparedSourceSet { placements })
}
