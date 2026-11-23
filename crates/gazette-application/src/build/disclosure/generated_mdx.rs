//! Inspect only the bounded serializer-produced MDX grammar, never arbitrary MDX.

use gazette_compiler::generated_mdx::{self, Error, Limits};
use gazette_core::source::EntryPath;

use super::{DisclosureDecoding, DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<DisclosureDecoding, DisclosureProbeError> {
    let markdown = scan.limits.markdown;
    let decoded = generated_mdx::decode(
        bytes,
        Limits {
            input_bytes: markdown.source_bytes.min(scan.limits.html.input_bytes),
            output_bytes: markdown
                .output_bytes
                .min(scan.limits.html.input_bytes)
                .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes)),
            tokens: markdown.nodes.min(scan.limits.html.tokens),
            depth: markdown.depth,
        },
    )
    .map_err(|error| match error {
        Error::Limit(_) => DisclosureProbeError::Limit("generated MDX decoding"),
        Error::Unsupported => DisclosureProbeError::Unsupported {
            path: path.clone(),
            kind: "generated literal MDX grammar",
        },
    })?;
    scan.decoded(
        decoded.decoded_bytes(),
        decoded.literal_count().saturating_add(1),
    )?;
    scan.text(
        path,
        decoded.title.as_bytes(),
        DisclosureRepresentation::JsonString,
    )?;
    super::html::scan_generated(scan, path, &decoded)?;
    Ok(DisclosureDecoding::GeneratedMdxLiteralSyntax {
        profile: generated_mdx::PROFILE,
        html_profile: gazette_compiler::html::PROFILE,
        css_profile: gazette_compiler::css::PROFILE,
    })
}
