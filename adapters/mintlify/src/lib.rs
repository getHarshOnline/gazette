//! Assembles a documentation repository for Mintlify from authored manual pages and
//! generated reference, locally.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it, and it takes the artifacts that
//! `gazette-adapter-documentation` generates. Native generator artifacts remain
//! byte-preserved inputs. Target-specific validation, configuration and packaging live
//! here, not in the compiler. It invokes no generator and publishes nothing remotely.

mod assemble;
mod catalog;
mod configuration;
mod manual;
mod markdown;
mod openapi;
mod output;
mod rustdoc;
mod sphinx;
mod swagger;
mod typedoc;

use assemble::Config;
use gazette_core::{
    documentation::{
        Error, Plan, Request, Target,
        projection::{AuthoringClosure, AuthoringManualPage, AuthoringReference},
    },
    source::{CapturedSource, Limits},
};
use gazette_ports::{documentation::DocumentationTarget, markdown::Markdown};

fn target_anchors() -> Result<gazette_core::documentation::projection::AnchorProjection, Error> {
    // Observed shell IDs in the qualified provider; this is not a claim about
    // every future provider ID. Escape the namespace itself to stay injective.
    gazette_core::documentation::projection::AnchorProjection::new(
        "gazette-anchor-",
        &["content", "page-title"],
    )
}

// The qualified provider normalizes a trailing `index` in navigation. Keep
// distinct generated files distinct, and escape the namespace itself so that a
// source already named `gazette-page-index` cannot collide with `index`.
fn page_route(route: &str) -> String {
    gazette_core::documentation::Target::mintlify_page_route(route)
}

/// Selected local Mintlify target. No installation or hosted effect is implied.
pub struct Mintlify<'a> {
    markdown: &'a dyn Markdown,
    preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
}

impl<'a> Mintlify<'a> {
    /// Bind existing Markdown mechanics without reading sources or selecting publication policy.
    pub fn new(markdown: &'a dyn Markdown) -> Self {
        Self {
            markdown,
            preview_link_bases: None,
        }
    }

    /// Project ordinary manual hyperlinks into explicitly selected local previews.
    pub fn with_preview_link_bases(
        mut self,
        bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
    ) -> Self {
        self.preview_link_bases = bases;
        self
    }

    /// Validate retained authored-manual pages through the same route/link owner used
    /// by final assembly. Generated references remain explicitly incomplete because
    /// editor sessions do not carry their provider artifacts.
    ///
    /// # Errors
    ///
    /// Returns the shared documentation-target error when a retained manual source,
    /// route, anchor, or link fails exact closure validation.
    pub fn authoring_closure(
        &self,
        pages: &[AuthoringManualPage<'_>],
        references: &[AuthoringReference<'_>],
        limits: Limits,
    ) -> Result<AuthoringClosure, Error> {
        manual::authoring_closure(pages, references, limits, self.markdown)
    }
}

impl DocumentationTarget for Mintlify<'_> {
    fn preview_link_bases(&self) -> Option<&gazette_core::publication::routing::PreviewLinkBases> {
        self.preview_link_bases
    }
    fn preflight(&self, plan: Plan<'_>) -> Result<(), Error> {
        assemble::preflight(plan)
    }

    fn assemble(&self, request: Request<'_>, limits: Limits) -> Result<CapturedSource, Error> {
        self.assemble_with_catalog(
            request,
            limits,
            gazette_core::documentation::CatalogLimits::default(),
        )
        .map(|assembled| assembled.output)
    }

    fn assemble_with_catalog(
        &self,
        request: Request<'_>,
        limits: Limits,
        catalog_limits: gazette_core::documentation::CatalogLimits,
    ) -> Result<gazette_core::documentation::TargetAssembly, Error> {
        let Target::Mintlify {
            theme,
            primary_color,
            light_color,
            dark_color,
            telemetry,
        } = request.target;
        assemble::assemble(
            Config {
                name: request.name,
                public_base_url: request.public_base_url,
                theme,
                primary_color,
                light_color: light_color.as_deref(),
                dark_color: dark_color.as_deref(),
                telemetry: *telemetry,
                usage: request.usage,
                preview_link_bases: self.preview_link_bases,
            },
            request.projects,
            limits,
            self.markdown,
            catalog_limits,
        )
    }
}

fn invalid(context: impl Into<String>, reason: &'static str) -> Error {
    Error::Invalid {
        context: context.into(),
        reason,
    }
}
