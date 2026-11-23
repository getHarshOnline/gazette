//! Reviewable LSP edits projected from current shared diagnostics and assistance.

use gazette_sdk::{authoring, authoring::session::Session, render::LimitedWriter};
use lsp_server::ErrorCode;
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CompletionTextEdit,
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, TextDocumentEdit,
    TextDocumentIdentifier, TextDocumentPositionParams, WorkspaceEdit,
};
use serde_json::{Value, json};

use crate::{Limits, completion, diagnostics, positions};

pub(super) fn query(
    state: &Session,
    params: CodeActionParams,
    limits: Limits,
) -> Result<Value, (ErrorCode, &'static str)> {
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "current reviewable source repairs are unavailable",
        )
    };
    if params.context.only.as_ref().is_some_and(|kinds| {
        !kinds.iter().any(|kind| {
            kind.as_str().is_empty() || CodeActionKind::QUICKFIX.as_str().starts_with(kind.as_str())
        })
    }) {
        return Ok(json!([]));
    }
    let uri = params.text_document.uri;
    let document = state
        .files()
        .get(uri.as_str())
        .ok_or((ErrorCode::InvalidParams, "document is not selected"))?;
    let current =
        diagnostics::current(state, uri.as_str(), document, limits).map_err(|_| unavailable())?;
    let mut triggering = params
        .context
        .diagnostics
        .into_iter()
        .filter(|candidate| {
            candidate.source.as_deref() == Some("gazette")
                && current.contains(candidate)
                // Proposals are queried at this exact position below. A broader
                // selection must not attach a different field's diagnostic to
                // an otherwise valid completion at the selection's start.
                && intersects(
                    lsp_types::Range::new(params.range.start, params.range.start),
                    candidate.range,
                )
        })
        .collect::<Vec<_>>();
    if triggering.is_empty() {
        return Ok(json!([]));
    }
    let Ok(analysis) = state.analysis() else {
        return Ok(json!([]));
    };
    let native = authoring::diagnostics_for(
        analysis,
        document.analysis_path(),
        document.path(),
        document.text(),
        limits.output_bytes,
    )
    .map_err(|_| unavailable())?;
    let unlocated_reference = if native.iter().any(|diagnostic| {
        !diagnostic.selection_wide
            && diagnostic.location.is_none()
            && diagnostic.code == "workspace_reference"
    }) {
        let offset = positions::offset(document.text(), params.range.start)
            .map_err(|_| (ErrorCode::InvalidParams, "invalid exact source position"))?;
        analysis
            .complete_reference(
                document.analysis_path(),
                document.text(),
                offset,
                limits.output_bytes,
            )
            .map_err(|_| unavailable())?
    } else {
        None
    };
    let mut accepted = Vec::new();
    for diagnostic in native {
        // A whole-file LSP display range is not native source evidence. Match
        // unlocated workspace findings only through the shared unique-reference
        // proof; selection-wide findings cannot authorize a source repair.
        if diagnostic.selection_wide
            || (diagnostic.location.is_none()
                && !(diagnostic.code == "workspace_reference"
                    && diagnostic.key.is_some_and(|key| {
                        unlocated_reference
                            .as_ref()
                            .is_some_and(|value| value.matches_unlocated_diagnostic(key))
                    })))
        {
            continue;
        }
        let mut data = serde_json::to_value(diagnostic).map_err(|_| unavailable())?;
        data["generation"] = json!(state.generation());
        accepted.push(data);
    }
    triggering.retain(|diagnostic| {
        diagnostic
            .data
            .as_ref()
            .is_some_and(|data| accepted.contains(data))
    });
    if triggering.is_empty() {
        return Ok(json!([]));
    }
    let proposals = completion::proposals(
        state,
        TextDocumentPositionParams::new(
            TextDocumentIdentifier { uri: uri.clone() },
            params.range.start,
        ),
        limits.output_bytes,
    )
    .map_err(|_| unavailable())?;
    let Some(proposals) = proposals else {
        return Ok(json!([]));
    };
    let mut actions = Vec::new();
    let mut budget = LimitedWriter::new(std::io::sink(), limits.output_bytes);
    for proposal in proposals.items {
        let Some(CompletionTextEdit::Edit(edit)) = proposal.text_edit else {
            continue;
        };
        let action = CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Review Gazette replacement: {}", proposal.label),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(triggering.clone()),
            edit: Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version: document.version(),
                    },
                    edits: vec![OneOf::Left(edit)],
                }])),
                ..WorkspaceEdit::default()
            }),
            command: None,
            is_preferred: Some(false),
            data: Some(json!({
                "generation": state.generation(),
                "proposal": proposal.data,
                "reviewRequired": true,
            })),
            ..CodeAction::default()
        });
        serde_json::to_writer(&mut budget, &action).map_err(|_| unavailable())?;
        if budget.exceeded() {
            return Err(unavailable());
        }
        actions.push(action);
    }
    let mut complete = LimitedWriter::new(std::io::sink(), limits.output_bytes);
    serde_json::to_writer(&mut complete, &actions).map_err(|_| unavailable())?;
    if complete.exceeded() {
        return Err(unavailable());
    }
    serde_json::to_value(actions).map_err(|_| unavailable())
}

fn intersects(left: lsp_types::Range, right: lsp_types::Range) -> bool {
    let (left_start, left_end) = (point(left.start), point(left.end));
    let (right_start, right_end) = (point(right.start), point(right.end));
    match (left_start == left_end, right_start == right_end) {
        (true, true) => left_start == right_start,
        (true, false) => right_start <= left_start && left_start < right_end,
        (false, true) => left_start <= right_start && right_start < left_end,
        (false, false) => left_start < right_end && right_start < left_end,
    }
}

fn point(position: lsp_types::Position) -> (u32, u32) {
    (position.line, position.character)
}
