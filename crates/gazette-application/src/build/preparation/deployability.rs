//! Whole-site delivery eligibility, separate from rendering and release authorization.

use std::fmt;

use gazette_compiler::workspace::{Key, Workspace};

/// Sites whose configured destinations cannot enter preparation while disabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployabilityError {
    sites: Vec<Key>,
}

impl DeployabilityError {
    /// Stable private failure class shared by SDK and interface projections.
    pub fn code(&self) -> &'static str {
        "publication_not_deployable"
    }

    /// Exact refused sites in caller order; no member is silently removed.
    pub fn sites(&self) -> &[Key] {
        &self.sites
    }
}

impl fmt::Display for DeployabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("configured delivery requires deployable=true for")?;
        for site in &self.sites {
            write!(f, " {site}")?;
        }
        f.write_str("; local build, check and preview remain available")
    }
}

impl std::error::Error for DeployabilityError {}

/// Refuse the complete destination-bearing selection when any site is disabled.
///
/// A site without a destination remains local-only and is deliberately allowed.
/// This check grants no release authority to an enabled site.
///
/// # Errors
/// Returns every selected destination-bearing site that is absent or disabled.
pub fn ensure_deployable_sites<'a>(
    workspace: &Workspace,
    sites: impl IntoIterator<Item = (&'a Key, bool)>,
) -> Result<(), DeployabilityError> {
    let sites = sites
        .into_iter()
        .filter_map(|(site, has_destination)| match workspace.site(site) {
            Some(configured) if !has_destination || configured.deployable => None,
            Some(_) | None => Some(site),
        })
        .cloned()
        .collect::<Vec<_>>();
    if sites.is_empty() {
        Ok(())
    } else {
        Err(DeployabilityError { sites })
    }
}
