//! Plain approval enters the existing custody and delivery flow with distinct evidence.

use super::{PreparationBatchRequest, PreparedAuthorizationError, PreparedAuthorizationOptions};
use gazette_core::release::{ReleaseBinding, ReleaseDocument, ReviewedBinding};
use gazette_ports::{
    destination::RootSeparation,
    release::{AuthoritySource, ReleaseWorktrees, WorktreeReader},
    store::PreparationReader,
};

/// Exact reviewed bytes plus independent confirmed digest, publisher scopes and preparations.
#[derive(Clone, Copy)]
pub struct PreparedReviewedApprovalRequest<'a> {
    /// Complete original plain approval document.
    pub approval: &'a [u8],
    /// Independently retained operator confirmation and workspace owner scopes.
    pub binding: &'a ReviewedBinding,
    /// Complete original preparation selection, never a successful subset.
    pub preparations: PreparationBatchRequest<'a>,
}

pub(super) enum Proof<'a> {
    Signed(&'a mut dyn AuthoritySource),
    Reviewed(&'a ReviewedBinding),
}
impl Proof<'_> {
    pub(super) fn acquire(
        &mut self,
    ) -> Result<ReleaseBinding, gazette_ports::release::AuthorityError> {
        match self {
            Self::Signed(source) => source.acquire_authority().map(Into::into),
            Self::Reviewed(binding) => Ok((*binding).clone().into()),
        }
    }
}

pub(super) struct CheckedRelease {
    pub document: ReleaseDocument,
    pub preparations_sha256: gazette_core::source::Sha256Digest,
}

pub(super) fn verify(
    bytes: &[u8],
    binding: &ReleaseBinding,
    limits: gazette_compiler::release::Limits,
) -> Result<ReleaseDocument, gazette_compiler::release::Error> {
    match binding {
        ReleaseBinding::Signed(binding) => {
            gazette_compiler::release::verify(bytes, binding, limits)
                .map(|checked| ReleaseDocument::Signed(Box::new(checked.authorization().clone())))
        }
        ReleaseBinding::Reviewed(binding) => {
            gazette_compiler::release::verify_reviewed(bytes, binding, limits)
                .map(|checked| ReleaseDocument::Reviewed(Box::new(checked.approval().clone())))
        }
    }
}

pub(super) fn check<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    bytes: &[u8],
    binding: &ReleaseBinding,
    preparations: PreparationBatchRequest<'_>,
    options: PreparedAuthorizationOptions,
) -> Result<CheckedRelease, PreparedAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    if let ReleaseBinding::Signed(binding) = binding {
        let checked = super::authorization::check_prepared_authorization(
            filesystem,
            git,
            store,
            super::PreparedAuthorizationRequest {
                authorization: bytes,
                binding,
                preparations,
            },
            options,
        )?;
        return Ok(CheckedRelease {
            document: ReleaseDocument::Signed(Box::new(checked.verified.authorization().clone())),
            preparations_sha256: checked.preparations_sha256,
        });
    }
    let document = verify(bytes, binding, options.authorization)
        .map_err(PreparedAuthorizationError::Authorization)?;
    let ReleaseDocument::Reviewed(body) = &document else {
        return Err(PreparedAuthorizationError::Mismatch);
    };
    if preparations.expected != *binding.journal()
        || preparations
            .selections
            .iter()
            .map(|row| &row.site)
            .ne(body.ordered_sites.iter())
    {
        return Err(PreparedAuthorizationError::Mismatch);
    }
    let admitted = super::preparations::inspect_admitted_preparations(
        filesystem,
        git,
        store,
        preparations,
        options.preparations,
    )
    .map_err(PreparedAuthorizationError::Preparation)?;
    let batch = admitted.batch();
    let evidence = batch.evidence();
    if body.ordered_targets != batch.targets()
        || body.validation_evidence_digest != evidence.validation_evidence_digest
        || body.disclosure_evidence_digest != evidence.disclosure_evidence_digest
        || body.policy_evidence_digest != evidence.policy_evidence_digest
        || body.scanner_revision != evidence.scanner_revision
        || body.generic_rule_set_digest != evidence.generic_rule_set_digest
        || body.website_policy_projection_digest != evidence.website_policy_projection_digest
    {
        return Err(PreparedAuthorizationError::Mismatch);
    }
    Ok(CheckedRelease {
        document,
        preparations_sha256: super::preparations::digest(
            batch,
            options.preparations.evidence_bytes,
        )
        .map_err(PreparedAuthorizationError::Preparation)?,
    })
}

/// Verify a reviewed document against complete admitted native preparations without recording it.
/// The returned preparation digest is evidence, not approval or permission to dispatch.
///
/// # Errors
/// Refuses changed scope, output, custody or incomplete technical admission.
pub fn verify_prepared_reviewed_approval<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    request: PreparedReviewedApprovalRequest<'_>,
    options: PreparedAuthorizationOptions,
) -> Result<gazette_core::source::Sha256Digest, PreparedAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    check(
        filesystem,
        git,
        store,
        request.approval,
        &request.binding.clone().into(),
        request.preparations,
        options,
    )
    .map(|checked| checked.preparations_sha256)
}

/// Record explicit reviewed-output approval after the same complete native preparation checks.
/// This does not consume the approval or dispatch a target.
///
/// # Errors
/// Refuses changed scope/output, wrong custody, incomplete preparations or uncertain writes.
pub fn register_reviewed_approval<F, G, S, W>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    request: PreparedReviewedApprovalRequest<'_>,
    options: super::RegisterAuthorizationOptions,
) -> Result<gazette_core::store::ReleaseAuthorizationReceipt, super::RegisterAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: gazette_ports::store::ReleaseAuthorizationWriter,
{
    use gazette_core::store::{Error, ReleaseAuthorizationInput};
    options.record.validate()?;
    let held = writer.inspect()?.header;
    if held != request.binding.journal || held != request.preparations.expected {
        return Err(Error::JournalMismatch.into());
    }
    let binding = request.binding.clone().into();
    let checked = check(
        filesystem,
        git,
        store,
        request.approval,
        &binding,
        request.preparations,
        options.verification,
    )
    .map_err(super::RegisterAuthorizationError::Verification)?;
    let authorization = std::str::from_utf8(request.approval)
        .map_err(|_| Error::ReleaseAuthorizationVerification)?;
    let ids: Vec<_> = request
        .preparations
        .selections
        .iter()
        .map(|row| row.preparation_id)
        .collect();
    if writer.inspect()?.header != held {
        return Err(Error::JournalMismatch.into());
    }
    writer
        .record_release_authorization(
            request.preparations.database,
            ReleaseAuthorizationInput {
                authorization,
                binding: &binding,
                preparations_sha256: checked.preparations_sha256,
                preparation_ids: &ids,
            },
            options.record,
        )
        .map_err(super::RegisterAuthorizationError::Store)
}

/// Consume plain approval only after complete preparations, leases and remote prestate checks.
///
/// # Errors
/// Preserves the same durable uncertainty as signed consumption; never dispatches or retries.
#[allow(clippy::too_many_arguments)]
pub fn consume_reviewed_approval<F, G, S, W, R>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    reader: &R,
    request: PreparedReviewedApprovalRequest<'_>,
    options: super::ConsumeAuthorizationOptions,
) -> Result<gazette_core::store::ReleaseConsumptionReceipt, super::ConsumeAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: gazette_ports::store::ReleaseConsumptionWriter,
    R: gazette_ports::release::RemoteReader,
{
    super::consumption::consume_with_proof(
        filesystem,
        git,
        store,
        writer,
        Proof::Reviewed(request.binding),
        reader,
        super::ConsumeAuthorizationRequest {
            authorization: request.approval,
            preparations: request.preparations,
        },
        options,
    )
}

/// One explicit attempt coordinate, separate from the complete reviewed approval.
#[derive(Clone, Copy)]
pub struct DispatchReviewedAttemptRequest<'a> {
    /// Complete exact reviewed approval and original preparations.
    pub reviewed: PreparedReviewedApprovalRequest<'a>,
    /// Original target ordinal.
    pub ordinal: u32,
    /// Consecutive explicit attempt number; an existing coordinate never pushes again.
    pub number: u64,
}

/// Dispatch a newly recorded plain attempt, then independently read back its target.
///
/// # Errors
/// Refuses changed output/prestate/custody or uncertain prior attempts without retry.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_reviewed_attempt<F, G, S, W, R, D>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    reader: &R,
    dispatcher: &D,
    request: DispatchReviewedAttemptRequest<'_>,
    options: super::DispatchAttemptOptions,
) -> Result<gazette_core::store::ReleaseAttempt, super::DispatchAttemptError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: gazette_ports::store::ReleaseReconciliationWriter
        + gazette_ports::store::ReleaseAttemptWriter
        + gazette_ports::store::PreparationWriter,
    R: gazette_ports::release::RemoteReader,
    D: gazette_ports::release::ReleaseDispatcher,
{
    super::dispatch::dispatch_with_proof(
        filesystem,
        git,
        store,
        writer,
        Proof::Reviewed(request.reviewed.binding),
        reader,
        dispatcher,
        super::DispatchAttemptRequest {
            authorization: request.reviewed.approval,
            preparations: request.reviewed.preparations,
            ordinal: request.ordinal,
            number: request.number,
        },
        options,
    )
}
