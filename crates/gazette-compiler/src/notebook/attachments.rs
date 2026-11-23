//! Cell-local embedded images reuse the stored-raster decoder and shared Markdown plan.

use std::collections::BTreeMap;

use gazette_core::raster;
use serde_json::Value;

use super::{RenderError, render::multiline};
use crate::markdown::EmbeddedImage;

pub(super) fn prepare(
    cell: &Value,
    index: usize,
    source_bytes: usize,
    inspect: &raster::Inspect<'_>,
    evidence: &mut Vec<raster::Inspection>,
) -> Result<BTreeMap<String, EmbeddedImage>, RenderError> {
    let mut images = BTreeMap::new();
    let Some(attachments) = cell.get("attachments").and_then(Value::as_object) else {
        return Ok(images);
    };
    for (name, bundle) in attachments {
        let bundle = bundle.as_object().ok_or(RenderError::Structure)?;
        let unsupported = || RenderError::Unsupported {
            cell: Some(index),
            feature: "attachment representation (requires one PNG or JPEG)",
        };
        if bundle.len() != 1 {
            return Err(unsupported());
        }
        let (mime, value) = bundle.iter().next().ok_or_else(unsupported)?;
        let format = match mime.as_str() {
            "image/png" => raster::Format::Png,
            "image/jpeg" => raster::Format::Jpeg,
            _ => return Err(unsupported()),
        };
        let encoded = multiline(value, source_bytes)?;
        let (bytes, image) = super::raster::decode(&encoded, format, source_bytes, inspect)
            .map_err(|error| RenderError::Raster { cell: index, error })?;
        let embedded = EmbeddedImage::new(&bytes, &image)
            .map_err(|error| RenderError::Raster { cell: index, error })?;
        images.insert(format!("attachment:{name}"), embedded);
        evidence.push(image);
    }
    Ok(images)
}
