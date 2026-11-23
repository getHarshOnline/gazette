//! Retained Swagger 2 input becomes a validated Mintlify `OpenAPI` artifact.
//!
//! Conversion reuses swagger2openapi 0.2.0 (BSD-3-Clause). The focused target
//! corrections preserve media alternatives and OAuth URLs that its conversion
//! otherwise drops; no native source, resolver or generator is invoked here.

mod fields;
mod forms;
mod security;

use crate::{Error, invalid, openapi, output};
use swagger2openapi::{Options, RefSiblings};

pub(super) fn convert(
    bytes: &[u8],
    context: &str,
    yaml: bool,
    ceiling: u64,
) -> Result<Vec<u8>, Error> {
    let original = openapi::parse(bytes, context, yaml)?;
    openapi::validate_swagger(&original, context)?;
    let mut input = original.clone();
    let literals = fields::prepare(&original, &mut input, context)?;
    let names = security::prepare(&mut input, context)?;
    forms::prepare(&mut input);
    let mut options = Options::new();
    options.patch = false;
    options.warn_only = false;
    options.resolve = false;
    options.resolve_internal = false;
    options.origin = false;
    options.anchors = false;
    options.target_version = Some("3.0.3".into());
    options.ref_siblings = RefSiblings::Preserve;
    options.rbname = "x-gazette-body-name".into();
    swagger2openapi::convert_obj(&input, &mut options).map_err(|_| {
        invalid(
            context,
            "Swagger conversion failed; no partial artifact accepted",
        )
    })?;
    let mut converted = options.openapi;
    security::restore(&original, &mut converted, &names, context)?;
    forms::restore(&original, &mut converted, context)?;
    literals.restore(&mut converted)?;
    openapi::validate_value(&converted, context)?;
    output::json(&converted, context, ceiling)
}
