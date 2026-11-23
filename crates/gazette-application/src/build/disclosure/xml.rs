//! XML channels feed the same retained-output comparison and evidence owner.

use gazette_compiler::xml::{self, Error, TextKind};
use gazette_core::source::EntryPath;

use super::{DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<(), DisclosureProbeError> {
    xml::inspect(bytes, scan.limits.xml, |kind, value| {
        match kind {
            TextKind::EventAttribute => scan.unsupported(path, "XML event attribute")?,
            TextKind::ProcessingInstruction => {
                scan.unsupported(path, "XML processing instruction")?;
            }
            _ => (),
        }
        let representation = match kind {
            TextKind::Text => DisclosureRepresentation::XmlText,
            TextKind::Attribute | TextKind::EventAttribute | TextKind::StyleAttribute => {
                DisclosureRepresentation::XmlAttribute
            }
            TextKind::Namespace => DisclosureRepresentation::XmlNamespace,
            TextKind::Comment => DisclosureRepresentation::XmlComment,
            TextKind::ProcessingInstruction => DisclosureRepresentation::XmlProcessingInstruction,
            TextKind::EmbeddedElement(_) => {
                return scan.unsupported(path, "XML embedded element");
            }
        };
        if kind == TextKind::StyleAttribute {
            scan.search(path, value.as_bytes(), representation)?;
            return super::css::scan(scan, path, gazette_compiler::css::Input::Decoded(value));
        }
        if matches!(
            kind,
            TextKind::Text | TextKind::Attribute | TextKind::Namespace | TextKind::EventAttribute
        ) {
            scan.text(path, value.as_bytes(), representation)
        } else {
            scan.search(path, value.as_bytes(), representation)
        }
    })
    .map_err(|error| {
        let reason = match error {
            Error::Consumer(error) => return error,
            Error::Limit(resource) => {
                return DisclosureProbeError::Limit(match resource {
                    "input bytes" => "XML input bytes",
                    "events" => "XML events",
                    "depth" => "XML depth",
                    "attributes" => "XML attributes",
                    "name bytes" => "XML name bytes",
                    "value bytes" => "XML value bytes",
                    "decoded bytes" => "XML decoded bytes",
                    _ => "XML decoding",
                });
            }
            Error::Utf8 => "requires UTF-8",
            Error::Malformed => "syntax, namespace or parser bound rejected",
            Error::Unsupported => "profile is unsupported",
        };
        DisclosureProbeError::Xml {
            path: path.clone(),
            reason,
        }
    })
}
