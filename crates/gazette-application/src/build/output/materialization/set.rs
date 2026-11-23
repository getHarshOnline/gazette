//! Complete-set journaled preflight and exact sequential effects belong to the application.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::{
    publication::Key,
    source::{Limits, Sha256Digest},
    store,
};
use gazette_ports::{
    destination::{OutputReader, OutputWriter, RootSeparation},
    store::MaterializationWriter,
};

use super::{
    MaterializationError, MaterializationOptions, MaterializationPlan, MaterializationResult,
};
use crate::build::{OutputSetPlanFailure, RenderedPublicationSet};

/// Every selected destination checked before any intent append or output write.
#[derive(Debug)]
pub struct MaterializationSetPlan<'a> {
    plans: BTreeMap<Key, MaterializationPlan<'a>>,
}

impl MaterializationSetPlan<'_> {
    /// Exact complete selection, in deterministic site-key order.
    pub fn sites(&self) -> &BTreeMap<Key, MaterializationPlan<'_>> {
        &self.plans
    }

    /// Consume original retained plans under the same already-held native journal.
    /// No failed or historical prefix becomes current complete-set success.
    ///
    /// # Errors
    /// Refuses permissions or incomplete/duplicate operation coordinates before any append.
    /// Later failures retain the exact completed prefix and failed site's evidence.
    pub fn materialize(
        self,
        history: &mut impl MaterializationWriter,
        destination: &impl OutputWriter,
        ids: &BTreeMap<Key, Sha256Digest>,
        options: MaterializationOptions,
    ) -> Result<BTreeMap<Key, MaterializationResult>, MaterializationSetWriteError> {
        let requested = self.plans.keys().cloned().collect::<Vec<_>>();
        let refused = |cause| MaterializationSetWriteError {
            requested: requested.clone(),
            completed: BTreeMap::new(),
            failed: None,
            cause,
        };
        if !options.allow_write {
            return Err(refused(MaterializationError::Permission));
        }
        options
            .history
            .validate()
            .map_err(|error| refused(MaterializationError::Store(error)))?;
        if !self.plans.keys().eq(ids.keys())
            || ids.values().copied().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(refused(MaterializationError::Store(
                store::Error::MaterializationConflict,
            )));
        }
        let mut remaining = options.evidence_bytes;
        for plan in self.plans.values() {
            remaining = remaining.checked_sub(plan.evidence.len()).ok_or_else(|| {
                refused(MaterializationError::Evidence(
                    gazette_core::render::Error::Limit("materialization set evidence"),
                ))
            })?;
        }
        let mut completed = BTreeMap::new();
        for (site, plan) in self.plans {
            match plan.materialize(history, destination, ids[&site], options) {
                Ok(result) => {
                    completed.insert(site, result);
                }
                Err(cause) => {
                    return Err(MaterializationSetWriteError {
                        requested,
                        completed,
                        failed: Some(site),
                        cause,
                    });
                }
            }
        }
        Ok(completed)
    }
}

/// Complete-set planning refusal, before either journal or filesystem effects.
#[derive(Debug)]
pub struct MaterializationSetPlanError {
    /// Every originally requested site, including failed render members.
    pub requested: Vec<Key>,
    /// Exact structural or per-site native cause.
    pub cause: MaterializationSetPlanFailure,
}

/// Shared output boundary or journal-aware single-site planning cause.
#[derive(Debug)]
pub enum MaterializationSetPlanFailure {
    /// Shared complete-set destination/aggregate validation.
    Output(OutputSetPlanFailure),
    /// Exact site whose original plan could not be established.
    Site {
        /// The member that blocked complete-set preflight.
        site: Key,
        /// Unchanged journal-aware native planning cause.
        error: MaterializationError,
    },
}

impl fmt::Display for MaterializationSetPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("journaled output set refused before effects: ")?;
        match &self.cause {
            MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Refused(reason)) => {
                f.write_str(reason)
            }
            MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Site { site, error }) => {
                write!(f, "{site}: {error}")
            }
            MaterializationSetPlanFailure::Site { site, error } => write!(f, "{site}: {error}"),
        }
    }
}
impl std::error::Error for MaterializationSetPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Refused(_)) => None,
            MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Site { error, .. }) => {
                Some(error)
            }
            MaterializationSetPlanFailure::Site { error, .. } => Some(error),
        }
    }
}

/// Exact prefix evidence and separately failed or unattempted set members.
#[derive(Debug)]
pub struct MaterializationSetWriteError {
    /// All selected sites in the original execution order.
    pub requested: Vec<Key>,
    /// Completed native operations or explicitly historical replay results only.
    pub completed: BTreeMap<Key, MaterializationResult>,
    /// No site when complete-set validation refused before effects.
    pub failed: Option<Key>,
    /// Original native phase failure; no implicit retry or rollback.
    pub cause: MaterializationError,
}
impl fmt::Display for MaterializationSetWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "journaled output set incomplete ({} of {} completed), failed site {:?}: {}; retain history and native evidence, do not retry automatically",
            self.completed.len(),
            self.requested.len(),
            self.failed,
            self.cause
        )
    }
}
impl std::error::Error for MaterializationSetWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

impl RenderedPublicationSet<'_> {
    /// Resolve all destinations, protected inputs, histories and bounds before exposing plans.
    ///
    /// # Errors
    /// Refuses incomplete rendering, overlapping roots, pending history or aggregate limits.
    /// No successful subset is returned and no intent is appended during planning.
    pub fn plan_materializations<'a>(
        &'a self,
        history: &mut impl MaterializationWriter,
        destination: &(impl OutputReader + RootSeparation),
        limits: Limits,
        evidence_bytes: usize,
    ) -> Result<MaterializationSetPlan<'a>, MaterializationSetPlanError> {
        let result = (|| {
            let resolved = self
                .output_destinations(destination)
                .map_err(MaterializationSetPlanFailure::Output)?;
            let protected = self.output_protected_roots();
            let mut remaining = limits;
            let mut remaining_evidence = evidence_bytes;
            let mut plans = BTreeMap::new();
            for (site, rendered) in self.sites() {
                let rendered = rendered.as_ref().map_err(|_| {
                    MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Refused(
                        "every requested site must have a complete render",
                    ))
                })?;
                let mut peers = protected.clone();
                peers.extend(
                    resolved
                        .iter()
                        .filter(|(other, _)| *other != site)
                        .map(|(_, root)| root.clone()),
                );
                let plan = rendered
                    .plan_materialization(
                        history,
                        destination,
                        &peers,
                        remaining,
                        remaining_evidence,
                    )
                    .map_err(|error| MaterializationSetPlanFailure::Site {
                        site: site.clone(),
                        error,
                    })?;
                super::super::set::debit(&mut remaining, plan.output())
                    .map_err(MaterializationSetPlanFailure::Output)?;
                remaining_evidence = remaining_evidence.checked_sub(plan.evidence.len()).ok_or(
                    MaterializationSetPlanFailure::Output(OutputSetPlanFailure::Refused(
                        "complete materialization evidence bound exhausted",
                    )),
                )?;
                plans.insert(site.clone(), plan);
            }
            Ok(MaterializationSetPlan { plans })
        })();
        result.map_err(|cause| MaterializationSetPlanError {
            requested: self.sites().keys().cloned().collect(),
            cause,
        })
    }
}
