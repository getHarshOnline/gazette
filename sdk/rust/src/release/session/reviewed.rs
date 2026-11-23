//! Plain review and confirmation use the held writer without a signing-authority provider.

use super::{Cause, Failure, OpenRequest, Operation, Outcome};
use crate::{release, source::Sha256Digest, store};
use std::path::{Path, PathBuf};

#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    writer: &mut store::Writer,
    scope: Option<&release::ReviewedScope>,
    selected: &OpenRequest,
    database: &Path,
    protected: &[PathBuf],
    operation: &Operation,
    expected: store::JournalHeader,
) -> Outcome {
    run(
        writer, scope, selected, database, protected, operation, expected,
    )
    .unwrap_or_else(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn run(
    writer: &mut store::Writer,
    scope: Option<&release::ReviewedScope>,
    selected: &OpenRequest,
    database: &Path,
    protected: &[PathBuf],
    operation: &Operation,
    expected: store::JournalHeader,
) -> Result<Outcome, Failure> {
    let scope = scope.ok_or_else(|| Failure::selection("release_reviewed_unconfigured"))?;
    let (selections, options, confirmed, observed_at) = match operation {
        Operation::Review { input, options, .. } => (
            &input.selections,
            *options,
            Sha256Digest::of(b"unconfirmed"),
            input.approved_at,
        ),
        Operation::RegisterReviewed { input, options, .. } => {
            options.record.validate().map_err(Failure::from)?;
            (
                &input.selections,
                options.verification,
                input.confirmed_sha256,
                input.observed_at,
            )
        }
        Operation::ConsumeReviewed { input, options, .. } => {
            options.leases.validate().map_err(Failure::from)?;
            options.consume.validate().map_err(Failure::from)?;
            options
                .read
                .validate()
                .map_err(|error| Failure::new(Cause::Read { error }))?;
            (
                &input.selections,
                options.verification,
                input.confirmed_sha256,
                input.observed_at,
            )
        }
        Operation::DispatchReviewed { input, options, .. } => {
            options.record.validate().map_err(Failure::from)?;
            options
                .dispatch
                .validate()
                .map_err(|error| Failure::new(Cause::DispatchSelection { error }))?;
            options
                .read
                .validate()
                .map_err(|error| Failure::new(Cause::Read { error }))?;
            (
                &input.selections,
                options.verification,
                input.confirmed_sha256,
                input.observed_at,
            )
        }
        _ => unreachable!("only reviewed operations reach this owner"),
    };
    let preparations = release::PreparationBatchRequest {
        database,
        expected,
        selections,
        protected_roots: protected,
    };
    let batch = release::inspect_preparations(preparations, options.preparations)
        .map_err(|error| Failure::new(Cause::Verification { code: error.code() }))?;
    let mut binding = scope
        .binding(
            &selected.coordinator.binding.coordinator_id,
            expected,
            &batch,
            selections,
            confirmed,
            observed_at,
        )
        .map_err(Failure::selection)?;
    if let Operation::Review { input, .. } = operation {
        let evidence = batch.evidence();
        let mut approval = release::ReviewedApproval {
            schema_id: release::ReviewedApprovalSchema::WebsiteReviewedReleaseApprovalV1,
            record_id: input.record_id.clone(),
            nonce: input.nonce.clone(),
            coordinator_id: binding.coordinator_id.clone(),
            journal_id: expected.journal_id,
            journal_epoch: expected.journal_epoch,
            writer_fence: expected.writer_fence,
            ordered_targets: binding.ordered_targets.clone(),
            ordered_sites: binding.ordered_sites.clone(),
            publisher_approvals: binding.publishers.clone(),
            validation_evidence_digest: evidence.validation_evidence_digest,
            disclosure_evidence_digest: evidence.disclosure_evidence_digest,
            policy_evidence_digest: evidence.policy_evidence_digest,
            scanner_revision: evidence.scanner_revision.clone(),
            generic_rule_set_digest: evidence.generic_rule_set_digest,
            website_policy_projection_digest: evidence.website_policy_projection_digest,
            approved_at: input.approved_at,
            canonical_body_sha256: confirmed,
        };
        approval.canonical_body_sha256 =
            release::reviewed_body_digest(&approval, options.authorization)
                .map_err(|error| Failure::new(Cause::Verification { code: error.code() }))?;
        binding.canonical_body_sha256 = approval.canonical_body_sha256;
        let bytes = serde_json::to_vec(&approval)
            .map_err(|_| Failure::selection("release_reviewed_encoding"))?;
        gazette_application::release::verify_prepared_reviewed_approval(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_git::Git,
            &gazette_adapter_sqlite::Sqlite,
            release::PreparedReviewedApprovalRequest {
                approval: &bytes,
                binding: &binding,
                preparations,
            },
            options,
        )
        .map_err(|error| Failure::new(Cause::Verification { code: error.code() }))?;
        return Ok(Outcome::Reviewed { approval });
    }
    let input = match operation {
        Operation::RegisterReviewed { input, .. }
        | Operation::ConsumeReviewed { input, .. }
        | Operation::DispatchReviewed { input, .. } => input,
        _ => unreachable!("review returned above"),
    };
    let request = release::PreparedReviewedApprovalRequest {
        approval: input.approval.as_bytes(),
        binding: &binding,
        preparations,
    };
    match operation {
        Operation::RegisterReviewed { options, .. } => writer
            .register_reviewed_approval(scope, request, *options)
            .map(|receipt| Outcome::Registered { receipt })
            .map_err(Failure::from),
        Operation::ConsumeReviewed { options, .. } => {
            let git = selected
                .git
                .as_ref()
                .ok_or_else(|| Failure::selection("release_session_git_unconfigured"))?;
            writer
                .consume_reviewed_approval(scope, git, request, *options)
                .map(|receipt| Outcome::Consumed { receipt })
                .map_err(Failure::from)
        }
        Operation::DispatchReviewed {
            ordinal,
            number,
            options,
            ..
        } => {
            let git = selected
                .git
                .as_ref()
                .ok_or_else(|| Failure::selection("release_session_git_unconfigured"))?;
            writer
                .dispatch_reviewed_attempt(
                    scope,
                    git,
                    release::DispatchReviewedAttemptRequest {
                        reviewed: request,
                        ordinal: *ordinal,
                        number: *number,
                    },
                    *options,
                )
                .map(|attempt| Outcome::Dispatched { attempt })
                .map_err(Failure::from)
        }
        _ => unreachable!("reviewed dispatch exhausts its variants"),
    }
}
