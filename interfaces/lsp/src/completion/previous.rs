//! Completion-only fallback; quick fixes and navigation keep current analysis.

use super::*;

pub(super) fn proposals(
    state: &Session,
    params: TextDocumentPositionParams,
    maximum: usize,
) -> Result<Option<CompletionList>, (ErrorCode, &'static str)> {
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "previous reference completion is unavailable",
        )
    };
    let id = params.text_document.uri.as_str();
    let Some(document) = state.files().get(id) else {
        return Ok(None);
    };
    let source = document.text();
    let offset = positions::offset(source, params.position).map_err(|_| unavailable())?;
    let Some(proposal) = state
        .complete_previous_reference(id, offset, maximum)
        .map_err(|_| unavailable())?
    else {
        return Ok(None);
    };
    let range = edit_range(source, proposal.location.span)?;
    let quote = proposal
        .preimage
        .chars()
        .next()
        .filter(|c| matches!(c, '\'' | '"'));
    let result = CompletionList {
        is_incomplete: true,
        items: proposal.candidates.iter().map(|candidate| CompletionItem {
            label: candidate.label.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            filter_text: Some(match quote {
                Some(quote) => format!("{quote}{}", candidate.label),
                None => candidate.label.clone(),
            }),
            detail: Some("From the last parsed workspace".into()),
            insert_text_mode: Some(InsertTextMode::AS_IS),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text: candidate.replacement.clone() })),
            data: Some(json!({
                "generation": state.generation(), "version": document.version(),
                "path": proposal.path, "source_sha256": proposal.source_sha256,
                "location": proposal.location, "preimage": proposal.preimage,
                "basis_sha256": proposal.basis_sha256, "basis_generation": proposal.basis_generation,
            })),
            ..Default::default()
        }).collect(),
    };
    let mut bounded = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut bounded, &result).map_err(|_| unavailable())?;
    if bounded.exceeded() {
        return Err(unavailable());
    }
    Ok(Some(result))
}
