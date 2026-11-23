//! Recursion, aggregate ceilings and evidence belong to the one output probe.

use gazette_compiler::encoded::{self, Error, Kind};
use gazette_core::source::{EntryPath, Sha256Digest};

use super::{
    DisclosureDecoding, DisclosureEmbeddedCoverage, DisclosureProbeError, DisclosureRepresentation,
    Scan, format::Format,
};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<(), DisclosureProbeError> {
    let mut limits = scan.limits.encoded;
    limits.decoded_bytes = limits
        .decoded_bytes
        .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes));
    let exhausted =
        scan.depth >= scan.limits.encoded_depth || scan.values >= scan.limits.encoded_values;
    if exhausted {
        // Refuse nonempty payload allocation before recording a value. Native
        // URL/metadata scratch still consumes the separately bounded input.
        limits.decoded_bytes = 0;
    }
    let Some(decoded) = encoded::decode(bytes, limits).map_err(|error| match error {
        Error::Limit("input bytes") => DisclosureProbeError::Limit("encoded input bytes"),
        Error::Limit(_) => DisclosureProbeError::Limit("encoded decoded bytes"),
        Error::Malformed => DisclosureProbeError::Encoded {
            path: path.clone(),
            reason: "syntax is malformed",
        },
        Error::Unsupported => DisclosureProbeError::Encoded {
            path: path.clone(),
            reason: "metadata is unsupported",
        },
    })?
    else {
        return Ok(());
    };
    if exhausted {
        return Err(DisclosureProbeError::Limit("encoded depth or values"));
    }
    scan.decoded(decoded.bytes.len(), 1)?;
    let (representation, media_type, format) = match decoded.kind {
        Kind::Percent => (DisclosureRepresentation::PercentDecoded, None, Format::Raw),
        Kind::Data {
            media_type,
            charset,
        } => {
            let format = Format::for_media(&media_type);
            if matches!(format, Format::Raw) {
                scan.unsupported(path, "data payload")?;
            }
            if matches!(format, Format::Raster(_)) && charset.is_some() {
                return Err(DisclosureProbeError::Encoded {
                    path: path.clone(),
                    reason: "binary charset is unsupported",
                });
            }
            if !matches!(format, Format::Raw | Format::Raster(_)) {
                if let Some(charset) = charset {
                    let supported = charset.eq_ignore_ascii_case("utf-8")
                        || (charset.eq_ignore_ascii_case("us-ascii") && decoded.bytes.is_ascii());
                    if !supported {
                        return Err(DisclosureProbeError::Encoded {
                            path: path.clone(),
                            reason: "charset is unsupported",
                        });
                    }
                }
                std::str::from_utf8(&decoded.bytes).map_err(|_| DisclosureProbeError::Encoded {
                    path: path.clone(),
                    reason: "payload requires UTF-8",
                })?;
            }
            (
                DisclosureRepresentation::DataPayload,
                Some(media_type),
                format,
            )
        }
    };
    let percent_text = representation == DisclosureRepresentation::PercentDecoded
        && std::str::from_utf8(&decoded.bytes).is_ok();
    if representation == DisclosureRepresentation::PercentDecoded && !percent_text {
        scan.unsupported(path, "percent-decoded text")?;
    }
    scan.depth += 1;
    let index = scan.embedded.len();
    scan.embedded.push(DisclosureEmbeddedCoverage {
        path: path.clone(),
        depth: scan.depth,
        representation,
        sha256: Sha256Digest::of(&decoded.bytes),
        bytes: decoded.bytes.len(),
        media_type,
        decoding: if percent_text {
            DisclosureDecoding::Utf8Text
        } else {
            format.coverage()
        },
    });
    let result = (|| {
        scan.search(path, &decoded.bytes, representation)?;
        if percent_text {
            // Percent escapes alone do not establish a markup language or MIME type.
            self::scan(scan, path, &decoded.bytes)
        } else {
            let decoding = format.scan(scan, path, &decoded.bytes)?;
            // Nested metadata candidates can append entries. Update this exact
            // parent's coverage only after its selected decoder finishes.
            scan.embedded[index].decoding = decoding;
            Ok(())
        }
    })();
    scan.depth -= 1;
    result
}
