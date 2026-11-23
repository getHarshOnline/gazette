//! Redirect destinations retain exact native identities instead of re-reading a URL.

use super::{ResolvedAuthored, SitePresentation};
use crate::markdown::LinkDestination;
use gazette_core::{markdown::LinkDestinationKind, publication::Key};

impl ResolvedAuthored<'_, '_> {
    /// Resolve an existing canonical redirect target relative to its serving site.
    /// This annotates already compiled routing; it neither invents a route nor
    /// turns a mirror into another local content identity.
    pub fn redirect_destination(
        &self,
        content: &Key,
        serving_site: &Key,
    ) -> Option<LinkDestination> {
        self.content().compilation().workspace.site(serving_site)?;
        let source = self.content().graph().node(content)?;
        self.content().route(content)?;
        let target = source.route.as_ref()?.canonical.as_ref().unwrap_or(content);
        let target = self.content().graph().node(target)?;
        self.content().route(&target.id)?;
        Some(LinkDestination {
            kind: if &target.site == serving_site {
                LinkDestinationKind::InternalNative
            } else {
                LinkDestinationKind::CrossSite
            },
            content: Some(target.id.clone()),
            site: Some(target.site.clone()),
            relationships: Vec::new(),
            presentation: self
                .content()
                .presentation_for(&target.site)
                .map(SitePresentation::from),
        })
    }
}
