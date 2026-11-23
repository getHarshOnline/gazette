//! Compiler admission and a complete source-bound plan; generic formatting is upstream-owned.

use super::{Budget, Error, LinkForm, LinkKind, ParsedMarkdown, Span, Summary};
use gazette_core::{
    markdown::{
        ComponentContext, ComponentHost, Excerpt, FootnoteBinding, HeadingBinding, LinkAnnotation,
        LinkBinding, LinkDestinationKind, LinkRelationship, Plan,
    },
    publication::Key,
};
use std::collections::BTreeMap;

use crate::authored::SitePresentation;

/// One selected immutable component catalog and its pure presentation consumer.
#[derive(Clone, Copy)]
pub struct ComponentPresentation<'a> {
    /// Definitions validated from the selected captured Theme manifest.
    pub definitions: &'a crate::theme::Components,
    /// Rendering only; it cannot select a definition, acquire sources or approve meaning.
    pub renderer: &'a dyn gazette_ports::markdown::ComponentRenderer,
}

/// Internal byte-bound image transport, never an arbitrary resolver-supplied URL.
pub(crate) struct EmbeddedImage(String);

impl EmbeddedImage {
    pub(crate) fn new(
        bytes: &[u8],
        image: &gazette_core::raster::Inspection,
    ) -> Result<Self, gazette_core::raster::Error> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        image.validate(image.format, bytes, gazette_core::raster::Limits::default())?;
        Ok(Self(format!(
            "data:{};base64,{}",
            image.format.media_type(),
            STANDARD.encode(bytes)
        )))
    }
}

/// Human and machine text from the same normalized, resolved syntax structure.
#[derive(Debug, PartialEq, Eq)]
pub struct RenderedMarkdown {
    /// One admitted main-body container placement; generated navigation is not authored text.
    pub container_contents_placed: bool,
    /// Formatter-proved first-H1 boundary in the complete emitted body.
    pub opening: Option<gazette_core::markdown::OpeningBoundary>,
    /// Optional normalized representation from the exact same admitted plan as HTML.
    pub body_markdown: Option<String>,
    /// Validated emitted component data, without adapter AST or private source coordinates.
    pub components: Vec<ComponentUse>,
    /// Compiler-proved annotations for emitted anchors in the same traversal order.
    pub links: Vec<LinkUse>,
    /// Exact emitted image uses, independent of static or compiled resource transport.
    pub images: Vec<ImageUse>,
    /// Complete body including an explicit continuation anchor when selected.
    pub body_html: String,
    /// Published body text; unused definition text is not source-inspection leakage.
    pub body_text: String,
    /// Explicit balanced prefix, without implicit truncation.
    pub summary: Option<Summary>,
}

/// Borrowed fragments from the exact body formatter, suitable for one composed page header.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct Opening<'a> {
    /// Complete leading H1 markup with its native anchor and inline formatting.
    pub heading_html: &'a str,
    /// Everything after that H1.
    pub remainder_html: &'a str,
}

/// One authored component occurrence shared by presentation, review and machine metadata.
/// Ordering preserves source traversal; arguments are data, not inferred publication claims.
#[derive(Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct ComponentUse {
    /// Questions and admitted answers from this declared FAQ group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faq: Option<super::Faq>,
    /// Stable occurrence identity and existing semantic host context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ComponentContext>,
    /// Exact selected definition name.
    pub name: gazette_core::markdown::ComponentName,
    /// Schema-validated original arguments; templates do not reconstruct these values.
    pub arguments: BTreeMap<String, serde_json::Value>,
    /// Definition-derived intent, not proof that its module compiled or mounted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<crate::theme::ComponentInteraction>,
}

/// Destination facts proved by the authored compiler before Markdown formatting.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct LinkDestination {
    /// Proven destination class.
    pub kind: LinkDestinationKind,
    /// Exact destination content identity, when route resolution proves one.
    pub content: Option<Key>,
    /// Exact configured destination site, when known.
    pub site: Option<Key>,
    /// Direct authored graph relation; ordinary Markdown contributes none.
    pub relationships: Vec<LinkRelationship>,
    /// Effective destination Theme/palette selected by the resolved workspace.
    pub presentation: Option<SitePresentation>,
}

/// One emitted anchor annotation shared by the HTML formatter and public machine model.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct LinkUse {
    /// Stable page-local occurrence identity, independent of native source coordinates.
    pub address: String,
    /// Final admitted destination emitted into HTML.
    pub destination: String,
    /// Proven destination class.
    pub kind: LinkDestinationKind,
    /// Exact destination content identity, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Key>,
    /// Exact configured destination site, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<Key>,
    /// Existing direct authored relationship, never inferred from ordinary Markdown.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schemars(default)]
    pub relationships: Vec<LinkRelationship>,
    /// Existing destination site presentation selected by workspace composition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
}

/// Explicit alternative destinations for one image's light and dark presentation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct ImageVariants {
    /// Resolved light-mode URL, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    /// Resolved dark-mode URL, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

/// Browser candidates for the same default/light/dark image identities as the fallback URLs.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct ResponsiveImage {
    /// Explicit layout declaration, shared by the image's display modes.
    pub sizes: String,
    /// Default presentation candidates, including its original fallback rendition.
    pub default: Vec<gazette_core::markdown::ImageCandidate>,
    /// Light presentation candidates, when the mode is explicitly declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<Vec<gazette_core::markdown::ImageCandidate>>,
    /// Dark presentation candidates, when the mode is explicitly declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<Vec<gazette_core::markdown::ImageCandidate>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ImagePresentation {
    pub responsive: Option<ResponsiveImage>,
    pub svg: Option<gazette_core::resource::SvgLayout>,
    pub loading: Option<gazette_core::markdown::ImageLoading>,
    pub fetch_priority: Option<gazette_core::markdown::ImageFetchPriority>,
    pub intrinsic: Option<gazette_core::raster::Dimensions>,
    pub inline_svg: Option<gazette_core::markdown::InlineSvg>,
    pub variants: Option<ImageVariants>,
    pub width: Option<std::num::NonZeroU32>,
}

/// One emitted image occurrence; no media-type, ownership or existence claim.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct ImageUse {
    /// Verified responsive alternatives; destination remains the full fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsive: Option<ResponsiveImage>,
    /// Admitted SVG root layout; separate from decoded raster dimensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<gazette_core::resource::SvgLayout>,
    /// Explicit browser loading hint, with no first-image or route inference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<gazette_core::markdown::ImageLoading>,
    /// Explicit relative request priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_priority: Option<gazette_core::markdown::ImageFetchPriority>,
    /// Independently verified raster display extent, separate from CSS display width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intrinsic: Option<gazette_core::raster::Dimensions>,
    /// Whether this image only supplies an explicitly presentation-only component.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) presentation_only: bool,
    /// One-based source traversal order within this rendered body.
    pub occurrence: usize,
    /// Stable hosted image address shared with the formatter's data attribute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Exact admitted destination rendered in the image src attribute.
    pub destination: String,
    /// Declared mode alternatives; destination remains the default fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<ImageVariants>,
    /// Intended display width in CSS pixels; a Theme may constrain it responsively.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<std::num::NonZeroU32>,
    /// Native image-label text, preserving an empty decorative label.
    pub alt: String,
    /// Optional native authored image title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Plain reading text from the explicitly bound figure-caption body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

pub(crate) struct ResolvedLink {
    pub destination: Option<String>,
    pub annotation: Option<LinkDestination>,
    pub image: Option<ImagePresentation>,
}

pub(crate) struct Projection<'a> {
    pub container_contents: Option<&'a gazette_core::markdown::ContainerContentsBinding>,
    pub namespace: &'a str,
    pub heading_offset: u8,
    pub images: &'a BTreeMap<String, EmbeddedImage>,
    pub components: Option<ComponentPresentation<'a>>,
    pub host: Option<&'a ComponentHost>,
    pub markdown_alternate: bool,
}

impl ResolvedLink {
    pub(crate) fn plain(destination: Option<String>) -> Self {
        Self {
            destination,
            annotation: None,
            image: None,
        }
    }
}

fn standalone(form: LinkForm, span: Span) -> Result<(), Error> {
    if form == LinkForm::WikiLink {
        return Err(Error::LinkResolution {
            span,
            reason: "symbolic references require the configured publication resolver",
        });
    }
    Ok(())
}

impl ParsedMarkdown<'_> {
    /// Render bounded standalone Markdown without source acquisition or publication certification.
    ///
    /// # Errors
    /// Refuses unadmitted HTML, unsafe URLs, invalid anchors and exhausted bounds.
    pub fn render_html(&self) -> Result<String, Error> {
        Ok(self
            .render_resolving_references(|_, _, _| Ok(None))?
            .body_html)
    }

    /// Resolve emitted navigation destinations while keeping image acquisition separate.
    /// Whole-source safety, including unused definitions and post-excerpt content,
    /// is checked before the first callback.
    ///
    /// # Errors
    /// Propagates syntax admission, resolver, mapped-URL or bounded formatting failure.
    pub fn render_html_resolving_links(
        &self,
        mut resolver: impl FnMut(&str, Span) -> Result<Option<String>, Error>,
    ) -> Result<String, Error> {
        Ok(self
            .render_resolving_references(|kind, uri, span| match kind {
                LinkKind::Link => resolver(uri, span),
                LinkKind::Image => Ok(None),
            })?
            .body_html)
    }

    /// Resolve each emitted link/image once for body, excerpt and normalized text.
    /// Non-emitted source occurrences remain safety evidence, not invented dependencies.
    ///
    /// # Errors
    /// Returns admission, mapping or formatting errors without partial output.
    pub fn render_resolving_references(
        &self,
        resolver: impl FnMut(LinkKind, &str, Span) -> Result<Option<String>, Error>,
    ) -> Result<RenderedMarkdown, Error> {
        self.render_in_namespace("", resolver)
    }

    /// Render an independently scoped fragment using the same admission and resolution path.
    ///
    /// # Errors
    /// Refuses invalid namespaces, original/mapped unsafe syntax, binding mismatch or exhausted bounds.
    pub fn render_in_namespace(
        &self,
        namespace: &str,
        mut resolver: impl FnMut(LinkKind, &str, Span) -> Result<Option<String>, Error>,
    ) -> Result<RenderedMarkdown, Error> {
        self.render_with_images(
            namespace,
            0,
            &BTreeMap::new(),
            None,
            None,
            |form, kind, value, span| {
                standalone(form, span)
                    .and_then(|()| resolver(kind, value, span))
                    .map(ResolvedLink::plain)
            },
        )
    }

    /// Render an authored body with a selected static component consumer.
    /// Headings, links, text and excerpts retain the ordinary shared resolution path.
    ///
    /// # Errors
    /// Refuses unknown/schema-invalid components, source mismatch and ordinary render failures.
    pub fn render_with_components(
        &self,
        components: Option<ComponentPresentation<'_>>,
        mut resolver: impl FnMut(LinkKind, &str, Span) -> Result<Option<String>, Error>,
    ) -> Result<RenderedMarkdown, Error> {
        self.render_with_images(
            "",
            0,
            &BTreeMap::new(),
            components,
            None,
            |form, kind, value, span| {
                standalone(form, span)
                    .and_then(|()| resolver(kind, value, span))
                    .map(ResolvedLink::plain)
            },
        )
    }

    /// Render an authored body with one exact semantic host for its component occurrences.
    /// This public utility retains the ordinary link resolver contract; full destination
    /// annotation remains owned by the authored projection that has workspace evidence.
    ///
    /// # Errors
    /// Refuses invalid host-independent syntax, unknown/schema-invalid components and
    /// ordinary rendering failures without exposing native source coordinates.
    pub fn render_with_hosted_components(
        &self,
        components: Option<ComponentPresentation<'_>>,
        host: &ComponentHost,
        mut resolver: impl FnMut(LinkKind, &str, Span) -> Result<Option<String>, Error>,
    ) -> Result<RenderedMarkdown, Error> {
        self.render_with_images(
            "",
            0,
            &BTreeMap::new(),
            components,
            Some(host),
            |form, kind, value, span| {
                standalone(form, span)
                    .and_then(|()| resolver(kind, value, span))
                    .map(ResolvedLink::plain)
            },
        )
    }

    /// Bind embedded images before general URL resolution; links cannot use this exception.
    pub(crate) fn render_with_images(
        &self,
        namespace: &str,
        heading_offset: u8,
        images: &BTreeMap<String, EmbeddedImage>,
        components: Option<ComponentPresentation<'_>>,
        host: Option<&ComponentHost>,
        resolver: impl FnMut(LinkForm, LinkKind, &str, Span) -> Result<ResolvedLink, Error>,
    ) -> Result<RenderedMarkdown, Error> {
        self.render_projection(
            Projection {
                container_contents: None,
                namespace,
                heading_offset,
                images,
                components,
                host,
                markdown_alternate: false,
            },
            resolver,
        )
    }

    /// Produce selected representations from one source-bound admission plan.
    pub(crate) fn render_projection(
        &self,
        projection: Projection<'_>,
        mut resolver: impl FnMut(LinkForm, LinkKind, &str, Span) -> Result<ResolvedLink, Error>,
    ) -> Result<RenderedMarkdown, Error> {
        let Projection {
            container_contents,
            namespace,
            heading_offset,
            images,
            components,
            host,
            markdown_alternate,
        } = projection;
        super::validate_namespace(namespace)?;
        if heading_offset > 5 {
            return Err(Error::InvalidOutput);
        }
        let syntax = self.document.syntax();
        let cut = self.renderable_summary_cut()?;
        for link in &syntax.links {
            if !(link.form == LinkForm::WikiLink && link.kind == LinkKind::Link
                || allowed_url(&link.destination, link.kind == LinkKind::Image)
                || link.kind == LinkKind::Image && images.contains_key(&link.destination))
            {
                return Err(Error::UnsafeUrl(link.location.span));
            }
        }
        self.check_adapter_rendering()?;
        // Omitted definitions still name selected publication targets. Validate
        // them through the same owner, without inventing emitted dependencies.
        for link in syntax
            .links
            .iter()
            .filter(|link| link.form == LinkForm::WikiLink && !link.emitted)
        {
            let resolved = resolver(link.form, link.kind, &link.destination, link.location.span)?;
            if !resolved
                .destination
                .as_deref()
                .is_some_and(|url| allowed_url(url, false))
            {
                return Err(Error::LinkResolution {
                    span: link.location.span,
                    reason: "symbolic reference requires a resolved publication destination",
                });
            }
        }
        let mut component_bindings = match components {
            Some(components) => components.definitions.bind(
                &syntax.directives,
                crate::toml::Limits {
                    source_bytes: self.limits.source_bytes,
                    nodes: self.limits.nodes,
                    depth: self.limits.depth,
                },
            )?,
            None if !syntax.directives.is_empty() => {
                return Err(Error::Adapter(gazette_core::markdown::Error::Unsupported {
                    location: syntax.directives[0].location,
                    reason: "components require a selected Theme definition and renderer",
                }));
            }
            None => Vec::new(),
        };
        let container_contents_placed = super::container_contents::bind(
            self,
            components,
            host,
            namespace,
            container_contents,
            &mut component_bindings,
        )?;
        if let Some(host) = host {
            let mut host_bytes =
                gazette_core::render::LimitedWriter::new(Vec::new(), self.limits.output_bytes);
            serde_json::to_writer(&mut host_bytes, host)
                .map_err(|_| Error::Limit("component host context bytes"))?;
            let exceeded = host_bytes.exceeded();
            let bytes = host_bytes.into_inner().len();
            if exceeded
                || bytes
                    .checked_mul(component_bindings.len())
                    .is_none_or(|bytes| bytes > self.limits.output_bytes)
            {
                return Err(Error::Limit("component host context bytes"));
            }
            for (index, binding) in component_bindings.iter_mut().enumerate() {
                binding.context = Some(ComponentContext {
                    address: occurrence_address(host, namespace, "component", index + 1),
                    occurrence: index + 1,
                    host: host.clone(),
                });
            }
        }
        let anchors = self.anchor_index()?;
        let namespaced = anchors.namespaced(namespace, syntax, self.limits.output_bytes)?;
        let mut budget = Budget(self.limits.output_bytes, "render plan string bytes");
        let mut links = Vec::new();
        let mut link_uses = Vec::new();
        let mut image_uses = Vec::new();
        let mut image_spans = Vec::new();
        let mut link_occurrence = 0usize;
        let initial_color_mode = components
            .map(|components| components.renderer.initial_color_mode())
            .unwrap_or_default();
        for link in syntax.links.iter().filter(|link| link.emitted) {
            if link.kind == LinkKind::Image
                && let Some(image) = images.get(&link.destination)
            {
                links.push(LinkBinding {
                    image_sources: None,
                    svg_layout: None,
                    image_width: None,
                    image_dimensions: None,
                    image_loading: None,
                    image_fetch_priority: None,
                    inline_svg: None,
                    image_address: None,
                    occurrence: link.clone(),
                    destination: budget.copy(&image.0)?,
                    annotation: None,
                });
                continue;
            }
            let resolved = resolver(link.form, link.kind, &link.destination, link.location.span)?;
            if link.form == LinkForm::WikiLink && resolved.destination.is_none() {
                return Err(Error::LinkResolution {
                    span: link.location.span,
                    reason: "symbolic reference requires a resolved publication destination",
                });
            }
            let effective = resolved.destination.as_deref().unwrap_or(&link.destination);
            if !allowed_url(effective, link.kind == LinkKind::Image) {
                return Err(Error::UnsafeUrl(link.location.span));
            }
            let destination =
                if namespace.is_empty() || !effective.starts_with('#') || effective == "#" {
                    budget.copy(effective)?
                } else {
                    let id = &effective[1..];
                    let target = namespaced
                        .get(id)
                        .map_or_else(|| format!("#{namespace}{id}"), |id| format!("#{id}"));
                    budget.charge(target.len())?;
                    target
                };
            let mut inline_svg = None;
            let mut svg_layout = None;
            let mut image_width = None;
            let mut image_dimensions = None;
            let mut image_loading = None;
            let mut image_fetch_priority = None;
            let mut responsive = None;
            let mut dark = None;
            let image_address = if link.kind == LinkKind::Image {
                let occurrence = image_uses.len() + 1;
                let address =
                    host.map(|host| occurrence_address(host, namespace, "image", occurrence));
                if let Some(address) = &address {
                    budget.charge(address.len().saturating_mul(2))?;
                }
                let alt = link.image_alt.as_deref().ok_or(Error::InvalidOutput)?;
                let (variants, width) = if let Some(image) = resolved.image {
                    svg_layout = image.svg;
                    image_width = image.width;
                    image_dimensions = image.intrinsic;
                    image_loading = image.loading;
                    image_fetch_priority = image.fetch_priority;
                    inline_svg = image.inline_svg;
                    responsive = image.responsive;
                    if let Some(sources) = &responsive {
                        if destination.contains('#') {
                            return Err(Error::LinkResolution {
                                span: link.location.span,
                                reason: "responsive image occurrences cannot select a fragment",
                            });
                        }
                        budget.charge(sources.sizes.len())?;
                        for candidate in sources
                            .default
                            .iter()
                            .chain(sources.light.iter().flatten())
                            .chain(sources.dark.iter().flatten())
                        {
                            if !allowed_url(&candidate.url, true) {
                                return Err(Error::UnsafeUrl(link.location.span));
                            }
                            budget.charge(candidate.url.len())?;
                        }
                    }
                    if inline_svg.is_some() && destination.contains(['?', '#']) {
                        return Err(Error::LinkResolution {
                            span: link.location.span,
                            reason: "inline SVG occurrences cannot select a query or fragment",
                        });
                    }
                    if let Some(svg) = &inline_svg {
                        budget.charge(svg.bytes())?;
                    }
                    if let Some(variants) = &image.variants {
                        for url in [&variants.light, &variants.dark].into_iter().flatten() {
                            if !allowed_url(url, true) {
                                return Err(Error::UnsafeUrl(link.location.span));
                            }
                            budget.charge(url.len())?;
                        }
                    }
                    (image.variants, image.width)
                } else {
                    (None, None)
                };
                if let Some(url) = variants.as_ref().and_then(|variants| variants.dark.clone()) {
                    // The URL stands alone in a source set, so it cannot carry a separator.
                    if url.contains(|character: char| character.is_ascii_whitespace())
                        || url.starts_with(',')
                        || url.ends_with(',')
                    {
                        return Err(Error::LinkResolution {
                            span: link.location.span,
                            reason: "dark image URL cannot contain a source-set separator",
                        });
                    }
                    dark = Some(gazette_core::markdown::DarkImageSource {
                        mode: initial_color_mode,
                        candidates: responsive
                            .as_ref()
                            .and_then(|sources: &super::render::ResponsiveImage| {
                                sources.dark.clone()
                            })
                            .unwrap_or_default(),
                        url,
                    });
                }
                image_spans.push(link.location.span);
                image_uses.push(ImageUse {
                    responsive: responsive.clone(),
                    svg: svg_layout,
                    loading: image_loading,
                    fetch_priority: image_fetch_priority,
                    intrinsic: image_dimensions,
                    caption: None,
                    presentation_only: component_bindings.iter().any(|binding| {
                        binding.presentation_only
                            && binding.occurrence.location.span.start <= link.location.span.start
                            && binding.occurrence.location.span.end >= link.location.span.end
                    }),
                    occurrence,
                    address: address.clone(),
                    destination: budget.copy(&destination)?,
                    variants,
                    width,
                    alt: budget.copy(alt)?,
                    title: (!link.title.is_empty())
                        .then(|| budget.copy(&link.title))
                        .transpose()?,
                });
                address
            } else {
                None
            };
            let annotation = if link.kind == LinkKind::Link {
                if let Some(annotation) = resolved.annotation {
                    let host = host.ok_or(Error::InvalidOutput)?;
                    link_occurrence += 1;
                    let address = occurrence_address(host, namespace, "link", link_occurrence);
                    let use_ = LinkUse {
                        address: address.clone(),
                        destination: destination.clone(),
                        kind: annotation.kind,
                        content: annotation.content,
                        site: annotation.site,
                        relationships: annotation.relationships,
                        presentation: annotation.presentation,
                    };
                    let formatter = LinkAnnotation {
                        address,
                        kind: use_.kind,
                        content: use_.content.clone(),
                        site: use_.site.clone(),
                        relationships: use_.relationships.clone(),
                        theme: use_
                            .presentation
                            .as_ref()
                            .map(|value| value.theme.as_key().clone()),
                        palette: use_
                            .presentation
                            .as_ref()
                            .and_then(|value| value.palette.clone()),
                    };
                    link_uses.push(use_);
                    Some(formatter)
                } else {
                    None
                }
            } else {
                None
            };
            links.push(LinkBinding {
                image_sources: (responsive.is_some() || dark.is_some()).then(|| {
                    let (sizes, candidates) = responsive
                        .map(|sources| (sources.sizes, sources.default))
                        .unwrap_or_default();
                    gazette_core::markdown::ImageSources {
                        sizes,
                        candidates,
                        dark,
                    }
                }),
                inline_svg,
                image_loading,
                image_fetch_priority,
                svg_layout,
                image_width,
                image_dimensions,
                image_address,
                occurrence: link.clone(),
                destination,
                annotation,
            });
        }
        let headings = anchors
            .bindings
            .iter()
            .map(|binding| {
                let id = format!("{namespace}{}", binding.id);
                budget.charge(id.len())?;
                Ok(HeadingBinding {
                    occurrence: binding.occurrence.clone(),
                    id,
                    level: binding.level.saturating_add(heading_offset).min(6),
                })
            })
            .collect::<Result<_, Error>>()?;
        let footnotes = anchors
            .footnotes
            .iter()
            .map(|binding| {
                let alias = format!("{namespace}{}", binding.alias);
                budget.charge(alias.len())?;
                Ok(FootnoteBinding {
                    occurrence: binding.occurrence.clone(),
                    alias,
                })
            })
            .collect::<Result<_, Error>>()?;
        let excerpt = cut
            .map(|cut| -> Result<Excerpt, Error> {
                let continuation = format!(
                    "{namespace}{}",
                    anchors.continuation.as_ref().ok_or(Error::InvalidOutput)?
                );
                budget.charge(continuation.len())?;
                Ok(Excerpt {
                    marker: cut.marker.clone(),
                    continuation,
                })
            })
            .transpose()?;
        let plan = Plan {
            identity: syntax.identity,
            headings,
            links,
            footnotes,
            excerpt,
            math: self.math.clone(),
            components: component_bindings,
        };
        let collector = components
            .map(|selected| {
                super::faq::Collector::new(selected, &plan.components, self.limits.output_bytes)
            })
            .transpose()?;
        let figures = components
            .map(|selected| {
                super::figures::Collector::new(
                    ComponentPresentation {
                        renderer: collector
                            .as_ref()
                            .ok_or(gazette_core::markdown::Error::InvalidPlan)?,
                        ..selected
                    },
                    &plan.components,
                    &plan.links,
                    self.limits.output_bytes,
                )
            })
            .transpose()?;
        let renderer = figures
            .as_ref()
            .map(|value| value as &dyn gazette_ports::markdown::ComponentRenderer);
        let rendered = self.document.render(&plan, renderer).map_err(Error::from)?;
        let mut captions = figures
            .map(super::figures::Collector::finish)
            .transpose()?
            .unwrap_or_default();
        for (span, image) in image_spans.into_iter().zip(&mut image_uses) {
            image.caption = captions.remove(&span);
        }
        // Embedded notebook attachments have no public ImageUse. Their admitted
        // caption still renders once, without publishing a data URI as metadata.
        if captions.keys().any(|span| {
            !plan.links.iter().any(|link| {
                link.occurrence.location.span == *span
                    && images.contains_key(&link.occurrence.destination)
            })
        }) {
            return Err(Error::InvalidOutput);
        }
        let mut faqs = collector
            .map(super::faq::Collector::finish)
            .transpose()?
            .unwrap_or_default();
        let body_markdown = markdown_alternate
            .then(|| {
                self.document
                    .render_markdown(&plan, components.map(|value| value.renderer))
                    .map_err(Error::from)
            })
            .transpose()?;
        let summary = match (&plan.excerpt, rendered.excerpt) {
            (Some(cut), Some(fragment)) => Some(Summary {
                html: fragment.html,
                text: fragment.text,
                marker: cut.marker.location.span,
                marker_precision: cut.marker.location.precision,
                continuation: format!("#{}", cut.continuation),
            }),
            (None, None) => None,
            _ => return Err(Error::InvalidOutput),
        };
        Ok(RenderedMarkdown {
            container_contents_placed,
            opening: rendered.opening,
            body_markdown,
            components: plan
                .components
                .into_iter()
                .map(|binding| {
                    let definition = components
                        .and_then(|selected| selected.definitions.definition(&binding.name))
                        .ok_or(Error::InvalidOutput)?;
                    Ok(ComponentUse {
                        faq: faqs.remove(&binding.occurrence.location.span),
                        context: binding.context,
                        name: binding.name,
                        arguments: binding.arguments,
                        interaction: definition.interaction.clone(),
                    })
                })
                .collect::<Result<_, Error>>()?,
            links: link_uses,
            images: image_uses,
            body_html: rendered.body.html,
            body_text: rendered.body.text,
            summary,
        })
    }
}

fn occurrence_address(
    host: &ComponentHost,
    namespace: &str,
    kind: &str,
    occurrence: usize,
) -> String {
    if namespace.is_empty() {
        format!("{}#{kind}-{occurrence}", host.content)
    } else {
        format!("{}#{kind}-{namespace}{occurrence}", host.content)
    }
}

// A conservative scheme/character gate, not URL validity or destination admission.
pub(crate) fn allowed_url(url: &str, image: bool) -> bool {
    if url.chars().any(|c| c.is_ascii_control() || c == '\\') {
        return false;
    }
    let url = url.trim_matches(' ');
    let Some(colon) = url.find(':') else {
        return true;
    };
    if url[..colon].contains(['/', '?', '#']) {
        return true;
    }
    let scheme = &url[..colon];
    scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("http")
        || (!image && scheme.eq_ignore_ascii_case("mailto"))
}
