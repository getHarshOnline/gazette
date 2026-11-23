//! One strict numeric/shape boundary before canonicalization or cryptographic work.

use super::{Error, Limits};
use gazette_core::render::LimitedWriter;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub(super) fn encode(value: &impl Serialize, limits: Limits) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), limits.bytes);
    let result = serde_json::to_writer(&mut output, value);
    if output.exceeded() {
        return Err(Error::Limit);
    }
    result.map_err(|_| Error::Shape)?;
    Ok(output.into_inner())
}

pub(super) fn decode<T: DeserializeOwned + Serialize>(
    bytes: &[u8],
    limits: Limits,
) -> Result<(T, Value), Error> {
    if bytes.len() > limits.bytes {
        return Err(Error::Limit);
    }
    let invalid_number = std::cell::Cell::new(false);
    let value = gazette_core::json::parse_with_number_validation(bytes, &|spelling| {
        // Check raw spelling before the native value decoder can normalize -0.
        // JCS converts through binary64, so only exact safe integers reach it.
        let valid = spelling.parse::<u64>().is_ok_and(|integer| {
            integer <= 9_007_199_254_740_991 && integer.to_string() == spelling
        });
        invalid_number.set(!valid);
        valid
    })
    .map_err(|_| {
        if invalid_number.get() {
            Error::Number
        } else {
            Error::Json
        }
    })?;
    let mut remaining = limits.nodes;
    check_value(&value, 1, &mut remaining, limits)?;
    let typed: T = serde_json::from_value(value.clone()).map_err(|_| Error::Shape)?;
    // Serde structs can deserialize positional arrays. Exact round-trip shape
    // rejects that alternate spelling, omitted defaults and discarded members.
    if serde_json::to_value(&typed).map_err(|_| Error::Shape)? != value {
        return Err(Error::Shape);
    }
    Ok((typed, value))
}

fn check_value(
    value: &Value,
    depth: usize,
    remaining: &mut usize,
    limits: Limits,
) -> Result<(), Error> {
    if depth > limits.depth || *remaining == 0 {
        return Err(Error::Limit);
    }
    *remaining -= 1;
    match value {
        Value::Array(values) => {
            for child in values {
                check_value(child, depth + 1, remaining, limits)?;
            }
        }
        Value::Object(values) => {
            for child in values.values() {
                check_value(child, depth + 1, remaining, limits)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn canonical(value: &Value, limits: Limits) -> Result<Vec<u8>, Error> {
    // JCS internally buffers object members. The input byte/node/depth checks,
    // not this output writer alone, bound the work supplied to that library.
    let mut output = LimitedWriter::new(Vec::new(), limits.bytes);
    let result = serde_jcs::to_writer(&mut output, value);
    if output.exceeded() {
        return Err(Error::Limit);
    }
    result.map_err(|_| Error::Shape)?;
    Ok(output.into_inner())
}

pub(super) fn body(mut value: Value, limits: Limits) -> Result<Vec<u8>, Error> {
    let body = value.as_object_mut().ok_or(Error::Shape)?;
    body.remove("canonical_body_sha256");
    let publishers = body
        .get_mut("publisher_authorizations")
        .and_then(Value::as_array_mut)
        .ok_or(Error::Shape)?;
    for publisher in publishers {
        publisher
            .as_object_mut()
            .ok_or(Error::Shape)?
            .remove("signature_envelope");
    }
    canonical(&value, limits)
}
