//! Native ref advertisements are observations, not complete namespace or release proof.

use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{process::Cleanup, source::CommitId};

/// One exact branch observation. Advertisement silence is not namespace absence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RefAdvertisement {
    /// The selected provider advertised this exact object for the requested branch.
    Commit {
        /// Full supported commit coordinate, never an abbreviated or moving ref.
        oid: CommitId,
    },
    /// No matching ref was advertised; this does not prove an unborn branch.
    NotAdvertised,
    /// A qualified native namespace inspection proved this exact ref absent.
    /// This is a sequential observation, not a lease or permission to create it.
    Absent,
}

/// Explicit effects and per-destination native transport limits.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteReadOptions {
    /// Permit the selected native provider invocation and destination read.
    pub allow_read: bool,
    /// Additionally permit supported network transports; local reads do not imply this.
    pub allow_network: bool,
    /// Deadline for the complete selected read operation.
    ///
    /// A multi-target observation passes each reader only the remaining budget, so
    /// the target count cannot multiply this bound. A one-target read applies the
    /// same deadline after process creation, including response EOF and direct-child exit.
    pub timeout: Duration,
    /// Maximum complete response bytes and, separately, local namespace metadata.
    /// Exhausted namespace qualification leaves advertisement silence indeterminate.
    #[schemars(range(max = usize::MAX))]
    pub max_response_bytes: usize,
}

impl Default for RemoteReadOptions {
    fn default() -> Self {
        Self {
            allow_read: false,
            allow_network: false,
            timeout: Duration::from_secs(30),
            max_response_bytes: 64 << 10,
        }
    }
}

impl RemoteReadOptions {
    /// Validate global invocation permissions and bounds before selecting any target.
    ///
    /// # Errors
    /// Refuses unacknowledged reads, deadlines outside 1 ms–5 min or responses above 1 MiB.
    pub fn validate(self) -> Result<(), RemoteReadError> {
        if !self.allow_read {
            return Err(RemoteReadError::new(RemoteReadFailure::Permission));
        }
        if self.timeout < Duration::from_millis(1)
            || self.timeout > Duration::from_secs(300)
            || self.max_response_bytes > 1 << 20
        {
            return Err(RemoteReadError::new(RemoteReadFailure::Limit));
        }
        Ok(())
    }
}

/// Redacted failure meaning; native stderr, credentials and endpoint details stay private.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteReadFailure {
    /// The requested effect was not explicitly permitted.
    Permission,
    /// Invalid exact destination, branch or native provider selection.
    Selection,
    /// The selected transport or host cannot implement this observation profile.
    Unsupported,
    /// The provider isolation directory is unsafe, nonempty or changed.
    Isolation,
    /// Native execution, connection or exit did not establish a complete advertisement.
    Transport,
    /// Conflicting, malformed, extra or unsupported advertisement bytes.
    Response,
    /// A request or complete response exceeded its logical bound.
    Limit,
    /// The post-spawn deadline expired.
    Timeout,
}

/// One failed native observation, preserving process custody without returning partial bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteReadError {
    /// Stable failure category.
    pub failure: RemoteReadFailure,
    /// Direct-child custody when a process provider was used; never descendant containment.
    pub cleanup: Option<Cleanup>,
}

impl RemoteReadError {
    /// Refusal before a process provider supplied custody evidence.
    pub const fn new(failure: RemoteReadFailure) -> Self {
        Self {
            failure,
            cleanup: None,
        }
    }

    /// Stable diagnostic code containing no selected native values.
    pub const fn code(self) -> &'static str {
        match self.failure {
            RemoteReadFailure::Permission => "release_read_permission",
            RemoteReadFailure::Selection => "release_read_selection",
            RemoteReadFailure::Unsupported => "release_read_unsupported",
            RemoteReadFailure::Isolation => "release_read_isolation",
            RemoteReadFailure::Transport => "release_read_transport",
            RemoteReadFailure::Response => "release_read_response",
            RemoteReadFailure::Limit => "release_read_limit",
            RemoteReadFailure::Timeout => "release_read_timeout",
        }
    }
}

impl fmt::Display for RemoteReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; no complete native ref observation",
            self.code()
        )
    }
}

impl std::error::Error for RemoteReadError {}
