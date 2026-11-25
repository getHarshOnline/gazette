//! Curated Markdown entrypoints from explicit selected content, never a second sitemap.

use super::{Discovery, RoutedPage, SitePages, SitePagesError, bounded, escaped, plain};
use gazette_core::publication::{
    Key, Visibility,
    content::{CollectionPurpose, ContentKind},
};
use std::io::{self, Write};

use crate::{
    authored::{ListingPage, ProjectedTaxonomyPage},
    workspace::LlmsCoverage,
};
use gazette_core::render::LimitedWriter;
use serde::Serialize;

impl SitePages<'_> {
    /// Emit owner-declared sections and exact Markdown/HTML pairs in authored order.
    /// Repeated entries remain repeated; there are no automatic authors or groups.
    ///
    /// # Errors
    /// Rejects wrong-owner discovery, disabled/missing profiles, invalid references,
    /// absent descriptions, invalid section headings or complete byte exhaustion.
    pub fn llms(
        &self,
        discovery: &Discovery<'_>,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, SitePagesError> {
        if !std::ptr::eq(self.owner, discovery.owner) {
            return Err(SitePagesError::Discovery);
        }
        let configuration = self
            .publication
            .machine
            .llms
            .as_ref()
            .filter(|c| c.enabled)
            .ok_or(SitePagesError::Machine(
                "llms requires an enabled curated configuration",
            ))?;
        if configuration.coverage != LlmsCoverage::Curated {
            return Err(SitePagesError::Machine(
                "navigation and all-pages llms coverage require the generated page model",
            ));
        }
        let profile = self
            .publication
            .machine
            .markdown_alternates
            .as_ref()
            .filter(|p| p.enabled)
            .ok_or(SitePagesError::Machine(
                "llms Markdown links require enabled alternates",
            ))?;
        let base = self
            .owner
            .base_url(&self.publication.site)
            .ok_or(SitePagesError::BrandRequired)?;
        let description_page = self.curated_page(discovery, &configuration.description_from)?;
        let description = description_page
            .node
            .description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                SitePagesError::DescriptionRequired(configuration.description_from.clone())
            })?;
        for section in &configuration.sections {
            if section.title.trim().is_empty() || section.title.chars().any(char::is_control) {
                return Err(SitePagesError::Machine(
                    "llms section titles require nonempty plain-text lines",
                ));
            }
            for id in &section.contents {
                self.curated_page(discovery, id)?;
            }
        }
        bounded(maximum_bytes, "llms bytes", |output| {
            output.write_all(b"# ")?;
            plain(output, self.name)?;
            output.write_all(b"\n\n")?;
            for line in description.lines() {
                output.write_all(b"> ")?;
                plain(output, line)?;
                output.write_all(b"\n")?;
            }
            for section in &configuration.sections {
                output.write_all(b"\n## ")?;
                plain(output, &section.title)?;
                output.write_all(b"\n\n")?;
                for id in &section.contents {
                    // Already validated before emission; no errors are converted to omissions.
                    let page = self.curated_page(discovery, id).map_err(io::Error::other)?;
                    let path = &page
                        .node
                        .route
                        .as_ref()
                        .ok_or_else(|| io::Error::other("missing proved route"))?
                        .path;
                    let alternate =
                        super::alternates::alternate(profile, base, &page.route.url, path)
                            .map_err(io::Error::other)?;
                    output.write_all(b"- ")?;
                    link(output, &page.node.title, &alternate.url)?;
                    if configuration.include_human_link {
                        output.write_all(b" (")?;
                        link(output, "HTML", &page.route.url)?;
                        output.write_all(b")")?;
                    }
                    output.write_all(b"\n")?;
                }
            }
            Ok(())
        })
    }

    fn curated_page(
        &self,
        discovery: &Discovery<'_>,
        id: &Key,
    ) -> Result<RoutedPage<'_>, SitePagesError> {
        self.pages
            .iter()
            .find(|page| {
                &page.node.id == id
                    && discovery.owns(page.node)
                    && discovery.contains(id)
                    && page.node.site == self.publication.site
                    && page
                        .node
                        .route
                        .as_ref()
                        .is_some_and(|route| route.canonical.is_none())
            })
            .copied()
            .ok_or_else(|| SitePagesError::LlmsReference(id.clone()))
    }
}

fn link(output: &mut impl Write, title: &str, url: &str) -> io::Result<()> {
    output.write_all(b"[")?;
    plain(output, title)?;
    output.write_all(b"](")?;
    escaped(output, url, |c| matches!(c, '(' | ')' | '\\' | '&'))?;
    output.write_all(b")")
}

/// The exact machine-reading projection supplied to a selected Theme entry.
#[derive(Debug, Serialize)]
pub struct LlmsDocument {
    /// Configured publication name.
    pub title: String,
    /// Authored description from the configured source, on one plain-text line.
    pub description: String,
    /// Native selection mode, independent of formatting.
    pub coverage: LlmsCoverage,
    /// Whether Theme should pair Markdown destinations with serving HTML links.
    pub include_human_link: bool,
    /// Complete serving-URL order in all-pages mode; no static catalogue.
    pub pages: Vec<LlmsPage>,
    /// Explicit authored order in curated mode.
    pub sections: Vec<LlmsReadingSection>,
}

/// One actual canonical HTML page and its configured machine representation.
#[derive(Clone, Debug, Serialize)]
pub struct LlmsPage {
    /// Existing content identity; generated taxonomy views never invent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Key>,
    /// Authored/provider title, or the literal provider route when no title is supplied.
    pub title: String,
    /// Explicit purpose metadata, never a generated summary of the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Actual reading endpoint: a configured Markdown alternate or a provider HTML route.
    pub url: String,
    /// Actual selected serving HTML endpoint.
    pub html_url: String,
    /// Continuations retain the existing index identity and purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Total views in this same generated reading index.
    pub page_count: Option<usize>,
    /// Exact configured listing view, absent for ordinary canonical pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_variant: Option<crate::workspace::ListingVariant>,
    /// Authored chronology direction for a generated listing view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_direction: Option<crate::workspace::ListingDirection>,
}

/// One explicit curated section, using the same native page metadata.
#[derive(Debug, Serialize)]
pub struct LlmsReadingSection {
    /// Literal configured section title.
    pub title: String,
    /// Configured exact content order.
    pub pages: Vec<LlmsPage>,
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl SitePages<'_> {
    /// Project the complete built-page catalogue or the explicit curated path.
    ///
    /// # Errors
    /// Refuses foreign selection, incomplete generated views, missing authored
    /// purposes, stale curated sections in all-pages mode or complete byte excess.
    pub fn llms_model(
        &self,
        discovery: &Discovery<'_>,
        taxonomies: &[ProjectedTaxonomyPage<'_>],
        listings: &[ListingPage],
        maximum_bytes: usize,
    ) -> Result<LlmsDocument, SitePagesError> {
        if !std::ptr::eq(self.owner, discovery.owner) {
            return Err(SitePagesError::Discovery);
        }
        let configuration = self
            .publication
            .machine
            .llms
            .as_ref()
            .filter(|configuration| configuration.enabled)
            .ok_or(SitePagesError::Machine(
                "llms output needs enabled configuration",
            ))?;
        let profile = self
            .publication
            .machine
            .markdown_alternates
            .as_ref()
            .filter(|profile| profile.enabled)
            .ok_or(SitePagesError::Machine(
                "llms Markdown references require enabled alternates",
            ))?;
        let base = self
            .owner
            .base_url(&self.publication.site)
            .ok_or(SitePagesError::BrandRequired)?;
        self.with_taxonomy_pages(taxonomies)?
            .with_listing_pages(listings)?;
        let description = self.curated_page(discovery, &configuration.description_from)?;
        let required_description = |page: RoutedPage<'_>| {
            page.node
                .description
                .as_deref()
                .map(one_line)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| SitePagesError::DescriptionRequired(page.node.id.clone()))
        };
        let entry = |page: RoutedPage<'_>| -> Result<LlmsPage, SitePagesError> {
            let path = &page
                .node
                .route
                .as_ref()
                .ok_or_else(|| SitePagesError::RouteRequired(page.node.id.clone()))?
                .path;
            let alternate = super::alternates::alternate(profile, base, &page.route.url, path)?;
            Ok(LlmsPage {
                id: Some(page.node.id.clone()),
                title: page.node.title.clone(),
                description: match configuration.coverage {
                    LlmsCoverage::AllPages => Some(required_description(page)?),
                    LlmsCoverage::Navigation if page.node.id == configuration.description_from => {
                        None
                    }
                    LlmsCoverage::Curated | LlmsCoverage::Navigation => page
                        .node
                        .description
                        .as_deref()
                        .map(one_line)
                        .filter(|value| !value.is_empty()),
                },
                url: alternate.url,
                html_url: page.route.url.clone(),
                page_number: None,
                page_count: None,
                listing_variant: None,
                listing_direction: None,
            })
        };
        let mut model = LlmsDocument {
            title: self.name.to_owned(),
            description: required_description(description)?,
            coverage: configuration.coverage,
            include_human_link: configuration.include_human_link,
            pages: Vec::new(),
            sections: Vec::new(),
        };
        match configuration.coverage {
            LlmsCoverage::Curated => {
                for section in &configuration.sections {
                    if section.title.trim().is_empty()
                        || section.title.chars().any(char::is_control)
                    {
                        return Err(SitePagesError::Machine(
                            "llms section titles require nonempty plain-text lines",
                        ));
                    }
                    model.sections.push(LlmsReadingSection {
                        title: section.title.clone(),
                        pages: section
                            .contents
                            .iter()
                            .map(|id| self.curated_page(discovery, id).and_then(entry))
                            .collect::<Result<_, _>>()?,
                    });
                }
            }
            LlmsCoverage::Navigation => {
                if !configuration.sections.is_empty() {
                    return Err(SitePagesError::Machine(
                        "navigation llms coverage cannot declare curated sections",
                    ));
                }
                let navigation = self
                    .owner
                    .compilation()
                    .workspace
                    .composition_for(&self.publication.site)
                    .and_then(|composition| composition.navigation().cloned())
                    .ok_or(SitePagesError::Machine(
                        "navigation llms coverage requires publication navigation",
                    ))?;
                let mut seen = std::collections::BTreeSet::new();
                let pages = navigation
                    .value()
                    .iter()
                    .filter(|item| seen.insert(item.destination.clone()))
                    .map(|item| {
                        self.curated_page(discovery, &item.destination)
                            .and_then(entry)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if pages.is_empty() {
                    return Err(SitePagesError::Machine(
                        "navigation llms coverage requires at least one destination",
                    ));
                }
                model.sections.push(LlmsReadingSection {
                    title: "Start here".to_owned(),
                    pages,
                });
                let projects = self.owner.projects();
                let mut work = Vec::new();
                for project in projects.filter(|project| project.visibility == Visibility::Public) {
                    let Some(id) = project
                        .resources
                        .as_ref()
                        .and_then(|resources| resources.landing.as_ref())
                    else {
                        continue;
                    };
                    let Ok(page) = self.curated_page(discovery, id) else {
                        continue;
                    };
                    if seen.insert(id.clone()) {
                        let mut page = entry(page)?;
                        if let Some(name) = &project.name {
                            page.title.clone_from(name);
                        }
                        work.push(page);
                    }
                }
                if !work.is_empty() {
                    model.sections.push(LlmsReadingSection {
                        title: "Projects and products".to_owned(),
                        pages: work,
                    });
                }
                for (title, selected) in [("Collections", true), ("Reading journeys", false)] {
                    let pages = self
                        .pages
                        .iter()
                        .filter(|page| {
                            let editorial = matches!(
                                page.node.structure,
                                ContentKind::Collection {
                                    parent: None,
                                    purpose: None | Some(CollectionPurpose::Editorial),
                                    ..
                                }
                            );
                            let journey =
                                matches!(page.node.structure, ContentKind::Journey { .. })
                                    && !self.owner.graph().is_documentation(&page.node.id);
                            (if selected { editorial } else { journey })
                                && discovery.includes_navigation_node(page.node)
                        })
                        .filter_map(|page| self.curated_page(discovery, &page.node.id).ok())
                        .filter(|page| seen.insert(page.node.id.clone()))
                        .map(entry)
                        .collect::<Result<Vec<_>, _>>()?;
                    if !pages.is_empty() {
                        model.sections.push(LlmsReadingSection {
                            title: title.to_owned(),
                            pages,
                        });
                    }
                }
            }
            LlmsCoverage::AllPages => {
                if !configuration.sections.is_empty() {
                    return Err(SitePagesError::Machine(
                        "all-pages llms coverage cannot declare curated sections",
                    ));
                }
                for page in &self.pages {
                    if discovery.contains(&page.node.id)
                        && discovery.owns(page.node)
                        && page
                            .node
                            .route
                            .as_ref()
                            .is_some_and(|route| route.canonical.is_none())
                    {
                        model.pages.push(entry(*page)?);
                    }
                }
                for taxonomy in taxonomies {
                    let declaration = self
                        .publication
                        .taxonomy_pages
                        .iter()
                        .find(|declaration| {
                            &declaration.taxonomy == taxonomy.model.taxonomy.id
                                && declaration.term.as_ref()
                                    == taxonomy.model.term.map(|term| &term.id)
                        })
                        .ok_or(SitePagesError::Taxonomy)?;
                    let alternate = super::alternates::alternate(
                        profile,
                        base,
                        &taxonomy.model.canonical_url,
                        &declaration.route,
                    )?;
                    model.pages.push(LlmsPage {
                        id: None,
                        title: taxonomy.model.title.to_owned(),
                        description: None,
                        url: alternate.url,
                        html_url: taxonomy.model.canonical_url.clone(),
                        page_number: None,
                        page_count: None,
                        listing_variant: None,
                        listing_direction: None,
                    });
                }
                model
                    .pages
                    .sort_unstable_by(|a, b| a.html_url.cmp(&b.html_url));
            }
        }
        let mut ceiling = LimitedWriter::new(io::sink(), maximum_bytes);
        let encoded = serde_json::to_writer(&mut ceiling, &model);
        if ceiling.exceeded() {
            return Err(SitePagesError::Limit("llms model bytes"));
        }
        encoded.map_err(|error| SitePagesError::Write(io::Error::other(error)))?;
        Ok(model)
    }
}

impl LlmsDocument {
    /// Neutral bounded fallback when the selected Theme has no machine entry.
    ///
    /// # Errors
    /// Complete output exhaustion fails without truncating the catalogue.
    pub fn markdown(&self, maximum_bytes: usize) -> Result<Vec<u8>, SitePagesError> {
        bounded(maximum_bytes, "llms bytes", |output| {
            output.write_all(b"# ")?;
            plain(output, &self.title)?;
            output.write_all(b"\n\n> ")?;
            plain(output, &self.description)?;
            output.write_all(b"\n")?;
            let entries =
                |output: &mut LimitedWriter<Vec<u8>>, pages: &[LlmsPage]| -> io::Result<()> {
                    for page in pages {
                        output.write_all(b"- ")?;
                        link(output, &page.title, &page.url)?;
                        if let Some(number) = page.page_number {
                            write!(output, " (page {number})")?;
                        }
                        if self.include_human_link && page.url != page.html_url {
                            output.write_all(b" (")?;
                            link(output, "HTML", &page.html_url)?;
                            output.write_all(b")")?;
                        }
                        if let Some(description) = &page.description {
                            output.write_all(b": ")?;
                            plain(output, description)?;
                        }
                        output.write_all(b"\n")?;
                    }
                    Ok(())
                };
            if self.coverage == LlmsCoverage::AllPages {
                output.write_all(b"\n## Pages\n\n")?;
                entries(output, &self.pages)?;
            } else {
                for section in &self.sections {
                    output.write_all(b"\n## ")?;
                    plain(output, &section.title)?;
                    output.write_all(b"\n\n")?;
                    entries(output, &section.pages)?;
                }
            }
            Ok(())
        })
    }
}
