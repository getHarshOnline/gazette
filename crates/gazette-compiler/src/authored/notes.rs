//! Article-owned commentary shares the retained Markdown compiler, not page identity.

use std::collections::BTreeMap;

use gazette_core::{publication::content::ContentNode, source::Sha256Digest};
use serde::Serialize;

use super::{AssetPlan, Origin, ProjectionError, ResolvedAuthored, StaticAsset};
use crate::{document, markdown};

pub(super) const NAMESPACE: &str = "gazette-notes-";

/// Resolved commentary belonging to one article; no route or independent content ID.
///
/// `notes.md`, when present, supplies `body_html`. Declared notebooks follow it as
/// structured cells in `notebooks`, and their text joins `body_text`.
#[derive(Debug, schemars::JsonSchema)]
pub struct ArticleNotes {
    /// Selected normalized commentary, preserving the notes heading/footnote namespace.
    #[schemars(skip)]
    pub body_markdown: Option<String>,
    /// Commentary HTML, presented separately from the continuous article body.
    pub body_html: String,
    /// Text projected from the same retained syntax and resolution.
    pub body_text: String,
    /// Emitted subordinate heading hierarchy and namespaced anchors.
    pub headings: Vec<markdown::Heading>,
    /// Same selected rich-component contract as the owning article.
    pub components: Vec<markdown::ComponentUse>,
    /// Proved link occurrences under the owning article, with distinct addresses.
    pub links: Vec<markdown::LinkUse>,
    /// Actual emitted commentary image occurrences under the notes namespace.
    pub images: Vec<markdown::ImageUse>,
    /// Exact retained `notes.md` bytes; absent when only notebooks supply the Notes.
    #[schemars(skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<Sha256Digest>,
    /// Declared notebooks in order, after any `notes.md` content. Only page
    /// templates receive them; search, listing and feed records omit the cells.
    #[schemars(skip_serializing_if = "Vec::is_empty", default)]
    pub notebooks: Vec<NotebookView>,
    #[schemars(skip)]
    source: Option<Origin>,
}

/// One notebook in the Notes, rendered from its stored cells without execution.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct NotebookView {
    /// Anchor namespace shared by this notebook's heading and footnote IDs.
    pub id: String,
    /// Cells in notebook order; code source and outputs are separate fragments.
    pub cells: Vec<crate::notebook::NotebookCell>,
    /// Exact notebook bytes as a site asset, when the page selected a download and
    /// the pre-public scan passed; otherwise null.
    pub download: Option<NotebookDownload>,
}

/// Public address of an emitted notebook source file.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct NotebookDownload {
    /// Absolute URL beside the owning page.
    pub url: String,
    /// Exact byte length of the retained notebook.
    pub bytes: u64,
    /// `application/x-ipynb+json`.
    pub media_type: &'static str,
}

/// Borrowed wire form, so non-page records can omit the notebook cells.
#[derive(Serialize)]
struct Wire<'a> {
    body_html: &'a str,
    body_text: &'a str,
    headings: &'a [markdown::Heading],
    components: &'a [markdown::ComponentUse],
    links: &'a [markdown::LinkUse],
    images: &'a [markdown::ImageUse],
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sha256: Option<&'a Sha256Digest>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    notebooks: &'a [NotebookView],
}

impl Serialize for ArticleNotes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wire(true).serialize(serializer)
    }
}

impl ArticleNotes {
    /// Private retained `notes.md` evidence; absent when only notebooks supply the Notes.
    pub fn source(&self) -> Option<&Origin> {
        self.source.as_ref()
    }

    /// The Notes without notebook cells, for search, listing and feed records.
    pub(crate) fn without_notebooks(&self) -> impl Serialize + '_ {
        self.wire(false)
    }

    fn wire(&self, notebooks: bool) -> Wire<'_> {
        Wire {
            body_html: &self.body_html,
            body_text: &self.body_text,
            headings: &self.headings,
            components: &self.components,
            links: &self.links,
            images: &self.images,
            source_sha256: self.source_sha256.as_ref(),
            notebooks: if notebooks { &self.notebooks } else { &[] },
        }
    }

    /// Empty Notes for an article whose only commentary is its notebooks.
    pub(super) fn notebooks_only(reading_markdown: bool) -> Self {
        Self {
            body_markdown: reading_markdown.then(String::new),
            body_html: String::new(),
            body_text: String::new(),
            headings: Vec::new(),
            components: Vec::new(),
            links: Vec::new(),
            images: Vec::new(),
            source_sha256: None,
            notebooks: Vec::new(),
            source: None,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    pub(super) fn project_notes<'a>(
        &self,
        content: &ContentNode,
        assets: &'a AssetPlan<'a>,
        components: Option<markdown::ComponentPresentation<'_>>,
        markdown_alternate: bool,
        dimensions: &BTreeMap<String, gazette_core::resource::ImageLayout>,
        raster: &gazette_core::raster::Inspect<'_>,
    ) -> Result<Option<(ArticleNotes, Vec<&'a StaticAsset<'a>>)>, ProjectionError> {
        let Some(companion) = self
            .page_package(&content.id)
            .and_then(|package| package.notes())
        else {
            return Ok(None);
        };
        let file =
            self.corpus
                .files()
                .get(companion.path())
                .ok_or_else(|| ProjectionError::Binding {
                    id: content.id.clone(),
                    reason: "retained notes companion is missing",
                })?;
        let body = file.body().ok_or_else(|| ProjectionError::Binding {
            id: content.id.clone(),
            reason: "notes companion has no retained Markdown body",
        })?;
        let source = Origin {
            path: companion.path().clone(),
            source_sha256: file.source_sha256(),
            span: None,
        };
        let failure = |error| ProjectionError::Document {
            origin: source.clone(),
            error: Box::new(document::Error::Markdown { error, offset: 0 }),
        };
        let original_headings = body.headings().map_err(&failure)?;
        let offset = u8::from(original_headings.iter().any(|heading| heading.level == 1));
        let host = gazette_core::markdown::ComponentHost {
            content: content.id.clone(),
            site: content.site.clone(),
            surface: (&content.structure).into(),
            project_resources: self.project_resources_for(&content.id)?,
        };
        let mut dependencies = BTreeMap::new();
        let image_presentations = self
            .project_body_images(&content.id, assets, &mut dependencies, dimensions)
            .map_err(&failure)?;
        let rendered = body
            .render_projection(
                markdown::Projection {
                    container_contents: None,
                    namespace: NAMESPACE,
                    heading_offset: offset,
                    images: &BTreeMap::new(),
                    components,
                    host: Some(&host),
                    markdown_alternate,
                },
                |form, kind, destination, span| {
                    let resolved = self.resolve_notes_reference(
                        &content.id,
                        assets,
                        form,
                        kind,
                        destination,
                        span,
                    )?;
                    if let Some(asset) = resolved.as_ref().and_then(|reference| reference.asset) {
                        dependencies.insert(asset.url.as_str(), asset);
                    }
                    let mapped = resolved
                        .as_ref()
                        .map(|reference| reference.destination.clone());
                    let effective = mapped.as_deref().unwrap_or(destination);
                    let annotation = (kind == markdown::LinkKind::Link).then(|| {
                        self.link_destination(content, destination, effective, resolved.as_ref())
                    });
                    let image = (kind == markdown::LinkKind::Image
                        && resolved
                            .as_ref()
                            .is_some_and(|reference| reference.resource))
                    .then(|| {
                        super::body_images::presentation_for(
                            &image_presentations,
                            resolved.as_ref(),
                            dimensions,
                            raster,
                        )
                    })
                    .flatten();
                    Ok(markdown::ResolvedLink {
                        image,
                        destination: mapped,
                        annotation,
                    })
                },
            )
            .map_err(&failure)?;
        let mut headings: Vec<_> = original_headings
            .iter()
            .map(|heading| markdown::Heading {
                level: heading.level.saturating_add(offset).min(6),
                id: format!("{NAMESPACE}{}", heading.id),
                fragment: format!("#{NAMESPACE}{}", heading.fragment.trim_start_matches('#')),
                ..heading.clone()
            })
            .collect();
        // Saturation at h6 can flatten a former parent/child pair. Derive
        // navigation parents from emitted levels, not the old source hierarchy.
        let mut hierarchy: Vec<(u8, usize)> = Vec::new();
        for (index, heading) in headings.iter_mut().enumerate() {
            while hierarchy
                .last()
                .is_some_and(|(level, _)| *level >= heading.level)
            {
                hierarchy.pop();
            }
            heading.parent = hierarchy.last().map(|(_, index)| *index);
            hierarchy.push((heading.level, index));
        }
        Ok(Some((
            ArticleNotes {
                body_markdown: rendered.body_markdown,
                body_html: rendered.body_html,
                body_text: rendered.body_text,
                headings,
                components: rendered.components,
                links: rendered.links,
                images: rendered.images,
                source_sha256: Some(source.source_sha256),
                notebooks: Vec::new(),
                source: Some(source),
            },
            dependencies.into_values().collect(),
        )))
    }
}
