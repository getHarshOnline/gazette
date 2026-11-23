//! Discussion follows publication source ownership, never About/Mentions relationships.

use gazette_core::publication::{
    Discussion, Key,
    content::{ContentKind, ContentNode, PageFamily},
};
use serde::Serialize;

use crate::workspace::{CapabilitySelection, ContentSource, ResolvedContent};

/// Exact selected public binding. Provider availability, consent and activation
/// remain separate capability and release evidence; this is not a working-widget claim.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiscussionView<'a> {
    /// The site or native project owning this conversation.
    pub owner: &'a Key,
    /// Its explicitly declared public repository, never a private destination remote.
    pub repository: &'a str,
    /// Complete owner-native atom, without site fallback or option merging.
    pub binding: &'a Discussion,
}

pub(super) fn resolve<'a>(
    resolved: &'a ResolvedContent<'_>,
    content: &ContentNode,
    provenance: Option<&ContentSource<'a>>,
) -> Option<DiscussionView<'a>> {
    let workspace = &resolved.compilation().workspace;
    let ContentKind::Page { family } = &content.structure else {
        return None;
    };
    if !matches!(family, PageFamily::Article | PageFamily::Documentation) {
        return None;
    }
    let (owner, repository, atom) =
        if let Some(project) = provenance.and_then(|source| source.project) {
            let project = resolved.project(project)?;
            (
                &project.id,
                project.repository.as_deref()?,
                project.discussion.as_ref()?,
            )
        } else if matches!(family, PageFamily::Article) {
            let site = workspace.site(&content.site)?;
            (
                &site.id,
                workspace.publication_for(&site.id)?.repository.as_deref()?,
                site.discussion.as_ref()?,
            )
        } else {
            return None;
        };
    let selected = workspace
        .composition_for(&content.site)?
        .capabilities()
        .get(&atom.capability)?;
    if !matches!(selected.value(), CapabilitySelection::Enabled { .. }) {
        return None;
    }
    Some(DiscussionView {
        owner,
        repository,
        binding: atom,
    })
}
