//! Structured current authoring facts for an editor, without a second parser.
use gazette_sdk::{authoring::session::Session, render::LimitedWriter, source::Sha256Digest};
use lsp_server::ErrorCode;
use lsp_types::{Position, TextDocumentIdentifier};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    generation: u64,
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    position: Position,
}
pub(super) fn query(
    state: &Session,
    value: Value,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let query: Request = serde_json::from_value(value)
        .map_err(|_| (ErrorCode::InvalidParams, "invalid authoring context query"))?;
    if query.generation != state.generation() {
        return Err((
            ErrorCode::InvalidRequest,
            "stale authoring context generation",
        ));
    }
    let id = query.text_document.uri.as_str();
    let document = state
        .files()
        .get(id)
        .ok_or((ErrorCode::InvalidParams, "document is not selected"))?;
    let source = document.text();
    let offset = crate::positions::offset(source, query.position)
        .map_err(|_| (ErrorCode::InvalidParams, "invalid source position"))?;
    let analysis = state
        .analysis()
        .as_ref()
        .map_err(|_| (ErrorCode::InvalidRequest, "current analysis unavailable"))?;
    let fields = if state.package_baseline(id).is_some() {
        Vec::new()
    } else {
        analysis
            .structure(document.analysis_path(), source, maximum)
            .map_err(|_| {
                (
                    ErrorCode::InvalidRequest,
                    "current source structure unavailable",
                )
            })?
    };
    let fields = fields
        .into_iter()
        .map(|field| {
            let range = crate::positions::range(source, field.location.span);
            json!({"field":field,"range":range})
        })
        .collect::<Vec<_>>();
    let diagnostics = crate::diagnostics::current(
        state,
        id,
        document,
        crate::Limits {
            output_bytes: maximum,
            ..crate::Limits::default()
        },
    )
    .map_err(|_| (ErrorCode::InvalidRequest, "current diagnostics unavailable"))?;
    let result = json!({"generation":state.generation(),"document":id,"path":document.path(),"version":document.version(),
        "source_sha256":Sha256Digest::of(source.as_bytes()),"structurally_valid":analysis.failures().is_empty(),
        "fields":fields,"structure_status":if state.package_baseline(id).is_some() {"not_available_for_package_manifest"} else {"available"},"diagnostics":diagnostics,
        "assistance":projection(analysis.assistance(document.analysis_path(),source,offset,maximum)),
        "component":projection(analysis.component_assistance(document.analysis_path(),source,offset,maximum)),
        "identity":projection(analysis.workspace_identity(document.analysis_path(),source,offset)),
        "navigation": match crate::navigation::query(state, lsp_types::TextDocumentPositionParams {text_document: query.text_document.clone(), position: query.position}, None, maximum) {
            Ok(Value::Null) => json!({"status":"not_applicable"}),
            Ok(value) => json!({"status":"available","value":value}),
            Err(_) => json!({"status":"unavailable_for_current_source"}),
        },
        "page_package":projection(state.page_package(id))});
    let mut writer = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut writer, &result)
        .map_err(|_| (ErrorCode::InvalidRequest, "authoring context exceeds limit"))?;
    if writer.exceeded() {
        return Err((ErrorCode::InvalidRequest, "authoring context exceeds limit"));
    }
    Ok(result)
}

fn projection<T: serde::Serialize, E>(result: Result<Option<T>, E>) -> Value {
    match result {
        Ok(Some(value)) => json!({"status":"available","value":value}),
        Ok(None) => json!({"status":"not_applicable"}),
        Err(_) => json!({"status":"unavailable_for_current_source"}),
    }
}
