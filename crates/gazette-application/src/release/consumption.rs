//! Connected local consumption; dispatch, reconciliation and publisher decisions stay separate.

use super::{
    PreparationBatchRequest, PreparedAuthorizationError, PreparedAuthorizationOptions,
    ReadbackReport,
};
use gazette_core::{
    release::{ReleaseBinding, RemoteReadError, RemoteReadOptions},
    store::{
        self, ReleaseConsumptionInput, ReleaseConsumptionOptions, ReleaseConsumptionReceipt,
        ReleaseCoordinator, ReleaseLeaseOptions, ReleaseLeases, ReleaseWriteError,
    },
};
use gazette_ports::{
    destination::RootSeparation,
    release::{AuthorityError, AuthoritySource, ReleaseWorktrees, RemoteReader, WorktreeReader},
    store::{PreparationReader, ReleaseConsumptionWriter},
};
use std::fmt;

/// Unchanged received intent and complete native preparation selection.
/// Independent authority is acquired through a separately selected source, not this body.
#[derive(Clone, Copy)]
pub struct ConsumeAuthorizationRequest<'a> {
    /// Exact original received authorization bytes, including signatures.
    pub authorization: &'a [u8],
    /// Complete native prepared set and independently retained journal coordinates.
    pub preparations: PreparationBatchRequest<'a>,
}

/// Explicit independent verification, provider-read, lease and consumption controls.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct ConsumeAuthorizationOptions {
    /// Existing signature and native preparation bounds.
    pub verification: PreparedAuthorizationOptions,
    /// Native complete-set deadline, per-target response limits and permissions; never push permission.
    pub read: RemoteReadOptions,
    /// Complete local destination-lease permission.
    pub leases: ReleaseLeaseOptions,
    /// Independent one-use local consumption permission.
    pub consume: ReleaseConsumptionOptions,
}

/// Exact failed phase; earlier lease/provider evidence remains separately retained.
#[derive(Debug)]
pub enum ConsumeFailure {
    /// Native gate or complete-set precondition refused.
    Store(store::Error),
    /// Current independent authority acquisition failed, without cached fallback.
    Authority(AuthorityError),
    /// Shared signature or native preparation verification failed.
    Verification(PreparedAuthorizationError),
    /// Native provider observation failed globally before a complete report.
    Read(RemoteReadError),
    /// Lease acquisition failed; its native uncertainty is preserved.
    Lease(ReleaseWriteError),
    /// Atomic consumption failed; inspect original history before retrying.
    Consumption(ReleaseWriteError),
}
impl From<store::Error> for ConsumeFailure {
    fn from(e: store::Error) -> Self {
        Self::Store(e)
    }
}
impl fmt::Display for ConsumeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Authority(e) => e.fmt(f),
            Self::Verification(e) => e.fmt(f),
            Self::Read(e) => e.fmt(f),
            Self::Lease(e) | Self::Consumption(e) => e.fmt(f),
        }
    }
}

/// Retained complete evidence at refusal; no error means earlier effects were undone.
#[derive(Debug)]
pub struct ConsumeAuthorizationError {
    /// Redacted failed phase and native write uncertainty where applicable.
    pub cause: ConsumeFailure,
    /// Complete acquired local leases, when that phase returned successfully.
    pub leases: Option<Box<ReleaseLeases>>,
    /// Complete native observation set, including unavailable or indeterminate members.
    pub observations: Option<Box<ReadbackReport>>,
}
impl fmt::Display for ConsumeAuthorizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; preserve original release evidence; no dispatch attempted",
            self.cause
        )
    }
}
impl std::error::Error for ConsumeAuthorizationError {}

pub(super) fn check_holder(
    held: &ReleaseCoordinator,
    binding: &ReleaseBinding,
    expected: store::JournalHeader,
) -> Result<(), store::Error> {
    if held.journal != expected || *binding.journal() != held.journal {
        return Err(store::Error::JournalMismatch);
    }
    if &held.binding.coordinator_id != binding.coordinator_id() {
        return Err(store::Error::CoordinatorMismatch);
    }
    Ok(())
}

/// Consume one pending authorization only after every native local and remote-read gate.
///
/// The selected authority source is queried before preparation/lease work and again
/// after complete destination observations. Its current key/frontier/time/stop contract
/// must be genuine; a caller-supplied cached binding is not acquired authority.
/// Both checks use the unchanged original received body and shared native verifier.
/// The anchored writer and complete leases stay held across all observations and CAS.
/// No push, automatic retry, settlement or public certification is performed here.
///
/// # Errors
/// Refuses missing permissions/custody, prior consumption, stale/changed authority or
/// preparations, conflicting leases, missing/moved/indeterminate pre-states and SQL
/// uncertainty. Complete earlier lease/read evidence survives subsequent refusal.
// Keep independently owned native ports explicit instead of inventing a combined provider.
#[allow(clippy::too_many_arguments)]
pub fn consume_prepared_authorization<F, G, S, W, A, R>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    authority: &mut A,
    reader: &R,
    request: ConsumeAuthorizationRequest<'_>,
    options: ConsumeAuthorizationOptions,
) -> Result<ReleaseConsumptionReceipt, ConsumeAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: ReleaseConsumptionWriter,
    A: AuthoritySource,
    R: RemoteReader,
{
    consume_with_proof(
        filesystem,
        git,
        store,
        writer,
        super::reviewed::Proof::Signed(authority),
        reader,
        request,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn consume_with_proof<F, G, S, W, R>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    mut proof: super::reviewed::Proof<'_>,
    reader: &R,
    request: ConsumeAuthorizationRequest<'_>,
    options: ConsumeAuthorizationOptions,
) -> Result<ReleaseConsumptionReceipt, ConsumeAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: ReleaseConsumptionWriter,
    R: RemoteReader,
{
    let mut leases = None;
    let mut observations = None;
    let result = (|| -> Result<ReleaseConsumptionReceipt, ConsumeFailure> {
        options.leases.validate()?;
        options.consume.validate()?;
        options.read.validate().map_err(ConsumeFailure::Read)?;
        let held = writer.inspect_release_coordinator()?;
        let initial = proof.acquire().map_err(ConsumeFailure::Authority)?;
        check_holder(&held, &initial, request.preparations.expected)?;
        let checked = super::reviewed::check(
            filesystem,
            git,
            store,
            request.authorization,
            &initial,
            request.preparations,
            options.verification,
        )
        .map_err(ConsumeFailure::Verification)?;
        let id = checked.document.record_id();
        if writer.inspect_release_consumption(id)?.record.is_some() {
            return Err(store::Error::ReleaseAlreadyConsumed.into());
        }
        let registration = writer
            .inspect_release_authorization(id)?
            .record
            .ok_or(store::Error::ReleaseConsumptionVerification)?;
        let selected: Vec<_> = request
            .preparations
            .selections
            .iter()
            .map(|p| p.preparation_id)
            .collect();
        if registration.authorization.as_bytes() != request.authorization
            || registration.preparations_sha256 != checked.preparations_sha256
            || registration.preparation_ids != selected
            || *registration.binding.journal() != held.journal
        {
            return Err(store::Error::ReleaseConsumptionVerification.into());
        }
        let targets = gazette_compiler::release::check_targets(
            initial.ordered_targets(),
            options.verification.authorization,
        )
        .map_err(|e| ConsumeFailure::Verification(PreparedAuthorizationError::Authorization(e)))?;
        leases = Some(Box::new(
            writer
                .acquire_release_leases(id, options.leases)
                .map_err(ConsumeFailure::Lease)?,
        ));
        let acquired = leases
            .as_ref()
            .ok_or(store::Error::ReleaseLeaseVerification)?;
        if acquired.coordinator != held {
            return Err(store::Error::ReleaseLeaseVerification.into());
        }
        acquired.validate(&registration)?;
        observations = Some(Box::new(
            super::observe_targets(reader, &targets, options.read).map_err(ConsumeFailure::Read)?,
        ));
        // Finish expensive native checks before the final authority/time observation.
        // Exclusive custody of prepared namespaces remains a separate native prerequisite.
        let rechecked = super::reviewed::check(
            filesystem,
            git,
            store,
            request.authorization,
            &initial,
            request.preparations,
            options.verification,
        )
        .map_err(ConsumeFailure::Verification)?;
        if writer.inspect_release_coordinator()? != held {
            return Err(store::Error::CoordinatorMismatch.into());
        }
        let current = proof.acquire().map_err(ConsumeFailure::Authority)?;
        check_holder(&held, &current, request.preparations.expected)?;
        if current.observed_at() < initial.observed_at() {
            return Err(store::Error::ReleaseConsumptionVerification.into());
        }
        super::reviewed::verify(
            request.authorization,
            &current,
            options.verification.authorization,
        )
        .map_err(|e| ConsumeFailure::Verification(PreparedAuthorizationError::Authorization(e)))?;
        let input = ReleaseConsumptionInput {
            registration: &registration,
            binding: &current,
            leases: acquired,
            readbacks: &observations
                .as_ref()
                .ok_or(store::Error::ReleaseConsumptionVerification)?
                .targets,
            preparations_sha256: rechecked.preparations_sha256,
        };
        input.validate()?;
        writer
            .consume_release_authorization(request.preparations.database, input, options.consume)
            .map_err(ConsumeFailure::Consumption)
    })();
    result.map_err(|cause| ConsumeAuthorizationError {
        cause,
        leases,
        observations,
    })
}
