//! Complete native target observations, never durable settlement or release permission.

use gazette_compiler::release::TargetSet;
use gazette_core::{
    release::{RemoteReadError, RemoteReadOptions},
    source::Sha256Digest,
};
use gazette_ports::release::RemoteReader;
use serde::Serialize;
use std::time::Instant;

pub use gazette_core::release::{RefComparison, TargetReadback, TargetReadbackRow};

/// Closed meaning of one native ref-readback report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum ReadbackStatus {
    /// Native ref observations were collected without claiming settlement.
    #[serde(rename = "native-ref-observations-not-settlement")]
    ObservedNotSettlement,
}

/// Point-in-time sequential observations bound to one complete original selection
/// and one aggregate deadline.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReadbackReport {
    status: ReadbackStatus,
    /// Identity of the exact checked selection, excluding all mutable observations.
    pub target_set_sha256: Sha256Digest,
    /// Entire ordered observation set, including each failed read.
    pub targets: Vec<TargetReadbackRow>,
}

/// Observe every configured target without acquiring publisher authority or mutating refs.
///
/// No wall-clock, signature activation or database prerequisite is imposed on read-only
/// reconciliation. Read failures remain explicit rows. Sequential matching reads do not
/// create an atomic remote snapshot, hold destination leases, prove an unborn ref, or
/// permit consumption/retry. The supplied reader owns its exact provider selection.
///
/// # Errors
/// Refuses global read permission or invalid logical work limits before invoking any reader.
pub fn observe_targets(
    reader: &impl RemoteReader,
    targets: &TargetSet,
    options: RemoteReadOptions,
) -> Result<ReadbackReport, RemoteReadError> {
    options.validate()?;
    let deadline = Instant::now() + options.timeout;
    let mut rows = Vec::with_capacity(targets.targets().len());
    for target in targets.targets() {
        let readback = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .map_or_else(
                || TargetReadback::Unavailable {
                    error: RemoteReadError::new(gazette_core::release::RemoteReadFailure::Timeout),
                },
                |remaining| {
                    observe_target(
                        reader,
                        target,
                        RemoteReadOptions {
                            timeout: remaining,
                            ..options
                        },
                    )
                },
            );
        rows.push(TargetReadbackRow {
            ordinal: target.ordinal,
            readback,
        });
    }
    Ok(ReadbackReport {
        status: ReadbackStatus::ObservedNotSettlement,
        target_set_sha256: targets.identity(),
        targets: rows,
    })
}

pub(super) fn observe_target(
    reader: &impl RemoteReader,
    target: &gazette_core::release::Target,
    options: RemoteReadOptions,
) -> TargetReadback {
    match reader.observe_remote(&target.remote, &target.branch, options) {
        Ok(advertisement) => {
            let comparison = RefComparison::compare(target, &advertisement);
            TargetReadback::Observed {
                advertisement,
                comparison,
            }
        }
        Err(error) => TargetReadback::Unavailable { error },
    }
}
