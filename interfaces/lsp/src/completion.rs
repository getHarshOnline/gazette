//! Standard completion edits projected from exact shared source-bound proposals.

use gazette_sdk::{authoring::session::Session, render::LimitedWriter};
use lsp_server::ErrorCode;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionTextEdit, Documentation,
    InsertTextMode, TextDocumentPositionParams, TextEdit,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use crate::positions;

mod previous;

pub(super) fn query(
    state: &Session,
    params: TextDocumentPositionParams,
    maximum: usize,
) -> Result<Value, (ErrorCode, &'static str)> {
    let current = proposals(state, params.clone(), maximum);
    let proposals = match &current {
        Ok(Some(items)) if !items.items.is_empty() => current,
        Err((ErrorCode::InvalidParams, _)) => current,
        _ => match previous::proposals(state, params, maximum)? {
            Some(items) => Ok(Some(items)),
            None => current,
        },
    };
    match proposals? {
        Some(result) => serde_json::to_value(result).map_err(|_| {
            (
                ErrorCode::InvalidRequest,
                "current complete source replacements are unavailable",
            )
        }),
        None => Ok(Value::Null),
    }
}

pub(super) fn proposals(
    state: &Session,
    params: TextDocumentPositionParams,
    maximum: usize,
) -> Result<Option<CompletionList>, (ErrorCode, &'static str)> {
    let unavailable = || {
        (
            ErrorCode::InvalidRequest,
            "current complete source replacements are unavailable",
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
    let mut items = Vec::new();
    let mut budget = LimitedWriter::new(std::io::sink(), maximum);
    let assistance = analysis
        .assistance(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?;
    let resource_items = match assistance.as_ref() {
        Some(value) => page_resource_items(state, params.text_document.uri.as_str(), value, source)
            .map_err(|_| unavailable())?,
        None => None,
    };
    if let Some(resource_items) = resource_items {
        for item in resource_items {
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else if let Some(assistance) = assistance.as_ref().filter(|value| {
        value.help.completion_location.is_some() && !value.help.candidates.is_empty()
    }) {
        let location = assistance
            .help
            .completion_location
            .ok_or_else(unavailable)?;
        let range = edit_range(source, location.span)?;
        let preimage = assistance
            .help
            .completion_preimage
            .as_deref()
            .ok_or_else(unavailable)?;
        let kind = assistance.help.completion_kind.ok_or_else(unavailable)?;
        for candidate in &assistance.help.candidates {
            let item = CompletionItem {
                label: candidate.label.clone(),
                kind: Some(match kind {
                    gazette_sdk::workspace::CompletionKind::Field => CompletionItemKind::FIELD,
                    gazette_sdk::workspace::CompletionKind::Literal => CompletionItemKind::VALUE,
                }),
                detail: candidate
                    .presence
                    .map(|presence| format!("{} field", crate::hover::presence(presence))),
                documentation: candidate
                    .description
                    .as_ref()
                    .map(|value| Documentation::String(value.clone())),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.replacement.clone(),
                })),
                data: Some(json!({
                    "generation": state.generation(),
                    "version": document.version(),
                    "path": assistance.path,
                    "source_sha256": assistance.source_sha256,
                    "scope": assistance.scope,
                    "location": location,
                    "preimage": preimage,
                    "candidate": candidate,
                })),
                ..Default::default()
            };
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else if let Some(component) = analysis
        .component_assistance(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?
    {
        let Some(completion) = component.completion else {
            return Ok(None);
        };
        match completion {
            gazette_sdk::authoring::ComponentCompletion::Name {
                location,
                preimage,
                candidates,
            } => {
                let range = edit_range(source, location.span)?;
                for candidate in candidates {
                    let item = CompletionItem {
                        label: candidate.name.to_string(),
                        kind: Some(CompletionItemKind::CLASS),
                        detail: Some("Selected Theme component".into()),
                        insert_text_mode: Some(InsertTextMode::AS_IS),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: candidate.replacement.as_ref().into(),
                        })),
                        data: Some(json!({
                            "generation": state.generation(),
                            "version": document.version(),
                            "path": component.path,
                            "source_sha256": component.source_sha256,
                            "location": location,
                            "preimage": preimage,
                            "component": candidate,
                        })),
                        ..Default::default()
                    };
                    serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
                    items.push(item);
                }
            }
            gazette_sdk::authoring::ComponentCompletion::Argument {
                location,
                preimage,
                completion_kind,
                candidates,
            } => {
                let range = edit_range(source, location.span)?;
                for candidate in candidates {
                    let item = CompletionItem {
                        label: candidate.label.clone(),
                        kind: Some(match completion_kind {
                            gazette_sdk::workspace::CompletionKind::Field => {
                                CompletionItemKind::FIELD
                            }
                            gazette_sdk::workspace::CompletionKind::Literal => {
                                CompletionItemKind::VALUE
                            }
                        }),
                        detail: candidate.presence.map(|presence| {
                            format!("{} argument", crate::hover::presence(presence))
                        }),
                        documentation: candidate
                            .description
                            .as_ref()
                            .map(|value| Documentation::String(value.clone())),
                        insert_text_mode: Some(InsertTextMode::AS_IS),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: candidate.replacement.clone(),
                        })),
                        data: Some(json!({
                            "generation": state.generation(),
                            "version": document.version(),
                            "path": component.path,
                            "source_sha256": component.source_sha256,
                            "location": location,
                            "preimage": preimage,
                            "component_argument": candidate,
                        })),
                        ..Default::default()
                    };
                    serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
                    items.push(item);
                }
            }
        }
    } else if let Some(completion) = analysis
        .complete_reference(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?
    {
        let range = edit_range(source, completion.location.span)?;
        // LSP completion edits must occupy one line. Native byte-span proposals remain
        // available through the SDK/MCP; never split a multiline scalar into false edits.
        for candidate in &completion.candidates {
            let item = CompletionItem {
                label: candidate.symbol.name().to_owned(),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.replacement.as_ref().into(),
                })),
                data: Some(
                    json!({"generation":state.generation(),"version":document.version(),
                "path":completion.path,"source_sha256":completion.source_sha256,
                "location":completion.location,"preimage":completion.preimage,"symbol":candidate.symbol,
                "definition":candidate.definition}),
                ),
                ..Default::default()
            };
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else if let Some(completion) = analysis
        .complete_editorial_reference(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?
    {
        let range = edit_range(source, completion.location.span)?;
        for candidate in &completion.candidates {
            let item = CompletionItem {
                label: candidate.declaration.identity.to_string(),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.replacement.into(),
                })),
                data: Some(json!({
                    "generation":state.generation(), "version":document.version(),
                    "path":completion.path, "source_sha256":completion.source_sha256,
                    "location":completion.location, "preimage":completion.preimage,
                    "field":completion.field, "declaration":candidate.declaration,
                })),
                ..Default::default()
            };
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else if let Some(completion) = analysis
        .complete_authored_reference(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?
    {
        let range = edit_range(source, completion.location.span)?;
        for candidate in &completion.candidates {
            let item = CompletionItem {
                label: candidate.identity.to_string(),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.replacement.into(),
                })),
                data: Some(json!({
                    "generation":state.generation(), "version":document.version(),
                    "path":completion.path, "source_sha256":completion.source_sha256,
                    "location":completion.location, "preimage":completion.preimage,
                    "field":completion.field, "candidate":candidate,
                })),
                ..Default::default()
            };
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else if let Some(completion) = analysis
        .complete_symbolic_reference(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?
    {
        let range = edit_range(source, completion.location.span)?;
        for candidate in completion.candidates {
            let item = CompletionItem {
                label: candidate.replacement.clone(),
                detail: Some(candidate.title.clone()),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.replacement.clone(),
                })),
                data: Some(json!({
                    "generation":state.generation(), "version":document.version(),
                    "path":completion.path, "source_sha256":completion.source_sha256,
                    "location":completion.location, "preimage":completion.preimage,
                    "target":candidate,
                })),
                ..Default::default()
            };
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else if let Some(completion) = analysis
        .complete_heading(document.analysis_path(), source, offset, maximum)
        .map_err(|_| unavailable())?
    {
        let range = edit_range(source, completion.location.span)?;
        for candidate in completion.candidates {
            let item = CompletionItem {
                label: candidate.fragment.clone(),
                detail: Some(candidate.text.clone()),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.fragment.clone(),
                })),
                data: Some(
                    json!({"generation":state.generation(),"version":document.version(),
                    "path":completion.path,"source_sha256":completion.source_sha256,
                    "location":completion.location,"preimage":completion.preimage,"heading":candidate}),
                ),
                ..Default::default()
            };
            serde_json::to_writer(&mut budget, &item).map_err(|_| unavailable())?;
            items.push(item);
        }
    } else {
        return Ok(None);
    }
    for item in &mut items {
        if let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit {
            let start = positions::offset(source, edit.range.start).map_err(|_| unavailable())?;
            // Editors match from the edit's start, including a source quote even
            // when the chosen declaration uses a different quote or escape form.
            let quote = source[start..]
                .chars()
                .next()
                .filter(|c| matches!(c, '\'' | '"'));
            item.filter_text = Some(match quote {
                Some(quote) => format!("{quote}{}", item.label),
                None => item.label.clone(),
            });
        }
    }
    let result = CompletionList {
        is_incomplete: false,
        items,
    };
    let mut bounded = LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut bounded, &result).map_err(|_| unavailable())?;
    Ok(Some(result))
}

fn page_resource_items(
    state: &Session,
    id: &str,
    assistance: &gazette_sdk::authoring::Assistance<'_>,
    source: &str,
) -> Result<Option<Vec<CompletionItem>>, ()> {
    use gazette_sdk::workspace::AuthoringPathSegment::{ArrayItem, Field};
    if assistance.scope != gazette_sdk::authoring::AssistanceScope::AuthoredFrontmatter {
        return Ok(None);
    }
    let Some(hover) = assistance.help.hover.as_ref() else {
        return Ok(None);
    };
    let location = assistance
        .help
        .completion_location
        .unwrap_or(hover.location);
    let resource_input = matches!(
        hover.path.as_slice(),
        [Field(resources), ArrayItem, Field(inputs), ArrayItem]
            if resources == "resources" && inputs == "inputs"
    ) || matches!(
        hover.path.as_slice(),
        [Field(resources), ArrayItem, Field(build), Field(entry)]
            if resources == "resources" && build == "build" && entry == "entry"
    );
    if !resource_input {
        return Ok(None);
    }
    let Some(ownership) = state.page_package(id).map_err(|_| ())? else {
        return Ok(None);
    };
    let range = edit_range(source, location.span).map_err(|_| ())?;
    let mut paths = BTreeSet::new();
    for resource in ownership.resources {
        paths.extend(resource.inputs.iter().cloned());
        paths.insert(resource.build.entry().clone());
    }
    let items = paths
        .into_iter()
        .map(|path| {
            let outputs = ownership
                .resources
                .iter()
                .filter(|resource| resource.inputs.contains(&path))
                .map(|resource| resource.output.clone())
                .collect::<Vec<_>>();
            Ok(CompletionItem {
                label: path.to_string(),
                kind: Some(CompletionItemKind::FILE),
                detail: Some("Compiler-resolved page-package resource input".into()),
                documentation: Some(Documentation::String(format!(
                    "Retained by page package {}. Compiled destination(s): {}",
                    ownership.content,
                    outputs
                        .iter()
                        .map(|output| output.output().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))),
                insert_text_mode: Some(InsertTextMode::AS_IS),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: serde_json::to_string(path.as_str()).map_err(|_| ())?,
                })),
                data: Some(json!({
                    "generation": state.generation(),
                    "path": assistance.path,
                    "source_sha256": assistance.source_sha256,
                    "owner": ownership.content,
                    "resource_input": path,
                    "compiled_destinations": outputs,
                })),
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>, ()>>()?;
    Ok(Some(items))
}

fn edit_range(
    source: &str,
    span: gazette_sdk::source::Span,
) -> Result<lsp_types::Range, (ErrorCode, &'static str)> {
    let range = positions::range(source, span).ok_or((
        ErrorCode::InvalidRequest,
        "replacement is outside exact current source coordinates",
    ))?;
    if range.start.line != range.end.line {
        return Err((
            ErrorCode::InvalidRequest,
            "multiline source cannot use an LSP completion edit",
        ));
    }
    Ok(range)
}
