//! Candidate checks precede every site program and retain their effectful prefix.

use std::{collections::BTreeMap, fmt, sync::Arc};

use gazette_core::{process, publication::Key};

use super::{
    CandidatePolicyRequirement, PolicyCheck, PolicyCheckError, PolicyInvocation, candidate,
    command, encode_candidate_request, invoke, match_response, policy_input,
};
use crate::build::{
    CandidateAdmission, ReviewLimits, ReviewResponseLimits, ReviewSubmission, SelectedPublication,
};

pub(in crate::build) struct CandidateSet {
    pub checks: BTreeMap<Key, Arc<PolicyCheck>>,
    pub admissions: Option<BTreeMap<Key, Arc<CandidateAdmission>>>,
}

/// Explicit pre-render checker selection. The command remains workspace-owned;
/// output checking is a separate phase, and neither zero exit grants approval.
#[derive(Clone, Copy, Debug)]
pub struct CandidateCheckOptions<'a> {
    /// Complete independently selected policy/evaluator set for native response capture.
    /// Omission retains exit-only checks with optional independently imported responses.
    pub requirements: Option<&'a BTreeMap<Key, CandidatePolicyRequirement>>,
    /// Native execution permission and bounded input/process custody.
    pub process: process::Options,
    /// Complete per-site review limits.
    pub review: ReviewLimits,
    /// Aggregate encoded review bytes retained before the first checker is invoked.
    pub bytes: usize,
    /// Independent per-site response byte and decision bounds.
    pub response: ReviewResponseLimits,
    /// Aggregate original response bytes, checked before matching or any execution.
    pub response_bytes: usize,
}

/// No rendered output is produced after a failed candidate check. Completed
/// invocations may have external effects; cancellation is not safe retry authority.
#[derive(Debug)]
pub struct CandidateCheckFailure {
    /// Every checker that returned zero, in stable site-key order.
    pub completed: BTreeMap<Key, PolicyInvocation>,
    /// Exact earlier matched responses survive a later site's refusal or uncertain effect.
    pub completed_responses: BTreeMap<Key, crate::build::MatchedReviewResponse>,
    /// The site whose request projection or native invocation failed.
    pub site: Key,
    /// Exact failing invocation and child custody, when execution was attempted.
    pub cause: PolicyCheckError,
}

impl fmt::Display for CandidateCheckFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "candidate check for {} failed after {} completed checks: {}; preserve invocation evidence; no automatic retry",
            self.site,
            self.completed.len(),
            self.cause
        )
    }
}

impl std::error::Error for CandidateCheckFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit source, response, execution and projection boundaries"
)]
pub(in crate::build) fn check_sites<'a>(
    selected: &SelectedPublication<'_, '_>,
    sites: impl Iterator<Item = (Key, &'a crate::build::AcquiredInputs)>,
    responses: Option<&BTreeMap<Key, ReviewSubmission>>,
    runner: &dyn gazette_ports::process::Runner,
    capture: &dyn gazette_ports::process::CaptureRunner,
    options: CandidateCheckOptions<'_>,
    raster: &gazette_core::raster::Inspect<'_>,
    markdown: &dyn gazette_ports::markdown::Markdown,
) -> Result<CandidateSet, Box<CandidateCheckFailure>> {
    let mut inputs = BTreeMap::new();
    let mut remaining = options.bytes;
    let mut response_remaining = options.response_bytes;
    let sites: Vec<_> = sites.collect();
    if let Some(requirements) = options.requirements
        && let Some((site, _)) = sites.first()
        && (responses.is_some()
            || requirements.len() != sites.len()
            || sites
                .iter()
                .any(|(site, _)| !requirements.contains_key(site))
            || options.response.bytes == 0
            || options.response.decisions == 0
            || options.response_bytes == 0)
    {
        return Err(Box::new(CandidateCheckFailure {
            completed: BTreeMap::new(),
            completed_responses: BTreeMap::new(),
            site: site.clone(),
            cause: PolicyCheckError::Response(crate::build::ReviewResponseError::Binding(
                "complete independently selected candidate policies",
            )),
        }));
    }
    // Complete every request before any effect: a known late review/bounds failure
    // must not execute a successful prefix of the configured checker.
    for (site, presentation) in sites {
        let input = (|| {
            let command = command(selected.inputs(), options.process)?;
            let requirement = options
                .requirements
                .and_then(|requirements| requirements.get(&site));
            if let Some(requirement) = requirement {
                requirement.check_command(command)?;
            }
            let policy_source = if let Some(requirement) = requirement {
                policy_input(selected.inputs(), Some(&requirement.policy))?
            } else {
                None
            };
            let submission = responses.and_then(|responses| responses.get(&site));
            if let Some(submission) = submission {
                response_remaining = response_remaining
                    .checked_sub(submission.response_json.len())
                    .ok_or(PolicyCheckError::Response(
                        crate::build::ReviewResponseError::Limit("aggregate response bytes"),
                    ))?;
            }
            let review = selected
                .review_site(&site, presentation, options.review, raster, markdown)
                .map_err(PolicyCheckError::Review)?;
            let response = match_response(&review, submission, options.response)?;
            let input = encode_candidate_request(
                &review,
                &response,
                requirement,
                policy_source.as_ref(),
                options.process.max_input_bytes.min(remaining),
            )
            .map_err(PolicyCheckError::Encoding)?;
            Ok((review, response, input, requirement, policy_source))
        })()
        .map_err(|cause| {
            Box::new(CandidateCheckFailure {
                completed: BTreeMap::new(),
                completed_responses: BTreeMap::new(),
                site: site.clone(),
                cause,
            })
        })?;
        remaining -= input.2.len();
        inputs.insert(site, input);
    }
    let mut completed = BTreeMap::new();
    let mut checks = BTreeMap::new();
    for (site, (review, response, input, requirement, policy_source)) in inputs {
        let result = command(selected.inputs(), options.process).and_then(|command| {
            if let Some(requirement) = requirement {
                let limits = ReviewResponseLimits {
                    bytes: options.response.bytes.min(response_remaining),
                    ..options.response
                };
                candidate::capture(
                    capture,
                    command,
                    options.process,
                    &input,
                    &review,
                    requirement,
                    limits,
                )
                .map(|(invocation, response, bytes)| {
                    response_remaining -= bytes;
                    (invocation, Some(response))
                })
            } else {
                invoke(runner, command, options.process, &input)
                    .map(|invocation| (invocation, response))
            }
        });
        match result {
            Ok((invocation, response)) => {
                completed.insert(site.clone(), invocation);
                checks.insert(
                    site,
                    PolicyCheck {
                        status: super::PolicyCheckStatus::PassedUnverified,
                        invocation,
                        review,
                        response,
                        requirement: requirement.cloned(),
                        policy_source,
                    },
                );
            }
            Err(cause) => {
                return Err(Box::new(CandidateCheckFailure {
                    completed,
                    completed_responses: checks
                        .into_iter()
                        .filter_map(|(site, check)| check.response.map(|response| (site, response)))
                        .collect(),
                    site,
                    cause,
                }));
            }
        }
    }
    let checks = checks
        .into_iter()
        .map(|(site, check)| (site, Arc::new(check)))
        .collect::<BTreeMap<_, _>>();
    let admissions = options.requirements.map(|requirements| {
        checks
            .iter()
            .map(|(site, check)| {
                let required = requirements.get(site).ok_or_else(|| {
                    Box::new(CandidateCheckFailure {
                        completed: checks
                            .iter()
                            .map(|(site, check)| (site.clone(), check.invocation()))
                            .collect(),
                        completed_responses: checks
                            .iter()
                            .filter_map(|(site, check)| {
                                check
                                    .response()
                                    .map(|response| (site.clone(), response.clone()))
                            })
                            .collect(),
                        site: site.clone(),
                        cause: PolicyCheckError::Admission(crate::build::AdmissionError::Binding(
                            "complete admitted site set",
                        )),
                    })
                })?;
                CandidateAdmission::derive(
                    site,
                    selected,
                    Arc::clone(check),
                    required,
                    options.review.bytes,
                )
                .map(|admission| (site.clone(), Arc::new(admission)))
                .map_err(|error| {
                    Box::new(CandidateCheckFailure {
                        completed: checks
                            .iter()
                            .map(|(site, check)| (site.clone(), check.invocation()))
                            .collect(),
                        completed_responses: checks
                            .iter()
                            .filter_map(|(site, check)| {
                                check
                                    .response()
                                    .map(|response| (site.clone(), response.clone()))
                            })
                            .collect(),
                        site: site.clone(),
                        cause: PolicyCheckError::Admission(error),
                    })
                })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
    });
    Ok(CandidateSet {
        checks,
        admissions: admissions.transpose()?,
    })
}
