//! Continuation transport over the same index context and compiler-owned reading slice.

use gazette_compiler::authored::ListingPage;
use gazette_core::{
    publication::Key,
    render,
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;
use serde_json::Value;

use super::{
    CompiledTheme, PageRenderError, SiteRenderError,
    serialization::{context, encode},
};

/// Private output proof; continuation views do not create authored identities.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListingRenderEvidence {
    /// Existing authored index identity.
    pub content: Key,
    /// Position of this output view in the complete listing.
    pub page_number: usize,
    /// Configured ordering view, independent from the existing Content identity.
    pub variant: gazette_compiler::workspace::ListingVariant,
    /// Exact selected retained Theme entry.
    pub template: EntryPath,
    /// Exact compiler-resolved output path.
    pub output: EntryPath,
    /// Digest of the complete shared rendering context.
    pub model_sha256: Sha256Digest,
    /// Digest of emitted UTF-8 bytes.
    pub html_sha256: Sha256Digest,
}

pub(super) struct Rendered {
    pub model: Value,
    pub html: Vec<u8>,
    pub evidence: ListingRenderEvidence,
}

pub(super) fn render(
    theme: &CompiledTheme,
    page: &ListingPage,
    owner: &Value,
    template: &EntryPath,
    maximum: usize,
    alternate: Option<&gazette_compiler::authored::MarkdownAlternate>,
) -> Result<Rendered, SiteRenderError> {
    if page.is_primary() || owner["content"]["id"].as_str() != Some(page.content.as_str()) {
        return Err(SiteRenderError::Configuration);
    }
    let maximum = maximum.min(theme.evidence().limits.context_bytes);
    // Charge the existing context before cloning it. Only transport is replaced;
    // identity, roles, presentation, resources and source meaning stay with its owner.
    encode(owner, maximum, "listing owner context bytes").map_err(SiteRenderError::Render)?;
    let mut model = owner.clone();
    // Exact page invitations belong to the authored index, not its generated slices.
    if let Some(fields) = model.as_object_mut() {
        fields.remove("invitations");
    }
    if let Some(presentation) = model.get_mut("presentation").and_then(Value::as_object_mut) {
        presentation.remove("markdown_alternate");
        presentation.remove("webmcp");
        if let Some(alternate) = alternate {
            presentation.insert(
                "markdown_alternate".into(),
                serde_json::to_value(alternate).map_err(encoding)?,
            );
        }
    } else {
        return Err(SiteRenderError::Configuration);
    }
    model["listing"] = serde_json::to_value(page).map_err(encoding)?;
    model["route"] = serde_json::to_value(page.serving_route()).map_err(encoding)?;
    if let Some(json_ld) = model.get_mut("json_ld") {
        let fields = serde_json::to_value(page.public_json_ld()).map_err(encoding)?;
        for field in ["@type", "@id", "identifier", "mainEntity", "url"] {
            json_ld[field] = fields[field].clone();
        }
    }
    let (model, bytes) =
        context(model, maximum, "listing context bytes").map_err(SiteRenderError::Render)?;
    let html = theme
        .render(template, &model)
        .map_err(SiteRenderError::Render)?;
    let html = String::from_utf8(html)
        .map_err(|error| SiteRenderError::Page(PageRenderError::Utf8(error)))?
        .into_bytes();
    let evidence = ListingRenderEvidence {
        content: page.content.clone(),
        page_number: page.page_number,
        variant: page.variant,
        template: template.clone(),
        output: page.output.clone(),
        model_sha256: Sha256Digest::of(&bytes),
        html_sha256: Sha256Digest::of(&html),
    };
    Ok(Rendered {
        model,
        html,
        evidence,
    })
}

fn encoding(error: serde_json::Error) -> SiteRenderError {
    SiteRenderError::Render(render::Error::Context(Box::new(error)))
}
