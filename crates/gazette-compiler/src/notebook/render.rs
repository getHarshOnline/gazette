//! Static projection of validated stored cells; no kernels, process execution or native I/O.

use std::{borrow::Cow, collections::BTreeMap, fmt, io::Write};

use base64::{display::Base64Display, engine::general_purpose::STANDARD};
use gazette_core::{raster, render::LimitedWriter, source::Sha256Digest};
use serde::Serialize;
use serde_json::Value;

use super::ParsedNotebook;
use crate::corpus::MathLocation;
use crate::markdown;

#[path = "markdown.rs"]
mod markdown_render;
use markdown_render::Projection;

/// Complete static syntax projection, not admission of stored output or source downloads.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct RenderedNotebook {
    /// Validated component occurrences in cell/output traversal order, not execution proof.
    pub components: Vec<markdown::ComponentUse>,
    /// Compiler-proved destination annotations in notebook traversal order.
    pub links: Vec<markdown::LinkUse>,
    /// Emitted Markdown images, with cell-scoped occurrence addresses.
    pub image_uses: Vec<markdown::ImageUse>,
    /// Escaped code/output and checked Markdown in the original cell order.
    pub html: String,
    /// Text from the same cells and supported stored representations.
    pub text: String,
    /// Fixed renderer semantics; no installed executable or ambient kernel is consulted.
    pub tool: &'static str,
    /// Exact retained notebook input, not normalized or reserialized JSON.
    pub source_sha256: Sha256Digest,
    /// Exact schema used to admit the retained syntax tree.
    pub schema_sha256: Sha256Digest,
    /// Exact decoded image evidence, private to the build; not a disclosure grant.
    pub images: Vec<raster::Inspection>,
    /// The same cells as `html`, split so a Theme can place source and outputs apart.
    pub cells: Vec<NotebookCell>,
}

/// One rendered cell. Fragments are exact slices of `RenderedNotebook::html`,
/// without the cell wrapper element, so heading and footnote IDs are unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotebookCell {
    /// Rendered Markdown cell.
    Markdown {
        /// nbformat cell ID, for a `data-cell-id` attribute; not an anchor.
        cell_id: String,
        /// Rendered Markdown, including its namespaced heading IDs.
        html: String,
    },
    /// Stored code cell; nothing was executed.
    Code {
        /// nbformat cell ID, for a `data-cell-id` attribute; not an anchor.
        cell_id: String,
        /// Stored execution count, or null when the cell never ran.
        execution_count: Option<serde_json::Number>,
        /// Language declared by the notebook metadata, when present.
        language: Option<String>,
        /// The `pre.notebook-source` block, highlighted when the language is known.
        source_html: String,
        /// Stored outputs in their original order; empty when there are none.
        outputs_html: String,
    },
}

/// Static rendering refuses unsupported content instead of choosing a lossy fallback.
#[derive(Debug)]
pub enum RenderError {
    /// Stored raster decoding or decoder evidence failed; no text fallback is accepted.
    Raster {
        /// Zero-based source cell.
        cell: usize,
        /// Bounded decoder or byte-binding failure.
        error: raster::Error,
    },
    /// A complete source, intermediate representation or output exceeds its ceiling.
    Limit(&'static str),
    /// A feature needs a separately selected rendering capability.
    Unsupported {
        /// Zero-based cell, absent for notebook-wide metadata.
        cell: Option<usize>,
        /// Bounded feature name, never an untrusted metadata or MIME value.
        feature: &'static str,
    },
    /// The shared Markdown parser or renderer rejected one cell or its namespace.
    Markdown {
        /// Zero-based cell, absent for the common namespace check.
        cell: Option<usize>,
        /// Original bounded Markdown failure, retaining cell-local source spans.
        error: markdown::Error,
    },
    /// The retained validated tree or generated UTF-8 violated an internal invariant.
    Structure,
}

impl RenderError {
    /// Stable failure class without echoing stored notebook data.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Raster { error, .. } => error.code(),
            Self::Limit(_) => "notebook_render_limit",
            Self::Unsupported { .. } => "notebook_render_unsupported",
            Self::Markdown { error, .. } => error.code(),
            Self::Structure => "notebook_render_structure",
        }
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raster { cell, error } => write!(f, "notebook raster cell {cell}: {error}"),
            Self::Limit(resource) => write!(
                f,
                "notebook rendering {resource} ceiling exceeded; no complete output returned"
            ),
            Self::Unsupported { cell, feature } => write!(
                f,
                "notebook cell {cell:?} requires unsupported {feature}; no static fallback or partial output returned"
            ),
            Self::Markdown { cell, error } => write!(f, "notebook Markdown cell {cell:?}: {error}"),
            Self::Structure => {
                f.write_str("notebook static renderer encountered an invalid retained structure")
            }
        }
    }
}
impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Raster { error, .. } => Some(error),
            Self::Markdown { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl ParsedNotebook<'_> {
    /// Render stored Markdown/code cells and text/JSON/raster outputs without executing anything.
    ///
    /// Source arrays concatenate exactly. Streams, stored errors and every supported
    /// MIME representation are retained; an unsupported representation rejects the
    /// entire notebook even when a text alternative exists. Stored HTML uses an
    /// explicit inert structural profile; raw cells, active HTML and widget state
    /// are unavailable. Metadata is not publication meaning
    /// and does not hide cells or authorize executable/browser output.
    ///
    /// Each Markdown cell and stored Markdown output has its own namespace beneath
    /// the caller's prefix. Unbound
    /// local paths fail; fragments and safe external URLs use the shared Markdown gate.
    /// No reference is fetched. Namespacing does not grant asset or disclosure approval.
    /// Stored PNG/JPEG bytes require the selected raster inspector, whose exact byte
    /// evidence is checked before emitting an image. Metadata is not copied into image
    /// attributes. This retains encoded image metadata; it does not sanitize or approve it.
    ///
    /// `source_bytes` bounds the exact notebook and joined text; Markdown event budgets
    /// are shared across cells and stored outputs. HTML and text each have one
    /// whole-notebook output cap.
    /// Logical bounds exclude allocator overhead; no successful partial result is returned.
    ///
    /// # Errors
    /// Refuses unsupported syntax/output, unsafe Markdown, invalid namespace or exhausted bounds.
    pub fn render_static(
        &self,
        namespace: &str,
        limits: markdown::Limits,
        raster: &raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<RenderedNotebook, RenderError> {
        self.render_static_resolving(namespace, limits, raster, markdown, |_, _, _| Ok(None))
    }

    /// Render with explicit retained-source mappings for local Markdown references.
    ///
    /// The caller owns source confinement and asset selection. Unmapped local
    /// references fail; both authored and mapped URLs pass the shared Markdown
    /// safety gate. No callback runs for safe external URLs or local fragments.
    /// Cell-local PNG/JPEG attachments are separately decoded before Markdown binding.
    ///
    /// # Errors
    /// Refuses invalid mappings and the same syntax, output and bounds as `render_static`.
    pub fn render_static_resolving(
        &self,
        namespace: &str,
        limits: markdown::Limits,
        raster: &raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        resolve: impl FnMut(
            markdown::LinkKind,
            &str,
            gazette_core::source::Span,
        ) -> Result<Option<String>, markdown::Error>,
    ) -> Result<RenderedNotebook, RenderError> {
        self.render_with_components(namespace, limits, raster, markdown, None, resolve)
    }

    /// Render cells and stored Markdown with one explicitly selected component catalog.
    /// Occurrences join the owning page's presentation and interaction requirements;
    /// absent definitions retain the standalone renderer's refusal behavior.
    ///
    /// # Errors
    /// Refuses invalid components and all source, link, raster and output failures of
    /// `render_static_resolving`. No cells execute and no partial projection is returned.
    pub fn render_with_components(
        &self,
        namespace: &str,
        limits: markdown::Limits,
        raster: &raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        components: Option<markdown::ComponentPresentation<'_>>,
        mut resolve: impl FnMut(
            markdown::LinkKind,
            &str,
            gazette_core::source::Span,
        ) -> Result<Option<String>, markdown::Error>,
    ) -> Result<RenderedNotebook, RenderError> {
        let mut resolve = |form: markdown::LinkForm,
                           kind: markdown::LinkKind,
                           destination: &str,
                           span: gazette_core::source::Span|
         -> Result<markdown::ResolvedLink, markdown::Error> {
            if form == markdown::LinkForm::WikiLink {
                return Err(markdown::Error::LinkResolution {
                    span,
                    reason: "symbolic references require the configured publication resolver",
                });
            }
            if destination.starts_with('#')
                || url::Url::parse(destination).is_ok_and(|url| {
                    matches!(url.scheme(), "http" | "https")
                        || kind == markdown::LinkKind::Link && url.scheme() == "mailto"
                })
            {
                Ok(markdown::ResolvedLink::plain(None))
            } else {
                resolve(kind, destination, span)?
                    .map(Some)
                    .ok_or(markdown::Error::LinkResolution {
                        span,
                        reason: "local notebook references require an explicit source mapping",
                    })
                    .map(markdown::ResolvedLink::plain)
            }
        };
        self.render_with_component_host(
            namespace,
            limits,
            raster,
            markdown,
            components,
            None,
            0,
            &mut resolve,
        )
    }

    /// Render for a page's Notes. Headings move down one level when the notebook's
    /// Markdown has an H1, the same rule Notes apply to `notes.md`; IDs are unchanged.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_with_components_hosted(
        &self,
        namespace: &str,
        limits: markdown::Limits,
        raster: &raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        components: Option<markdown::ComponentPresentation<'_>>,
        host: &gazette_core::markdown::ComponentHost,
        mut resolve: impl FnMut(
            markdown::LinkForm,
            markdown::LinkKind,
            &str,
            gazette_core::source::Span,
        ) -> Result<markdown::ResolvedLink, markdown::Error>,
    ) -> Result<RenderedNotebook, RenderError> {
        let heading_offset = u8::from(self.has_top_heading(limits, markdown)?);
        self.render_with_component_host(
            namespace,
            limits,
            raster,
            markdown,
            components,
            Some(host),
            heading_offset,
            &mut resolve,
        )
    }

    /// Whether any Markdown cell or stored Markdown output contains an H1.
    fn has_top_heading(
        &self,
        limits: markdown::Limits,
        adapter: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<bool, RenderError> {
        let cells = self
            .value()
            .get("cells")
            .and_then(Value::as_array)
            .ok_or(RenderError::Structure)?;
        for (index, cell) in cells.iter().enumerate() {
            let mut sources = Vec::new();
            match cell.get("cell_type").and_then(Value::as_str) {
                Some("markdown") => sources.push(cell.get("source").ok_or(RenderError::Structure)?),
                Some("code") => sources.extend(
                    cell.get("outputs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|output| output.pointer("/data/text~1markdown")),
                ),
                _ => {}
            }
            for source in sources {
                let source = multiline(source, limits.source_bytes)?;
                let failure = |error| RenderError::Markdown {
                    cell: Some(index),
                    error,
                };
                let parsed = markdown::ParsedMarkdown::parse_with_profile(
                    &source,
                    self.profile,
                    limits,
                    adapter,
                )
                .map_err(failure)?;
                if parsed
                    .headings()
                    .map_err(failure)?
                    .iter()
                    .any(|heading| heading.level == 1)
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_with_component_host(
        &self,
        namespace: &str,
        limits: markdown::Limits,
        raster: &raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        components: Option<markdown::ComponentPresentation<'_>>,
        host: Option<&gazette_core::markdown::ComponentHost>,
        heading_offset: u8,
        resolve: &mut markdown_render::Resolve<'_>,
    ) -> Result<RenderedNotebook, RenderError> {
        markdown::validate_namespace(namespace)
            .map_err(|error| RenderError::Markdown { cell: None, error })?;
        if self.source().len() > limits.source_bytes {
            return Err(RenderError::Limit("source bytes"));
        }
        if self
            .value()
            .get("metadata")
            .and_then(|metadata| metadata.get("widgets"))
            .is_some()
        {
            return Err(RenderError::Unsupported {
                cell: None,
                feature: "widget state",
            });
        }
        let cells = self
            .value()
            .get("cells")
            .and_then(Value::as_array)
            .ok_or(RenderError::Structure)?;
        let mut output = Output {
            html: LimitedWriter::new(Vec::new(), limits.output_bytes),
            text: LimitedWriter::new(Vec::new(), limits.output_bytes),
            images: Vec::new(),
            components: Vec::new(),
            links: Vec::new(),
            image_uses: Vec::new(),
        };
        output.markup("<section class=\"gazette-notebook\">")?;
        let adapter = markdown;
        let language = self
            .value()
            .pointer("/metadata/language_info/name")
            .or_else(|| self.value().pointer("/metadata/kernelspec/language"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let mut markdown = Projection::new(
            limits,
            markdown,
            components,
            host,
            resolve,
            self,
            heading_offset,
        );
        let mut spans = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter().enumerate() {
            let id = cell
                .get("id")
                .and_then(Value::as_str)
                .ok_or(RenderError::Structure)?;
            let unsupported = |feature| RenderError::Unsupported {
                cell: Some(index),
                feature,
            };
            let source = multiline(
                cell.get("source").ok_or(RenderError::Structure)?,
                limits.source_bytes,
            )?;
            // Delimited ordinals cannot overlap across cells; authored IDs do not
            // participate in allocation. Code-output ordinals extend this prefix.
            let prefix = format!("{namespace}-cell-{index}-");
            match cell.get("cell_type").and_then(Value::as_str) {
                Some("markdown") => {
                    let images = super::attachments::prepare(
                        cell,
                        index,
                        limits.source_bytes,
                        raster,
                        &mut output.images,
                    )?;
                    let rendered = markdown.render(
                        &source,
                        &prefix,
                        &images,
                        MathLocation::NotebookCell { cell: index },
                    )?;
                    output.cell_start("notebook-markdown", id)?;
                    output.markup(">")?;
                    let start = output.position();
                    output.markup(&rendered.body_html)?;
                    spans.push(CellSpans::Markdown {
                        id,
                        html: start..output.position(),
                    });
                    output.markup("</div>")?;
                    output.plain(&rendered.body_text)?;
                    output.components.extend(rendered.components);
                    output.links.extend(rendered.links);
                    output.image_uses.extend(rendered.images);
                }
                Some("code") => {
                    output.cell_start("notebook-code", id)?;
                    output.markup(" data-execution-count=\"")?;
                    output.json_attribute(
                        cell.get("execution_count").ok_or(RenderError::Structure)?,
                    )?;
                    output.markup("\">")?;
                    let highlighted = language
                        .map(|language| adapter.highlight_code(&source, language, limits))
                        .transpose()
                        .map_err(|error| RenderError::Markdown {
                            cell: Some(index),
                            error: markdown::Error::Adapter(error),
                        })?
                        .flatten();
                    let source_start = output.position();
                    output.source(&source, language, highlighted.as_deref())?;
                    let outputs_start = output.position();
                    for (ordinal, stored) in cell
                        .get("outputs")
                        .and_then(Value::as_array)
                        .ok_or(RenderError::Structure)?
                        .iter()
                        .enumerate()
                    {
                        let location = StoredLocation {
                            cell: index,
                            ordinal,
                            prefix: format!("{prefix}output-{ordinal}-"),
                        };
                        output.stored(stored, &location, limits, raster, &mut markdown)?;
                    }
                    spans.push(CellSpans::Code {
                        id,
                        execution_count: cell.get("execution_count").and_then(
                            |count| match count {
                                Value::Number(number) => Some(number.clone()),
                                _ => None,
                            },
                        ),
                        source: source_start..outputs_start,
                        outputs: outputs_start..output.position(),
                    });
                    output.markup("</div>")?;
                }
                Some("raw") => return Err(unsupported("raw cells")),
                _ => return Err(RenderError::Structure),
            }
        }
        markdown.finish()?;
        output.markup("</section>")?;
        let original = self.inspect();
        let html =
            String::from_utf8(output.html.into_inner()).map_err(|_| RenderError::Structure)?;
        let fragment = |range: std::ops::Range<usize>| {
            html.get(range)
                .map(str::to_owned)
                .ok_or(RenderError::Structure)
        };
        let cells = spans
            .into_iter()
            .map(|span| {
                Ok(match span {
                    CellSpans::Markdown { id, html } => NotebookCell::Markdown {
                        cell_id: id.to_owned(),
                        html: fragment(html)?,
                    },
                    CellSpans::Code {
                        id,
                        execution_count,
                        source,
                        outputs,
                    } => NotebookCell::Code {
                        cell_id: id.to_owned(),
                        execution_count,
                        language: language.map(str::to_owned),
                        source_html: fragment(source)?,
                        outputs_html: fragment(outputs)?,
                    },
                })
            })
            .collect::<Result<_, RenderError>>()?;
        Ok(RenderedNotebook {
            cells,
            html,
            text: String::from_utf8(output.text.into_inner())
                .map_err(|_| RenderError::Structure)?,
            tool: "gazette.notebook-static/1;nbformat/4.5",
            source_sha256: original.sha256,
            schema_sha256: original.schema_sha256,
            images: output.images,
            components: output.components,
            links: output.links,
            image_uses: output.image_uses,
        })
    }
}

enum CellSpans<'v> {
    Markdown {
        id: &'v str,
        html: std::ops::Range<usize>,
    },
    Code {
        id: &'v str,
        execution_count: Option<serde_json::Number>,
        source: std::ops::Range<usize>,
        outputs: std::ops::Range<usize>,
    },
}

struct Output {
    html: LimitedWriter<Vec<u8>>,
    text: LimitedWriter<Vec<u8>>,
    images: Vec<raster::Inspection>,
    components: Vec<markdown::ComponentUse>,
    links: Vec<markdown::LinkUse>,
    image_uses: Vec<markdown::ImageUse>,
}

struct StoredLocation {
    cell: usize,
    ordinal: usize,
    prefix: String,
}

impl Output {
    fn position(&self) -> usize {
        self.html.get_ref().len()
    }

    fn source(
        &mut self,
        value: &str,
        language: Option<&str>,
        highlighted: Option<&str>,
    ) -> Result<(), RenderError> {
        self.markup("<pre class=\"notebook-source\"><code")?;
        if let Some(language) = language {
            write!(
                self.html,
                " class=\"language-{}\"",
                quick_xml::escape::escape(language)
            )
            .map_err(|_| RenderError::Limit("HTML bytes"))?;
        }
        self.markup(">")?;
        match highlighted {
            Some(html) => self.markup(html)?,
            None => self.markup(&quick_xml::escape::escape(value))?,
        }
        self.markup("</code></pre>")?;
        self.plain(value)
    }

    fn cell_start(&mut self, class: &'static str, id: &str) -> Result<(), RenderError> {
        // The retained nbformat schema restricts IDs to ASCII letters, digits,
        // hyphens and underscores. Keep that source ID separate from anchor allocation.
        write!(self.html, "<div class=\"{class}\" data-cell-id=\"{id}\"")
            .map_err(|_| RenderError::Limit("HTML bytes"))
    }

    fn markup(&mut self, value: &str) -> Result<(), RenderError> {
        self.html
            .write_all(value.as_bytes())
            .map_err(|_| RenderError::Limit("HTML bytes"))
    }

    fn plain(&mut self, value: &str) -> Result<(), RenderError> {
        self.text
            .write_all(value.as_bytes())
            .and_then(|()| self.text.write_all(b"\n"))
            .map_err(|_| RenderError::Limit("text bytes"))
    }

    fn pre(&mut self, value: &str, class: &'static str) -> Result<(), RenderError> {
        write!(self.html, "<pre class=\"{class}\"><code>")
            .map_err(|_| RenderError::Limit("HTML bytes"))?;
        self.markup(&quick_xml::escape::escape(value))?;
        self.markup("</code></pre>")?;
        self.plain(value)
    }

    fn json_attribute(&mut self, value: &Value) -> Result<(), RenderError> {
        if !(value.is_number() || value.is_null()) {
            return Err(RenderError::Structure);
        }
        serde_json::to_writer(&mut self.html, value).map_err(|_| RenderError::Limit("HTML bytes"))
    }

    fn stored(
        &mut self,
        stored: &Value,
        location: &StoredLocation,
        limits: markdown::Limits,
        raster: &raster::Inspect<'_>,
        markdown: &mut Projection<'_>,
    ) -> Result<(), RenderError> {
        let unsupported = |feature| RenderError::Unsupported {
            cell: Some(location.cell),
            feature,
        };
        match stored.get("output_type").and_then(Value::as_str) {
            Some("stream") => {
                let name = stored
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(RenderError::Structure)?;
                let label = match name {
                    "stdout" => "stdout",
                    "stderr" => "stderr",
                    _ => return Err(RenderError::Structure),
                };
                self.markup("<div class=\"notebook-stream\"><span>")?;
                self.markup(label)?;
                self.markup("</span>")?;
                self.plain(label)?;
                self.pre(
                    &multiline(
                        stored.get("text").ok_or(RenderError::Structure)?,
                        limits.source_bytes,
                    )?,
                    "notebook-output",
                )?;
                self.markup("</div>")
            }
            Some("error") => {
                self.markup("<div class=\"notebook-error\">")?;
                for key in ["ename", "evalue"] {
                    self.pre(
                        stored
                            .get(key)
                            .and_then(Value::as_str)
                            .ok_or(RenderError::Structure)?,
                        "notebook-error-message",
                    )?;
                }
                for line in stored
                    .get("traceback")
                    .and_then(Value::as_array)
                    .ok_or(RenderError::Structure)?
                {
                    self.pre(
                        line.as_str().ok_or(RenderError::Structure)?,
                        "notebook-traceback",
                    )?;
                }
                self.markup("</div>")
            }
            Some("display_data" | "execute_result") => {
                let data = stored
                    .get("data")
                    .and_then(Value::as_object)
                    .ok_or(RenderError::Structure)?;
                if data.keys().any(|mime| {
                    !matches!(
                        mime.as_str(),
                        "text/plain"
                            | "text/markdown"
                            | "text/html"
                            | "application/json"
                            | "image/png"
                            | "image/jpeg"
                    )
                }) {
                    return Err(unsupported("MIME representation"));
                }
                self.markup("<div class=\"notebook-result\"")?;
                if let Some(count) = stored.get("execution_count") {
                    self.markup(" data-execution-count=\"")?;
                    self.json_attribute(count)?;
                    self.markup("\"")?;
                }
                self.markup(">")?;
                for (mime, value) in data {
                    match mime.as_str() {
                        "text/plain" => {
                            self.pre(&multiline(value, limits.source_bytes)?, "notebook-text")?
                        }
                        "text/markdown" => {
                            let source = multiline(value, limits.source_bytes)?;
                            let rendered = markdown.render(
                                &source,
                                &location.prefix,
                                &BTreeMap::new(),
                                MathLocation::NotebookOutput {
                                    cell: location.cell,
                                    output: location.ordinal,
                                },
                            )?;
                            self.markup("<div class=\"notebook-markdown-output\">")?;
                            self.markup(&rendered.body_html)?;
                            self.markup("</div>")?;
                            self.plain(&rendered.body_text)?;
                            self.components.extend(rendered.components);
                            self.links.extend(rendered.links);
                            self.image_uses.extend(rendered.images);
                        }
                        "text/html" => {
                            let source = multiline(value, limits.source_bytes)?;
                            let (html, text) = super::html::render(&source, limits, location.cell)?;
                            self.markup("<div class=\"notebook-html-output\">")?;
                            self.markup(&html)?;
                            self.markup("</div>")?;
                            self.plain(&text)?;
                        }
                        "application/json" => {
                            let mut encoded = LimitedWriter::new(Vec::new(), limits.output_bytes);
                            serde_json::to_writer(&mut encoded, value)
                                .map_err(|_| RenderError::Limit("JSON output bytes"))?;
                            let encoded = String::from_utf8(encoded.into_inner())
                                .map_err(|_| RenderError::Structure)?;
                            self.pre(&encoded, "notebook-json")?;
                        }
                        "image/png" | "image/jpeg" => {
                            let format = if mime == "image/png" {
                                raster::Format::Png
                            } else {
                                raster::Format::Jpeg
                            };
                            let encoded = multiline(value, limits.source_bytes)?;
                            let (bytes, image) = super::raster::decode(
                                &encoded,
                                format,
                                limits.source_bytes,
                                raster,
                            )
                            .map_err(|error| RenderError::Raster {
                                cell: location.cell,
                                error,
                            })?;
                            write!(self.html, "<img class=\"notebook-image\" src=\"data:{};base64,{}\" width=\"{}\" height=\"{}\" alt=\"Stored notebook image\">",
                                format.media_type(), Base64Display::new(&bytes, &STANDARD), image.width, image.height)
                                .map_err(|_| RenderError::Limit("HTML bytes"))?;
                            self.plain(&format!(
                                "[Stored notebook image: {}, {} × {} pixels]",
                                format.media_type(),
                                image.width,
                                image.height
                            ))?;
                            self.images.push(image);
                        }
                        _ => return Err(RenderError::Structure),
                    }
                }
                self.markup("</div>")
            }
            _ => Err(RenderError::Structure),
        }
    }
}

pub(super) fn multiline(value: &Value, maximum: usize) -> Result<Cow<'_, str>, RenderError> {
    if let Some(text) = value.as_str() {
        return if text.len() <= maximum {
            Ok(Cow::Borrowed(text))
        } else {
            Err(RenderError::Limit("joined source bytes"))
        };
    }
    let mut joined = LimitedWriter::new(Vec::new(), maximum);
    for part in value.as_array().ok_or(RenderError::Structure)? {
        joined
            .write_all(part.as_str().ok_or(RenderError::Structure)?.as_bytes())
            .map_err(|_| RenderError::Limit("joined source bytes"))?;
    }
    String::from_utf8(joined.into_inner())
        .map(Cow::Owned)
        .map_err(|_| RenderError::Structure)
}
