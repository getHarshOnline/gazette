//! Implicit theme trees may omit ignored tooling; shared content keeps its selection.

use super::*;

pub(super) fn exclusive_theme_sources(workspace: &workspace::Workspace) -> BTreeSet<Key> {
    let mut sources: BTreeSet<_> = workspace
        .theme_packages()
        .map(|package| package.source.clone())
        .collect();
    for source in workspace
        .content_bindings()
        .chain(workspace.legal_sources())
    {
        sources.remove(source.source_key());
    }
    for package in workspace.plugin_packages() {
        sources.remove(&package.source);
    }
    if let Some(policy) = workspace.policy_source() {
        sources.remove(&policy.source);
    }
    for brand in workspace.brands() {
        for publication in &brand.publications {
            let Some(build) = publication
                .documentation_repository
                .as_ref()
                .and_then(|repository| repository.build.as_ref())
            else {
                continue;
            };
            for module in &build.modules {
                if let Some(manual) = &module.manual {
                    sources.remove(&manual.source);
                }
                for item in &module.navigation {
                    if let Some((_, _, input)) = item.reference() {
                        sources.remove(input.source());
                    }
                }
            }
        }
    }
    sources
}
