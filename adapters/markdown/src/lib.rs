//! Upstream Markdown mechanics over retained bytes, without publication policy or native effects.
//!
//! Parser arenas live for one operation only. Analysis preserves whole-source
//! evidence; rendering normalizes a new tree from the identical retained source.

use gazette_core::markdown::{
    Error, HeadingBinding, Identity, Limits, LinkBinding, Plan, Profile, Rendered, Syntax,
};
use gazette_core::source::{Location, Sha256Digest};
use gazette_ports::markdown::{ComponentRenderer, Document, Markdown};

mod alerts;
mod bounded;
mod container_contents;
mod highlight;
mod html_markdown;
mod html_transport;
mod html_tree;
mod inspect;
pub mod math;
mod mdx;
mod mdx_text;
mod plan;
mod render;
mod rewrite;
mod semantics;
mod source;

/// Pinned retained-source Markdown behavior used by incremental dependency identity.
pub const PROFILE: &str =
    "comrak@0.54.0/syntect@5.3.0/two-face@0.5.2+bat-0.26.1/gazette-publication-markdown-v13";

/// Stateless Comrak implementation of Gazette's bounded Markdown port.
#[derive(Clone, Copy, Debug, Default)]
pub struct Comrak;

struct Retained {
    source: String,
    syntax: Syntax,
    limits: Limits,
}

impl Markdown for Comrak {
    fn highlight_code(
        &self,
        code: &str,
        language: &str,
        limits: Limits,
    ) -> Result<Option<String>, Error> {
        use comrak::adapters::SyntaxHighlighterAdapter;
        if code.len().saturating_add(language.len()) > limits.source_bytes {
            return Err(Error::Limit("source bytes"));
        }
        let mut output = bounded::Output::new(limits.output_bytes);
        highlight::Highlight
            .write_highlighted(&mut output, Some(language), code)
            .map_err(|_| output.error())?;
        Ok(Some(output.value))
    }

    fn profile(&self) -> &'static str {
        PROFILE
    }

    fn transport_static_html(
        &self,
        html: &str,
        location: Location,
        limits: Limits,
    ) -> Result<String, Error> {
        html_transport::encode(html, limits, location)
    }

    fn reading_static_html(
        &self,
        html: &str,
        location: Location,
        limits: Limits,
    ) -> Result<String, Error> {
        html_markdown::encode(html, limits, location)
    }

    fn parse(
        &self,
        source: &str,
        profile: Profile,
        limits: Limits,
    ) -> Result<Box<dyn Document>, Error> {
        if source.len() > limits.source_bytes {
            return Err(Error::Limit("source bytes"));
        }
        let identity = Identity {
            source_sha256: Sha256Digest::of(source.as_bytes()),
            profile,
        };
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, source, &options(profile, true));
        let syntax = inspect::analyze(root, source, identity, limits)?;
        Ok(Box::new(Retained {
            source: source.to_owned(),
            syntax,
            limits,
        }))
    }
}

impl Document for Retained {
    fn syntax(&self) -> &Syntax {
        &self.syntax
    }

    fn check_rendering(&self) -> Result<(), Error> {
        plan::check_source(&self.syntax)
    }

    fn render(
        &self,
        plan: &Plan,
        components: Option<&dyn ComponentRenderer>,
    ) -> Result<Rendered, Error> {
        let bindings = plan::validate(&self.syntax, plan, self.limits)?;
        render::document(
            &self.source,
            &self.syntax,
            plan,
            bindings,
            components,
            self.limits,
        )
    }

    fn rewrite_links(&self, identity: Identity, links: &[LinkBinding]) -> Result<String, Error> {
        rewrite::document(&self.source, &self.syntax, identity, links, self.limits)
    }

    fn transport_mdx(
        &self,
        identity: Identity,
        links: &[LinkBinding],
        headings: &[HeadingBinding],
        title: Option<&HeadingBinding>,
    ) -> Result<String, Error> {
        rewrite::mdx(
            &self.source,
            &self.syntax,
            identity,
            links,
            mdx::Projection {
                headings,
                title,
                anchors: None,
            },
            self.limits,
        )
    }

    fn transport_mdx_with_anchors(
        &self,
        identity: Identity,
        links: &[LinkBinding],
        headings: &[HeadingBinding],
        title: Option<&HeadingBinding>,
        anchors: &gazette_core::documentation::projection::AnchorProjection,
    ) -> Result<String, Error> {
        rewrite::mdx(
            &self.source,
            &self.syntax,
            identity,
            links,
            mdx::Projection {
                headings,
                title,
                anchors: Some(anchors),
            },
            self.limits,
        )
    }

    fn project_mdx(
        &self,
        identity: Identity,
        links: &[LinkBinding],
        headings: &[HeadingBinding],
        title: Option<&HeadingBinding>,
        anchors: Option<&gazette_core::documentation::projection::AnchorProjection>,
    ) -> Result<gazette_core::markdown::TransportedMarkdown, Error> {
        rewrite::project_mdx(
            &self.source,
            &self.syntax,
            identity,
            links,
            mdx::Projection {
                headings,
                title,
                anchors,
            },
            self.limits,
        )
    }

    fn render_markdown(
        &self,
        plan: &Plan,
        components: Option<&dyn ComponentRenderer>,
    ) -> Result<String, Error> {
        let bindings = plan::validate(&self.syntax, plan, self.limits)?;
        render::markdown(
            &self.source,
            &self.syntax,
            plan,
            bindings,
            components,
            self.limits,
        )
    }
}

fn options(profile: Profile, preserve: bool) -> comrak::Options<'static> {
    let mut options = comrak::Options::default();
    match profile {
        Profile::Publication | Profile::PublicationMath => {
            options.extension.table = true;
            options.extension.tasklist = true;
            options.extension.strikethrough = true;
            options.extension.footnotes = true;
            options.extension.header_attributes = true;
            options.extension.block_directive = true;
            options.extension.wikilinks_title_after_pipe = true;
        }
    }
    if profile == Profile::PublicationMath {
        options.extension.math_dollars = true;
        options.extension.math_code = true;
    }
    options.parse.leave_footnote_definitions = preserve;
    options
}
