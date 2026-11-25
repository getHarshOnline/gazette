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
    /// Canonical website root used to keep the principal's public identity stable.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) identity_url: String,
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
                identity_url: url.clone(),
                destination: LinkDestination {
                    kind: LinkDestinationKind::External,
                    content: None,
                    site: None,
                    site_name: None,
                    relationships: Vec::new(),
                    presentation: None,
                },
            }),
            PrincipalWebsite::Site { site } => {
                let canonical = self.compilation().workspace.public_base_url(site)?.as_str();
                let profile = self.profile_route(&principal.id);
                Some(PrincipalWebsiteView {
                    destination: LinkDestination {
                        kind: if site == source_site {
                            LinkDestinationKind::InternalNative
                        } else {
                            LinkDestinationKind::CrossSite
                        },
                        content: profile.map(|(content, _)| content.clone()),
                        site: Some(site.clone()),
                        site_name: None,
                        relationships: Vec::new(),
                        presentation: self.presentation_for(site).map(SitePresentation::from),
                    },
                    url: profile
                        .map_or(self.base_url(site)?.as_str(), |(_, route)| &route.url)
                        .to_owned(),
                    canonical_url: profile
                        .map_or(canonical, |(_, route)| &route.public_url)
                        .to_owned(),
                    identity_url: canonical.to_owned(),
                })
            }
        }
    }
}
