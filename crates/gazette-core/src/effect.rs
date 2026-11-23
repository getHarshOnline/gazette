//! Portable pre-execution effect disclosure shared by native operations.

use schemars::JsonSchema;
use serde::Serialize;

/// How an operation reads its selected native sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceReads {
    /// No selected source is opened.
    None,
    /// The selected source universe is acquired once.
    Once,
    /// The same fixed source universe is reacquired until the operation stops.
    Repeated,
}

/// Network effects attributable to the operation itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NetworkEffects {
    /// Bind or serve explicit loopback endpoints.
    pub loopback: bool,
    /// Contact a non-loopback destination directly.
    pub outbound: bool,
}

/// Git effects attributable to the operation itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GitEffects {
    /// Read repository metadata or objects.
    pub reads: bool,
    /// Create or change objects, references, worktrees or remotes.
    pub mutation: bool,
}

/// Complete portable effect classes selected before native execution.
///
/// This is disclosure, not permission, execution evidence or a release receipt.
/// A selected unconfined process may have effects beyond Gazette's direct effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Plan {
    /// Selected native source acquisition.
    pub source_reads: SourceReads,
    /// Writes to a derived registry, cache or other durable derived state.
    pub derived_state_writes: bool,
    /// Writes to generated publication output trees.
    pub output_writes: bool,
    /// Opens listeners that remain held during the operation.
    pub listeners: bool,
    /// Direct network behavior owned by Gazette.
    pub network: NetworkEffects,
    /// Direct Git behavior owned by Gazette.
    pub git: GitEffects,
    /// Consumes release authority or dispatches a release effect.
    pub release_authority_consumption: bool,
    /// Runs an explicitly selected native provider outside a Gazette sandbox.
    pub unconfined_process_execution: bool,
}
