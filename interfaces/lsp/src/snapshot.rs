//! Bounded generation-pinned handoff of the current editor selection.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gazette_sdk::{authoring::session::Session, render::LimitedWriter};
use lsp_server::ErrorCode;
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) fn manifest(
    state: &crate::documents::State,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let mut value = serde_json::to_value(state.snapshot()).map_err(|_| unavailable())?;
    if let Some(selection) = state.handoff_selection() {
        value["native_selection"] = serde_json::to_value(selection).map_err(|_| unavailable())?;
    }
    bounded(value, maximum)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    generation: u64,
    document: String,
    offset: usize,
    length: usize,
}
pub(super) fn source(
    state: &Session,
    value: Value,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let query: Chunk = serde_json::from_value(value)
        .map_err(|_| (ErrorCode::InvalidParams, "invalid snapshot source query"))?;
    let bytes = state
        .source_chunk(
            query.generation,
            &query.document,
            query.offset,
            query.length,
        )
        .map_err(|_| {
            (
                ErrorCode::InvalidRequest,
                "snapshot source changed or is unavailable",
            )
        })?;
    bounded(
        json!({"generation":state.generation(),"document":query.document,"offset":query.offset,"bytes_base64":STANDARD.encode(bytes)}),
        maximum,
    )
}
fn unavailable() -> (ErrorCode, &'static str) {
    (
        ErrorCode::InvalidRequest,
        "complete snapshot exceeds output limit",
    )
}
fn bounded(value: Value, maximum: usize) -> Result<Value, (ErrorCode, &'static str)> {
    let mut output = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut output, &value).map_err(|_| unavailable())?;
    if output.exceeded() {
        return Err(unavailable());
    }
    Ok(value)
}
