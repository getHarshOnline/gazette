//! Source observation, preparation and staging, distinct from publication admission.

use gazette_core::source::{
    CapturedSource, EntryPath, Error, Inventory, Limits, Subdirectory, TreeIdentity,
};
use gazette_ports::source::{SelectedSourceCapture, SourceCapture, SourceReader};
use std::path::Path;

mod acquire;
pub(crate) use acquire::bind_selected;
mod batch;
mod destination;
mod development;
pub use development::{DevelopmentBaseline, DevelopmentSource, acquire_development};
mod materialization;
pub(crate) use destination::collision_key;
pub use destination::{CheckedDestination, check_destination};
pub use materialization::{
    GENERATED_TREE_MANIFEST, GeneratedSource, GeneratedTreeBinding, GeneratedTreeInspection,
    GeneratedTreeManifest, PlacementIdentity, StageContext, VerifiedMaterialization,
    verify_generated_tree,
};
mod prepare;
mod revision;
mod stage;
pub use acquire::{AcquiredSource, acquire, acquire_submodule};
pub(crate) use batch::{AcquisitionMode, Batch, BatchError};
pub use prepare::{
    AcquiredInput, PlacementRequest, PreparationError, PreparationLimits, PreparedPlacement,
    PreparedSourceSet, SourceSelection, prepare,
};
pub use revision::{VerifiedSource, VerifiedSubmodule, verify, verify_submodule};
pub use stage::{check_stage, stage};

/// Observe a source using the selected reader and the shared inventory contract.
///
/// Root-relative output includes no wall clock or source-root location.
/// Source observations do not establish rights, meaning, admission or release
/// authority. This operation is shared by every public interface.
///
/// # Errors
///
/// Propagates the reader's failure without returning a partial inventory.
pub fn inspect(
    reader: &impl SourceReader,
    root: &Path,
    subdirectory: Option<&Subdirectory>,
    limits: Limits,
) -> Result<Inventory, Error> {
    reader.read(root, subdirectory, limits)
}

/// Retain the complete selected tree for a consumer that must not reopen live files.
///
/// This is an unpinned observation, not revision verification or admission.
/// Native confinement and byte binding use the same capture port as acquisition.
///
/// # Errors
///
/// Propagates capture failures without returning partial source bytes.
pub fn capture(
    reader: &impl SourceCapture,
    root: &Path,
    subdirectory: Option<&Subdirectory>,
    limits: Limits,
) -> Result<CapturedSource, Error> {
    reader.capture(root, subdirectory, limits)
}

/// Retain only an explicit, complete set of root-relative regular files.
///
/// This operation performs no directory discovery. The selected source port
/// confines every path, rejects links and special files, and rechecks all held
/// files before returning their immutable bytes.
///
/// # Errors
///
/// Rejects duplicate, unsafe, missing, changing or over-limit selections without
/// returning a partial capture.
pub fn capture_selected(
    reader: &impl SelectedSourceCapture,
    root: &Path,
    paths: &[EntryPath],
    limits: Limits,
) -> Result<CapturedSource, Error> {
    reader.capture_selected(root, paths, limits)
}

/// Capture a complete tree once and compare it with independently retained identity.
///
/// Consumers borrow these verified bytes, never reopen a live path after checking.
/// The identity covers every file and directory, not only a document selected later.
/// This proves a bounded structural match, not revision lineage, rights, immutable
/// native storage, certification or cleanup authority. The pin must not be learned
/// from the same untrusted tree being checked.
///
/// # Errors
///
/// Rejects unsafe/changing or over-limit sources and mismatched complete trees.
pub fn capture_pinned(
    reader: &impl SourceCapture,
    root: &Path,
    expected: TreeIdentity,
    limits: Limits,
) -> Result<CapturedSource, Error> {
    let captured = capture(reader, root, None, limits)?;
    let actual = captured.inventory().identity();
    if actual != expected {
        return Err(Error::TreeMismatch {
            path: root.to_owned(),
            expected,
            actual,
        });
    }
    Ok(captured)
}
