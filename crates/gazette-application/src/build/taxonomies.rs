//! Render a compiler-owned taxonomy listing without inventing an authored object.

use gazette_compiler::authored::ProjectedTaxonomyPage;
use gazette_core::{
    render,
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;
use serde_json::Value;

use super::{
    CompiledTheme, PageRenderError, SiteRenderError,
    presentation::Presentation,
    serialization::{context, encode},
};

#[derive(Serialize)]
struct Model<'a> {
    #[serde(flatten)]
    page: &'a gazette_compiler::authored::TaxonomyListing<'a>,
    presentation: &'a Presentation<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_ld: Option<Value>,
}

/// Private template/model/output binding for a generated taxonomy navigation page.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaxonomyRenderEvidence {
    /// Explicit retained Theme entry, not a discovered template name.
    pub template: EntryPath,
    /// Exact configured site-relative output path.
    pub output: EntryPath,
    /// Compact encoding with sorted object keys, shared by Theme and site JSON.
    pub model_sha256: Sha256Digest,
    /// Exact emitted UTF-8 HTML bytes.
    pub html_sha256: Sha256Digest,
}

pub(super) struct Rendered {
    pub model: Value,
    pub html: Vec<u8>,
    pub evidence: TaxonomyRenderEvidence,
}

pub(super) fn render(
    theme: &CompiledTheme,
    page: &ProjectedTaxonomyPage<'_>,
    presentation: &Presentation<'_>,
    publication: &gazette_compiler::workspace::BrandPublication,
    maximum: usize,
) -> Result<Rendered, SiteRenderError> {
    let maximum = maximum.min(theme.evidence().limits.context_bytes);
    let source = Model {
        page: &page.model,
        presentation,
        json_ld: publication.machine.json_ld.then(|| {
            let mut value = page.model.json_ld();
            if let Some(terms) = publication.terms_url() {
                value["usageInfo"] = serde_json::json!({ "@id": terms });
            }
            value
        }),
    };
    let encoded =
        encode(&source, maximum, "taxonomy model bytes").map_err(SiteRenderError::Render)?;
    let model: Value = serde_json::from_slice(&encoded)
        .map_err(|error| SiteRenderError::Render(render::Error::Context(Box::new(error))))?;
    let (model, encoded) =
        context(model, maximum, "taxonomy model bytes").map_err(SiteRenderError::Render)?;
    let html = theme
        .render(&page.template, &model)
        .map_err(SiteRenderError::Render)?;
    let html = String::from_utf8(html)
        .map_err(|error| SiteRenderError::Page(PageRenderError::Utf8(error)))?
        .into_bytes();
    let evidence = TaxonomyRenderEvidence {
        template: page.template.clone(),
        output: page.model.output.clone(),
        model_sha256: Sha256Digest::of(&encoded),
        html_sha256: Sha256Digest::of(&html),
    };
    Ok(Rendered {
        model,
        html,
        evidence,
    })
}
