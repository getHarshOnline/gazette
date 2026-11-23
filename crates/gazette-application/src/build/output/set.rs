//! Complete configured-output preflight and exact sequential local effects.

use std::{collections::BTreeMap, fmt, path::PathBuf};

use gazette_core::{destination::OutputWriteError, publication::Key, source::Limits};
use gazette_ports::destination::{OutputReader, OutputWriter, RootSeparation};

use super::{MaterializedOutput, OutputPlan, OutputPlanError};
use crate::build::RenderedPublicationSet;

/// All requested destinations checked before any configured-output write.
///
/// Plans borrow the original rendered bytes. This is neither a lock nor a
/// cross-process ownership record; every writer rechecks its native boundary.
#[derive(Debug)]
pub struct OutputSetPlan<'a> {
    plans: BTreeMap<Key, OutputPlan<'a>>,
}

impl OutputSetPlan<'_> {
    /// Exact complete membership, in deterministic site-key order.
    pub fn sites(&self) -> &BTreeMap<Key, OutputPlan<'_>> {
        &self.plans
    }

    /// Consume the original plans without reacquiring or rendering any input.
    ///
    /// A failed site stops the remaining set. Completed receipts and the failed
    /// site's partial effects survive separately; no prefix becomes set success.
    /// This is local output materialization, not release-set settlement.
    ///
    /// # Errors
    /// Preserves exact full membership, actual completed receipts and the native
    /// failed effect. No automatic retry, rollback or temporary-file cleanup.
    pub fn materialize(
        self,
        writer: &impl OutputWriter,
        allow_write: bool,
    ) -> Result<BTreeMap<Key, MaterializedOutput>, OutputSetWriteError> {
        let requested = self.plans.keys().cloned().collect();
        let mut completed = BTreeMap::new();
        for (site, plan) in self.plans {
            match plan.materialize(writer, allow_write) {
                Ok(receipt) => {
                    completed.insert(site, receipt);
                }
                Err(error) => {
                    return Err(OutputSetWriteError {
                        requested,
                        completed,
                        failed: site,
                        error,
                    });
                }
            }
        }
        Ok(completed)
    }
}

/// Exact reason a complete-set plan was refused before any output effect.
#[derive(Debug)]
pub enum OutputSetPlanFailure {
    /// Incomplete membership, unrelated prior receipts or exhausted set bounds.
    Refused(&'static str),
    /// The original configured-destination or native-separation failure.
    Site {
        /// Exact site that prevented complete-set preflight.
        site: Key,
        /// Unchanged per-site cause; no successful plan subset escapes.
        error: OutputPlanError,
    },
}

/// Complete requested membership remains visible when preflight fails.
#[derive(Debug)]
pub struct OutputSetPlanError {
    /// Every requested site, including those whose render failed.
    pub requested: Vec<Key>,
    /// Structural or exact site-native cause.
    pub cause: OutputSetPlanFailure,
}

impl fmt::Display for OutputSetPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("configured output set refused before writes: ")?;
        match &self.cause {
            OutputSetPlanFailure::Refused(reason) => f.write_str(reason),
            OutputSetPlanFailure::Site { site, error } => write!(f, "{site}: {error}"),
        }
    }
}
impl std::error::Error for OutputSetPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            OutputSetPlanFailure::Refused(_) => None,
            OutputSetPlanFailure::Site { error, .. } => Some(error),
        }
    }
}

/// Actual prefix receipts and later partial effects, never complete-set success.
#[derive(Debug)]
pub struct OutputSetWriteError {
    /// Every selected site in the original deterministic execution order.
    pub requested: Vec<Key>,
    /// Only sites whose complete native write/read-back succeeded.
    pub completed: BTreeMap<Key, MaterializedOutput>,
    /// Exact failing site; subsequent requested sites were not attempted.
    pub failed: Key,
    /// Native failure, including attempted paths and any unsettled residue.
    pub error: OutputWriteError,
}
impl fmt::Display for OutputSetWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "configured output set incomplete ({} of {} completed); {}: {}; preserve receipts and effects, do not retry automatically",
            self.completed.len(),
            self.requested.len(),
            self.failed,
            self.error,
        )
    }
}
impl std::error::Error for OutputSetWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl RenderedPublicationSet<'_> {
    pub(super) fn output_protected_roots(&self) -> Vec<PathBuf> {
        self.sites()
            .values()
            .filter_map(|rendered| rendered.as_ref().ok())
            .flat_map(|rendered| rendered.protected_roots().iter().cloned())
            .collect()
    }

    /// Resolve and protect the complete selected set before exposing writable plans.
    /// Both synchronous and journaled materialization use this same native boundary.
    pub(super) fn output_destinations(
        &self,
        reader: &impl RootSeparation,
    ) -> Result<BTreeMap<Key, PathBuf>, OutputSetPlanFailure> {
        use OutputSetPlanFailure::{Refused, Site};
        if self.sites().is_empty() || !self.is_complete() {
            return Err(Refused("every requested site must have a complete render"));
        }
        let protected = self.output_protected_roots();
        let mut roots = BTreeMap::new();
        for (site, rendered) in self.sites() {
            let rendered = rendered
                .as_ref()
                .map_err(|_| Refused("every requested site must have a complete render"))?;
            let root = rendered
                .inputs()
                .output_directory()
                .map_err(|error| Site {
                    site: site.clone(),
                    error: OutputPlanError::Coordinate(error),
                })?
                .ok_or_else(|| Site {
                    site: site.clone(),
                    error: OutputPlanError::Unconfigured,
                })?;
            reader
                .check_root_separation(&root, &protected)
                .map_err(|error| Site {
                    site: site.clone(),
                    error: OutputPlanError::Observation(error),
                })?;
            roots.insert(site.clone(), root);
        }
        for (site, root) in &roots {
            let other_roots = roots
                .iter()
                .filter(|(other, _)| *other != site)
                .map(|(_, root)| root.clone())
                .collect::<Vec<_>>();
            reader
                .check_root_separation(root, &other_roots)
                .map_err(|error| Site {
                    site: site.clone(),
                    error: OutputPlanError::Observation(error),
                })?;
        }
        Ok(roots)
    }

    /// Plan all configured destinations before exposing any writable subset.
    ///
    /// Native ancestry checks reject equal, aliased or nested output roots. All
    /// other destinations remain protected at each site's later write boundary.
    /// The bounds cover the sum of complete observed and proposed inventories,
    /// including preserved user entries. Source roots remain independently protected.
    /// Prior ownership comes only from actual private materialization receipts.
    ///
    /// # Errors
    /// Refuses incomplete renders, foreign prior receipts, missing destinations,
    /// native overlap, unsafe observations and aggregate bounds without effects.
    pub fn plan_outputs<'a>(
        &'a self,
        reader: &(impl OutputReader + RootSeparation),
        previous: &BTreeMap<Key, MaterializedOutput>,
        limits: Limits,
    ) -> Result<OutputSetPlan<'a>, OutputSetPlanError> {
        use OutputSetPlanFailure::{Refused, Site};
        let result = (|| {
            if previous.keys().any(|site| !self.sites().contains_key(site)) {
                return Err(Refused("prior receipts name an unselected site"));
            }
            let resolved = self.output_destinations(reader)?;
            let protected = self.output_protected_roots();
            let mut plans = BTreeMap::new();
            let mut remaining = limits;
            for (site, rendered) in self.sites() {
                let rendered = rendered
                    .as_ref()
                    .map_err(|_| Refused("every requested site must have a complete render"))?;
                let other_roots = resolved
                    .iter()
                    .filter(|(other, _)| *other != site)
                    .map(|(_, root)| root.clone())
                    .collect::<Vec<_>>();
                let mut plan = rendered
                    .plan_output(reader, previous.get(site), remaining)
                    .map_err(|error| Site {
                        site: site.clone(),
                        error,
                    })?;
                debit(&mut remaining, &plan)?;
                plan.protected.extend(other_roots);
                plan.protected.extend_from_slice(&protected);
                plan.protected.sort();
                plan.protected.dedup();
                plans.insert(site.clone(), plan);
            }
            Ok(OutputSetPlan { plans })
        })();
        result.map_err(|cause| OutputSetPlanError {
            requested: self.sites().keys().cloned().collect(),
            cause,
        })
    }
}

pub(super) fn debit(
    limits: &mut Limits,
    plan: &OutputPlan<'_>,
) -> Result<(), OutputSetPlanFailure> {
    use OutputSetPlanFailure::Refused;
    let entries = plan
        .observed()
        .entries()
        .len()
        .checked_add(plan.expected().entries().len())
        .ok_or(Refused("complete observed/proposed entry bound exhausted"))?;
    let bytes = plan
        .observed()
        .total_bytes()
        .checked_add(plan.expected().total_bytes())
        .ok_or(Refused("complete observed/proposed byte bound exhausted"))?;
    limits.entries = limits
        .entries
        .checked_sub(entries)
        .ok_or(Refused("complete observed/proposed entry bound exhausted"))?;
    limits.total_bytes = limits
        .total_bytes
        .checked_sub(bytes)
        .ok_or(Refused("complete observed/proposed byte bound exhausted"))?;
    Ok(())
}
