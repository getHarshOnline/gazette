//! Complete requested membership and native evidence, never a smaller successful set.

use super::super::execution_evidence::{MathFailure, PolicyFailure, math, policy};
use crate::{build, workspace::Key};
use serde::{Serialize, Serializer};
use std::{collections::BTreeMap, fmt};

/// Private complete-set result. A qualified site is supported-profile technical evidence,
/// not publisher approval, whole-product certification or proof of a remote deployment.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct QualificationReport {
    pub(super) requested: Vec<Key>,
    pub(super) candidate_checks: BTreeMap<Key, build::PolicyInvocation>,
    pub(super) sites: Vec<SiteResult>,
    pub(super) failure: Option<Failure>,
}

impl QualificationReport {
    pub(super) fn new(requested: Vec<Key>) -> Self {
        let sites = requested
            .iter()
            .cloned()
            .map(|site| SiteResult {
                site,
                outcome: Outcome::NotRun,
            })
            .collect();
        Self {
            requested,
            sites,
            candidate_checks: BTreeMap::new(),
            failure: None,
        }
    }

    /// True only for a nonempty complete requested set with every technical check satisfied.
    pub fn is_complete(&self) -> bool {
        self.failure.is_none()
            && !self.requested.is_empty()
            && self.sites.len() == self.requested.len()
            && self
                .sites
                .iter()
                .zip(&self.requested)
                .all(|(site, requested)| {
                    site.site == *requested && matches!(site.outcome, Outcome::Qualified { .. })
                })
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub(super) struct SiteResult {
    pub site: Key,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(super) enum Outcome {
    NotRun,
    #[serde(rename = "supported-profile-technically-qualified")]
    Qualified {
        certificate: Box<build::TechnicalCertificate>,
        output_policy: Option<Box<build::OutputPolicyEvidence>>,
    },
    Failed {
        failure: Failure,
        output_policy: Option<Box<build::OutputPolicyEvidence>>,
    },
}

#[derive(Debug, schemars::JsonSchema)]
#[schemars(with = "FailureView<'_>")]
pub(super) enum Failure {
    Before { code: &'static str, message: String },
    Candidate(Box<build::CandidateCheckFailure>),
    Math(Box<build::MathGenerationError>),
    Diagram(Box<build::DiagramGenerationError>),
    Check(Box<build::PreparationError>),
}

impl Failure {
    pub(super) fn before(code: &'static str, error: impl fmt::Display) -> Self {
        Self::Before {
            code,
            message: error.to_string(),
        }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "phase", rename_all = "kebab-case")]
enum FailureView<'a> {
    Before {
        code: &'a str,
        message: &'a str,
    },
    Candidate {
        completed: &'a BTreeMap<Key, build::PolicyInvocation>,
        completed_responses: &'a BTreeMap<Key, build::MatchedReviewResponse>,
        site: &'a Key,
        evidence: PolicyFailure<'a>,
    },
    Math {
        evidence: MathFailure<'a>,
    },
    Diagram {
        evidence: &'a build::DiagramGenerationError,
    },
    Check {
        message: String,
        policy: Option<PolicyFailure<'a>>,
    },
}

impl Serialize for Failure {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let view = match self {
            Self::Before { code, message } => FailureView::Before { code, message },
            Self::Candidate(error) => FailureView::Candidate {
                completed: &error.completed,
                completed_responses: &error.completed_responses,
                site: &error.site,
                evidence: policy(&error.cause),
            },
            Self::Math(error) => FailureView::Math {
                evidence: math(error),
            },
            Self::Diagram(error) => FailureView::Diagram { evidence: error },
            Self::Check(error) => FailureView::Check {
                message: error.to_string(),
                policy: match error.cause.as_ref() {
                    build::PreparationFailure::Policy(error) => Some(policy(error)),
                    _ => None,
                },
            },
        };
        view.serialize(serializer)
    }
}
