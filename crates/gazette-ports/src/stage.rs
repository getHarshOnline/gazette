//! A fresh-stage effect, separate from replacing or publishing an existing tree.

use gazette_core::source::{Limits, TreeInput};
use gazette_core::stage::{Error, Location};
use std::path::PathBuf;

use crate::destination::RootSeparation;

/// Explicit synchronization of a newly created stage, preserving residue on failure.
pub trait StageRetention {
    /// Consume the native cleanup guard and independently verify retained bytes.
    ///
    /// # Errors
    /// Reports unresolved residue; failure never authorizes automatic cleanup or retry.
    fn retain(self) -> Result<gazette_core::stage::RetainedStage, Error>;
}

/// Held native parent observation spanning durable intent recording and creation.
pub trait StageTarget {
    /// Created tree with explicit retention, not implicit publication.
    type Stage: StageRetention;
    /// Canonical parent and exact child validated by the native owner.
    fn address(&self) -> &gazette_core::stage::Address;
    /// Recheck the held parent/source identities, then create the exact child once.
    ///
    /// # Errors
    /// Refuses changed roots, collisions, limits or incomplete writes; never adopts a path.
    fn create(self, input: TreeInput<'_>, limits: Limits) -> Result<Self::Stage, Error>;
}

/// Resolve a named target without creating it, so intent can become durable first.
pub trait StagePlanner: RootSeparation {
    /// Native observation; not reconstructible from a serialized address.
    type Target: StageTarget;
    /// Hold verified parent/source identities and validate one exact child name.
    ///
    /// # Errors
    /// Refuses invalid names, unsafe parents and overlaps without filesystem writes.
    fn plan_stage(
        &self,
        parent: &std::path::Path,
        name: &str,
        protected_roots: &[PathBuf],
    ) -> Result<Self::Target, Error>;
}

/// Write one complete captured tree into a new private child of an explicit root.
pub trait StageWriter: RootSeparation {
    /// Native lifetime guard returned only after full write/read-back verification.
    type Stage;

    /// Admit root separation before writes; never replace an existing target.
    /// Complete writes and read-back are required, not durable retention. The
    /// native guard must expose retention as a separate explicit operation.
    /// Named locations must be validated before writes and attempted exactly once;
    /// an existing child is a refusal, never an alternate-name or adoption request.
    ///
    /// # Errors
    ///
    /// Rejects unsafe roots and source overlap; propagates creation, write,
    /// flush/read-back and cleanup failures with explicit residual paths.
    fn stage(
        &self,
        location: Location<'_>,
        protected_roots: &[PathBuf],
        input: TreeInput<'_>,
        limits: Limits,
    ) -> Result<Self::Stage, Error>;
}
