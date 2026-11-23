//! Complete-set local orchestration over existing per-site preparation owners.

use std::{collections::BTreeSet, fmt, path::PathBuf};

use gazette_compiler::workspace::Key;
use gazette_core::store;
use gazette_ports::{
    process::{CaptureRunner, Runner},
    release::{ReleaseWorktrees, WorktreeReader},
    source::SourceCapture,
    stage::{StagePlanner, StageTarget},
    store::{PreparationWriter, WriterConnector},
};
use serde::Serialize;

use super::{
    DeployabilityError, LocalPreparationError, LocalPreparationFailure, LocalPreparationOptions,
    LocalPreparationRequest, PreparationError, PreparationFailure, ensure_deployable_sites,
};
use crate::build::{DisclosureInspectors, RenderedPublicationSet};

/// One selected site and its exact existing local preparation contract.
#[derive(Clone, Copy, Debug)]
pub struct SitePreparationRequest<'a> {
    /// Must occur exactly once in the complete rendered set.
    pub site: &'a Key,
    /// Every entry names the same journal and initial expected header.
    pub local: LocalPreparationRequest<'a>,
}

/// A completed site retains its original native history, not a new batch authority.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SitePreparationReceipt {
    /// Exact configured site, in the requested preparation order.
    pub site: Key,
    /// Existing stage/check/Git evidence, including the actual resulting generation.
    pub preparation: store::PreparationReceipt,
}

/// A structural refusal or the unchanged native failure of one site.
#[derive(Debug)]
pub enum LocalPreparationSetFailure {
    /// Invalid complete-set request; no preparation started.
    Refused(&'static str),
    /// The complete destination-bearing set contains at least one disabled site.
    Deployability(DeployabilityError),
    /// Preflight or execution failed for this site; inspect its native effect evidence.
    Site {
        /// Exact failed site.
        site: Key,
        /// May include later observations than the completed prefix.
        error: Box<LocalPreparationError>,
    },
}

/// No successful prefix is promoted into a complete-set result or automatic retry.
#[derive(Debug)]
pub struct LocalPreparationSetError {
    /// Complete caller-supplied order, retained even when the request is refused.
    pub requested: Vec<Key>,
    /// Completed prefix only; the failing site's later evidence remains in `cause`.
    pub completed: Vec<SitePreparationReceipt>,
    /// Structural or site-native failure; this is not release settlement.
    pub cause: Box<LocalPreparationSetFailure>,
}

impl fmt::Display for LocalPreparationSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "local preparation set incomplete ({} of {} completed): ",
            self.completed.len(),
            self.requested.len()
        )?;
        match self.cause.as_ref() {
            LocalPreparationSetFailure::Refused(reason) => f.write_str(reason)?,
            LocalPreparationSetFailure::Deployability(error) => error.fmt(f)?,
            LocalPreparationSetFailure::Site { site, error } => write!(f, "{site}: {error}")?,
        }
        f.write_str(
            "; preserve all receipts and reconcile; no automatic retry, rollback or publication",
        )
    }
}

impl std::error::Error for LocalPreparationSetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.cause.as_ref() {
            LocalPreparationSetFailure::Refused(_) => None,
            LocalPreparationSetFailure::Deployability(error) => Some(error),
            LocalPreparationSetFailure::Site { error, .. } => Some(error.as_ref()),
        }
    }
}

impl RenderedPublicationSet<'_> {
    /// Prepare every selected site in the explicit order, using one initial journal header.
    /// Validates the entire membership, native mode, permissions, recipe, output/evidence
    /// bounds and canonical-caseless destination separation before the first write.
    /// Each existing per-site owner rechecks its effect boundary; only an actual returned
    /// receipt advances the next site's expected generation. No source is reacquired.
    ///
    /// Read-only preflight is not a lock or a promise that a child is absent. Trusted
    /// exclusive namespace control remains required. Per-site reservations are not held
    /// between sites. A process/checker or competing writer can still fail a later phase.
    /// Paths used for cross-target comparison must have a lossless UTF-8 representation;
    /// comparison never rewrites their native spelling. No remote is contacted by Gazette.
    ///
    /// Success is local preparation only, not certification, durable authorization-set
    /// consumption or settlement. Existing native batch inspection remains the later join.
    ///
    /// # Errors
    /// Refuses incomplete, duplicate, unselected or preview inputs before effects. A later
    /// native failure preserves the full request order, completed prefix and failing-site
    /// evidence; no site is dropped, rebased, retried or recreated.
    #[expect(
        clippy::too_many_arguments,
        reason = "Independent native ports remain explicit; the SDK owns concrete composition"
    )]
    pub fn prepare_local<F, G, C, R>(
        &self,
        filesystem: &F,
        git: &G,
        connector: &C,
        runner: &R,
        inspectors: DisclosureInspectors<'_>,
        requests: &[SitePreparationRequest<'_>],
        options: LocalPreparationOptions,
    ) -> Result<Vec<SitePreparationReceipt>, LocalPreparationSetError>
    where
        F: StagePlanner + SourceCapture + WorktreeReader,
        G: ReleaseWorktrees,
        C: WriterConnector,
        C::Writer: PreparationWriter,
        R: Runner + CaptureRunner,
    {
        let requested = requests
            .iter()
            .map(|item| item.site.clone())
            .collect::<Vec<_>>();
        let mut completed = Vec::with_capacity(self.sites().len());
        let result = (|| {
            use LocalPreparationSetFailure::Refused;
            if requests.is_empty()
                || requests.len() != self.sites().len()
                || requests
                    .iter()
                    .map(|item| item.site)
                    .collect::<BTreeSet<_>>()
                    != self.sites().keys().collect()
            {
                return Err(Refused(
                    "requests must cover every rendered site exactly once",
                ));
            }
            if !self.is_complete() || self.selection().is_none() || self.preview_urls().is_some() {
                return Err(Refused(
                    "a complete explicitly selected non-preview render is required",
                ));
            }
            let workspace = &self.compilation().workspace;
            ensure_deployable_sites(
                workspace,
                requests
                    .iter()
                    .map(|item| (item.site, workspace.destination(item.site).is_some())),
            )
            .map_err(LocalPreparationSetFailure::Deployability)?;
            let first = requests[0].local.stage;
            if requests.iter().any(|item| {
                item.local.stage.journal != first.journal
                    || item.local.stage.expected != first.expected
                    || item.local.coordinator != requests[0].local.coordinator
            }) {
                return Err(Refused(
                    "all sites must name the same journal, initial expected header and coordinator selection",
                ));
            }
            if requests
                .iter()
                .map(|item| *item.local.stage.id.as_bytes())
                .collect::<BTreeSet<_>>()
                .len()
                != requests.len()
            {
                return Err(Refused(
                    "preparation IDs must be distinct across the complete set",
                ));
            }
            let mut protected = BTreeSet::new();
            for rendered in self
                .sites()
                .values()
                .filter_map(|value| value.as_ref().ok())
            {
                protected.extend(rendered.protected_roots().iter().cloned());
            }
            protected.extend(requests.iter().filter_map(|item| {
                item.local
                    .parent_source
                    .map(|source| source.path().to_owned())
            }));
            let protected = protected.into_iter().collect::<Vec<_>>();
            if let Some(selected) = requests[0].local.coordinator {
                let mut custody_protected = protected.clone();
                custody_protected.extend(
                    requests
                        .iter()
                        .map(|item| item.local.stage.parent.to_owned()),
                );
                crate::store::coordinator::check_selection_roots(
                    filesystem,
                    selected,
                    &custody_protected,
                )
                .map_err(|error| {
                    stage_failure(
                        requests[0].site,
                        PreparationError::before(PreparationFailure::Journal(error)),
                    )
                })?;
            }
            let mut addresses: Vec<PathBuf> = Vec::with_capacity(requests.len());
            for item in requests {
                let Some(Ok(rendered)) = self.sites().get(item.site) else {
                    return Err(Refused("a requested site has no complete render"));
                };
                rendered
                    .validate_local(filesystem, item.local, options)
                    .map_err(|error| site_failure(item.site, error))?;
                shared_roots(filesystem, item, &protected)?;
                let plan = rendered
                    .plan_preparation(filesystem, item.local.stage, options.stage)
                    .map_err(|error| stage_failure(item.site, error))?;
                let path = plan.target.address().path();
                let key = PathBuf::from(crate::source::collision_key(path.to_str().ok_or(
                    Refused("stage addresses require lossless UTF-8 for cross-target comparison"),
                )?));
                if addresses
                    .iter()
                    .any(|prior| prior.starts_with(&key) || key.starts_with(prior))
                {
                    return Err(Refused(
                        "stage destinations overlap or have canonical-caseless aliases",
                    ));
                }
                addresses.push(key);
            }
            let mut expected = first.expected;
            for item in requests {
                shared_roots(filesystem, item, &protected)?;
                let mut local = item.local;
                local.stage.expected = expected;
                let Some(Ok(rendered)) = self.sites().get(item.site) else {
                    return Err(Refused("a requested site has no complete render"));
                };
                let preparation = rendered
                    .prepare_local(
                        filesystem, git, connector, runner, inspectors, local, options,
                    )
                    .map_err(|error| site_failure(item.site, error))?;
                expected = preparation.journal;
                completed.push(SitePreparationReceipt {
                    site: item.site.clone(),
                    preparation,
                });
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(completed),
            Err(cause) => Err(LocalPreparationSetError {
                requested,
                completed,
                cause: Box::new(cause),
            }),
        }
    }
}

fn site_failure(site: &Key, error: LocalPreparationError) -> LocalPreparationSetFailure {
    LocalPreparationSetFailure::Site {
        site: site.clone(),
        error: Box::new(error),
    }
}

fn stage_failure(site: &Key, error: PreparationError) -> LocalPreparationSetFailure {
    site_failure(
        site,
        LocalPreparationError::new(LocalPreparationFailure::Stage(error), None),
    )
}

fn shared_roots(
    filesystem: &impl StagePlanner,
    item: &SitePreparationRequest<'_>,
    protected: &[PathBuf],
) -> Result<(), LocalPreparationSetFailure> {
    for root in [item.local.stage.journal, item.local.stage.parent] {
        filesystem
            .check_root_separation(root, protected)
            .map_err(|error| {
                stage_failure(
                    item.site,
                    PreparationError::before(PreparationFailure::Separation(error)),
                )
            })?;
    }
    Ok(())
}
