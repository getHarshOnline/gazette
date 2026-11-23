//! Typed historical ref evidence; classification never establishes who caused an effect.

use serde::{Deserialize, Serialize};

use super::{RefAdvertisement, RemoteReadError, RemoteState, Target};

/// Comparison against one exact action and reviewed pre-state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefComparison {
    /// The intended commit was advertised, not necessarily caused by this caller.
    Intended,
    /// The reviewed commit or independently qualified absent pre-state was observed.
    Prestate,
    /// A different commit was advertised.
    Conflict,
    /// No advertised ref; hidden/inaccessible and unborn remain indistinguishable.
    Indeterminate,
}

impl RefComparison {
    /// Classify native evidence without inferring absence from silence, approval or settlement.
    pub fn compare(target: &Target, advertisement: &RefAdvertisement) -> Self {
        match advertisement {
            RefAdvertisement::Commit { oid } if oid == &target.local_commit_oid => Self::Intended,
            RefAdvertisement::Commit { oid } if matches!(&target.expected_remote_oid_or_unborn, RemoteState::Commit { oid: expected } if expected == oid) => {
                Self::Prestate
            }
            RefAdvertisement::Commit { .. } => Self::Conflict,
            RefAdvertisement::NotAdvertised => Self::Indeterminate,
            RefAdvertisement::Absent
                if matches!(target.expected_remote_oid_or_unborn, RemoteState::Unborn) =>
            {
                Self::Prestate
            }
            RefAdvertisement::Absent => Self::Conflict,
        }
    }
}

/// Complete native advertisement or explicit failure, never a partial response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetReadback {
    /// Native response and its comparison to the retained target.
    Observed {
        /// Exact evidence; only independently qualified absence can match an unborn pre-state.
        advertisement: RefAdvertisement,
        /// Comparison using the shared classification rule.
        comparison: RefComparison,
    },
    /// The provider produced no usable complete response.
    Unavailable {
        /// Redacted failure and direct-child custody, when known.
        error: RemoteReadError,
    },
}

/// One original ordinal, including unavailable and indeterminate observations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetReadbackRow {
    /// Exact zero-based position; no sorting or omission is permitted.
    pub ordinal: u32,
    /// Sequential observation, not consumption, an attempt or settlement.
    pub readback: TargetReadback,
}
