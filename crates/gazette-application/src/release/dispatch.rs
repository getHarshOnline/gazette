//! Current-holder dispatch connects immutable consumed intent, native effects and read-back.

use super::{PreparationBatchRequest, PreparedAuthorizationError, PreparedAuthorizationOptions};
use gazette_core::{
    release::{
        DispatchOptions, DispatchRequest, RemoteReadError, RemoteReadOptions, TargetReadback,
    },
    store::{
        self, ReleaseAttempt, ReleaseAttemptKey, ReleaseAttemptOptions, ReleaseAttemptResult,
        ReleaseAttemptStart, ReleaseWriteError,
    },
};
use gazette_ports::{
    destination::RootSeparation,
    release::{
        AuthorityError, AuthoritySource, ReleaseDispatcher, ReleaseWorktrees, RemoteReader,
        WorktreeReader,
    },
    store::{
        PreparationReader, PreparationWriter, ReleaseAttemptWriter, ReleaseReconciliationWriter,
    },
};
use std::fmt;

/// One explicit attempt of an original consumed target; no automatic batch loop or retry.
#[derive(Clone, Copy)]
pub struct DispatchAttemptRequest<'a> {
    /// Exact originally received authorization bytes.
    pub authorization: &'a [u8],
    /// Complete original native preparation selection, never a silently reduced batch.
    pub preparations: PreparationBatchRequest<'a>,
    /// Original target position to dispatch.
    pub ordinal: u32,
    /// Explicit one-based attempt coordinate; replay does not invoke the provider again.
    pub number: u64,
}

/// Separate native verification, read, local journal and dispatch permissions.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct DispatchAttemptOptions {
    /// Existing shared signature/preparation verification.
    pub verification: PreparedAuthorizationOptions,
    /// Explicit independent destination observation permissions and limits.
    pub read: RemoteReadOptions,
    /// Independent durable attempt-evidence permission.
    pub record: ReleaseAttemptOptions,
    /// Exact selected native effect permission and bounded work.
    pub dispatch: DispatchOptions,
}

/// Refused phase; transport uncertainty itself is retained in an attempt result, not hidden here.
#[derive(Debug)]
pub enum DispatchAttemptFailure {
    /// Native history, custody or exact selection refused.
    Store(store::Error),
    /// Independently selected current authority could not be acquired.
    Authority(AuthorityError),
    /// Shared signature or complete native preparation verification refused.
    Verification(PreparedAuthorizationError),
    /// Global provider-read permission or bounds refused.
    Read(RemoteReadError),
    /// Dispatch options or exact recipe refused before its start was recorded.
    Selection(gazette_core::release::DispatchError),
    /// Durable start append failed; inspect original history, never retry blindly.
    Start(ReleaseWriteError),
    /// Effect/read-back occurred but its durable result was not returned successfully.
    Finish(ReleaseWriteError),
}
impl From<store::Error> for DispatchAttemptFailure {
    fn from(error: store::Error) -> Self {
        Self::Store(error)
    }
}
impl fmt::Display for DispatchAttemptFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(e) => e.fmt(f),
            Self::Authority(e) => e.fmt(f),
            Self::Verification(e) => e.fmt(f),
            Self::Read(e) => e.fmt(f),
            Self::Selection(e) => e.fmt(f),
            Self::Start(e) | Self::Finish(e) => e.fmt(f),
        }
    }
}

/// Preserve the exact known effect boundary when durable delivery fails.
#[derive(Debug)]
pub struct DispatchAttemptError {
    /// Redacted failed phase, including native journal uncertainty.
    pub cause: DispatchAttemptFailure,
    /// Exact acquired pre-state evidence, including conflict or unavailability before any start.
    pub prestate: Option<Box<TargetReadback>>,
    /// Returned durable start, if that phase completed successfully.
    pub started: Option<Box<ReleaseAttemptStart>>,
    /// Actual transport/read-back observation, not necessarily durably recorded.
    pub observed: Option<Box<ReleaseAttemptResult>>,
}
impl fmt::Display for DispatchAttemptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; preserve exact attempt history and reconcile before retrying",
            self.cause
        )
    }
}
impl std::error::Error for DispatchAttemptError {}

/// Dispatch one exact consumed target under its original held native coordinator.
///
/// Complete shared native preparation verification and fresh authority precede a durable
/// start. Only a newly appended start reaches the provider. Every native return, including
/// errors, is followed by an independent read-back and an immutable result append. A
/// returned attempt is evidence, not certification or complete-set settlement. Uncertain
/// starts, later-custody recovery and ambiguous prior effects never automatically redispatch.
/// Prepared paths and the selected native provider remain under trusted exclusive custody.
///
/// # Errors
/// Refuses missing permissions, changed original intent, inactive custody/leases, stale
/// authority, changed preparations, moved pre-state or uncertain durable evidence delivery.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_prepared_attempt<F, G, S, W, A, R, D>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    authority: &mut A,
    reader: &R,
    dispatcher: &D,
    request: DispatchAttemptRequest<'_>,
    options: DispatchAttemptOptions,
) -> Result<ReleaseAttempt, DispatchAttemptError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: ReleaseReconciliationWriter + ReleaseAttemptWriter + PreparationWriter,
    A: AuthoritySource,
    R: RemoteReader,
    D: ReleaseDispatcher,
{
    dispatch_with_proof(
        filesystem,
        git,
        store,
        writer,
        super::reviewed::Proof::Signed(authority),
        reader,
        dispatcher,
        request,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_with_proof<F, G, S, W, R, D>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    mut proof: super::reviewed::Proof<'_>,
    reader: &R,
    dispatcher: &D,
    request: DispatchAttemptRequest<'_>,
    options: DispatchAttemptOptions,
) -> Result<ReleaseAttempt, DispatchAttemptError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: ReleaseReconciliationWriter + ReleaseAttemptWriter + PreparationWriter,
    R: RemoteReader,
    D: ReleaseDispatcher,
{
    let mut started = None;
    let mut observed = None;
    let mut prestate_evidence = None;
    let result = (|| -> Result<ReleaseAttempt, DispatchAttemptFailure> {
        options.record.validate()?;
        options
            .dispatch
            .validate()
            .map_err(DispatchAttemptFailure::Selection)?;
        options
            .read
            .validate()
            .map_err(DispatchAttemptFailure::Read)?;
        let held = writer.inspect_release_coordinator()?;
        let initial = proof.acquire().map_err(DispatchAttemptFailure::Authority)?;
        super::consumption::check_holder(&held, &initial, request.preparations.expected)?;
        let checked = super::reviewed::check(
            filesystem,
            git,
            store,
            request.authorization,
            &initial,
            request.preparations,
            options.verification,
        )
        .map_err(DispatchAttemptFailure::Verification)?;
        let id = checked.document.record_id();
        let registration = writer
            .inspect_release_authorization(id)?
            .record
            .ok_or(store::Error::ReleaseAttemptVerification)?;
        let reconciled =
            writer.inspect_release_reconciliations(request.preparations.database, id)?;
        let terminal = reconciled
            .records
            .last()
            .is_some_and(|row| row.settlement.state.is_terminal());
        let history = reconciled.history;
        let consumed = history
            .consumption
            .ok_or(store::Error::ReleaseAttemptVerification)?;
        consumed.validate(&registration)?;
        if consumed.leases.coordinator != held
            || registration.authorization.as_bytes() != request.authorization
            || consumed.preparations_sha256 != checked.preparations_sha256
            || registration.preparation_ids.iter().copied().ne(request
                .preparations
                .selections
                .iter()
                .map(|row| row.preparation_id))
        {
            return Err(store::Error::ReleaseAttemptVerification.into());
        }
        let key = ReleaseAttemptKey {
            authorization_id: id.clone(),
            ordinal: request.ordinal,
            number: request.number,
        };
        // Existing coordinates are observations, never permission to repeat a side effect.
        if let Some(existing) = history.attempts.iter().find(|row| row.start.key == key) {
            return Ok(existing.clone());
        }
        if terminal {
            return Err(store::Error::ReleaseTerminal.into());
        }
        let target = consumed
            .targets
            .iter()
            .find(|row| row.target.ordinal == request.ordinal)
            .map(|row| &row.target)
            .ok_or(store::Error::ReleaseAttemptVerification)?;
        let selection = request
            .preparations
            .selections
            .get(request.ordinal as usize)
            .ok_or(store::Error::ReleaseAttemptVerification)?;
        let preparation = writer
            .inspect_preparation(selection.preparation_id)?
            .record
            .ok_or(store::Error::ReleaseAttemptVerification)?;
        let worktree = preparation
            .worktree
            .as_ref()
            .ok_or(store::Error::ReleaseAttemptVerification)?;
        let repository = preparation.intent.address.path();
        let native = DispatchRequest {
            repository: &repository,
            target,
            metadata: &worktree.metadata,
            output: preparation.intent.output,
        };
        native
            .validate()
            .map_err(DispatchAttemptFailure::Selection)?;
        let prestate = super::observation::observe_target(reader, target, options.read);
        prestate_evidence = Some(Box::new(prestate.clone()));
        // Close the expensive filesystem/Git/evidence join before the final authority sample.
        let rechecked = super::reviewed::check(
            filesystem,
            git,
            store,
            request.authorization,
            &initial,
            request.preparations,
            options.verification,
        )
        .map_err(DispatchAttemptFailure::Verification)?;
        if rechecked.preparations_sha256 != consumed.preparations_sha256
            || writer.inspect_release_coordinator()? != held
        {
            return Err(store::Error::ReleaseAttemptVerification.into());
        }
        let current = proof.acquire().map_err(DispatchAttemptFailure::Authority)?;
        super::consumption::check_holder(&held, &current, request.preparations.expected)?;
        if current.observed_at() < initial.observed_at() {
            return Err(store::Error::ReleaseAttemptVerification.into());
        }
        super::reviewed::verify(
            request.authorization,
            &current,
            options.verification.authorization,
        )
        .map_err(|e| {
            DispatchAttemptFailure::Verification(PreparedAuthorizationError::Authorization(e))
        })?;
        let start = ReleaseAttemptStart {
            profile: options.dispatch.profile,
            key,
            binding: current,
            prestate,
            created: held.journal,
        };
        start.validate(&registration, &consumed)?;
        start.follows(
            history
                .attempts
                .iter()
                .rev()
                .find(|row| row.start.key.ordinal == request.ordinal),
        )?;
        let receipt = writer
            .start_release_attempt(request.preparations.database, &start, options.record)
            .map_err(DispatchAttemptFailure::Start)?;
        if !receipt.created {
            return Ok(receipt.record);
        }
        if receipt.record.start != start || receipt.record.result.is_some() {
            return Err(store::Error::ReleaseAttemptVerification.into());
        }
        started = Some(Box::new(start.clone()));
        let transport = dispatcher.dispatch(native, options.dispatch);
        let readback = super::observation::observe_target(reader, target, options.read);
        let result = ReleaseAttemptResult {
            key: start.key,
            transport,
            readback,
            created: held.journal,
        };
        observed = Some(Box::new(result.clone()));
        writer
            .finish_release_attempt(&result, options.record)
            .map_err(DispatchAttemptFailure::Finish)
    })();
    result.map_err(|cause| DispatchAttemptError {
        cause,
        prestate: prestate_evidence,
        started,
        observed,
    })
}
