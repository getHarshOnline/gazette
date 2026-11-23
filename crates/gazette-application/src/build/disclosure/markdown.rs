//! Native retained Markdown facts through the one bounded disclosure pipeline.

use std::io::Write;

use gazette_core::{
    markdown::{Error, Identity, Profile},
    render::LimitedWriter,
    source::{EntryPath, Sha256Digest},
};

use super::{DisclosureDecoding, DisclosureProbeError, DisclosureRepresentation, Scan};

pub(super) const PROFILE: &str = "gazette-disclosure-markdown/2";

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<DisclosureDecoding, DisclosureProbeError> {
    let parser = scan
        .markdown
        .ok_or_else(|| DisclosureProbeError::Unsupported {
            path: path.clone(),
            kind: "Markdown parser",
        })?;
    let parser_profile = parser.profile();
    if parser_profile.is_empty()
        || parser_profile.len() > 256
        || !parser_profile.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(DisclosureProbeError::Unsupported {
            path: path.clone(),
            kind: "Markdown parser profile",
        });
    }
    let source = std::str::from_utf8(bytes).map_err(|_| DisclosureProbeError::Encoded {
        path: path.clone(),
        reason: "Markdown requires UTF-8",
    })?;
    let mut limits = scan.limits.markdown;
    limits.fact_bytes = limits
        .fact_bytes
        .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes));
    let document = parser
        .parse(source, Profile::PublicationMath, limits)
        .map_err(|error| match error {
            Error::Limit(_) => DisclosureProbeError::Limit("Markdown parser"),
            _ => DisclosureProbeError::Encoded {
                path: path.clone(),
                reason: "Markdown syntax is unsupported or invalid",
            },
        })?;
    let syntax = document.syntax();
    if syntax.identity
        != (Identity {
            source_sha256: Sha256Digest::of(bytes),
            profile: Profile::PublicationMath,
        })
        || syntax.node_count > limits.nodes
    {
        return Err(DisclosureProbeError::Inventory);
    }
    // Final publication Markdown has no unlowered component argument language.
    // Raw inspection is never promoted to a complete directive decoder.
    if !syntax.directives.is_empty() {
        return Err(DisclosureProbeError::Unsupported {
            path: path.clone(),
            kind: "Markdown directive",
        });
    }

    for link in &syntax.links {
        value(
            scan,
            path,
            &link.destination,
            DisclosureRepresentation::MarkdownDestination,
        )?;
        value(
            scan,
            path,
            &link.title,
            DisclosureRepresentation::MarkdownTitle,
        )?;
    }
    for heading in &syntax.headings {
        if let Some(id) = &heading.id {
            value(scan, path, id, DisclosureRepresentation::MarkdownMetadata)?;
        }
        for class in &heading.classes {
            value(
                scan,
                path,
                class,
                DisclosureRepresentation::MarkdownMetadata,
            )?;
        }
        for (name, attribute) in &heading.attributes {
            value(scan, path, name, DisclosureRepresentation::MarkdownMetadata)?;
            value(
                scan,
                path,
                attribute,
                DisclosureRepresentation::MarkdownMetadata,
            )?;
        }
    }
    for note in &syntax.footnotes {
        value(
            scan,
            path,
            &note.name,
            DisclosureRepresentation::MarkdownMetadata,
        )?;
    }
    for reference in &syntax.footnote_references {
        value(
            scan,
            path,
            &reference.name,
            DisclosureRepresentation::MarkdownMetadata,
        )?;
    }
    let maximum = limits
        .output_bytes
        .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes));
    let mut joined = LimitedWriter::new(Vec::new(), maximum);
    for piece in &syntax.text {
        value(
            scan,
            path,
            &piece.value,
            DisclosureRepresentation::MarkdownText,
        )?;
        joined
            .write_all(piece.value.as_bytes())
            .map_err(|_| DisclosureProbeError::Limit("Markdown joined text"))?;
    }
    let joined = joined.into_inner();
    // Native structural separators survive; inline emphasis/code boundaries do
    // not hide a literal spread across several textual AST occurrences.
    scan.decoded(joined.len(), 1)?;
    scan.text(path, &joined, DisclosureRepresentation::MarkdownText)?;
    for html in &syntax.html {
        scan.decoded(html.literal.len(), 1)?;
    }
    if !syntax.html.is_empty() {
        super::html::scan_markdown(scan, path, syntax)?;
    }
    Ok(DisclosureDecoding::MarkdownSyntax {
        profile: PROFILE,
        parser_profile,
        html_profile: gazette_compiler::html::PROFILE,
        css_profile: gazette_compiler::css::PROFILE,
        javascript_profile: gazette_compiler::javascript::PROFILE,
    })
}

fn value(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    value: &str,
    representation: DisclosureRepresentation,
) -> Result<(), DisclosureProbeError> {
    if !value.is_empty() {
        scan.decoded(value.len(), 1)?;
        scan.text(path, value.as_bytes(), representation)?;
    }
    Ok(())
}
