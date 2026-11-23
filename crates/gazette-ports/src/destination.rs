//! Native destination observation, never writer or publication authority.

use std::path::{Path, PathBuf};

use gazette_core::destination::{Error, OutputObservation};
use gazette_core::source::{Inventory, Limits};

/// Read-only native separation of an existing root from protected inputs.
pub trait RootSeparation {
    /// Check root identity, ancestry and separation from every protected input.
    ///
    /// Do not read unrelated children or create any path. Success is an observation,
    /// not a durable permission or a promise that a later write will succeed.
    /// The writer must repeat these checks at its effect boundary.
    ///
    /// # Errors
    ///
    /// Rejects unsafe, missing, changing or overlapping native roots.
    fn check_root_separation(&self, root: &Path, protected_roots: &[PathBuf]) -> Result<(), Error>;
}

/// Inspect an existing content root disjoint from explicitly protected sources.
pub trait DestinationReader {
    /// Read the complete existing tree without following descendant symlinks.
    ///
    /// Compare native ancestry in both directions, not merely path spelling.
    /// Recheck root identities and detect changes during the observation. No
    /// directory, marker, database or other file may be created or modified.
    /// This is not an atomic snapshot or a capability to write later.
    ///
    /// # Errors
    ///
    /// Rejects missing/unsafe roots, either-direction source overlap, unsafe or
    /// changing descendants, and exceeded inventory limits.
    fn read_destination(
        &self,
        root: &Path,
        protected_roots: &[PathBuf],
        limits: Limits,
    ) -> Result<Inventory, Error>;
}

/// Observe a publication destination without treating its Git administration as output.
///
/// Existing user files remain in the inventory. A root `.git` directory or pointer
/// file is held and rechecked separately, never traversed, copied or claimed.
/// This read does not establish generated ownership or authorize replacement.
pub trait OutputReader {
    /// Read the bounded, complete non-administrative tree through stable handles.
    /// Bind it to canonical root and stable native root/admin identities, not
    /// mutable timestamps or generated ownership. Pointer changes during this
    /// read are refused independently of that stable binding.
    ///
    /// # Errors
    /// Rejects unsafe roots, protected-source overlap, links, special files,
    /// nested/case-variant Git paths, changing entries and exhausted limits.
    fn read_output(
        &self,
        root: &Path,
        protected_roots: &[PathBuf],
        limits: Limits,
    ) -> Result<OutputObservation, Error>;
}

/// Apply one application-authorized local output transition under exclusive local control.
pub trait OutputWriter: OutputReader {
    /// Reobserve before effects, apply exact transitions and verify complete read-back.
    /// Compare the bound native root/admin facts as well as inventory contents;
    /// identical bytes at a replacement directory do not admit the old plan.
    ///
    /// This is not a multi-file atomic transaction, durable ownership journal,
    /// publication permission or safe automatic retry after interruption. The
    /// destination namespace must remain under trusted exclusive caller control.
    ///
    /// # Errors
    /// Refuses absent permission, changed observations or unsafe native state.
    /// Later failure preserves partial-effect evidence and unsettled residue.
    fn write_output(
        &self,
        mutation: gazette_core::destination::OutputMutation<'_>,
        allow_write: bool,
    ) -> Result<OutputObservation, gazette_core::destination::OutputWriteError>;
}
