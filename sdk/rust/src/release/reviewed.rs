//! Workspace-selected publisher scopes and credentials remain independent of approval bytes.

use super::*;
use crate::{build::WorkspaceInput, source::Sha256Digest, workspace::Key};
use gazette_ports::release::{ReleaseDispatcher, RemoteReader};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Explicit owner credential selection for one workspace publisher; never an account-membership claim.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewedCredential {
    /// Existing workspace publisher principal.
    pub publisher: Key,
    /// Operator-confirmed owner of the selected credential.
    pub owner: String,
    /// Literal helper and separate permission to invoke it for HTTPS.
    pub credential: GitCredentialSelection,
}

/// Independently supplied configuration and complete selection, fixed before obtaining approval.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewedSelection {
    /// Supplied configuration or exact local document set; never read from approval fields.
    pub workspace: WorkspaceInput,
    /// Explicit complete set, or the workspace's configured build set when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<Key>")]
    pub sites: Option<Vec<Key>>,
    /// Exactly one explicit credential selection per selected publisher.
    pub credentials: Vec<ReviewedCredential>,
    /// Permit local workspace capture, independently of release effects.
    #[serde(default)]
    pub allow_read: bool,
    /// Bounds for a supplied workspace document.
    #[serde(default)]
    pub limits: crate::toml::Limits,
}

struct SiteScope {
    site: Key,
    domain: String,
    destination: PublicationDestination,
    credential: GitCredentialSelection,
}

/// Validated workspace scopes; construction never reads an approval document.
pub struct ReviewedScope {
    sites: Vec<SiteScope>,
    publishers: Vec<ReviewedPublisher>,
}

impl ReviewedSelection {
    /// Resolve complete publisher and delivery scopes without acquiring a writer or approval.
    ///
    /// # Errors
    /// Refuses unknown/ineligible sites, missing or duplicate owner credentials and denied reads.
    pub fn resolve(&self) -> Result<ReviewedScope, &'static str> {
        let resolved = match &self.workspace {
            WorkspaceInput::Supplied { source } => {
                gazette_application::configuration::resolve(source, self.limits)
            }
            WorkspaceInput::Local {
                root,
                workspace,
                limits,
            } => {
                if !self.allow_read {
                    return Err("release_reviewed_workspace_permission");
                }
                crate::configuration::load_workspace(root, workspace, *limits)
                    .map(crate::configuration::LoadedWorkspace::into_resolved)
            }
        }
        .map_err(|_| "release_reviewed_workspace")?;
        let workspace = &resolved.compilation().workspace;
        let selected = workspace
            .build_selection(self.sites.as_deref())
            .map_err(|_| "release_reviewed_sites")?;
        if selected.sites.len() > Limits::default().targets
            || self.credentials.len() > Limits::default().targets
        {
            return Err("release_reviewed_sites");
        }
        let mut credentials = BTreeMap::new();
        for value in &self.credentials {
            if value.owner.is_empty()
                || value.owner.trim() != value.owner
                || value.owner.chars().any(char::is_control)
                || !value.credential.is_valid()
                || credentials.insert(&value.publisher, value).is_some()
            {
                return Err("release_reviewed_credentials");
            }
        }
        let mut publishers = BTreeMap::<Key, ReviewedPublisher>::new();
        let mut sites = Vec::new();
        let mut destinations = std::collections::BTreeSet::new();
        for (ordinal, id) in selected.sites.into_iter().enumerate() {
            let site = workspace.site(&id).ok_or("release_reviewed_sites")?;
            if !site.deployable {
                return Err("release_reviewed_ineligible");
            }
            let credential = credentials
                .get(&site.publisher)
                .ok_or("release_reviewed_credentials")?;
            let destination = workspace
                .destination(&id)
                .ok_or("release_reviewed_destination")?
                .clone();
            let PublicationDestination::Git { remote, branch } = &destination;
            if !destinations.insert((remote.clone(), branch.clone())) {
                return Err("release_reviewed_destination");
            }
            let publisher =
                publishers
                    .entry(site.publisher.clone())
                    .or_insert_with(|| ReviewedPublisher {
                        publisher_id: site.publisher.clone(),
                        authorized_target_ordinals: vec![],
                        credential_owner: credential.owner.clone(),
                        credential_helper: credential.credential.helper.clone(),
                    });
            publisher
                .authorized_target_ordinals
                .push(u32::try_from(ordinal).map_err(|_| "release_reviewed_sites")?);
            sites.push(SiteScope {
                domain: workspace
                    .public_base_url(&id)
                    .and_then(|base| base.domain())
                    .ok_or("release_reviewed_domain")?,
                site: id,
                destination,
                credential: credential.credential.clone(),
            });
        }
        if publishers.len() != credentials.len() {
            return Err("release_reviewed_credentials");
        }
        Ok(ReviewedScope {
            sites,
            publishers: publishers.into_values().collect(),
        })
    }
}

impl ReviewedScope {
    /// Compare the entire ordered native batch with the independently selected workspace.
    ///
    /// # Errors
    /// Refuses omitted/reordered sites or substituted destinations.
    pub fn check(
        &self,
        selections: &[PreparationSelection],
        targets: &[Target],
    ) -> Result<(), &'static str> {
        if selections.len() != self.sites.len() || targets.len() != self.sites.len() {
            return Err("release_reviewed_complete_set");
        }
        for ((scope, selection), target) in self.sites.iter().zip(selections).zip(targets) {
            let PublicationDestination::Git { remote, branch } = &scope.destination;
            if scope.site != selection.site
                || scope.domain != target.domain
                || scope.domain != selection.domain
                || remote != &target.remote
                || branch != &target.branch
            {
                return Err("release_reviewed_workspace_scope");
            }
        }
        Ok(())
    }

    /// Retain independent scope and explicit confirmed digest beside the exact native targets.
    /// This does not infer confirmation from a document's self-declared digest.
    ///
    /// # Errors
    /// Refuses a partial or substituted set before any release effect.
    pub fn binding(
        &self,
        coordinator_id: &str,
        journal: crate::store::JournalHeader,
        batch: &PreparationBatch,
        selections: &[PreparationSelection],
        confirmed_sha256: Sha256Digest,
        observed_at: u64,
    ) -> Result<ReviewedBinding, &'static str> {
        self.check(selections, batch.targets())?;
        Ok(ReviewedBinding {
            schema_id: ReviewedBindingSchema::V1,
            canonical_body_sha256: confirmed_sha256,
            coordinator_id: coordinator_id.to_owned(),
            journal,
            ordered_targets: batch.targets().to_vec(),
            ordered_sites: self.sites.iter().map(|site| site.site.clone()).collect(),
            publishers: self.publishers.clone(),
            observed_at,
            emergency_stop: false,
        })
    }

    pub(crate) fn check_binding(
        &self,
        binding: &ReviewedBinding,
        selections: &[PreparationSelection],
    ) -> Result<(), &'static str> {
        self.check(selections, &binding.ordered_targets)?;
        if binding.publishers != self.publishers
            || binding
                .ordered_sites
                .iter()
                .ne(self.sites.iter().map(|site| &site.site))
        {
            return Err("release_reviewed_workspace_scope");
        }
        Ok(())
    }

    pub(crate) fn transport<'a>(&'a self, git: &'a GitSelection) -> ScopedTransport<'a> {
        ScopedTransport { scope: self, git }
    }
}

pub(crate) struct ScopedTransport<'a> {
    scope: &'a ReviewedScope,
    git: &'a GitSelection,
}
impl ScopedTransport<'_> {
    fn selection(&self, remote: &str, branch: &str) -> Option<GitSelection> {
        if self.git.credentials.is_some() {
            return None;
        }
        let scope = self.scope.sites.iter().find(|site| {
            let PublicationDestination::Git {
                remote: selected,
                branch: selected_branch,
            } = &site.destination;
            selected == remote && selected_branch == branch
        })?;
        Some(GitSelection {
            credentials: Some(scope.credential.clone()),
            ..self.git.clone()
        })
    }
}
impl RemoteReader for ScopedTransport<'_> {
    fn observe_remote(
        &self,
        remote: &str,
        branch: &str,
        options: RemoteReadOptions,
    ) -> Result<RefAdvertisement, RemoteReadError> {
        let selection = self
            .selection(remote, branch)
            .ok_or_else(|| RemoteReadError::new(RemoteReadFailure::Selection))?;
        gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: &selection,
        }
        .observe_remote(remote, branch, options)
    }
}
impl ReleaseDispatcher for ScopedTransport<'_> {
    fn dispatch(
        &self,
        request: DispatchRequest<'_>,
        options: DispatchOptions,
    ) -> Result<DispatchObservation, DispatchError> {
        let selection = self
            .selection(&request.target.remote, &request.target.branch)
            .ok_or_else(|| DispatchError::refused(DispatchFailure::Selection))?;
        gazette_adapter_git::Dispatcher {
            runner: &gazette_adapter_process::Runner,
            selection: &selection,
        }
        .dispatch(request, options)
    }
}

#[cfg(test)]
#[path = "../../tests/internal/reviewed.rs"]
mod tests;
