//! Complete-set configured repository setup before the existing complete output planner.

use super::{CheckoutMaterializationError, Writer};
use crate::{build, release, source, workspace::Key};
use gazette_ports::release::RemoteReader;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Explicit remote pre-state and acquisition source; read failure is never absence.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckoutHistory {
    /// Require a qualified native observation that the remote branch is absent.
    Unborn,
    /// Reuse a complete independently retained local parent source.
    Retained {
        /// Exact observed remote commit.
        expected: source::CommitId,
        /// Existing independent local history.
        source: release::ParentSource,
    },
    /// Acquire complete history into a fresh private bare repository.
    Acquire {
        /// Exact independently observed remote commit.
        expected: source::CommitId,
        /// Fresh absent local repository root, retained on failure.
        root: PathBuf,
        /// Independent permission to write the fresh history repository.
        allow_acquire: bool,
    },
}

/// One site's explicit native setup selections; remote/branch come only from the workspace.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckoutSelection {
    /// Literal Git executable, isolated environment and optional explicit helper.
    pub git: release::GitSelection,
    /// Independently selected current local metadata state.
    pub expected: release::OutputCheckoutState,
    /// Exact remote pre-state and complete local-history choice.
    pub history: CheckoutHistory,
    /// Explicit remote-read/network permission and process limits.
    pub read: release::RemoteReadOptions,
    /// Independent local metadata permissions and native admission limits.
    pub options: release::OutputCheckoutOptions,
}

/// Exact setup failure and completed prefix, separate from generated output history.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CheckoutSetError {
    /// Full requested site set, never a successful subset.
    pub requested: Vec<Key>,
    /// Independently completed metadata setups before the refusal.
    pub completed: BTreeMap<Key, release::OutputCheckoutReport>,
    /// Selected site whose setup failed; None means complete-set preflight refused.
    pub failed: Option<Key>,
    /// Redacted failure code or explanation.
    pub reason: &'static str,
    /// Exact local residue/process evidence when native setup failed.
    pub native: Option<release::RepositorySetupError>,
    /// Successful parent acquisition before a later setup or output-planning failure.
    pub acquired: Option<release::ParentAcquisitionReport>,
}
impl std::fmt::Display for CheckoutSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason)
    }
}
impl std::error::Error for CheckoutSetError {}

/// Decode bounded named checkout selections, rejecting duplicates, tuples and unknown fields.
///
/// # Errors
/// Refuses malformed, empty or excessive complete-set requests before native effects.
pub fn parse_output_checkouts(
    bytes: &[u8],
) -> Result<BTreeMap<Key, CheckoutSelection>, &'static str> {
    if bytes.len() > 4 << 20 {
        return Err("output_checkout_input_limit");
    }
    let supplied = gazette_core::json::parse(bytes).map_err(|_| "output_checkout_input")?;
    let parsed: BTreeMap<Key, CheckoutSelection> =
        serde_json::from_value(supplied.clone()).map_err(|_| "output_checkout_input")?;
    let normalized = serde_json::to_value(&parsed).map_err(|_| "output_checkout_input")?;
    fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => a
                .iter()
                .all(|(key, value)| b.get(key).is_some_and(|other| same(value, other))),
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
            }
            _ => a == b,
        }
    }
    if parsed.is_empty()
        || parsed.len() > 64
        || !supplied.is_object()
        || !same(&supplied, &normalized)
    {
        return Err("output_checkout_input");
    }
    Ok(parsed)
}

impl Writer {
    /// Prepare exactly the rendered set's configured repositories, then leave output writes
    /// to the existing complete-set journal planner. No per-site interface mutation loop exists.
    ///
    /// # Errors
    /// Refuses incomplete membership, permissions or pending history before setup, and retains
    /// each completed prefix plus native uncertainty on later acquisition/setup failure.
    pub fn prepare_configured_checkouts(
        &mut self,
        rendered: &build::RenderedPublicationSet<'_>,
        selections: &BTreeMap<Key, CheckoutSelection>,
        ids: &BTreeMap<Key, source::Sha256Digest>,
        limits: source::Limits,
        evidence_bytes: usize,
    ) -> Result<BTreeMap<Key, release::OutputCheckoutReport>, CheckoutSetError> {
        let requested = rendered.sites().keys().cloned().collect::<Vec<_>>();
        let mut error = CheckoutSetError {
            requested,
            completed: BTreeMap::new(),
            failed: None,
            reason: "output_checkout_selection",
            native: None,
            acquired: None,
        };
        if selections.len() > 64
            || rendered.sites().keys().ne(selections.keys())
            || rendered.sites().keys().ne(ids.keys())
            || ids.values().collect::<BTreeSet<_>>().len() != ids.len()
            || rendered.sites().values().any(Result::is_err)
        {
            return Err(error);
        }
        let mut roots = Vec::new();
        for (id, selected) in selections {
            if !selected.git.is_valid()
                || selected.read.validate().is_err()
                || match selected.expected {
                    release::OutputCheckoutState::Commit { .. } => !selected.options.allow_update,
                    _ => !selected.options.allow_initialize,
                }
                || matches!(
                    selected.history,
                    CheckoutHistory::Acquire {
                        allow_acquire: false,
                        ..
                    }
                )
            {
                error.reason = "output_checkout_permission_or_selection";
                return Err(error);
            }
            let Some(Ok(view)) = rendered.sites().get(id) else {
                return Err(error);
            };
            match self.checkout_selection(view, &[]) {
                Ok((root, _, _)) => roots.push(root),
                Err(cause) => {
                    error.reason = reason(&cause);
                    return Err(error);
                }
            }
        }
        // Reject overlapping configured roots before the first repository effect, including
        // pairs whose directories do not exist yet.
        if roots.iter().enumerate().any(|(i, a)| {
            roots
                .iter()
                .skip(i + 1)
                .any(|b| a.starts_with(b) || b.starts_with(a))
        }) {
            error.reason = "output_checkout_overlap";
            return Err(error);
        }
        for (ordinal, (id, selected)) in selections.iter().enumerate() {
            error.failed = Some(id.clone());
            let Some(Ok(view)) = rendered.sites().get(id) else {
                return Err(error);
            };
            let protected = roots
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != ordinal)
                .map(|(_, p)| p.clone())
                .collect::<Vec<_>>();
            let (_, destination, _) =
                self.checkout_selection(view, &protected).map_err(|cause| {
                    error.reason = reason(&cause);
                    take_error(&mut error)
                })?;
            let release::PublicationDestination::Git { remote, branch } = &destination;
            let expected = match &selected.history {
                CheckoutHistory::Unborn => release::RefAdvertisement::Absent,
                CheckoutHistory::Retained { expected, .. }
                | CheckoutHistory::Acquire { expected, .. } => release::RefAdvertisement::Commit {
                    oid: expected.clone(),
                },
            };
            let reader = gazette_adapter_git::RemoteReader {
                runner: &gazette_adapter_process::Runner,
                selection: &selected.git,
            };
            match reader.observe_remote(remote, branch, selected.read) {
                Ok(observed) if observed == expected => (),
                Ok(_) => {
                    error.reason = "output_checkout_remote_prestate";
                    return Err(error);
                }
                Err(cause) => {
                    error.reason = "output_checkout_remote_read";
                    error.native = Some(release::RepositorySetupError {
                        reason: error.reason,
                        effect: release::RepositorySetupEffect::NotAttempted,
                        cleanup: cause.cleanup,
                    });
                    return Err(error);
                }
            }
            let parent = match &selected.history {
                CheckoutHistory::Unborn => None,
                CheckoutHistory::Retained { expected, source } => {
                    Some(release::OutputCheckoutParent {
                        source: source.clone(),
                        commit: expected.clone(),
                    })
                }
                CheckoutHistory::Acquire {
                    expected,
                    root,
                    allow_acquire,
                } => {
                    let acquired = match self.acquire_configured_parent(
                        view,
                        &selected.git,
                        root,
                        expected,
                        &protected,
                        release::ParentAcquisitionOptions {
                            allow_acquire: *allow_acquire,
                            read: selected.read,
                            limits: selected.options.limits,
                        },
                    ) {
                        Ok(value) => value,
                        Err(cause) => {
                            capture_failure(&mut error, id, cause);
                            return Err(error);
                        }
                    };
                    let parent = release::OutputCheckoutParent {
                        source: acquired.source.clone(),
                        commit: acquired.parent.clone(),
                    };
                    error.acquired = Some(acquired);
                    Some(parent)
                }
            };
            match self.plan_materialization_with_checkout(
                view,
                &selected.expected,
                parent.as_ref(),
                &protected,
                selected.options,
                limits,
                evidence_bytes,
            ) {
                Ok(prepared) => {
                    error.completed.insert(id.clone(), prepared.checkout);
                    error.acquired = None;
                }
                Err(cause) => {
                    capture_failure(&mut error, id, cause);
                    return Err(error);
                }
            }
        }
        Ok(error.completed)
    }
}

fn reason(error: &CheckoutMaterializationError) -> &'static str {
    match error {
        CheckoutMaterializationError::Selection(reason) => reason,
        CheckoutMaterializationError::Store(error) => error.code(),
        CheckoutMaterializationError::Setup(error) => error.reason,
        CheckoutMaterializationError::Plan { error, .. } => error.code(),
    }
}
fn capture_failure(out: &mut CheckoutSetError, id: &Key, cause: CheckoutMaterializationError) {
    out.reason = reason(&cause);
    match cause {
        CheckoutMaterializationError::Setup(error) => out.native = Some(error),
        CheckoutMaterializationError::Plan {
            checkout: Some(report),
            ..
        } => {
            out.completed.insert(id.clone(), report);
        }
        _ => (),
    }
}
fn take_error(error: &mut CheckoutSetError) -> CheckoutSetError {
    CheckoutSetError {
        requested: std::mem::take(&mut error.requested),
        completed: std::mem::take(&mut error.completed),
        failed: error.failed.take(),
        reason: error.reason,
        native: error.native.take(),
        acquired: error.acquired.take(),
    }
}
