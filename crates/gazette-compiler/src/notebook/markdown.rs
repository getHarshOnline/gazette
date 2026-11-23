//! One Markdown admission/resolution path and node budget for cells and stored outputs.

use std::collections::{BTreeMap, BTreeSet};

use crate::markdown::{self, EmbeddedImage, ParsedMarkdown, RenderedMarkdown};
use gazette_core::source::Span;

use super::RenderError;
use crate::corpus::MathLocation;

pub(super) type Resolve<'a> = dyn FnMut(
        markdown::LinkForm,
        markdown::LinkKind,
        &str,
        Span,
    ) -> Result<markdown::ResolvedLink, markdown::Error>
    + 'a;

pub(super) struct Projection<'a> {
    limits: markdown::Limits,
    nodes: usize,
    adapter: &'a dyn gazette_ports::markdown::Markdown,
    components: Option<markdown::ComponentPresentation<'a>>,
    host: Option<&'a gazette_core::markdown::ComponentHost>,
    resolve: &'a mut Resolve<'a>,
    profile: gazette_core::markdown::Profile,
    fragments: &'a BTreeMap<MathLocation, super::super::math::Fragment>,
    seen: BTreeSet<MathLocation>,
    heading_offset: u8,
}

impl<'a> Projection<'a> {
    pub(super) fn new(
        limits: markdown::Limits,
        adapter: &'a dyn gazette_ports::markdown::Markdown,
        components: Option<markdown::ComponentPresentation<'a>>,
        host: Option<&'a gazette_core::markdown::ComponentHost>,
        resolve: &'a mut Resolve<'a>,
        notebook: &'a super::super::ParsedNotebook<'_>,
        heading_offset: u8,
    ) -> Self {
        Self {
            limits,
            nodes: limits.nodes,
            adapter,
            components,
            host,
            resolve,
            profile: notebook.profile,
            fragments: &notebook.fragments,
            seen: BTreeSet::new(),
            heading_offset,
        }
    }

    pub(super) fn render(
        &mut self,
        source: &str,
        prefix: &str,
        images: &BTreeMap<String, EmbeddedImage>,
        location: MathLocation,
    ) -> Result<RenderedMarkdown, RenderError> {
        let cell = match location {
            MathLocation::NotebookCell { cell } | MathLocation::NotebookOutput { cell, .. } => cell,
            MathLocation::Body => return Err(RenderError::Structure),
        };
        let failure = |error| RenderError::Markdown {
            cell: Some(cell),
            error,
        };
        let parsed = ParsedMarkdown::parse_with_profile(
            source,
            self.profile,
            markdown::Limits {
                nodes: self.nodes,
                ..self.limits
            },
            self.adapter,
        )
        .map_err(failure)?;
        self.nodes = self
            .nodes
            .checked_sub(parsed.node_count())
            .ok_or(RenderError::Limit("Markdown nodes"))?;
        let parsed = if let Some(fragment) = self.fragments.get(&location) {
            if !self.seen.insert(location) {
                return Err(RenderError::Structure);
            }
            parsed
                .with_math(fragment.identity, fragment.bindings.clone())
                .map_err(failure)?
        } else if !self.fragments.is_empty() {
            return Err(RenderError::Structure);
        } else {
            parsed
        };
        parsed
            .render_with_images(
                prefix,
                self.heading_offset,
                images,
                self.components,
                self.host,
                |form, kind, destination, span| (self.resolve)(form, kind, destination, span),
            )
            .map_err(failure)
    }

    pub(super) fn finish(self) -> Result<(), RenderError> {
        if self.seen.len() == self.fragments.len() {
            Ok(())
        } else {
            Err(RenderError::Structure)
        }
    }
}
