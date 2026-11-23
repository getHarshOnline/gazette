//! Private semantic review checks selected definitions but never executes Theme templates.

use gazette_compiler::toml;
use gazette_core::{
    markdown::{ComponentBinding, Error, Fragment},
    publication::Key,
};
use gazette_ports::markdown::ComponentRenderer;

use super::{
    super::{AcquiredInputs, CompositionMetadata, MetadataError},
    ReviewLimits,
};

pub(super) struct SemanticChildren;

impl ComponentRenderer for SemanticChildren {
    fn render(&self, _: &ComponentBinding, body: &Fragment, _: bool) -> Result<String, Error> {
        // The compiler has already checked identity and typed arguments. Review retains
        // those structured uses separately; this is not a successful Theme rendering.
        Ok(body.html.clone())
    }
}

pub(super) fn metadata<'a>(
    inputs: &'a AcquiredInputs,
    site: &Key,
    limits: ReviewLimits,
) -> Result<Option<CompositionMetadata<'a>>, MetadataError> {
    let workspace = &inputs.compilation().workspace;
    let Some(_) = workspace
        .composition_for(site)
        .and_then(|composition| composition.theme())
    else {
        return Ok(None);
    };
    if inputs.site() != site {
        return Err(MetadataError::Binding {
            owner: site.to_string(),
            reason: "semantic review site does not match the acquired package composition",
        });
    }
    let metadata = inputs.check_metadata(toml::Limits {
        source_bytes: limits.bytes,
        nodes: limits.uses,
        ..Default::default()
    })?;
    Ok(Some(metadata))
}
