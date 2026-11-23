//! Container qualification joins the same bounded scanner; no image bytes are rewritten.

use gazette_core::{
    raster::{Error, Format},
    source::EntryPath,
};

use super::{DisclosureDecoding, DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    format: Format,
    bytes: &[u8],
) -> Result<DisclosureDecoding, DisclosureProbeError> {
    let Some(inspect) = scan.metadata else {
        scan.unsupported(path, "raster metadata")?;
        return Ok(DisclosureDecoding::None);
    };
    let mut limits = scan.limits.raster;
    limits.decoded_bytes = limits
        .decoded_bytes
        .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes));
    limits.values = limits
        .values
        .min(scan.limits.encoded_values.saturating_sub(scan.values));
    let failure = |error| match error {
        Error::Unsupported(_) => DisclosureProbeError::Unsupported {
            path: path.clone(),
            kind: "raster metadata",
        },
        Error::Limit(_) => DisclosureProbeError::Limit("raster metadata"),
        Error::Decode => DisclosureProbeError::Raster {
            path: path.clone(),
            reason: "container is malformed",
        },
        Error::Binding => DisclosureProbeError::Raster {
            path: path.clone(),
            reason: "provider evidence does not bind input",
        },
    };
    let metadata = inspect(format, bytes, limits).map_err(failure)?;
    metadata.validate(format, bytes, limits).map_err(failure)?;
    scan.decoded(metadata.decoded_bytes, metadata.fields.len())?;
    for field in &metadata.fields {
        scan.text(
            path,
            field.as_bytes(),
            DisclosureRepresentation::RasterMetadata,
        )?;
    }
    Ok(DisclosureDecoding::RasterMetadata {
        format,
        profile: metadata.tool,
    })
}
