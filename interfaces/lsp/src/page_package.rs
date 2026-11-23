//! Compiler-owned page-package ownership and proven resource destinations.

use gazette_sdk::{authoring::session::Session, render::LimitedWriter};
use lsp_server::ErrorCode;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    generation: u64,
    document: String,
}

pub(super) fn query(
    state: &Session,
    value: Value,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let request: Request = serde_json::from_value(value)
        .map_err(|_| (ErrorCode::InvalidParams, "invalid page-package query"))?;
    if request.generation != state.generation() {
        return Err((ErrorCode::InvalidRequest, "stale page-package generation"));
    }
    let Some(ownership) = state.page_package(&request.document).map_err(|_| {
        (
            ErrorCode::InvalidRequest,
            "page-package source binding unavailable",
        )
    })?
    else {
        return Ok(Value::Null);
    };
    let source_uri = state
        .files()
        .iter()
        .find(|(_, document)| document.path() == ownership.package.document())
        .map(|(uri, _)| uri);
    let companions = ownership
        .package
        .companions()
        .iter()
        .map(|companion| {
            let uri = state
                .page_inputs()
                .iter()
                .find(|(_, input)| input.path() == companion.path())
                .map(|(uri, _)| uri);
            json!({"uri":uri,"companion":companion})
        })
        .collect::<Vec<_>>();
    let value = json!({
        "generation":state.generation(),
        "ownership":ownership,
        "retained_source":{"uri":source_uri,"path":ownership.package.document()},
        "retained_companions":companions,
        "compiled_destinations":ownership.resources,
    });
    let mut bounded = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut bounded, &value).map_err(|_| {
        (
            ErrorCode::InvalidRequest,
            "page-package output exceeds limit",
        )
    })?;
    if bounded.exceeded() {
        return Err((
            ErrorCode::InvalidRequest,
            "page-package output exceeds limit",
        ));
    }
    Ok(value)
}
