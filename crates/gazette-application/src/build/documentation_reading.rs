//! Provider-owned documentation reading pages in publication discovery outputs.

use super::{DocumentationCatalog, serialization::encode};
use gazette_compiler::{
    authored::{LlmsDocument, LlmsPage, ResolvedAuthored},
    workspace::{LlmsCoverage, SearchConfiguration, SearchField},
};
use gazette_core::{
    publication::{Key, Project, Visibility},
    render,
};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

fn invalid(reason: &'static str) -> render::Error {
    render::Error::Context(Box::new(std::io::Error::other(reason)))
}

pub(super) fn project_work(
    catalog: Option<&DocumentationCatalog>,
    project: &Project,
) -> Result<Option<gazette_compiler::authored::ProjectDocumentation>, render::Error> {
    let Some(destination) = project
        .resources
        .as_ref()
        .and_then(|resources| resources.code_documentation.as_deref())
    else {
        return Ok(None);
    };
    let Some(catalog) = catalog else {
        return Ok(None);
    };
    if !catalog.complete() {
        return Err(invalid(
            "project documentation requires complete provider evidence",
        ));
    }
    for entry in catalog
        .entries()
        .iter()
        .filter(|entry| entry.project == project.id)
    {
        let Some(public_url) = catalog
            .canonical_url(entry)
            .map_err(|_| invalid("invalid validated documentation reading address"))?
        else {
            continue;
        };
        if public_url == destination {
            return Ok(Some(gazette_compiler::authored::ProjectDocumentation {
                public_url,
                title: entry.page.title.clone().filter(|title| !title.is_empty()),
            }));
        }
    }
    Ok(None)
}

struct Reading<'a> {
    entry: &'a super::DocumentationCatalogEntry,
    site: &'a Key,
    project: &'a Project,
    label: &'a str,
    url: String,
    canonical_url: String,
}

fn readings<'a>(
    catalog: Option<&'a DocumentationCatalog>,
    authored: &'a ResolvedAuthored<'_, '_>,
) -> Result<Vec<Reading<'a>>, render::Error> {
    let Some(catalog) = catalog else {
        return Ok(Vec::new());
    };
    if !catalog.complete() {
        return Err(invalid(
            "documentation discovery requires complete provider evidence",
        ));
    }
    // No public documentation address means no public reading links can be emitted.
    if catalog.canonical_base().is_none() {
        return Ok(Vec::new());
    }
    let mut readings = Vec::new();
    for entry in catalog.entries() {
        let project = authored
            .content()
            .project(&entry.project)
            .ok_or_else(|| invalid("documentation project has no retained publication owner"))?;
        if project.visibility != Visibility::Public {
            continue;
        }
        let canonical_url = catalog
            .canonical_url(entry)
            .map_err(|_| invalid("invalid validated documentation reading address"))?
            .ok_or_else(|| invalid("documentation public address is unavailable"))?;
        let url = authored
            .preview_public_link(&canonical_url)
            .map_err(|_| {
                invalid(
                    "documentation links require an explicit local provider and safe public URLs",
                )
            })?
            .unwrap_or_else(|| canonical_url.clone());
        let label = entry
            .page
            .title
            .as_deref()
            .filter(|title| !title.is_empty())
            .unwrap_or(entry.page.route.as_str());
        readings.push(Reading {
            entry,
            site: catalog.site(),
            project,
            label,
            url,
            canonical_url,
        });
    }
    Ok(readings)
}

#[derive(Serialize)]
struct Content<'a> {
    /// Actual canonical provider address, not an invented authored-content key.
    id: &'a str,
    /// Display label; original optional provider title is retained separately.
    title: &'a str,
    site: &'a Key,
    documentation: bool,
}
#[derive(Serialize)]
struct Route<'a> {
    url: &'a str,
    canonical_url: &'a str,
}
#[derive(Serialize)]
struct ProjectSummary<'a> {
    id: &'a Key,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}
#[derive(Serialize)]
struct ProjectOwner<'a> {
    project: ProjectSummary<'a>,
}
#[derive(Serialize)]
struct Page<'a> {
    module: &'a str,
    route: &'a gazette_core::source::EntryPath,
    title: Option<&'a str>,
    label_source: &'static str,
    body_available: bool,
}
#[derive(Serialize)]
struct Destination {
    kind: gazette_core::markdown::LinkDestinationKind,
}
#[derive(Serialize)]
struct SearchRecord<'a> {
    content: Content<'a>,
    route: Route<'a>,
    body: &'a str,
    projects: [ProjectOwner<'a>; 1],
    documentation_page: Page<'a>,
    destination: Destination,
    indexed: Indexed<'a>,
}
struct Indexed<'a> {
    reading: &'a Reading<'a>,
    selection: &'a SearchConfiguration,
}
impl Serialize for Indexed<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.selection.fields().len()))?;
        for field in self.selection.fields() {
            match field {
                SearchField::Title => map.serialize_entry("title", self.reading.label)?,
                SearchField::Route => map.serialize_entry("route", &self.reading.url)?,
                SearchField::Body => map.serialize_entry(
                    "body",
                    self.reading.entry.page.body_text.as_deref().unwrap_or(""),
                )?,
                SearchField::Projects => {
                    let values: Vec<_> = std::iter::once(self.reading.project.id.as_str())
                        .chain(self.reading.project.name.as_deref())
                        .collect();
                    map.serialize_entry("projects", &values)?;
                }
                SearchField::Keywords => map.serialize_entry("keywords", &[] as &[&str])?,
                SearchField::Description => map.serialize_entry("description", "")?,
                SearchField::Notes => map.serialize_entry("notes", "")?,
                SearchField::Summary => map.serialize_entry("summary", "")?,
            }
        }
        map.end()
    }
}

pub(super) fn search(
    catalog: Option<&DocumentationCatalog>,
    authored: &ResolvedAuthored<'_, '_>,
    selection: &SearchConfiguration,
    maximum: usize,
) -> Result<Vec<Value>, render::Error> {
    let mut output = Vec::new();
    let mut remaining = maximum;
    for reading in readings(catalog, authored)? {
        let record = SearchRecord {
            content: Content {
                id: &reading.canonical_url,
                title: reading.label,
                site: reading.site,
                documentation: true,
            },
            route: Route {
                url: &reading.url,
                canonical_url: &reading.canonical_url,
            },
            body: reading.entry.page.body_text.as_deref().unwrap_or(""),
            projects: [ProjectOwner {
                project: ProjectSummary {
                    id: &reading.project.id,
                    name: reading.project.name.as_deref(),
                },
            }],
            documentation_page: Page {
                module: &reading.entry.page.module,
                route: &reading.entry.page.route,
                title: reading.entry.page.title.as_deref(),
                label_source: if reading
                    .entry
                    .page
                    .title
                    .as_deref()
                    .is_some_and(|title| !title.is_empty())
                {
                    "provider-title"
                } else {
                    "reading-route"
                },
                body_available: reading.entry.page.body_text.is_some(),
            },
            // Provider documentation is not a Gazette authored page or content key.
            destination: Destination {
                kind: gazette_core::markdown::LinkDestinationKind::External,
            },
            indexed: Indexed {
                reading: &reading,
                selection,
            },
        };
        let bytes = encode(&record, remaining, "documentation search bytes")?;
        remaining = remaining
            .checked_sub(bytes.len().saturating_add(1))
            .ok_or(render::Error::Limit("documentation search bytes"))?;
        output.push(
            serde_json::from_slice(&bytes)
                .map_err(|error| render::Error::Context(Box::new(error)))?,
        );
    }
    Ok(output)
}

pub(super) fn extend_llms(
    document: &mut LlmsDocument,
    catalog: Option<&DocumentationCatalog>,
    authored: &ResolvedAuthored<'_, '_>,
    maximum: usize,
) -> Result<(), render::Error> {
    if document.coverage != LlmsCoverage::AllPages {
        return Ok(());
    }
    let mut remaining = maximum;
    let mut urls: std::collections::BTreeSet<_> =
        document.pages.iter().map(|page| page.url.clone()).collect();
    for reading in readings(catalog, authored)? {
        if !urls.insert(reading.url.clone()) {
            return Err(invalid(
                "documentation reading address collides with another llms page",
            ));
        }
        let page = LlmsPage {
            id: None,
            title: reading.label.to_owned(),
            description: None,
            url: reading.url.clone(),
            html_url: reading.url,
            page_number: None,
            page_count: None,
            listing_variant: None,
            listing_direction: None,
        };
        let bytes = encode(&page, remaining, "documentation llms bytes")?;
        remaining = remaining
            .checked_sub(bytes.len())
            .ok_or(render::Error::Limit("documentation llms bytes"))?;
        document.pages.push(page);
    }
    document.pages.sort_by(|a, b| a.url.cmp(&b.url));
    check_llms_model(document, maximum)
}

fn check_llms_model(document: &LlmsDocument, maximum: usize) -> Result<(), render::Error> {
    encode(document, maximum, "documentation llms model bytes").map(drop)
}

#[cfg(test)]
#[path = "../../tests/internal/documentation_reading.rs"]
mod tests;
