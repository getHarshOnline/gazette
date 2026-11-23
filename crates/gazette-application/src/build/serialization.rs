//! One bounded JSON transport for page context and the assembled machine model.

use gazette_core::render::{Error, LimitedWriter};
use serde::Serialize;
use serde_json::Value;

/// Materialize one template context and the exact compact bytes used for its evidence.
/// Object order is explicit despite additive Serde feature unification; array order,
/// strings and exact native numbers are untouched. Inputs have already crossed their
/// typed byte ceiling. This is not signature canonicalization or a raw-record rewrite.
pub(super) fn context(
    mut value: Value,
    maximum: usize,
    resource: &'static str,
) -> Result<(Value, Vec<u8>), Error> {
    value.sort_all_objects();
    let bytes = encode(&value, maximum, resource)?;
    Ok((value, bytes))
}

pub(super) fn encode(
    value: &impl Serialize,
    maximum: usize,
    resource: &'static str,
) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    let result = serde_json::to_writer(&mut output, value);
    if output.exceeded() {
        return Err(Error::Limit(resource));
    }
    result.map_err(|error| Error::Context(Box::new(error)))?;
    Ok(output.into_inner())
}

#[cfg(test)]
#[path = "../../tests/internal/context.rs"]
mod tests;
