//! Captions bind to source image occurrences through explicit component scopes.

use std::{collections::BTreeMap, sync::Mutex};

use gazette_core::{
    markdown::{ComponentBinding, Error, Fragment, LinkBinding, LinkKind},
    source::Span,
};
use gazette_ports::markdown::ComponentRenderer;

use super::ComponentPresentation;
use crate::theme::ComponentSemantic;

struct Figure<'a> {
    binding: &'a ComponentBinding,
    caption: Option<Span>,
}
struct Captured {
    remaining: usize,
    captions: BTreeMap<Span, String>,
}

pub(super) struct Collector<'a> {
    selected: ComponentPresentation<'a>,
    /// Caption occurrence to its sole admitted image occurrence.
    images: BTreeMap<Span, Span>,
    captured: Mutex<Captured>,
}

fn failure(binding: &ComponentBinding, reason: &'static str) -> Error {
    Error::Unsupported {
        location: binding.occurrence.location,
        reason,
    }
}
fn encloses(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

impl<'a> Collector<'a> {
    pub(super) fn new(
        selected: ComponentPresentation<'a>,
        bindings: &'a [ComponentBinding],
        links: &[LinkBinding],
        maximum: usize,
    ) -> Result<Self, Error> {
        let mut figures: BTreeMap<Span, Figure<'a>> = BTreeMap::new();
        let mut ordered: Vec<_> = bindings.iter().collect();
        ordered.sort_by_key(|binding| binding.occurrence.location.span);
        let mut stack: Vec<(&ComponentBinding, Option<&ComponentSemantic>)> = Vec::new();
        let mut remaining = maximum;
        for binding in ordered {
            let span = binding.occurrence.location.span;
            while stack
                .last()
                .is_some_and(|(parent, _)| !encloses(parent.occurrence.location.span, span))
            {
                stack.pop();
            }
            let semantic = selected
                .definitions
                .definition(&binding.name)
                .ok_or(Error::InvalidPlan)?
                .semantic
                .as_ref();
            let figure_role = semantic.filter(|role| {
                matches!(
                    role,
                    ComponentSemantic::Figure | ComponentSemantic::FigureCaption
                )
            });
            let parent = stack.iter().rev().find(|(_, role)| role.is_some());
            if (figure_role.is_some() && stack.iter().any(|(parent, _)| parent.presentation_only))
                || (binding.presentation_only && parent.is_some())
            {
                return Err(failure(
                    binding,
                    "figures and captions cannot contain or belong to presentation-only components",
                ));
            }
            if semantic.is_some()
                && matches!(parent, Some((_, Some(ComponentSemantic::FigureCaption))))
            {
                return Err(failure(
                    binding,
                    "figure captions cannot contain semantic components",
                ));
            }
            match figure_role {
                Some(ComponentSemantic::Figure) => {
                    if parent.is_some() {
                        return Err(failure(binding, "declared figures cannot be nested"));
                    }
                    remaining = remaining
                        .checked_sub(std::mem::size_of::<Figure<'_>>())
                        .ok_or(Error::Limit("figure projection bytes"))?;
                    figures.insert(
                        span,
                        Figure {
                            binding,
                            caption: None,
                        },
                    );
                }
                Some(ComponentSemantic::FigureCaption) => {
                    let Some((parent, Some(ComponentSemantic::Figure))) = parent else {
                        return Err(failure(
                            binding,
                            "figure captions require an enclosing declared figure",
                        ));
                    };
                    let figure = figures
                        .get_mut(&parent.occurrence.location.span)
                        .ok_or(Error::InvalidPlan)?;
                    if figure.caption.replace(span).is_some() {
                        return Err(failure(
                            binding,
                            "a declared figure requires exactly one caption",
                        ));
                    }
                }
                _ => {}
            }
            stack.push((binding, figure_role));
        }
        let mut images = BTreeMap::new();
        for (span, figure) in figures {
            let caption = figure.caption.ok_or_else(|| {
                failure(
                    figure.binding,
                    "a declared figure requires exactly one caption",
                )
            })?;
            let mut candidates = links.iter().filter(|link| {
                link.occurrence.kind == LinkKind::Image
                    && encloses(span, link.occurrence.location.span)
            });
            let image = candidates.next().ok_or_else(|| {
                failure(
                    figure.binding,
                    "a declared figure requires exactly one emitted image",
                )
            })?;
            if candidates.next().is_some() || encloses(caption, image.occurrence.location.span) {
                return Err(failure(
                    figure.binding,
                    "a declared figure requires one image outside its caption",
                ));
            }
            remaining = remaining
                .checked_sub(std::mem::size_of::<(Span, Span)>())
                .ok_or(Error::Limit("figure projection bytes"))?;
            images.insert(caption, image.occurrence.location.span);
        }
        Ok(Self {
            selected,
            images,
            captured: Mutex::new(Captured {
                remaining,
                captions: BTreeMap::new(),
            }),
        })
    }

    pub(super) fn finish(self) -> Result<BTreeMap<Span, String>, Error> {
        let mut captured = self.captured.into_inner().map_err(|_| Error::InvalidPlan)?;
        let mut result = BTreeMap::new();
        for (caption, image) in self.images {
            result.insert(
                image,
                captured
                    .captions
                    .remove(&caption)
                    .ok_or(Error::InvalidPlan)?,
            );
        }
        if !captured.captions.is_empty() {
            return Err(Error::InvalidPlan);
        }
        Ok(result)
    }
}

impl ComponentRenderer for Collector<'_> {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error> {
        let output = self.selected.renderer.render(binding, body, excerpt)?;
        if !excerpt && self.images.contains_key(&binding.occurrence.location.span) {
            if body.text.trim().is_empty() {
                return Err(failure(
                    binding,
                    "figure captions require nonempty admitted reading text",
                ));
            }
            let mut captured = self.captured.lock().map_err(|_| Error::InvalidPlan)?;
            captured.remaining = captured
                .remaining
                .checked_sub(body.text.len())
                .ok_or(Error::Limit("figure projection bytes"))?;
            if captured
                .captions
                .insert(binding.occurrence.location.span, body.text.clone())
                .is_some()
            {
                return Err(Error::InvalidPlan);
            }
        }
        Ok(output)
    }
}

/// Check that a figure template preserves its admitted body inside the declared element.
/// Inert wrapper attributes and surrounding whitespace are allowed; body markup stays intact.
///
/// # Errors
/// Refuses changed content, an invalid or active wrapper, or oversized output.
pub fn validate_figure_html(
    binding: &ComponentBinding,
    semantic: &ComponentSemantic,
    body: &Fragment,
    rendered: &str,
    limits: crate::html::Limits,
) -> Result<(), Error> {
    let (opening, closing) = match semantic {
        ComponentSemantic::Figure => ("<figure", "</figure>"),
        ComponentSemantic::FigureCaption => ("<figcaption", "</figcaption>"),
        _ => return Err(Error::InvalidPlan),
    };
    if rendered.len() > limits.input_bytes || body.html.len() > limits.input_bytes {
        return Err(Error::Limit("figure template bytes"));
    }
    let changed = || {
        failure(
            binding,
            "figure template must preserve its admitted body inside its declared element",
        )
    };
    let rendered = rendered.trim();
    let rest = wrapper_body(rendered, opening).ok_or_else(changed)?;
    let inner = rest.strip_suffix(closing).ok_or_else(changed)?;
    if inner.trim() != body.html.trim() {
        return Err(changed());
    }
    // Markdown, MathML and inline SVG have already admitted the body through
    // their own profiles. Check only the template's new wrapper, preserving the
    // body byte-for-byte rather than re-admitting it as generic Theme SVG.
    let header = &rendered[..rendered.len() - rest.len()];
    let size = header
        .len()
        .checked_add(closing.len())
        .and_then(|size| size.checked_add(1))
        .filter(|size| *size <= limits.input_bytes)
        .ok_or(Error::Limit("figure template bytes"))?;
    let mut wrapper = String::with_capacity(size);
    wrapper.push_str(header);
    wrapper.push('x');
    wrapper.push_str(closing);
    crate::html::inspect(wrapper.as_bytes(), limits, |kind, _| match kind {
        crate::html::TextKind::Text | crate::html::TextKind::Attribute => Ok(()),
        _ => Err(()),
    })
    .map_err(|_| {
        failure(
            binding,
            "figure template wrapper is invalid, active or exceeds its HTML bounds",
        )
    })?;
    Ok(())
}

// The native HTML inspector validates the wrapper's syntax. This narrow scan locates
// the opening tag's boundary without mistaking quoted attribute text for it.
fn wrapper_body<'a>(rendered: &'a str, opening: &str) -> Option<&'a str> {
    let rest = rendered.strip_prefix(opening)?;
    // HTML tag names end only at ASCII HTML whitespace or the closing bracket.
    // Unicode spaces and longer names must not impersonate the declared wrapper.
    if !matches!(
        rest.as_bytes().first()?,
        b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c'
    ) {
        return None;
    }
    let mut quote = None;
    for (index, byte) in rest.bytes().enumerate() {
        match (quote, byte) {
            (Some(delimiter), byte) if delimiter == byte => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return rest.get(index + 1..),
            _ => {}
        }
    }
    None
}
