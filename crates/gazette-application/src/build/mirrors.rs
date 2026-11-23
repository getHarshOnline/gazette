//! Redirect-only presentation over exact compiler-owned cross-site routes.

use gazette_compiler::authored::MirroredPage;
use gazette_core::{
    publication::Key,
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;
use serde_json::Value;

use super::{
    CompiledTheme, PageRenderError, SiteRenderError,
    presentation::{Presentation, Presented},
    serialization::{self, encode},
};

/// Shared redirect metadata. It carries no foreign body, authors or source paths.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct MirrorModel {
    /// Exact canonical content identity, not a new object owned by this site.
    pub content: Key,
    /// The unchanged home site of the canonical content object.
    pub canonical_site: Key,
    /// The site serving this explicitly declared redirect presentation.
    pub site: Key,
    /// Exact compiled serving address, not the canonical target.
    pub url: String,
    /// Minimal target envelope shared with the existing alias-template contract.
    pub route: MirrorRoute,
}

/// Canonical destination borrowed semantically from the route compiler.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct MirrorRoute {
    /// Exact target; no alias lookup, fallback or independent URL reconstruction.
    pub canonical_url: String,
}

/// Private evidence binds redirect meaning plus the complete presented model emitted by the site.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MirrorRenderEvidence {
    /// Exact redirect semantics, without copied authored meaning or presentation transport.
    pub model: MirrorModel,
    /// Explicitly selected redirect template.
    pub template: EntryPath,
    /// Compact presented-model encoding with sorted object keys, supplied to Theme and site JSON.
    pub model_sha256: Sha256Digest,
    /// Exact UTF-8 redirect output, not hosting or browser-behavior proof.
    pub html_sha256: Sha256Digest,
}

pub(super) fn render(
    theme: &CompiledTheme,
    selected: &MirroredPage<'_>,
    template: &EntryPath,
    maximum_model: usize,
    presentation: &Presentation<'_>,
) -> Result<(Vec<u8>, Value, MirrorRenderEvidence), SiteRenderError> {
    let model = MirrorModel {
        content: selected.node.id.clone(),
        canonical_site: selected.node.site.clone(),
        site: selected.mirror.site.clone(),
        url: selected.mirror.url.clone(),
        route: MirrorRoute {
            canonical_url: selected.route.canonical_url.clone(),
        },
    };
    let presented = Presented {
        value: &model,
        presentation,
    };
    let encoded =
        encode(&presented, maximum_model, "mirror model bytes").map_err(SiteRenderError::Render)?;
    let context = serde_json::from_slice(&encoded).map_err(|error| {
        SiteRenderError::Render(gazette_core::render::Error::Context(Box::new(error)))
    })?;
    let (context, encoded) = serialization::context(context, maximum_model, "mirror model bytes")
        .map_err(SiteRenderError::Render)?;
    let html = theme
        .render(template, &context)
        .map_err(SiteRenderError::Render)?;
    let html = String::from_utf8(html)
        .map_err(|error| SiteRenderError::Page(PageRenderError::Utf8(error)))?
        .into_bytes();
    let evidence = MirrorRenderEvidence {
        model,
        template: template.clone(),
        model_sha256: Sha256Digest::of(&encoded),
        html_sha256: Sha256Digest::of(&html),
    };
    Ok((html, context, evidence))
}
