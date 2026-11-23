//! Explicit principal websites share the selected publication address model.

use super::SitePresentation;
use crate::markdown::LinkDestination;
use gazette_core::{
    markdown::LinkDestinationKind,
    publication::{Key, Principal, PrincipalWebsite},
};
use serde::Serialize;

use crate::workspace::ResolvedContent;

/// One website with separate human navigation and stable public identity addresses.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PrincipalWebsiteView {
    /// Selected public or validated local-preview destination for an ordinary link.
    pub url: String,
    /// Configured public destination for structured identity metadata.
    pub canonical_url: String,
    /// Exact native destination facts relative to the rendered owning site.
    pub destination: LinkDestination,
}

impl ResolvedContent<'_> {
    pub(crate) fn principal_website(
        &self,
        principal: &Principal,
        source_site: &Key,
    ) -> Option<PrincipalWebsiteView> {
        match principal.website.as_ref()? {
            PrincipalWebsite::External { url } => Some(PrincipalWebsiteView {
                url: url.clone(),
                canonical_url: url.clone(),
                destination: LinkDestination {
                    kind: LinkDestinationKind::External,
                    content: None,
                    site: None,
                    relationships: Vec::new(),
                    presentation: None,
                },
            }),
            PrincipalWebsite::Site { site } => Some(PrincipalWebsiteView {
                destination: LinkDestination {
                    kind: if site == source_site {
                        LinkDestinationKind::InternalNative
                    } else {
                        LinkDestinationKind::CrossSite
                    },
                    content: None,
                    site: Some(site.clone()),
                    relationships: Vec::new(),
                    presentation: self.presentation_for(site).map(SitePresentation::from),
                },
                url: self.base_url(site)?.as_str().to_owned(),
                canonical_url: self
                    .compilation()
                    .workspace
                    .public_base_url(site)?
                    .as_str()
                    .to_owned(),
            }),
        }
    }
}
