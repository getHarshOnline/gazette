//! Trusted-local coordinator enrollment, separate from release or publisher authority.

use gazette_core::{
    source::Sha256Digest,
    store::{
        Error, JournalHeader, LocalCoordinatorBinding, LocalCoordinatorSeal,
        LocalCoordinatorSelection, LocalCoordinatorState, SessionOptions,
    },
};
use gazette_ports::{
    destination::RootSeparation,
    store::{LocalCoordinatorStore, RestoreAdopter},
};
use std::path::{Path, PathBuf};

/// Check a selected custody binding and its independent permission without native access.
/// This is request validation, not proof of the anchor's current state or database bytes.
///
/// # Errors
/// Refuses missing permission, relative custody paths or a different journal identity/epoch.
pub fn validate_selection(
    selected: &LocalCoordinatorSelection,
    journal: &Path,
    expected: JournalHeader,
    allow_anchor_write: bool,
) -> Result<(), Error> {
    if !allow_anchor_write {
        return Err(Error::CoordinatorPermission);
    }
    if !selected.anchor_root.is_absolute() || !journal.is_absolute() {
        return Err(Error::UnsafePath);
    }
    if selected.binding.journal_root != journal
        || selected.binding.journal_id != expected.journal_id
        || selected.binding.journal_epoch != expected.journal_epoch
    {
        return Err(Error::CoordinatorMismatch);
    }
    Ok(())
}

/// Protect all selected native sources, staging and isolation roots before anchor access.
/// Each caller supplies its complete operation-specific root set; no roots are discovered.
///
/// # Errors
/// Refuses unsafe or overlapping roots, including the selected journal itself.
pub fn check_selection_roots(
    filesystem: &impl RootSeparation,
    selected: &LocalCoordinatorSelection,
    protected: &[PathBuf],
) -> Result<(), Error> {
    if !selected.anchor_root.is_absolute()
        || !selected.binding.journal_root.is_absolute()
        || protected.iter().any(|path| !path.is_absolute())
    {
        return Err(Error::UnsafePath);
    }
    filesystem
        .check_root_separation(&selected.anchor_root, protected)
        .and_then(|()| {
            filesystem.check_root_separation(
                &selected.anchor_root,
                std::slice::from_ref(&selected.binding.journal_root),
            )
        })
        .map_err(|_| Error::UnsafePath)
}

/// Explicit local custody choices for a fresh, never-written journal.
pub struct EnrollmentRequest<'a> {
    /// Existing empty private directory, independently retained outside store backups.
    pub anchor_root: &'a Path,
    /// Existing freshly initialized journal, not a restored or adopted database.
    pub journal_root: &'a Path,
    /// Exact coordinator ID shared with release binding, not a credential or hash alias.
    pub coordinator_id: &'a str,
    /// Selected trusted-local principal scope; native access control remains required.
    pub principal_scope: Sha256Digest,
    /// Sources, staging, outputs and backup roots neither custody root may overlap.
    pub protected_roots: &'a [PathBuf],
}

/// Bind a fresh local journal only after checking both custody roots against protected inputs.
///
/// This does not establish cross-host exclusion, approve a restore or consume authority.
/// Native enrollment rechecks exact paths under held reservations before writing its anchor.
///
/// # Errors
/// Refuses missing permission, unsafe/overlapping roots, nonfresh history and native failures.
/// An uncertain anchor-write error requires preserving the original roots, not reenrollment.
pub fn enroll(
    filesystem: &impl RootSeparation,
    provider: &impl LocalCoordinatorStore,
    request: EnrollmentRequest<'_>,
    options: SessionOptions,
    allow_anchor_write: bool,
) -> Result<LocalCoordinatorSeal, Error> {
    if !allow_anchor_write {
        return Err(Error::CoordinatorPermission);
    }
    options.validate()?;
    if !request.anchor_root.is_absolute()
        || !request.journal_root.is_absolute()
        || request
            .protected_roots
            .iter()
            .any(|path| !path.is_absolute())
    {
        return Err(Error::UnsafePath);
    }
    for root in [request.anchor_root, request.journal_root] {
        filesystem
            .check_root_separation(root, request.protected_roots)
            .map_err(|_| Error::UnsafePath)?;
    }
    filesystem
        .check_root_separation(request.anchor_root, &[request.journal_root.to_owned()])
        .map_err(|_| Error::UnsafePath)?;
    provider.enroll_local_coordinator(
        request.anchor_root,
        request.journal_root,
        request.coordinator_id,
        request.principal_scope,
        options,
        allow_anchor_write,
    )
}

/// Inspect local coordinator evidence without repairing its state or acquiring a writer.
///
/// # Errors
/// Preserves binding, permission, bounded-decoding and native path failures.
pub fn inspect(
    provider: &impl LocalCoordinatorStore,
    anchor_root: &Path,
    expected: &LocalCoordinatorBinding,
    options: SessionOptions,
) -> Result<LocalCoordinatorState, Error> {
    options.validate()?;
    if !anchor_root.is_absolute() || !expected.journal_root.is_absolute() {
        return Err(Error::UnsafePath);
    }
    provider.inspect_local_coordinator(anchor_root, expected, options)
}

/// Exact paths and independently retained evidence for one fresh-root adoption.
pub struct AdoptionRequest<'a> {
    /// Independently retained anchor root.
    pub anchor_root: &'a Path,
    /// Exact currently selected clean binding.
    pub binding: &'a LocalCoordinatorBinding,
    /// Independently retained last clean seal.
    pub previous: &'a LocalCoordinatorSeal,
    /// Exact detached restore directory.
    pub restore_root: &'a Path,
    /// Independently retained restore receipt pin.
    pub receipt_sha256: Sha256Digest,
    /// Existing empty private directory for the successor.
    pub successor_root: &'a Path,
    /// Complete caller-known source, output, stage and backup roots.
    pub protected_roots: &'a [PathBuf],
}

/// Adopt only an exact restore of the latest independently sealed clean state.
/// Every selected root remains intact; the successor advances exactly one local fence.
///
/// # Errors
///
/// Refuses unsafe or overlapping roots, inconsistent clean-frontier evidence, missing
/// permissions, stale custody, nonfresh successor state, or failed native verification.
pub fn adopt(
    filesystem: &impl RootSeparation,
    provider: &impl RestoreAdopter,
    request: AdoptionRequest<'_>,
    options: gazette_core::store::RestoreAdoptionOptions,
) -> Result<gazette_core::store::RestoreAdoptionReceipt, Error> {
    options.validate()?;
    let roots = [
        request.anchor_root,
        &request.binding.journal_root,
        request.restore_root,
        request.successor_root,
    ];
    if roots.iter().any(|path| !path.is_absolute())
        || request
            .protected_roots
            .iter()
            .any(|path| !path.is_absolute())
        || request.previous.binding != *request.binding
    {
        return Err(Error::UnsafePath);
    }
    for (index, root) in roots.iter().enumerate() {
        filesystem
            .check_root_separation(root, request.protected_roots)
            .map_err(|_| Error::UnsafePath)?;
        let other = roots
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, path)| (*path).to_owned())
            .collect::<Vec<_>>();
        filesystem
            .check_root_separation(root, &other)
            .map_err(|_| Error::UnsafePath)?;
    }
    provider.adopt_restore(
        gazette_ports::store::RestoreAdoptionRequest {
            anchor_root: request.anchor_root,
            expected: request.binding,
            previous: request.previous,
            restore_root: request.restore_root,
            receipt_sha256: request.receipt_sha256,
            successor_root: request.successor_root,
        },
        options,
    )
}
