//! Workspace-owned local build defaults, distinct from invocation and admission.

use super::{Diagnostic, Key, Site};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Explicit default site set and developer draft policy. Neither authorizes effects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// Exact configured sites, preserving declared order; never every known site by inference.
    pub sites: Vec<Key>,
    /// Local-only rendering policy. Candidate admission always retains its non-draft gate.
    pub local_drafts: LocalDrafts,
    /// Aggregate authored input bytes, including assets; invocation limits may override this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = usize::MAX))]
    pub corpus_bytes: Option<usize>,
    /// Select native math generation; execution permission belongs to each invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub math: Option<bool>,
}

/// Whether an unfiltered local build retains draft content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LocalDrafts {
    /// Render selected drafts for private development; this grants no publication approval.
    Include,
    /// Derive a complete non-draft selection before rendering; broken closure is an error.
    Exclude,
}

/// Where the exact effective site set was selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BuildOrigin {
    /// Explicit workspace default set.
    Workspace,
    /// Explicit invocation override, independent of the configured default set.
    Invocation,
}

/// Resolved local intent carried into acquisition evidence, not an approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct BuildSelection {
    /// Exactly the chosen set in its declared order, including any later site failure.
    pub sites: Vec<Key>,
    /// Owner of the effective site selection.
    pub origin: BuildOrigin,
    /// Configured local draft policy; absence does not manufacture a workspace policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_drafts: Option<LocalDrafts>,
}

impl super::Workspace {
    /// Resolve configured defaults or one explicit override before acquisition.
    ///
    /// # Errors
    /// Refuses absent defaults, empty/duplicate overrides and undeclared sites.
    /// An explicit empty override is never silently replaced by configuration.
    pub fn build_selection(
        &self,
        invocation: Option<&[Key]>,
    ) -> Result<BuildSelection, &'static str> {
        let (sites, origin) = match invocation {
            Some(sites) => (sites, BuildOrigin::Invocation),
            None => (
                self.build()
                    .ok_or("workspace has no configured build site set")?
                    .sites
                    .as_slice(),
                BuildOrigin::Workspace,
            ),
        };
        if sites.is_empty() {
            return Err("effective build site set is empty");
        }
        let mut seen = BTreeSet::new();
        for site in sites {
            if !seen.insert(site) {
                return Err("duplicate site in effective build selection");
            }
            if self.site(site).is_none() {
                return Err("effective build selection names an undeclared site");
            }
        }
        Ok(BuildSelection {
            sites: sites.to_vec(),
            origin,
            local_drafts: self.build().map(|build| build.local_drafts),
        })
    }
}

impl Build {
    pub(super) fn validate(&self, sites: &BTreeMap<Key, Site>, errors: &mut Vec<Diagnostic>) {
        if self.corpus_bytes == Some(0) {
            errors.push(Diagnostic {
                code: "workspace_build_limit",
                field: "build.corpus_bytes".into(),
                key: None,
            });
        }
        if self.sites.is_empty() {
            errors.push(Diagnostic {
                code: "workspace_build_empty",
                field: "build.sites".into(),
                key: None,
            });
        }
        let mut seen = BTreeSet::new();
        for site in &self.sites {
            let code = if !seen.insert(site) {
                Some("workspace_duplicate")
            } else if !sites.contains_key(site) {
                Some("workspace_reference")
            } else {
                None
            };
            if let Some(code) = code {
                errors.push(Diagnostic {
                    code,
                    field: "build.sites".into(),
                    key: Some(site.clone()),
                });
            }
        }
    }
}
