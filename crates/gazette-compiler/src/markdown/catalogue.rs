//! Authored catalogue entries borrow the heading and destination admitted for rendering.

use std::collections::BTreeMap;

use gazette_core::{
    markdown::{LinkKind, Plan},
    source::Span,
};
use serde::Serialize;

use crate::theme::{ComponentSemantic, Components};

use super::{Budget, Error};

/// The caption and resolved destination of an authored catalogue entry.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CatalogueItem {
    /// The text of the entry's authored heading.
    pub title: String,
    /// The same resolved URL as the link in that heading.
    pub url: String,
    /// Exact content key resolved from the heading link.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) destination: Option<gazette_core::publication::Key>,
    /// Public page or project identity, resolved from that content key.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) identity: Option<String>,
    /// Schema type of that page or project.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) kind: Option<&'static str>,
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) software: Option<gazette_core::publication::ProjectSoftware>,
}

fn encloses(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

pub(super) fn collect(
    plan: &Plan,
    definitions: &Components,
    budget: &mut Budget,
) -> Result<BTreeMap<Span, Vec<CatalogueItem>>, Error> {
    let mut catalogues = BTreeMap::new();
    for binding in &plan.components {
        let Some(semantic) = definitions
            .definition(&binding.name)
            .and_then(|definition| definition.semantic.as_ref())
        else {
            continue;
        };
        let span = binding.occurrence.location.span;
        match semantic {
            ComponentSemantic::Catalogue => {
                catalogues.insert(span, Vec::new());
            }
            ComponentSemantic::CatalogueEntry => {
                let entries = catalogues
                    .range_mut(..=span)
                    .rev()
                    .find(|(group, _)| encloses(**group, span))
                    .map(|(_, entries)| entries)
                    .ok_or(Error::InvalidOutput)?;
                let heading = plan
                    .headings
                    .iter()
                    .find(|heading| encloses(span, heading.occurrence.location.span))
                    .ok_or(Error::InvalidOutput)?;
                let mut links = plan.links.iter().filter(|link| {
                    link.occurrence.kind == LinkKind::Link
                        && encloses(
                            heading.occurrence.location.span,
                            link.occurrence.location.span,
                        )
                });
                let link = links.next().ok_or(Error::InvalidOutput)?;
                if links.next().is_some() {
                    return Err(Error::InvalidOutput);
                }
                entries.push(CatalogueItem {
                    title: budget.copy(&heading.occurrence.text)?,
                    url: budget.copy(&link.destination)?,
                    destination: link
                        .annotation
                        .as_ref()
                        .and_then(|link| link.content.clone()),
                    identity: None,
                    kind: None,
                    software: None,
                });
            }
            _ => {}
        }
    }
    Ok(catalogues)
}
