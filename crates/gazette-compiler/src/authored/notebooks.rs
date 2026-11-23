//! Ordered notebook attachments resolve through the page's existing source binding.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::{
    publication::{Key, content::ContentNode, routing::RoutePath},
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;

use super::{
    ArticleNotes, AssetError, AssetPlan, NotebookDownload, NotebookView, ProjectionError,
    ResolvedAuthored, SourceBindingError, StaticAsset,
};
use crate::{corpus::File, markdown, notebook::ParsedNotebook};

/// One ordered `notebooks` entry: a capture-relative path, or a table that also
/// opts that notebook into a source download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(super) enum NotebookDeclaration {
    /// `"analysis/results.ipynb"`: render in Notes; no download.
    Path(EntryPath),
    /// `{ path = "analysis/results.ipynb", download = true }`.
    Table(NotebookTable),
}

/// Table form of a notebook declaration; unknown keys are refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct NotebookTable {
    path: EntryPath,
    /// Emit the exact notebook bytes as a site asset after the pre-public scan passes.
    #[serde(default)]
    download: bool,
}

impl NotebookDeclaration {
    pub(super) fn path(&self) -> &EntryPath {
        match self {
            Self::Path(path) | Self::Table(NotebookTable { path, .. }) => path,
        }
    }

    pub(super) fn download(&self) -> bool {
        matches!(self, Self::Table(NotebookTable { download: true, .. }))
    }
}

/// Accept strings and tables in one array, keeping each form's own errors.
pub(super) fn declarations<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Vec<NotebookDeclaration>, D::Error> {
    struct Entry(NotebookDeclaration);
    impl<'de> serde::Deserialize<'de> for Entry {
        fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = NotebookDeclaration;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a notebook path or a { path, download } table")
                }
                fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                    value
                        .parse()
                        .map(NotebookDeclaration::Path)
                        .map_err(E::custom)
                }
                fn visit_map<M: serde::de::MapAccess<'de>>(
                    self,
                    map: M,
                ) -> Result<Self::Value, M::Error> {
                    NotebookTable::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                        .map(NotebookDeclaration::Table)
                }
            }
            decoder.deserialize_any(Visitor).map(Entry)
        }
    }
    use serde::Deserialize;
    Vec::<Entry>::deserialize(decoder)
        .map(|entries| entries.into_iter().map(|Entry(value)| value).collect())
}

/// Borrowed retained syntax and exact logical input, not a public download or render.
#[derive(Debug)]
pub struct BoundNotebook<'a, 's> {
    /// Complete logical corpus path; native filesystem coordinates remain upstream.
    pub path: EntryPath,
    /// Original validated notebook, with no repair, execution or additional JSON parse.
    pub notebook: &'a ParsedNotebook<'s>,
}

/// Private evidence for one actually rendered attachment in declaration order.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct NotebookEvidence {
    /// Original stored raster identities and actual decode evidence in emitted order.
    /// These images retain their metadata and are not sanitized or public-admitted.
    pub images: Vec<gazette_core::raster::Inspection>,
    /// Exact retained logical source, not a downloadable route.
    pub path: EntryPath,
    /// Original notebook bytes, including stored outputs and metadata.
    pub source_sha256: Sha256Digest,
    /// Exact schema used by the retained parser.
    pub schema_sha256: Sha256Digest,
    /// Static renderer profile; never evidence of cell execution or public admission.
    pub tool: &'static str,
    /// Exact rendered fragment consumed by the shared page projection.
    pub html_sha256: Sha256Digest,
    /// Exact corresponding searchable text.
    pub text_sha256: Sha256Digest,
    /// Why this page's selected download was withheld: the field and class that
    /// failed the pre-public scan, never the matched value. Absent when it passed
    /// or no download was selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_refusal: Option<crate::notebook::ScanFinding>,
}

/// A declared attachment cannot be satisfied by the page's configured source.
#[derive(Debug)]
pub enum NotebookError {
    /// The owning page has no valid same-site content-source binding.
    Source(SourceBindingError),
    /// The exact input is missing, wrong-kind, repeated or excluded.
    Input {
        /// Declaring page, never inferred from notebook metadata.
        content: Key,
        /// Literal capture-relative declaration, not an ambient filesystem path.
        input: EntryPath,
        /// Bounded failure explanation; no notebook values are disclosed.
        reason: &'static str,
    },
}

impl NotebookError {
    /// Stable diagnostic class shared by direct callers and publication resolution.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Source(error) => error.code(),
            Self::Input { .. } => "notebook_binding",
        }
    }
}

impl fmt::Display for NotebookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(f),
            Self::Input {
                content,
                input,
                reason,
            } => {
                write!(
                    f,
                    "page {content} notebook {input}: {reason}; no substitute selected"
                )
            }
        }
    }
}

impl std::error::Error for NotebookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Input { .. } => None,
        }
    }
}

impl<'a, 's> ResolvedAuthored<'a, 's> {
    /// Resolve ordered attachments from the same retained capture as their authored page.
    /// A notebook is supplementary syntax, not another content identity. Empty means
    /// no declared attachments; callers must independently establish the page identity.
    /// Binding alone does not discharge the site's uncompiled notebook obligation.
    ///
    /// # Errors
    /// Rejects unknown source/home, repeated, missing, non-notebook or excluded inputs.
    /// No basename discovery, cross-source fallback, native reads or execution occurs.
    pub fn notebooks(&self, id: &Key) -> Result<Vec<BoundNotebook<'a, 's>>, NotebookError> {
        let Some(inputs) = self.notebooks.get(id) else {
            return Ok(Vec::new());
        };
        let source = self.source_binding(id).map_err(NotebookError::Source)?;
        let mut seen = BTreeSet::new();
        inputs
            .iter()
            .map(|input| {
                let invalid = |reason| NotebookError::Input {
                    content: id.clone(),
                    input: input.clone(),
                    reason,
                };
                if !seen.insert(input) {
                    return Err(invalid("attachment is repeated on the same page"));
                }
                let path = source
                    .logical_path(input)
                    .map_err(|_| invalid("invalid source-relative input"))?;
                if self
                    .selected_assets
                    .as_ref()
                    .is_some_and(|selected| !selected.contains(&path))
                {
                    return Err(invalid("selected page requires an included notebook"));
                }
                match self.corpus.files().get(&path) {
                    Some(File::Notebook(notebook)) => Ok(BoundNotebook { path, notebook }),
                    Some(_) => Err(invalid("input is not a validated notebook")),
                    None => Err(invalid("input is absent from the page's retained source")),
                }
            })
            .collect()
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Logical notebook paths that a page on `site` selected for download, each with
    /// its first selecting page in identity order. Binding failures surface through
    /// `notebooks`, which every resolution and projection runs.
    pub(super) fn notebook_download_owners(&self, site: &Key) -> BTreeMap<EntryPath, Key> {
        let mut owners = BTreeMap::new();
        for (id, inputs) in &self.notebook_downloads {
            if self
                .content
                .graph()
                .node(id)
                .is_none_or(|node| &node.site != site)
            {
                continue;
            }
            let Ok(source) = self.source_binding(id) else {
                continue;
            };
            for input in inputs {
                if let Ok(path) = source.logical_path(input) {
                    owners.entry(path).or_insert_with(|| id.clone());
                }
            }
        }
        owners
    }

    /// Whether this page itself selected a download of the logical notebook path.
    pub(super) fn selects_notebook_download(&self, id: &Key, path: &EntryPath) -> bool {
        let Some(inputs) = self.notebook_downloads.get(id) else {
            return false;
        };
        self.source_binding(id).is_ok_and(|source| {
            inputs.iter().any(|input| {
                source
                    .logical_path(input)
                    .is_ok_and(|logical| &logical == path)
            })
        })
    }

    /// A download sits beside its owning page, under the notebook's file name.
    /// Another output at that address fails the site's collision-checked inventory.
    pub(super) fn notebook_download_route(
        &self,
        owner: &Key,
        path: &EntryPath,
    ) -> Result<RoutePath, AssetError> {
        let invalid = || AssetError::Route(path.clone());
        let route = self
            .content
            .graph()
            .node(owner)
            .and_then(|node| node.route.as_ref())
            .ok_or_else(invalid)?;
        let name: EntryPath = path
            .as_str()
            .rsplit('/')
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?;
        route.path.join_file(&name).map_err(|_| invalid())
    }
}

/// Media type reported for an emitted notebook download.
const NOTEBOOK_MEDIA_TYPE: &str = "application/x-ipynb+json";

/// Page inputs that notebook rendering shares with the essay and `notes.md`.
pub(super) struct NotebookContext<'p, 'x> {
    pub content: &'p ContentNode,
    pub asset_plan: &'x AssetPlan<'x>,
    pub image_presentations: &'p BTreeMap<EntryPath, markdown::ImagePresentation>,
    pub dimensions: &'p BTreeMap<String, gazette_core::resource::ImageLayout>,
    pub raster: &'p gazette_core::raster::Inspect<'p>,
    pub markdown: &'p dyn gazette_ports::markdown::Markdown,
    pub components: Option<markdown::ComponentPresentation<'p>>,
    pub host: &'p gazette_core::markdown::ComponentHost,
}

impl ResolvedAuthored<'_, '_> {
    /// Render the page's declared notebooks into its Notes, after any `notes.md`.
    ///
    /// Creates Notes when the page has none. Cells keep their namespaced anchors;
    /// notebook text joins the Notes text, and the reading Markdown joins the Notes
    /// Markdown. `budget` is the page's remaining output allowance for all notebooks,
    /// and `page_markdown` is the essay's reading Markdown length when it is selected.
    ///
    /// # Errors
    /// Binding, anchor, render and limit failures; a refused download is evidence, not an error.
    pub(super) fn project_notebooks<'x>(
        &self,
        context: &NotebookContext<'_, 'x>,
        anchor_conflict: impl Fn(&str) -> Result<bool, ProjectionError>,
        notes: &mut Option<ArticleNotes>,
        assets: &mut BTreeMap<&'x str, &'x StaticAsset<'x>>,
        mut budget: usize,
        page_markdown: Option<usize>,
    ) -> Result<Vec<NotebookEvidence>, ProjectionError> {
        let id = &context.content.id;
        let binding = |reason| ProjectionError::Binding {
            id: id.clone(),
            reason,
        };
        let bound = self
            .notebooks(id)
            .map_err(ProjectionError::NotebookBinding)?;
        let mut evidence = Vec::with_capacity(bound.len());
        if bound.is_empty() {
            return Ok(evidence);
        }
        let notes =
            notes.get_or_insert_with(|| ArticleNotes::notebooks_only(page_markdown.is_some()));
        for bound in bound {
            let namespace = format!(
                "gazette-notebook-{}-",
                Sha256Digest::of(bound.path.as_str().as_bytes())
            );
            if anchor_conflict(&namespace)? {
                return Err(binding(
                    "primary body anchor conflicts with its notebook namespace",
                ));
            }
            let projection = bound
                .notebook
                .render_with_components_hosted(
                    &namespace,
                    markdown::Limits {
                        output_bytes: budget.saturating_sub(1),
                        ..markdown::Limits::default()
                    },
                    context.raster,
                    context.markdown,
                    context.components,
                    context.host,
                    |form, kind, destination, span| {
                        let resolved = self.resolve_notebook_reference(
                            (id, &bound.path),
                            context.asset_plan,
                            form,
                            kind,
                            destination,
                            span,
                        )?;
                        if let Some(asset) = resolved.as_ref().and_then(|value| value.asset) {
                            assets.insert(asset.url.as_str(), asset);
                        }
                        let mapped = resolved
                            .as_ref()
                            .map(|reference| reference.destination.clone());
                        let effective = mapped.as_deref().unwrap_or(destination);
                        let annotation = (kind == markdown::LinkKind::Link).then(|| {
                            self.link_destination(
                                context.content,
                                destination,
                                effective,
                                resolved.as_ref(),
                            )
                        });
                        let image = (kind == markdown::LinkKind::Image
                            && resolved
                                .as_ref()
                                .is_some_and(|reference| reference.resource))
                        .then(|| {
                            super::body_images::presentation_for(
                                context.image_presentations,
                                resolved.as_ref(),
                                context.dimensions,
                                context.raster,
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
                .map_err(|error| ProjectionError::Notebook {
                    path: bound.path.clone(),
                    error,
                })?;
            budget = budget
                .checked_sub(
                    projection
                        .html
                        .len()
                        .max(projection.text.len())
                        .saturating_add(1),
                )
                .ok_or(ProjectionError::Limit("page and notebook output bytes"))?;
            if let Some(alternate) = &mut notes.body_markdown {
                // Stored notebook output already passed its native inert HTML profile.
                // The reading alternate preserves cell order, literal outputs and
                // links as Markdown. No kernel or typesetter runs again.
                let remaining = self
                    .projection_limits
                    .text_bytes
                    .checked_sub(
                        alternate
                            .len()
                            .saturating_add(page_markdown.unwrap_or(0))
                            .saturating_add(4),
                    )
                    .ok_or(ProjectionError::Limit("page and notebook Markdown bytes"))?;
                let transported = context
                    .markdown
                    .reading_static_html(
                        &projection.html,
                        gazette_core::source::Location {
                            span: gazette_core::source::Span {
                                start: 0,
                                end: bound.notebook.source().len(),
                            },
                            precision: gazette_core::source::Precision::Construct,
                        },
                        markdown::Limits {
                            output_bytes: remaining,
                            ..markdown::Limits::default()
                        },
                    )
                    .map_err(|error| ProjectionError::Notebook {
                        path: bound.path.clone(),
                        error: crate::notebook::RenderError::Markdown {
                            cell: None,
                            error: markdown::Error::Adapter(error),
                        },
                    })?;
                alternate.push_str("\n\n");
                alternate.push_str(&transported);
                alternate.push_str("\n\n");
            }
            let selected = self.selects_notebook_download(id, &bound.path);
            let download_refusal = context
                .asset_plan
                .download_refusal(&bound.path)
                .filter(|_| selected)
                .cloned();
            let download = match context.asset_plan.get(&bound.path) {
                Some(asset) if selected => {
                    assets.insert(asset.url.as_str(), asset);
                    Some(NotebookDownload {
                        url: asset.url.clone(),
                        bytes: asset.bytes,
                        media_type: NOTEBOOK_MEDIA_TYPE,
                    })
                }
                None if selected && download_refusal.is_none() => {
                    return Err(binding("selected notebook download has no site asset"));
                }
                _ => None,
            };
            evidence.push(NotebookEvidence {
                images: projection.images,
                path: bound.path,
                source_sha256: projection.source_sha256,
                schema_sha256: projection.schema_sha256,
                tool: projection.tool,
                html_sha256: Sha256Digest::of(projection.html.as_bytes()),
                text_sha256: Sha256Digest::of(projection.text.as_bytes()),
                download_refusal,
            });
            if !notes.body_text.is_empty() {
                notes.body_text.push('\n');
            }
            notes.body_text.push_str(&projection.text);
            notes.components.extend(projection.components);
            notes.links.extend(projection.links);
            notes.images.extend(projection.image_uses);
            notes.notebooks.push(NotebookView {
                id: namespace.trim_end_matches('-').to_owned(),
                cells: projection.cells,
                download,
            });
        }
        Ok(evidence)
    }
}
