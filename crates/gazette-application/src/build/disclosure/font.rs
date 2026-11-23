//! Complete WOFF2 decode joins strict disclosure without changing retained output bytes.

use gazette_core::{font::Error, source::EntryPath};

use super::{DisclosureDecoding, DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<DisclosureDecoding, DisclosureProbeError> {
    let Some(inspect) = scan.font else {
        scan.unsupported(path, "WOFF2 font")?;
        return Ok(DisclosureDecoding::None);
    };
    let mut limits = scan.limits.font;
    limits.decoded_bytes = limits
        .decoded_bytes
        .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes));
    let failure = |error| match error {
        Error::Unsupported(_) => DisclosureProbeError::Unsupported {
            path: path.clone(),
            kind: "WOFF2 font",
        },
        Error::Limit(_) => DisclosureProbeError::Limit("WOFF2 font"),
        Error::Decode => DisclosureProbeError::Font {
            path: path.clone(),
            reason: "container or required metadata is malformed",
        },
        Error::Binding => DisclosureProbeError::Font {
            path: path.clone(),
            reason: "provider evidence does not bind input",
        },
    };
    let decoded = inspect(bytes, limits).map_err(failure)?;
    decoded.validate(bytes, limits).map_err(failure)?;
    scan.decoded(decoded.bytes.len(), 1)?;
    scan.search(path, &decoded.bytes, DisclosureRepresentation::FontDecoded)?;
    Ok(DisclosureDecoding::Woff2Font {
        profile: decoded.inspection.tool,
        tables: decoded.inspection.tables,
        glyphs: decoded.inspection.glyphs,
    })
}
