//! Native CSS channels share the retained-output probe's comparison and failure boundary.

use gazette_compiler::css::{self, Error, Input, TextKind};
use gazette_core::source::EntryPath;

use super::{DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    input: Input<'_>,
) -> Result<(), DisclosureProbeError> {
    css::inspect(input, scan.limits.css, |kind, value| {
        let representation = match kind {
            TextKind::Identifier => DisclosureRepresentation::CssIdentifier,
            TextKind::String => DisclosureRepresentation::CssString,
            TextKind::Url => DisclosureRepresentation::CssUrl,
        };
        if kind == TextKind::Identifier {
            scan.search(path, value.as_bytes(), representation)
        } else {
            scan.text(path, value.as_bytes(), representation)
        }
    })
    .map_err(|error| {
        let reason = match error {
            Error::Consumer(error) => return error,
            Error::Limit(resource) => {
                return DisclosureProbeError::Limit(match resource {
                    "input bytes" => "CSS input bytes",
                    "tokens" => "CSS tokens",
                    "nesting depth" => "CSS nesting depth",
                    "decoded bytes" => "CSS decoded bytes",
                    _ => "CSS decoding",
                });
            }
            Error::Utf8 => "requires UTF-8",
            Error::Malformed => "token syntax or termination rejected",
            Error::Unsupported => "profile is unsupported",
        };
        DisclosureProbeError::Css {
            path: path.clone(),
            reason,
        }
    })
}
