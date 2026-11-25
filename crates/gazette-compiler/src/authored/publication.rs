//! One resolved public metadata view, distinct from principals and private publication machinery.

use std::{collections::BTreeMap, fmt};

use gazette_core::publication::{Key, LanguageTag, Principal};
use serde::Serialize;

use super::ResolvedAuthored;
use crate::workspace::{PublicationContacts, ResolvedContent, SocialProfile};

/// Explicit display parentage, without role, contact or author inheritance.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PublicationBrand<'a> {
    /// Exact selected brand identity.
    pub id: &'a Key,
    /// Explicit display name, not a natural identity key.
    pub name: &'a str,
    /// Explicit validated parent; absence is never inferred from shared publication machinery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<&'a Key>,
    /// A parent's public home when its configured publication is unambiguous.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_home: Option<&'a str>,
}

/// Shallow home reference over the selected graph and its already compiled serving address.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PublicationHome<'a> {
    /// Exact same-site content identity, not a generated second object.
    pub id: &'a Key,
    /// Explicit content title, independent from brand display naming.
    pub title: &'a str,
    /// Existing serving URL, including a validated preview base when selected.
    pub url: &'a str,
}

/// Shared publication metadata for page, Theme, discovery and local inspection consumers.
/// These public interfaces establish neither identity admission nor provider activation.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PublicationMetadata<'a> {
    /// Exact publication site.
    pub site: &'a Key,
    /// Exact configured site rights holder, independent from publisher and page attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights_holder: Option<&'a Principal>,
    /// Supplied render-year context; compiler projections never consult a clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_year: Option<RenderYear>,
    /// Sole associated brand and explicit parentage only.
    pub brand: PublicationBrand<'a>,
    /// Exact declared routed home, absent when the workspace omitted it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<PublicationHome<'a>>,
    /// Routed About page that names the brand as its primary subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<&'a Key>,
    /// Explicit normalized publication language; also supplies omitted page language.
    /// Inheritance does not create an authored source span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<&'a LanguageTag>,
    /// Independent, explicit contact interfaces, with omitted roles remaining omitted.
    pub contacts: &'a PublicationContacts,
    /// Named public profile references; never automatically emitted as `sameAs`.
    pub social: &'a BTreeMap<String, SocialProfile>,
    /// Explicit public publication repository, not private delivery coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<&'a str>,
}

/// A declared metadata endpoint cannot bind to the selected publication graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationMetadataError {
    /// Site declaring the failing interface.
    pub site: Key,
    /// Exact missing, wrong-site or unrouted home identity (or unknown site).
    pub id: Key,
    /// Stable binding failure explanation, without fabricated source coordinates.
    pub reason: &'static str,
}

impl PublicationMetadataError {
    /// Stable diagnostic class for the shared metadata boundary.
    pub fn code(&self) -> &'static str {
        "publication_metadata_binding"
    }
}
impl fmt::Display for PublicationMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "publication {} metadata endpoint {}: {}",
            self.site, self.id, self.reason
        )
    }
}
impl std::error::Error for PublicationMetadataError {}

impl ResolvedContent<'_> {
    /// Validate the separately selected search page without borrowing a sibling site's choice.
    pub(crate) fn publication_search_page<'a>(
        &'a self,
        site: &Key,
    ) -> Result<Option<&'a Key>, PublicationMetadataError> {
        let workspace = &self.compilation().workspace;
        let Some(id) = workspace
            .publication_for(site)
            .and_then(|publication| publication.search_page.as_ref())
        else {
            return Ok(None);
        };
        let error = |reason| PublicationMetadataError {
            site: site.clone(),
            id: id.clone(),
            reason,
        };
        let node = self
            .graph()
            .node(id)
            .ok_or_else(|| error("search page is absent from the selected content graph"))?;
        if &node.site != site {
            return Err(error("search page belongs to another publication site"));
        }
        if !matches!(
            node.structure,
            gazette_core::publication::content::ContentKind::Page { .. }
        ) {
            return Err(error("search page must be a page"));
        }
        if node
            .route
            .as_ref()
            .is_some_and(|route| route.canonical.is_some())
        {
            return Err(error("search page must have its own canonical route"));
        }
        self.route(id)
            .ok_or_else(|| error("search page has no resolved serving route"))?;
        Ok(Some(id))
    }

    /// Resolve the single public publication metadata contract from its native owners.
    /// No contact/profile is joined to a principal or used as publication permission.
    ///
    /// # Errors
    /// Refuses an unknown site and any absent, foreign-site or unrouted home reference.
    pub fn publication_metadata<'a>(
        &'a self,
        site: &Key,
    ) -> Result<Option<PublicationMetadata<'a>>, PublicationMetadataError> {
        let error = |id: &Key, reason| PublicationMetadataError {
            site: site.clone(),
            id: id.clone(),
            reason,
        };
        let workspace = &self.compilation().workspace;
        let site = workspace
            .site(site)
            .ok_or_else(|| error(site, "metadata site is not declared"))?;
        let Some((brand, publication)) = workspace.brands().find_map(|brand| {
            brand
                .publications
                .iter()
                .find(|publication| publication.site == site.id)
                .map(|publication| (brand, publication))
        }) else {
            return Ok(None);
        };
        self.publication_search_page(&site.id)?;
        if let Some(selection) = publication
            .processing
            .as_ref()
            .and_then(|processing| processing.preferred_sources.as_ref())
        {
            for id in selection.pages.keys() {
                let node = self.graph().node(id).ok_or_else(|| {
                    error(
                        id,
                        "preferred-source page is absent from the selected content graph",
                    )
                })?;
                if node.site != site.id {
                    return Err(error(
                        id,
                        "preferred-source page belongs to another publication site",
                    ));
                }
                if !matches!(
                    node.structure,
                    gazette_core::publication::content::ContentKind::Page { .. }
                ) {
                    return Err(error(id, "preferred-source override must select a page"));
                }
                self.route(id).ok_or_else(|| {
                    error(id, "preferred-source page has no resolved serving route")
                })?;
            }
        }
        let home = publication
            .home
            .as_ref()
            .map(|id| {
                let node = self
                    .graph()
                    .node(id)
                    .ok_or_else(|| error(id, "home is absent from the selected content graph"))?;
                if node.site != site.id {
                    return Err(error(id, "home belongs to another publication site"));
                }
                let route = self
                    .route(id)
                    .ok_or_else(|| error(id, "home has no resolved serving route"))?;
                Ok(PublicationHome {
                    id: &node.id,
                    title: &node.title,
                    url: &route.url,
                })
            })
            .transpose()?;
        let about = publication.about.as_ref();
        if let Some(id) = about {
            let node = self
                .graph()
                .node(id)
                .ok_or_else(|| error(id, "brand About page is absent"))?;
            if node.site != site.id
                || node.schema_type
                    != Some(gazette_core::publication::content::SchemaType::AboutPage)
                || node.subject.is_some()
                || self.route(id).is_none()
                || home.is_none()
            {
                return Err(error(
                    id,
                    "brand About must be a routed same-site About page with an unassigned subject and a declared home",
                ));
            }
        }
        let parent_home = brand.parent.as_ref().and_then(|parent| {
            let parent = workspace.brands().find(|value| &value.id == parent)?;
            let [publication] = parent.publications.as_slice() else {
                return None;
            };
            self.route(publication.home.as_ref()?)
                .map(|route| route.public_url.as_str())
        });
        Ok(Some(PublicationMetadata {
            site: &site.id,
            rights_holder: site
                .rights_holder
                .as_ref()
                .and_then(|id| workspace.principal(id)),
            render_year: None,
            brand: PublicationBrand {
                id: &brand.id,
                name: &brand.name,
                parent: brand.parent.as_ref(),
                parent_home,
            },
            home,
            about,
            language: publication.language.as_ref(),
            contacts: &publication.contacts,
            social: &publication.social,
            repository: publication.repository.as_deref(),
        }))
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Borrow the same resolved contract used by site selection and all projected pages.
    ///
    /// # Errors
    /// Refuses unknown sites or invalid explicit home endpoints, without metadata fallback.
    pub fn publication_metadata<'a>(
        &'a self,
        site: &Key,
    ) -> Result<Option<PublicationMetadata<'a>>, PublicationMetadataError> {
        self.content().publication_metadata(site)
    }

    pub(crate) fn validate_publication_metadata(&self) -> Result<(), PublicationMetadataError> {
        // A selected publication never acquires another site's home merely because
        // its workspace declaration remains available for crossing resolution.
        let represented: std::collections::BTreeSet<_> = self
            .content()
            .graph()
            .nodes()
            .map(|node| &node.site)
            .collect();
        for site in represented {
            self.publication_metadata(site)?;
        }
        Ok(())
    }
}

/// Explicit calendar year for presentation, never an inferred publication date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct RenderYear(#[schemars(range(min = 1, max = 9999))] u16);

impl RenderYear {
    /// Validate the supported positive four-digit calendar range.
    ///
    /// # Errors
    /// Refuses zero and years greater than 9999.
    pub fn new(year: u16) -> Result<Self, &'static str> {
        if (1..=9999).contains(&year) {
            Ok(Self(year))
        } else {
            Err("render year must be in 1..=9999")
        }
    }
    /// Exact caller-supplied year.
    pub fn get(self) -> u16 {
        self.0
    }
}
impl<'de> serde::Deserialize<'de> for RenderYear {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::new(<u16 as serde::Deserialize>::deserialize(decoder)?)
            .map_err(serde::de::Error::custom)
    }
}
impl std::str::FromStr for RenderYear {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(
            value
                .parse()
                .map_err(|_| "render year must be in 1..=9999")?,
        )
    }
}
