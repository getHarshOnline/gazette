//! Translate native HTML token channels into the existing immutable-output probe.

use gazette_compiler::html::{self, Error, TextKind};
use gazette_core::render::LimitedWriter;
use gazette_core::source::EntryPath;
use std::io::Write;

use super::{DisclosureProbeError, DisclosureRepresentation, Scan, css, javascript, json};

pub(super) fn scan(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    bytes: &[u8],
) -> Result<(), DisclosureProbeError> {
    inspect(scan, path, Input::Html(bytes), false)
}

pub(super) fn scan_generated(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    decoded: &gazette_compiler::generated_mdx::Decoded,
) -> Result<(), DisclosureProbeError> {
    inspect(scan, path, Input::Generated(decoded), true)
}

pub(super) fn scan_markdown(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    syntax: &gazette_core::markdown::Syntax,
) -> Result<(), DisclosureProbeError> {
    inspect(scan, path, Input::Markdown(syntax), false)
}

enum Input<'a> {
    Html(&'a [u8]),
    Markdown(&'a gazette_core::markdown::Syntax),
    Generated(&'a gazette_compiler::generated_mdx::Decoded),
}

fn inspect(
    scan: &mut Scan<'_>,
    path: &EntryPath,
    input: Input<'_>,
    join: bool,
) -> Result<(), DisclosureProbeError> {
    let limits = scan.limits.html;
    let maximum = limits
        .decoded_bytes
        .min(scan.limits.encoded_bytes.saturating_sub(scan.encoded_bytes));
    let mut joined = LimitedWriter::new(Vec::new(), maximum);
    let consumer = |kind, value: &str| {
        if join && kind == TextKind::Text {
            joined
                .write_all(value.as_bytes())
                .map_err(|_| DisclosureProbeError::Limit("generated MDX joined text"))?;
        }
        match kind {
            TextKind::DataScript => scan.unsupported(path, "HTML data-script")?,
            TextKind::EventAttribute => scan.unsupported(path, "HTML event attribute")?,
            TextKind::ExternalScript if !value.is_empty() => {
                scan.unsupported(path, "external script body")?;
            }
            _ => (),
        }
        let representation = match kind {
            TextKind::Text => DisclosureRepresentation::HtmlText,
            TextKind::Attribute | TextKind::StyleAttribute | TextKind::EventAttribute => {
                DisclosureRepresentation::HtmlAttribute
            }
            TextKind::Script
            | TextKind::ModuleScript
            | TextKind::DataScript
            | TextKind::ExternalScript
            | TextKind::JsonScript => DisclosureRepresentation::HtmlScript,
            TextKind::Style => DisclosureRepresentation::HtmlStyle,
        };
        if matches!(
            kind,
            TextKind::Text | TextKind::Attribute | TextKind::EventAttribute
        ) {
            scan.text(path, value.as_bytes(), representation)?;
        } else {
            scan.search(path, value.as_bytes(), representation)?;
        }
        if kind == TextKind::JsonScript {
            json::scan(scan, path, value.as_bytes())?;
        }
        let goal = match kind {
            TextKind::Script => Some(gazette_compiler::javascript::Goal::Script),
            TextKind::ModuleScript => Some(gazette_compiler::javascript::Goal::Module),
            _ => None,
        };
        if let Some(goal) = goal {
            javascript::scan(scan, path, value.as_bytes(), goal)?;
        }
        if matches!(kind, TextKind::Style | TextKind::StyleAttribute) {
            css::scan(scan, path, gazette_compiler::css::Input::Decoded(value))?;
        }
        Ok(())
    };
    match input {
        Input::Html(bytes) => html::inspect(bytes, limits, consumer),
        Input::Markdown(syntax) => html::inspect_markdown(syntax, limits, consumer),
        Input::Generated(decoded) => {
            gazette_compiler::generated_mdx::inspect(decoded, limits, consumer)
        }
    }
    .map_err(|error| {
        let (reason, line) = match error {
            Error::Consumer(error) => return error,
            Error::Limit(resource) => {
                return DisclosureProbeError::Limit(match resource {
                    "input bytes" => "HTML input bytes",
                    "unemitted input bytes" => "HTML unemitted input bytes",
                    "tokens" => "HTML tokens",
                    "decoded bytes" => "HTML decoded bytes",
                    _ => "HTML decoding",
                });
            }
            Error::Utf8 => ("requires UTF-8", None),
            Error::Incomplete => ("has unconsumed input", None),
            Error::Malformed { line } => ("tokenization failed", (line != 0).then_some(line)),
            Error::Unsupported { line } => ("context is unsupported", (line != 0).then_some(line)),
        };
        DisclosureProbeError::Html {
            path: path.clone(),
            reason,
            line,
        }
    })?;
    if join {
        let joined = joined.into_inner();
        scan.decoded(joined.len(), 1)?;
        scan.text(path, &joined, DisclosureRepresentation::HtmlText)?;
    }
    Ok(())
}
