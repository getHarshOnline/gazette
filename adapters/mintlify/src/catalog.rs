//! Module-bound descriptions over routes already admitted by each provider.

use crate::{Error, invalid};
use gazette_core::{
    documentation::{CatalogEvidence, CatalogLimits, projection::LinkProjection},
    source::EntryPath,
};

#[derive(Clone, Copy)]
pub(super) struct Scope<'a> {
    pub(super) module: &'a str,
    pub(super) limits: CatalogLimits,
}
impl Scope<'_> {
    pub(super) fn physical(
        self,
        links: &mut LinkProjection,
        route: &str,
        title: Option<String>,
        text: Option<String>,
        companion: bool,
    ) -> Result<(), Error> {
        let target = links.page_route(route);
        let path = |suffix| {
            format!("{target}.{suffix}")
                .parse::<EntryPath>()
                .map_err(Error::Tree)
        };
        let evidence = CatalogEvidence::Physical {
            provider: path(if companion { "mdx" } else { "md" })?,
            native_markdown: companion.then(|| path("md")).transpose()?,
        };
        links.describe(route, self.module, title, text, evidence, self.limits)
    }
    pub(super) fn virtual_page(
        self,
        links: &mut LinkProjection,
        route: &str,
        title: Option<String>,
        text: Option<String>,
        artifacts: &[&str],
    ) -> Result<(), Error> {
        let artifacts = artifacts
            .iter()
            .map(|path| path.parse::<EntryPath>().map_err(Error::Tree))
            .collect::<Result<_, _>>()?;
        links.describe(
            route,
            self.module,
            title,
            text,
            CatalogEvidence::QualifiedVirtual { artifacts },
            self.limits,
        )
    }
    pub(super) fn title(self, title: Option<&str>) -> Result<Option<String>, Error> {
        title
            .map(|title| {
                if title.len() > self.limits.bytes {
                    return Err(invalid(self.module, "catalog title byte bound exceeded"));
                }
                Ok(title.to_owned())
            })
            .transpose()
    }
}
