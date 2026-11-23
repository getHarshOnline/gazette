//! Exact native field origins with only checked UTF-16 transport projection added.

use gazette_sdk::{
    authoring::session::{OriginQuery, Session},
    render::LimitedWriter,
};
use lsp_server::ErrorCode;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    generation: u64,
    query: OriginQuery,
}

pub(super) fn query(
    state: &Session,
    value: Value,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    if !value.is_object() || !value.pointer("/query/query").is_some_and(Value::is_object) {
        return Err((
            ErrorCode::InvalidParams,
            "origin requires named generation and query",
        ));
    }
    let request: Request = serde_json::from_value(value)
        .map_err(|_| (ErrorCode::InvalidParams, "invalid origin query"))?;
    let result = state
        .origin(request.generation, &request.query)
        .map_err(|e| (ErrorCode::InvalidRequest, e.code()))?;
    let text = state
        .files()
        .get(result.document)
        .ok_or((ErrorCode::InvalidRequest, "origin source unavailable"))?
        .text();
    let range = crate::positions::range(text, result.value.span())
        .ok_or((ErrorCode::InvalidRequest, "origin position unavailable"))?;
    let value = json!({"native":result,"range":range});
    let mut bounded = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut bounded, &value)
        .map_err(|_| (ErrorCode::InvalidRequest, "origin output exceeds limit"))?;
    if bounded.exceeded() {
        return Err((ErrorCode::InvalidRequest, "origin output exceeds limit"));
    }
    Ok(value)
}
