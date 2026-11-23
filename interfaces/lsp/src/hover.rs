//! Client-selected plain-text or Markdown help over current native facts.

use gazette_sdk::{authoring::session::Session, render::LimitedWriter};
use lsp_server::ErrorCode;
use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, TextDocumentPositionParams};
use serde_json::Value;
use std::io::Write;

use crate::positions;

pub(super) fn query(
    state: &Session,
    params: TextDocumentPositionParams,
    output_bytes: usize,
    format: MarkupKind,
) -> Result<Value, (ErrorCode, &'static str)> {
    let markdown = format == MarkupKind::Markdown;
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "current workspace identity is unavailable",
        )
    };
    let document = state
        .files()
        .get(params.text_document.uri.as_str())
        .ok_or((ErrorCode::InvalidParams, "document is not selected"))?;
    let source = document.text();
    let offset = positions::offset(source, params.position).map_err(|_| {
        (
            ErrorCode::InvalidParams,
            "position is outside an exact UTF-16 source boundary",
        )
    })?;
    let analysis = state.analysis().as_ref().map_err(|_| unavailable())?;
    let identity = analysis.workspace_identity(document.analysis_path(), source, offset);
    let mut text = LimitedWriter::new(Vec::new(), output_bytes);
    let range = if let Ok(Some(identity)) = identity {
        // The enum's own serialization owns namespace spelling; no protocol catalog.
        let kind = match identity.symbol.kind() {
            gazette_sdk::workspace::WorkspaceSymbolKind::Declaration(kind) => {
                serde_json::to_value(kind).map_err(|_| unavailable())?
            }
            gazette_sdk::workspace::WorkspaceSymbolKind::ResourceInput => {
                Value::String("resource-input".into())
            }
        };
        let kind = kind.as_str().ok_or_else(unavailable)?;
        write!(text, "Resolved {kind}: ").map_err(|_| unavailable())?;
        literal(&mut text, identity.symbol.name(), markdown).map_err(|_| unavailable())?;
        if let Some(composition) = identity.composition {
            text.write_all(b"\n\nConfigured presentation:\n\n")
                .map_err(|_| unavailable())?;
            if markdown {
                text.write_all(b"```json\n").map_err(|_| unavailable())?;
            }
            serde_json::to_writer_pretty(&mut text, composition).map_err(|_| unavailable())?;
            if markdown {
                text.write_all(b"\n```").map_err(|_| unavailable())?;
            }
        }
        identity.occurrence.span
    } else if let Some(component) = analysis
        .component_assistance(document.analysis_path(), source, offset, output_bytes)
        .map_err(|_| unavailable())?
        .and_then(|value| value.hover)
    {
        text.write_all(b"Selected Theme component: ")
            .map_err(|_| unavailable())?;
        literal(&mut text, component.name.as_str(), markdown).map_err(|_| unavailable())?;
        text.write_all(b"\n\nTemplate: ")
            .map_err(|_| unavailable())?;
        literal(&mut text, component.template.as_str(), markdown).map_err(|_| unavailable())?;
        if let Some(children) = component.landing_children {
            write!(
                text,
                "\n\nChild sections: {} selected Theme components; ",
                children.named_components.len()
            )
            .map_err(|_| unavailable())?;
            literal(&mut text, children.unnamed_section_requirement, markdown)
                .map_err(|_| unavailable())?;
            text.write_all(b". Complete a child component name to see the catalog.")
                .map_err(|_| unavailable())?;
        }
        text.write_all(b"\n\nArguments:\n\n")
            .map_err(|_| unavailable())?;
        if markdown {
            text.write_all(b"```json\n").map_err(|_| unavailable())?;
        }
        serde_json::to_writer_pretty(&mut text, component.arguments).map_err(|_| unavailable())?;
        if markdown {
            text.write_all(b"\n```").map_err(|_| unavailable())?;
        }
        component.location.span
    } else {
        let Some(assistance) = analysis
            .assistance(document.analysis_path(), source, offset, output_bytes)
            .map_err(|_| unavailable())?
        else {
            return Ok(Value::Null);
        };
        let Some(hover) = assistance.help.hover else {
            return Ok(Value::Null);
        };
        if let Some(field) = &hover.field {
            literal(&mut text, &field.name, markdown).map_err(|_| unavailable())?;
            write!(text, " ({})", presence(field.presence)).map_err(|_| unavailable())?;
            if let Some(description) = &field.description {
                text.write_all(b"\n").map_err(|_| unavailable())?;
                literal(&mut text, description, markdown).map_err(|_| unavailable())?;
            }
            write!(text, "\n\nAccepts: {}.", value_kinds(&field.value_kinds))
                .map_err(|_| unavailable())?;
        } else {
            if let Some(description) = &hover.node.description {
                literal(&mut text, description, markdown).map_err(|_| unavailable())?;
            }
            write!(
                text,
                "\n\nAccepts: {}.",
                value_kinds(&hover.node.value_kinds)
            )
            .map_err(|_| unavailable())?;
        }
        if !hover.node.allowed_values.is_empty() {
            text.write_all(b"\nAllowed values:")
                .map_err(|_| unavailable())?;
            for candidate in &hover.node.allowed_values {
                text.write_all(b"\n- ").map_err(|_| unavailable())?;
                match &candidate.value {
                    gazette_sdk::workspace::AuthoringLiteral::String(value) => {
                        serde_json::to_writer(
                            LiteralWriter {
                                output: &mut text,
                                markdown,
                            },
                            value,
                        )
                    }
                    gazette_sdk::workspace::AuthoringLiteral::Integer(value) => {
                        serde_json::to_writer(&mut text, value)
                    }
                    gazette_sdk::workspace::AuthoringLiteral::Boolean(value) => {
                        serde_json::to_writer(&mut text, value)
                    }
                }
                .map_err(|_| unavailable())?;
                if let Some(description) = &candidate.description {
                    text.write_all(b": ").map_err(|_| unavailable())?;
                    literal(&mut text, description, markdown).map_err(|_| unavailable())?;
                }
            }
        }
        hover.location.span
    };
    let text = String::from_utf8(text.into_inner()).map_err(|_| unavailable())?;
    let result = Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: format,
            value: text,
        }),
        range: Some(positions::range(source, range).ok_or_else(unavailable)?),
    };
    let mut bounded = LimitedWriter::new(std::io::sink(), output_bytes);
    serde_json::to_writer(&mut bounded, &result).map_err(|_| unavailable())?;
    if bounded.exceeded() {
        return Err(unavailable());
    }
    serde_json::to_value(result).map_err(|_| unavailable())
}

// Escape ASCII punctuation as it is written so Markdown cannot reinterpret
// opaque identities, paths, schema text or JSON literals. The destination's
// existing limit charges the expanded bytes before accepting the response.
struct LiteralWriter<'a, W> {
    output: &'a mut W,
    markdown: bool,
}

impl<W: Write> Write for LiteralWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if !self.markdown {
            return self.output.write(bytes);
        }
        for byte in bytes {
            if byte.is_ascii_punctuation() {
                self.output.write_all(b"\\")?;
            }
            self.output.write_all(&[*byte])?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

fn literal(output: &mut impl Write, value: &str, markdown: bool) -> std::io::Result<()> {
    LiteralWriter { output, markdown }.write_all(value.as_bytes())
}

pub(super) fn presence(value: gazette_sdk::workspace::AuthoringFieldPresence) -> &'static str {
    use gazette_sdk::workspace::AuthoringFieldPresence;
    match value {
        AuthoringFieldPresence::Required => "required",
        AuthoringFieldPresence::Optional => "optional",
        AuthoringFieldPresence::VariantDependent => "required for some choices",
    }
}

fn value_kinds(values: &[gazette_sdk::workspace::AuthoringValueKind]) -> String {
    use gazette_sdk::workspace::AuthoringValueKind;
    values
        .iter()
        .map(|value| match value {
            AuthoringValueKind::Boolean => "true or false",
            AuthoringValueKind::Integer => "a whole number",
            AuthoringValueKind::String => "text",
            AuthoringValueKind::Array => "a list",
            AuthoringValueKind::Table => "a table",
        })
        .collect::<Vec<_>>()
        .join(", ")
}
