//! One local preparation sequence over retained bytes, without publication authority.

use std::fmt;

use gazette_core::{
    process,
    release::{CommitMetadata, ParentSource, WorktreeOptions},
    store,
};
use gazette_ports::{
    process::{CaptureRunner, Runner},
    release::{ReleaseWorktrees, WorktreeReader},
    source::SourceCapture,
    stage::StagePlanner,
    store::{PreparationWriter, WriterConnector},
};

use super::{
    PreparationCheckOptions, PreparationCheckRequest, PreparationError, PreparationFailure,
    PreparationOptions, PreparationRequest,
};
use crate::{
    build::{
        DisclosureInspectors, DisclosurePolicy, DisclosureProbeLimits, PolicyCheckError,
        PublicationRender, policy,
    },
    release::{
        JournalWorktreeError, JournalWorktreeOptions, JournalWorktreeRequest,
        prepare_journal_worktree,
    },
};

/// Exact local stage, check selection and commit recipe; no remote destination or authority.
#[derive(Clone, Copy, Debug)]
pub struct LocalPreparationRequest<'a> {
    /// Existing journal generation and exact new stage coordinate.
    pub stage: PreparationRequest<'a>,
    /// Deterministic native commit fields, not publisher identity approval.
    pub metadata: &'a CommitMetadata,
    /// Explicit offline source required exactly when the commit has a parent.
    pub parent_source: Option<&'a ParentSource>,
    /// Complete typed disclosure policy, retained by digest rather than copied into public output.
    pub disclosure: &'a DisclosurePolicy,
    /// Independent output-check binding; absence preserves explicitly unqualified exit-only mode.
    pub output_policy: Option<&'a crate::build::OutputPolicyRequirement>,
    /// Optional independent supported-output expectations, never full publication approval.
    pub qualification: Option<&'a super::PreparationQualification>,
    /// Optional independently retained local custody selection, implemented by the connector.
    pub coordinator: Option<&'a store::LocalCoordinatorSelection>,
}

/// Independent native permissions and bounds, with one journal policy shared by every phase.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalPreparationOptions {
    /// Stage limits and the sole record/writer/evidence settings for this sequence.
    pub stage: PreparationOptions,
    /// Required only when this workspace selects an external output checker.
    /// Execution is not sandboxed; the configured command retains its native authority.
    pub policy: process::Options,
    /// Required disclosure scan bounds over the original immutable output.
    pub disclosure: DisclosureProbeLimits,
    /// Explicit local Git creation permission and native storage/history bounds.
    pub worktree: WorktreeOptions,
    /// Independent permission to update an explicitly selected local custody anchor.
    pub allow_anchor_write: bool,
}

/// The failed phase retains its own native observations and effect uncertainty.
#[derive(Debug)]
pub enum LocalPreparationFailure {
    /// Preview, missing candidate selection, staging preflight, journal or native stage failure.
    Stage(PreparationError),
    /// Configured checker permission, delivery, execution or nonzero result.
    Policy(PolicyCheckError),
    /// Required disclosure or exact immutable check-history failure.
    Checks(PreparationError),
    /// Commit preflight, Git creation, read-back or durable completion failure.
    Worktree(JournalWorktreeError),
}

/// A failed sequence is not a rollback or permission to retry completed native effects.
#[derive(Debug)]
pub struct LocalPreparationError {
    /// Failed phase, possibly carrying observations newer than the prior completed receipt.
    pub cause: Box<LocalPreparationFailure>,
    /// Last successfully completed phase before this failure. The cause retains any later
    /// writer generation, append receipt or native uncertainty; never discard it on retry.
    pub receipt: Option<Box<store::PreparationReceipt>>,
    /// Completed checker observation when a later check-history write failed. It is
    /// moved from the original check, not rerun or reconstructed from the staged receipt.
    pub output_policy: Option<Box<crate::build::OutputPolicyEvidence>>,
    /// Completed technical qualification retained if its later check-history append fails.
    pub technical_certificate: Option<Box<super::TechnicalCertificate>>,
}

impl LocalPreparationError {
    pub(super) fn new(
        cause: LocalPreparationFailure,
        receipt: Option<store::PreparationReceipt>,
    ) -> Self {
        Self {
            cause: Box::new(cause),
            receipt: receipt.map(Box::new),
            output_policy: None,
            technical_certificate: None,
        }
    }
}

impl fmt::Display for LocalPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (phase, error): (&str, &dyn fmt::Display) = match self.cause.as_ref() {
            LocalPreparationFailure::Stage(error) => ("stage", error),
            LocalPreparationFailure::Policy(error) => ("policy", error),
            LocalPreparationFailure::Checks(error) => ("checks", error),
            LocalPreparationFailure::Worktree(error) => ("worktree", error),
        };
        write!(
            f,
            "local preparation {phase} failed: {error}; preserve prior receipt and later phase evidence; no automatic retry or publication"
        )
    }
}

impl std::error::Error for LocalPreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self.cause.as_ref() {
            LocalPreparationFailure::Stage(error) | LocalPreparationFailure::Checks(error) => error,
            LocalPreparationFailure::Policy(error) => error,
            LocalPreparationFailure::Worktree(error) => error,
        })
    }
}

impl<'a> LocalPreparationRequest<'a> {
    /// Resolve the single required checker. Conflicting duplicate selections refuse.
    ///
    /// # Errors
    /// Returns a redacted technical qualification mismatch before effects.
    pub fn effective_output_policy(
        self,
    ) -> Result<Option<&'a crate::build::OutputPolicyRequirement>, PreparationError> {
        if let Some(qualification) = self.qualification {
            qualification.validate()?;
            if self
                .output_policy
                .is_some_and(|selected| selected != &qualification.output_policy)
            {
                return Err(PreparationError::before(PreparationFailure::Qualification));
            }
            Ok(Some(&qualification.output_policy))
        } else {
            Ok(self.output_policy)
        }
    }

    /// Validate stage/store/Git permissions, disclosure bounds and the native Git recipe
    /// without reading sources, opening a store or executing a checker. Render-dependent
    /// checks and exact native effect-boundary validation remain required afterward.
    ///
    /// # Errors
    /// Returns the original failed phase with no preparation receipt or native effect.
    pub fn validate(self, options: LocalPreparationOptions) -> Result<(), LocalPreparationError> {
        use LocalPreparationFailure as Failure;
        let before_stage = |error| LocalPreparationError::new(Failure::Stage(error), None);
        options.stage.validate().map_err(before_stage)?;
        if let Some(qualification) = self.qualification {
            qualification
                .check_resource_limits(options.disclosure)
                .map_err(before_stage)?;
        }
        if let Some(required) = self.effective_output_policy().map_err(before_stage)? {
            required.validate().map_err(|error| {
                LocalPreparationError::new(
                    Failure::Policy(PolicyCheckError::OutputRequirement(error)),
                    None,
                )
            })?;
        }
        if let Some(selected) = self.coordinator {
            crate::store::coordinator::validate_selection(
                selected,
                self.stage.journal,
                self.stage.expected,
                options.allow_anchor_write,
            )
            .map_err(|error| {
                before_stage(PreparationError::before(PreparationFailure::Journal(error)))
            })?;
        }
        self.disclosure
            .validate(options.disclosure)
            .map_err(|error| {
                LocalPreparationError::new(
                    Failure::Checks(PreparationError::before(PreparationFailure::Disclosure(
                        error,
                    ))),
                    None,
                )
            })?;
        JournalWorktreeOptions {
            record: options.stage.record,
            writer: options.stage.writer,
            worktree: options.worktree,
        }
        .validate(self.metadata, self.parent_source)
        .map_err(|error| LocalPreparationError::new(Failure::Worktree(error), None))
    }
}

impl PublicationRender<'_> {
    /// Check local phase selections and source separation before any stage effect.
    pub(super) fn validate_local<F: StagePlanner>(
        &self,
        filesystem: &F,
        request: LocalPreparationRequest<'_>,
        options: LocalPreparationOptions,
    ) -> Result<bool, LocalPreparationError> {
        use LocalPreparationFailure as Failure;
        let before_stage = |error| LocalPreparationError::new(Failure::Stage(error), None);
        self.ensure_preparation_mode().map_err(before_stage)?;
        request.validate(options)?;
        let required_policy = request.effective_output_policy().map_err(before_stage)?;
        if let Some(qualification) = request.qualification {
            qualification
                .check_render(self, options.stage.evidence_bytes)
                .map_err(before_stage)?;
            if request
                .disclosure
                .validate(options.disclosure)
                .map_err(|error| {
                    before_stage(PreparationError::before(PreparationFailure::Disclosure(
                        error,
                    )))
                })?
                != qualification.disclosure_policy_sha256
            {
                return Err(before_stage(PreparationError::before(
                    PreparationFailure::Qualification,
                )));
            }
        }
        if let Some(selected) = request.coordinator {
            let mut protected = self.protected_roots().to_vec();
            protected.push(request.stage.parent.to_owned());
            protected.extend(request.parent_source.map(|source| source.path().to_owned()));
            crate::store::coordinator::check_selection_roots(filesystem, selected, &protected)
                .map_err(|error| {
                    before_stage(PreparationError::before(PreparationFailure::Journal(error)))
                })?;
        }
        let has_policy = self
            .inputs()
            .compilation()
            .workspace
            .policy_check()
            .is_some();
        if has_policy || required_policy.is_some() {
            policy::policy_input(self.inputs(), required_policy.map(|r| &r.policy))
                .map_err(|error| LocalPreparationError::new(Failure::Policy(error), None))?;
            let command = policy::command(self.inputs(), options.policy)
                .map_err(|error| LocalPreparationError::new(Failure::Policy(error), None))?;
            if let Some(required) = required_policy {
                required
                    .check_command(command)
                    .map_err(|error| LocalPreparationError::new(Failure::Policy(error), None))?;
            }
        }
        // A selected history source must be protected before the *stage* effect,
        // not only when the later Git adapter imports its parent objects.
        if let Some(source) = request.parent_source {
            let protected = [source.path().to_owned()];
            for root in [request.stage.journal, request.stage.parent] {
                filesystem
                    .check_root_separation(root, &protected)
                    .map_err(|error| {
                        before_stage(PreparationError::before(PreparationFailure::Separation(
                            error,
                        )))
                    })?;
            }
        }
        Ok(has_policy)
    }

    /// Retain, check and prepare this exact output through the existing native owners.
    /// Requires the explicit complete non-draft candidate used to render these bytes.
    /// Permissions, recipe grammar and configured command selection are checked before staging.
    /// Each successful phase's actual generation is the next phase's expectation; no observed
    /// conflict is silently rebased, and no stage, process or Git effect is retried.
    ///
    /// An absent configured checker is recorded explicitly, never as a passing policy result.
    /// Required disclosure still runs. Success binds local checks and native Git read-back;
    /// optional qualification covers only its selected technical profile. It is not full
    /// certification, a release-batch receipt, publisher approval or deployment.
    /// No source is reacquired or rendered again. Gazette performs no remote Git operation;
    /// a selected external checker is independently effectful and is not a sandboxed program.
    ///
    /// # Errors
    /// Preserves the last completed phase and all later native failure evidence. Existing
    /// preparation requires inspection/reconciliation, even if its stage is now missing.
    #[expect(
        clippy::too_many_arguments,
        reason = "Independent effect and inspection ports remain explicit; the SDK owns concrete composition"
    )]
    pub fn prepare_local<F, G, C, R>(
        &self,
        filesystem: &F,
        git: &G,
        connector: &C,
        runner: &R,
        inspectors: DisclosureInspectors<'_>,
        request: LocalPreparationRequest<'_>,
        options: LocalPreparationOptions,
    ) -> Result<store::PreparationReceipt, LocalPreparationError>
    where
        F: StagePlanner + SourceCapture + WorktreeReader,
        G: ReleaseWorktrees,
        C: WriterConnector,
        C::Writer: PreparationWriter,
        R: Runner + CaptureRunner,
    {
        use LocalPreparationFailure as Failure;
        let before_stage = |error| LocalPreparationError::new(Failure::Stage(error), None);
        self.validate_local(filesystem, request, options)?;
        let git_options = JournalWorktreeOptions {
            record: options.stage.record,
            writer: options.stage.writer,
            worktree: options.worktree,
        };
        let staged = self
            .prepare_stage(filesystem, connector, request.stage, options.stage)
            .map_err(before_stage)?;
        let mut checks = match self.check_preparation_with_inspectors(
            runner,
            inspectors,
            request.disclosure,
            options.policy,
            options.disclosure,
            request.effective_output_policy().map_err(before_stage)?,
        ) {
            Ok(checks) => checks,
            Err(error) => {
                let failure = match *error.cause {
                    PreparationFailure::Policy(error) => Failure::Policy(error),
                    cause => Failure::Checks(PreparationError {
                        cause: Box::new(cause),
                        ..error
                    }),
                };
                return Err(LocalPreparationError::new(failure, Some(staged)));
            }
        };
        if let Some(requirements) = request.qualification
            && let Err(error) = checks.qualify(requirements, options.stage.evidence_bytes)
        {
            let mut failure = LocalPreparationError::new(Failure::Checks(error), Some(staged));
            failure.output_policy = checks.into_policy_evidence().map(Box::new);
            return Err(failure);
        }
        let checked = match checks.record_preparation_check(
            filesystem,
            connector,
            PreparationCheckRequest {
                journal: request.stage.journal,
                expected: staged.journal,
                preparation: &staged.record,
            },
            PreparationCheckOptions {
                record: options.stage.record,
                writer: options.stage.writer,
                evidence_bytes: options.stage.evidence_bytes,
            },
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let mut failure = LocalPreparationError::new(Failure::Checks(error), Some(staged));
                failure.technical_certificate = checks.certificate().cloned().map(Box::new);
                failure.output_policy = checks.into_policy_evidence().map(Box::new);
                return Err(failure);
            }
        };
        match prepare_journal_worktree(
            filesystem,
            git,
            connector,
            JournalWorktreeRequest {
                journal: request.stage.journal,
                expected: checked.journal,
                preparation: &checked.record,
                metadata: request.metadata,
                parent_source: request.parent_source,
                protected_roots: self.protected_roots(),
            },
            git_options,
        ) {
            Ok(receipt) => Ok(receipt),
            Err(error) => Err(LocalPreparationError::new(
                Failure::Worktree(error),
                Some(checked),
            )),
        }
    }
}
