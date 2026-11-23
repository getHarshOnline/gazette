//! Explicit generated-reading selections, never another content catalogue.

use gazette_core::{publication::Key, source::EntryPath};
use serde::{Deserialize, Serialize};

/// A generated view enriching an existing declaration-only index page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListingDeclaration {
    /// Exact existing index identity; its source, title and route remain authoritative.
    pub content: Key,
    /// Candidate classification uses the complete editorial graph before discovery filtering.
    pub select: ListingSelection,
    /// Explicit chronology or priority; neither filenames nor filesystem dates participate.
    pub order: ListingOrder,
    /// Optional opposite publication chronology over the same complete candidate set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_order: Option<ListingAlternateOrder>,
    /// Bounded slices and confined routes beneath the existing index.
    pub pagination: ListingPagination,
}

/// One additional chronology under a confined directory of the existing index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListingAlternateOrder {
    /// Opposite published direction; candidate selection and pagination stay shared.
    pub order: ListingOrder,
    /// Confined relative directory beneath the index, without URL interpretation.
    pub path: EntryPath,
}

/// Output-view identity, independent from the existing authored content identity.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ListingVariant {
    /// Existing owner route and its continuations.
    Primary,
    /// Explicit configured opposite chronology and its continuations.
    Alternate,
}

impl ListingDeclaration {
    /// Borrow each admitted ordering and its optional relative root.
    pub fn orders(
        &self,
    ) -> impl Iterator<Item = (ListingVariant, &ListingOrder, Option<&EntryPath>)> {
        std::iter::once((ListingVariant::Primary, &self.order, None)).chain(
            self.alternate_order.iter().map(|alternate| {
                (
                    ListingVariant::Alternate,
                    &alternate.order,
                    Some(&alternate.path),
                )
            }),
        )
    }

    /// Admit only the opposite chronology over the same complete candidate set.
    ///
    /// # Errors
    /// Refuses priority alternates or an identical published direction.
    pub fn validate_alternate(&self) -> Result<(), &'static str> {
        if let Some(alternate) = &self.alternate_order {
            match (&self.order, &alternate.order) {
                (
                    ListingOrder::Published {
                        direction: left, ..
                    },
                    ListingOrder::Published {
                        direction: right, ..
                    },
                ) if left != right => {}
                _ => return Err("alternate order requires opposite published directions"),
            }
        }
        Ok(())
    }
}

/// Supported structural selections, not free-form template queries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ListingSelection {
    /// All canonical page, collection and journey records on this publication, except this index.
    AllPages,
    /// Articles without any declared immediate editorial parent.
    StandaloneArticles,
    /// Editorial collections without a parent; documentation has its own semantic category.
    RootCollections,
    /// Canonical articles with direct project assertions or emitted links to its landing.
    ProjectArticles {
        /// Exact configured subject.
        project: Key,
        /// Legacy explicit landing; omission uses only the exact project-owned landing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        landing: Option<Key>,
        /// Article residence, independent from source folders.
        #[serde(default)]
        scope: ListingProjectScope,
        /// Exact editorial residence, independent from source paths.
        #[serde(default)]
        residence: ListingProjectResidence,
        /// Optional own-publication unfiltered reading index for onward exploration.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        explore: Option<Key>,
    },
}

impl ListingSelection {
    pub(crate) fn project_landing<'a>(
        &'a self,
        content: &'a super::ResolvedContent<'_>,
    ) -> Result<Option<&'a Key>, &'static str> {
        let Self::ProjectArticles {
            project, landing, ..
        } = self
        else {
            return Ok(None);
        };
        let project = content
            .project(project)
            .ok_or("project article subject is absent from the configured catalog")?;
        let selected = project
            .resources
            .as_ref()
            .and_then(|resources| resources.landing.as_ref());
        if landing
            .as_ref()
            .zip(selected)
            .is_some_and(|(legacy, selected)| legacy != selected)
        {
            return Err("project article landing conflicts with the project-owned landing");
        }
        landing
            .as_ref()
            .or(selected)
            .map(Some)
            .ok_or("project article listing requires an explicit project landing")
    }
}

/// Residence of an explicitly selected project-article corpus.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ListingProjectScope {
    /// Articles owned by the listing's publication.
    #[default]
    Site,
    /// Articles in every captured publication in the resolved ecosystem.
    Ecosystem,
}

/// Editorial residence of the canonical articles in one subject view.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ListingProjectResidence {
    /// Any canonical article, preserving the original selector's behavior.
    #[default]
    All,
    /// Articles with no declared collection or journey parent.
    Standalone,
    /// Articles with an immediate collection or journey parent.
    Collected,
}

/// One complete ordering policy; unknown or combined policies are refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ListingOrder {
    /// Publication chronology retains authored precision; no modification-date fallback.
    Published {
        /// The authored field supplying chronology.
        by: ListingDateField,
        /// Explicit oldest-first or newest-first authored chronology.
        direction: ListingDirection,
        /// Explicit disposition for absent publication dates.
        undated: ListingUndated,
    },
    /// Owner-selected priorities followed by deterministic remaining candidates.
    Priority {
        /// Existing eligible candidate identities in the declared order.
        first: Vec<Key>,
        /// Stable ordering of candidates not explicitly prioritized.
        remaining: ListingRemaining,
    },
}

/// Supported authored chronology field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ListingDateField {
    /// Explicit publication date or timestamp, independent from lifecycle.
    Published,
}

/// Supported chronology direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ListingDirection {
    /// Latest authored chronology first; equal values use title then stable identity.
    Descending,
    /// Oldest authored chronology first; ties retain title then stable identity.
    Ascending,
}

/// Missing dates remain missing rather than becoming inferred timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ListingUndated {
    /// After dated candidates, in literal title then identity order.
    LastTitleThenId,
}

/// Deterministic remainder policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ListingRemaining {
    /// Literal authored title followed by stable identity, without locale inference.
    TitleThenId,
}

/// Pagination is transport under the index, not additional editorial membership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListingPagination {
    /// Positive number of references per page; projection limits bound the complete result.
    #[schemars(range(min = 1))]
    pub page_size: usize,
    /// Relative directory route containing exactly one `{number}` placeholder.
    pub next_pages: String,
}

impl ListingPagination {
    /// Resolve a continuation directory without interpreting an absolute URL or source path.
    ///
    /// # Errors
    /// Refuses zero size, first-page ordinals, ambiguous placeholders and unconfined paths.
    pub fn continuation(&self, number: usize) -> Result<EntryPath, &'static str> {
        if self.page_size == 0 || number < 2 {
            return Err(
                "pagination requires a positive page size and continuation number above one",
            );
        }
        if self.next_pages.matches("{number}").count() != 1 {
            return Err("pagination route requires exactly one {number} placeholder");
        }
        let expanded = self.next_pages.replace("{number}", &number.to_string());
        let relative = expanded
            .strip_suffix('/')
            .ok_or("pagination route must name a relative directory")?;
        if relative.contains(['{', '}', '%', '?', '#']) {
            return Err("pagination route contains unsupported placeholders or URL syntax");
        }
        relative
            .parse()
            .map_err(|_| "pagination route must remain a confined relative directory")
    }
}
