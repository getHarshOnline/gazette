//! Borrowed operator evidence; native effects and receipts are never reconstructed.

use super::LocalPreparationRunError;
use crate::build::execution_evidence::{
    MathFailure as MathEvidence, PolicyFailure as PolicyEvidence, math, policy,
};
use gazette_application::{build, release};
use gazette_core::{release::WorktreeEffect, store};
use serde::{Serialize, Serializer};
use std::path::Path;

/// Borrowed complete operator outcome shared by CLI and protocol interfaces.
/// Serialization retains native phase evidence; it grants no certification or replay authority.
#[derive(schemars::JsonSchema)]
#[schemars(with = "Report<'a>")]
pub struct LocalPreparationRunReport<'a> {
    result: &'a Result<Vec<build::SitePreparationReceipt>, LocalPreparationRunError>,
}

impl<'a> LocalPreparationRunReport<'a> {
    /// Project the exact returned outcome without copying or reinterpreting native receipts.
    pub fn new(
        result: &'a Result<Vec<build::SitePreparationReceipt>, LocalPreparationRunError>,
    ) -> Self {
        Self { result }
    }
}

impl Serialize for LocalPreparationRunReport<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.result {
            Ok(sites) => Report::Completed { sites }.serialize(serializer),
            Err(error) => error.serialize(serializer),
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Report<'a> {
    DiagramGenerationFailed {
        evidence: &'a build::DiagramGenerationError,
    },
    MathGenerationFailed {
        evidence: MathEvidence<'a>,
    },
    #[serde(rename = "local-preparations-complete-not-certified")]
    Completed {
        sites: &'a [build::SitePreparationReceipt],
    },
    NotStarted {
        code: &'a str,
        message: &'a str,
    },
    Incomplete {
        candidate_checks:
            &'a std::collections::BTreeMap<crate::workspace::Key, build::PolicyInvocation>,
        requested: &'a [crate::workspace::Key],
        completed: &'a [build::SitePreparationReceipt],
        failure: Failure<'a>,
    },
    CandidateFailed {
        completed: &'a std::collections::BTreeMap<crate::workspace::Key, build::PolicyInvocation>,
        completed_responses:
            &'a std::collections::BTreeMap<crate::workspace::Key, build::MatchedReviewResponse>,
        site: &'a crate::workspace::Key,
        evidence: PolicyEvidence<'a>,
    },
    RenderFailed {
        requested: &'a [crate::workspace::Key],
        candidate_checks:
            &'a std::collections::BTreeMap<crate::workspace::Key, build::PolicyInvocation>,
        failures: &'a std::collections::BTreeMap<
            crate::workspace::Key,
            super::LocalPreparationRenderFailure,
        >,
    },
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Failure<'a> {
    Refused {
        message: &'a str,
    },
    Deployability {
        code: &'static str,
        sites: &'a [crate::workspace::Key],
        message: String,
    },
    Site {
        site: &'a crate::workspace::Key,
        prior_receipt: Option<&'a store::PreparationReceipt>,
        output_policy: Option<&'a build::OutputPolicyEvidence>,
        #[serde(skip_serializing_if = "Option::is_none")]
        technical_certificate: Option<&'a build::TechnicalCertificate>,
        phase: Phase<'a>,
    },
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Phase<'a> {
    Stage { evidence: Evidence<'a> },
    Policy { evidence: PolicyEvidence<'a> },
    Checks { evidence: Evidence<'a> },
    Worktree { evidence: Evidence<'a> },
}

#[derive(Serialize, Default, schemars::JsonSchema)]
struct Evidence<'a> {
    message: String,
    receipt: Option<&'a store::PreparationReceipt>,
    journal: Option<store::JournalHeader>,
    writer_acquired: bool,
    generation_effect: Option<store::GenerationEffect>,
    record_effect: Option<store::PreparationEffect>,
    stage: Option<&'a Path>,
    stage_residue: Option<&'a Path>,
    git_effect: Option<&'a WorktreeEffect>,
    native_git_effect: Option<&'a WorktreeEffect>,
}

fn stage(error: &build::PreparationError) -> Evidence<'_> {
    let mut view = Evidence {
        message: error.to_string(),
        receipt: error.receipt.as_deref(),
        journal: error.journal,
        writer_acquired: error.writer_acquired,
        stage: error.stage.as_deref(),
        ..Default::default()
    };
    match error.cause.as_ref() {
        build::PreparationFailure::Writer(e) => {
            view.generation_effect = Some(e.generation);
            view.message = e.to_string();
        }
        build::PreparationFailure::Record(e) => {
            view.record_effect = Some(e.effect);
            view.message = e.to_string();
        }
        build::PreparationFailure::Stage(e) => {
            view.message = e.to_string();
            if let gazette_core::stage::Error::Residue { path, .. } = e {
                view.stage_residue = Some(path);
            }
        }
        _ => {}
    }
    view
}

fn worktree(error: &release::JournalWorktreeError) -> Evidence<'_> {
    let mut view = Evidence {
        message: error.to_string(),
        receipt: error.receipt.as_deref(),
        journal: error.journal,
        writer_acquired: error.writer_acquired,
        git_effect: Some(&error.effect),
        ..Default::default()
    };
    match error.cause.as_ref() {
        release::JournalWorktreeFailure::Writer(e) => {
            view.generation_effect = Some(e.generation);
            view.message = e.to_string();
        }
        release::JournalWorktreeFailure::Record(e) => {
            view.record_effect = Some(e.effect);
            view.message = e.to_string();
        }
        release::JournalWorktreeFailure::Worktree(e) => {
            view.native_git_effect = Some(&e.effect);
            view.message = e.to_string();
        }
        _ => {}
    }
    view
}

impl Serialize for LocalPreparationRunError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let report = match self {
            Self::Diagram(error) => Report::DiagramGenerationFailed { evidence: error },
            Self::Math(error) => Report::MathGenerationFailed {
                evidence: math(error),
            },
            Self::Before { code, message } => Report::NotStarted { code, message },
            Self::Candidate(error) => Report::CandidateFailed {
                completed: &error.completed,
                completed_responses: &error.completed_responses,
                site: &error.site,
                evidence: policy(&error.cause),
            },
            Self::Render {
                requested,
                candidate_checks,
                failures,
            } => Report::RenderFailed {
                requested,
                candidate_checks,
                failures,
            },
            Self::Preparation {
                candidate_checks,
                error,
            } => Report::Incomplete {
                candidate_checks,
                requested: &error.requested,
                completed: &error.completed,
                failure: match error.cause.as_ref() {
                    build::LocalPreparationSetFailure::Refused(message) => {
                        Failure::Refused { message }
                    }
                    build::LocalPreparationSetFailure::Deployability(error) => {
                        Failure::Deployability {
                            code: error.code(),
                            sites: error.sites(),
                            message: error.to_string(),
                        }
                    }
                    build::LocalPreparationSetFailure::Site { site, error } => Failure::Site {
                        site,
                        prior_receipt: error.receipt.as_deref(),
                        output_policy: error.output_policy.as_deref(),
                        technical_certificate: error.technical_certificate.as_deref(),
                        phase: match error.cause.as_ref() {
                            build::LocalPreparationFailure::Stage(e) => {
                                Phase::Stage { evidence: stage(e) }
                            }
                            build::LocalPreparationFailure::Checks(e) => {
                                Phase::Checks { evidence: stage(e) }
                            }
                            build::LocalPreparationFailure::Policy(e) => Phase::Policy {
                                evidence: policy(e),
                            },
                            build::LocalPreparationFailure::Worktree(e) => Phase::Worktree {
                                evidence: worktree(e),
                            },
                        },
                    },
                },
            },
        };
        report.serialize(serializer)
    }
}
