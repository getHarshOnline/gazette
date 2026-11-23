//! Borrowed private operation facts, never deserializable ownership or write authority.

use std::{collections::BTreeMap, path::Path};

use gazette_core::{
    destination::{self, OutputBinding},
    publication::Key,
    render,
    source::{self, EntryPath, Inventory, Sha256Digest, TreeIdentity},
    store,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::{
    MaterializationError, MaterializationResult, MaterializationSetPlanError,
    MaterializationSetPlanFailure, MaterializationSetWriteError, MaterializedOutput,
    OutputPlanError,
};
use crate::build::OutputSetPlanFailure;

/// Actual synchronous native output read-back; not an imported ownership receipt.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterializedOutputInspection<'a> {
    /// Serving publication selected by the original plan.
    pub site: &'a Key,
    /// Explicit configured native destination.
    pub root: &'a Path,
    /// Native root and administration identity observed during this operation.
    pub binding: &'a OutputBinding,
    /// Exact generated ownership subset.
    pub generated: &'a Inventory,
    /// Complete actual read-back, including preserved unowned entries.
    pub inventory: &'a Inventory,
}

/// Exact durable history and, separately, this invocation's actual native output.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterializationInspection<'a> {
    /// Independently verified original intent and completion, including replay status.
    pub history: &'a store::MaterializationReceipt,
    /// Null on historical replay; history does not prove current filesystem contents.
    pub output: Option<MaterializedOutputInspection<'a>>,
}

/// Storage diagnostic chain without formatting provider messages or copying SQL data.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterializationStoreInspection {
    /// Native stable diagnostic, including coordinator uncertainty when present.
    pub code: &'static str,
    /// Native provider or operating-system numeric code, when available.
    pub native_code: Option<i32>,
    /// Nested native coordinator cause; absence is not a successful operation.
    pub cause: Option<Box<Self>>,
}

impl MaterializationStoreInspection {
    fn from_error(error: &store::Error) -> Self {
        Self {
            code: error.code(),
            native_code: match error {
                store::Error::Provider { code } => *code,
                store::Error::Io(error) => error.raw_os_error(),
                _ => None,
            },
            cause: match error {
                store::Error::CoordinatorUnverified(error) => {
                    Some(Box::new(Self::from_error(error)))
                }
                _ => None,
            },
        }
    }
}

/// Native source observation facts; no path or inventory is reconstructed.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputSourceInspection<'a> {
    /// Failed operating-system observation.
    Io {
        /// Original operation.
        operation: &'static str,
        /// Exact selected native path.
        path: &'a Path,
        /// Native operating-system diagnostic code.
        native_code: Option<i32>,
    },
    /// Unsupported native entry kind.
    Unsupported {
        /// Exact native path.
        path: &'a Path,
    },
    /// Ambiguous native name.
    InvalidName {
        /// Exact native path.
        path: &'a Path,
    },
    /// Inventory admission invariant failed.
    InvalidInventory {
        /// Exact native path.
        path: &'a Path,
        /// Native invariant.
        reason: &'static str,
    },
    /// Complete observation exceeded its declared ceiling.
    Limit {
        /// Exact native path.
        path: &'a Path,
        /// Native bounded resource.
        resource: &'static str,
    },
    /// Native entry changed while being observed.
    Changed {
        /// Exact native path.
        path: &'a Path,
    },
    /// Checked-out branch differs from the source requirement.
    BranchMismatch {
        /// Selected repository worktree.
        path: &'a Path,
        /// Required local branch name.
        expected: &'a str,
        /// Observed branch ref; absent for detached HEAD.
        actual: Option<&'a str>,
    },
    /// Explicit repository pin differs from the observed revision.
    RevisionMismatch {
        /// Selected repository.
        path: &'a Path,
        /// Exact requested revision.
        expected: &'a str,
        /// Exact observed revision.
        actual: &'a str,
    },
    /// Selected tree entry does not match its retained content.
    ContentMismatch {
        /// Exact native path.
        path: &'a Path,
    },
    /// Complete retained and observed trees differ.
    TreeMismatch {
        /// Selected root.
        path: &'a Path,
        /// Exact retained structural identity.
        expected: &'a TreeIdentity,
        /// Exact observed structural identity.
        actual: &'a TreeIdentity,
    },
}

impl<'a> OutputSourceInspection<'a> {
    fn from_error(error: &'a source::Error) -> Self {
        match error {
            source::Error::Io {
                operation,
                path,
                source,
            } => Self::Io {
                operation,
                path,
                native_code: source.raw_os_error(),
            },
            source::Error::Unsupported(path) => Self::Unsupported { path },
            source::Error::InvalidName(path) => Self::InvalidName { path },
            source::Error::InvalidInventory { path, reason } => {
                Self::InvalidInventory { path, reason }
            }
            source::Error::Limit { path, resource } => Self::Limit { path, resource },
            source::Error::Changed(path) => Self::Changed { path },
            source::Error::BranchMismatch {
                path,
                expected,
                actual,
            } => Self::BranchMismatch {
                path,
                expected,
                actual: actual.as_deref(),
            },
            source::Error::RevisionMismatch {
                path,
                expected,
                actual,
            } => Self::RevisionMismatch {
                path,
                expected,
                actual,
            },
            source::Error::ContentMismatch(path) => Self::ContentMismatch { path },
            source::Error::TreeMismatch {
                path,
                expected,
                actual,
            } => Self::TreeMismatch {
                path,
                expected,
                actual,
            },
        }
    }
}

/// Complete native destination refusal, including protected roots and collisions.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputDestinationInspection<'a> {
    /// Native source observation failed.
    Observation {
        /// Original stable source diagnostic.
        code: &'static str,
        /// Exact structured native cause.
        cause: OutputSourceInspection<'a>,
    },
    /// Destination and protected source are not natively separate.
    SourceOverlap {
        /// Actual protected source root.
        source: &'a Path,
        /// Actual configured destination root.
        destination: &'a Path,
    },
    /// Native namespace collision.
    Collision {
        /// First exact spelling.
        first: &'a str,
        /// Conflicting exact spelling.
        second: &'a str,
    },
}

impl<'a> OutputDestinationInspection<'a> {
    fn from_error(error: &'a destination::Error) -> Self {
        match error {
            destination::Error::Observation(error) => Self::Observation {
                code: error.code(),
                cause: OutputSourceInspection::from_error(error),
            },
            destination::Error::SourceOverlap {
                source,
                destination,
            } => Self::SourceOverlap {
                source,
                destination,
            },
            destination::Error::Collision { first, second } => Self::Collision { first, second },
        }
    }
}

/// Original write-free output-plan refusal.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputPlanCauseInspection<'a> {
    /// Local hyperlink projections cannot enter maintained publication output.
    Preview,
    /// Publication has no configured destination.
    Unconfigured,
    /// Detached configuration has no retained relative-path coordinate.
    Coordinate,
    /// Prior ownership belongs to another site or native binding.
    PriorOwner,
    /// Previously generated entry changed.
    Changed {
        /// Exact original entry.
        path: &'a EntryPath,
    },
    /// Generated path enters reserved administration.
    Reserved {
        /// Exact proposed entry.
        path: &'a EntryPath,
    },
    /// Proposed entry collides with retained or unowned content.
    Collision {
        /// First exact spelling.
        first: &'a EntryPath,
        /// Conflicting exact spelling.
        second: &'a EntryPath,
    },
    /// Native observation failed before effects.
    Observation {
        /// Exact native cause.
        cause: OutputDestinationInspection<'a>,
    },
    /// Complete proposed tree failed admission.
    Output {
        /// Native stable source diagnostic.
        code: &'static str,
        /// Exact native cause.
        cause: OutputSourceInspection<'a>,
    },
}

impl<'a> OutputPlanCauseInspection<'a> {
    fn from_error(error: &'a OutputPlanError) -> Self {
        match error {
            OutputPlanError::Preview => Self::Preview,
            OutputPlanError::Unconfigured => Self::Unconfigured,
            OutputPlanError::Coordinate(_) => Self::Coordinate,
            OutputPlanError::PriorOwner => Self::PriorOwner,
            OutputPlanError::Changed(path) => Self::Changed { path },
            OutputPlanError::Reserved(path) => Self::Reserved { path },
            OutputPlanError::Collision { first, second } => Self::Collision { first, second },
            OutputPlanError::Observation(error) => Self::Observation {
                cause: OutputDestinationInspection::from_error(error),
            },
            OutputPlanError::Output(error) => Self::Output {
                code: error.code(),
                cause: OutputSourceInspection::from_error(error),
            },
        }
    }
}

/// Exact typed evidence admission failure; opaque renderer errors retain their class.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterializationEvidenceInspection<'a> {
    /// Invalid retained render input.
    Input {
        /// Exact input coordinate.
        path: &'a EntryPath,
        /// Native invariant.
        reason: &'static str,
    },
    /// Bounded evidence could not be completely encoded.
    Limit {
        /// Exact bounded resource.
        resource: &'static str,
    },
    /// Context is not an object.
    ContextShape,
    /// Opaque native template provider failed; no provider message is copied.
    Template,
    /// Opaque native context encoder failed; no provider message is copied.
    Context,
}

impl<'a> MaterializationEvidenceInspection<'a> {
    fn from_error(error: &'a render::Error) -> Self {
        match error {
            render::Error::Input { path, reason } => Self::Input { path, reason },
            render::Error::Limit(resource) => Self::Limit { resource },
            render::Error::ContextShape => Self::ContextShape,
            render::Error::Template(_) => Self::Template,
            render::Error::Context(_) => Self::Context,
        }
    }
}

/// Exact failed phase and independently retained native effect facts.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum MaterializationErrorInspection<'a> {
    /// Filesystem effects were not permitted.
    Permission {
        /// Stable application diagnostic.
        code: &'static str,
    },
    /// Write-free output planning failed.
    Plan {
        /// Stable application diagnostic.
        code: &'static str,
        /// Exact original output refusal.
        cause: OutputPlanCauseInspection<'a>,
    },
    /// Held history observation or permission failed.
    Store {
        /// Stable application diagnostic.
        code: &'static str,
        /// Native history diagnostic chain.
        cause: MaterializationStoreInspection,
    },
    /// Complete private evidence could not be encoded.
    Evidence {
        /// Stable application diagnostic.
        code: &'static str,
        /// Native evidence diagnostic.
        cause_code: &'static str,
        /// Typed retained evidence failure.
        cause: MaterializationEvidenceInspection<'a>,
    },
    /// Intent append or independent verification failed; no FS dispatch followed.
    History {
        /// Stable application diagnostic.
        code: &'static str,
        /// Exact attempted operation, not a fabricated durable receipt.
        id: &'a Sha256Digest,
        /// Native history diagnostic chain.
        cause: MaterializationStoreInspection,
        /// Actual storage delivery uncertainty.
        effect: &'a store::MaterializationEffect,
    },
    /// Actual native writer failed after intent retention.
    Write {
        /// Stable application diagnostic.
        code: &'static str,
        /// Exact original operation.
        id: &'a Sha256Digest,
        /// Exact native writer refusal.
        cause: OutputDestinationInspection<'a>,
        /// Ordered original native filesystem attempts, including failed attempts.
        attempted: &'a [EntryPath],
        /// Exact native residue, if any; no cleanup or rollback is implied.
        residue: Option<&'a Path>,
    },
    /// Actual native read-back succeeded but durable completion is uncertain.
    Completion {
        /// Stable application diagnostic.
        code: &'static str,
        /// Exact original operation.
        id: &'a Sha256Digest,
        /// Native history diagnostic chain.
        cause: MaterializationStoreInspection,
        /// Actual completion storage delivery uncertainty.
        effect: &'a store::MaterializationEffect,
        /// Successful native output read-back, independently of journal delivery.
        output: MaterializedOutputInspection<'a>,
    },
}

/// Complete-set write-free refusal; never a partial successful plan.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterializationSetPlanInspection<'a> {
    /// Original complete requested site set.
    pub requested: &'a [Key],
    /// Exact failed set or site condition.
    pub cause: MaterializationPlanCauseInspection<'a>,
}

/// Exact native complete-set planning failure.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterializationPlanCauseInspection<'a> {
    /// Full membership, roots or aggregate budget refused before effects.
    Refused {
        /// Original native invariant.
        reason: &'static str,
    },
    /// Site output preflight failed.
    Output {
        /// Exact failed site.
        site: &'a Key,
        /// Stable original output diagnostic.
        code: &'static str,
        /// Exact output-plan refusal.
        cause: OutputPlanCauseInspection<'a>,
    },
    /// Site held history or full materialization planning failed.
    Site {
        /// Exact failed site.
        site: &'a Key,
        /// Original failed phase and cause.
        cause: MaterializationErrorInspection<'a>,
    },
}

/// Complete-set failure preserves every successful original prefix and failed site.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterializationSetWriteInspection<'a> {
    /// Original complete requested site set.
    pub requested: &'a [Key],
    /// Independently retained original successful results, not rollback claims.
    pub completed: BTreeMap<&'a Key, MaterializationInspection<'a>>,
    /// Exact failed site; null means complete-set validation failed before iteration.
    pub failed: Option<&'a Key>,
    /// Original failed phase and retained native effects.
    pub cause: MaterializationErrorInspection<'a>,
}

impl MaterializedOutput {
    /// Borrow exact native output facts without conferring ownership or write permission.
    pub fn inspect(&self) -> MaterializedOutputInspection<'_> {
        MaterializedOutputInspection {
            site: self.site(),
            root: self.root(),
            binding: self.binding(),
            generated: self.generated(),
            inventory: self.inventory(),
        }
    }
}

impl MaterializationResult {
    /// Borrow original history, retaining null actual output for historical replay.
    pub fn inspect(&self) -> MaterializationInspection<'_> {
        MaterializationInspection {
            history: &self.history,
            output: self.output.as_ref().map(MaterializedOutput::inspect),
        }
    }
}

impl MaterializationError {
    /// Borrow structured native effect and diagnostic facts without formatting messages.
    pub fn inspect(&self) -> MaterializationErrorInspection<'_> {
        let code = self.code();
        match self {
            Self::Permission => MaterializationErrorInspection::Permission { code },
            Self::Plan(error) => MaterializationErrorInspection::Plan {
                code,
                cause: OutputPlanCauseInspection::from_error(error),
            },
            Self::Store(error) => MaterializationErrorInspection::Store {
                code,
                cause: MaterializationStoreInspection::from_error(error),
            },
            Self::Evidence(error) => MaterializationErrorInspection::Evidence {
                code,
                cause_code: error.code(),
                cause: MaterializationEvidenceInspection::from_error(error),
            },
            Self::History { id, cause } => MaterializationErrorInspection::History {
                code,
                id,
                cause: MaterializationStoreInspection::from_error(&cause.cause),
                effect: &cause.effect,
            },
            Self::Write { id, cause } => MaterializationErrorInspection::Write {
                code,
                id,
                cause: OutputDestinationInspection::from_error(&cause.cause),
                attempted: &cause.attempted,
                residue: cause.residue.as_deref(),
            },
            Self::Completion { id, cause, output } => MaterializationErrorInspection::Completion {
                code,
                id,
                cause: MaterializationStoreInspection::from_error(&cause.cause),
                effect: &cause.effect,
                output: output.inspect(),
            },
        }
    }
}

impl MaterializationSetPlanError {
    /// Borrow the exact full-set write-free refusal; no effect is inferred.
    pub fn inspect(&self) -> MaterializationSetPlanInspection<'_> {
        MaterializationSetPlanInspection {
            requested: &self.requested,
            cause: match &self.cause {
                MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Refused(reason)) => {
                    MaterializationPlanCauseInspection::Refused { reason }
                }
                MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Site {
                    site,
                    error,
                }) => MaterializationPlanCauseInspection::Output {
                    site,
                    code: error.code(),
                    cause: OutputPlanCauseInspection::from_error(error),
                },
                MaterializationSetPlanFailure::Site { site, error } => {
                    MaterializationPlanCauseInspection::Site {
                        site,
                        cause: error.inspect(),
                    }
                }
            },
        }
    }
}

impl MaterializationSetWriteError {
    /// Borrow the original requested set, actual successful prefix and native failure.
    pub fn inspect(&self) -> MaterializationSetWriteInspection<'_> {
        MaterializationSetWriteInspection {
            requested: &self.requested,
            completed: self
                .completed
                .iter()
                .map(|(site, result)| (site, result.inspect()))
                .collect(),
            failed: self.failed.as_ref(),
            cause: self.cause.inspect(),
        }
    }
}
