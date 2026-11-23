//! Scheduled protocol lifecycle; selected source and version state has one owner.

use std::{io::Write, sync::mpsc::Receiver};

use gazette_sdk::authoring::session;
use lsp_server::{ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CompletionOptions, CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolParams,
    GotoDefinitionParams, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, LogMessageParams, MessageType, OneOf, PositionEncodingKind, ReferenceParams,
    SaveOptions, ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, WorkspaceSymbolParams,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::scheduling::{Input, Scheduler};
use crate::{
    Error, Limits, code_actions, completion, diagnostics, documents, hover, navigation, symbols,
    transport,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    Ready,
    Shutdown,
}

struct Session {
    phase: Phase,
    documents: Option<documents::State>,
    limits: Limits,
    published: std::collections::BTreeSet<String>,
    hover_format: lsp_types::MarkupKind,
}

pub(super) fn serve(
    receiver: &Receiver<Input>,
    writer: &mut impl Write,
    limits: Limits,
) -> Result<(), Error> {
    let mut session = Session {
        phase: Phase::New,
        documents: None,
        limits,
        published: std::collections::BTreeSet::new(),
        hover_format: lsp_types::MarkupKind::PlainText,
    };
    let mut scheduler = Scheduler::default();
    loop {
        let input = scheduler.next(receiver)?;
        if input.is_mutation() {
            let batch = scheduler.batch(input, receiver)?;
            let mut changed = false;
            for notification in batch.notifications.iter().cloned() {
                changed |= session.mutation(writer, notification)?;
            }
            scheduler.observe(batch.elapsed());
            scheduler.drain(receiver)?;
            if changed && !scheduler.has_newer_mutation(batch.generation) {
                session.publish(writer)?;
            }
            continue;
        }
        match input.input {
            Input::Message(Message::Request(request)) => session.request(writer, request)?,
            Input::Message(Message::Notification(notification)) => {
                if notification.method == "exit" {
                    if !notification.params.is_null() {
                        return Err(Error::Protocol("exit takes no parameters"));
                    }
                    return if session.phase == Phase::Shutdown {
                        Ok(())
                    } else {
                        Err(Error::Protocol("exit before shutdown"))
                    };
                }
                session.notification(writer, notification)?;
            }
            Input::Message(Message::Response(_)) => {
                return Err(Error::Protocol("unsolicited response"));
            }
            Input::ReadError(error) => return Err(error),
            Input::Eof => return Err(Error::Protocol("EOF before exit")),
        }
    }
}

impl Session {
    fn request(&mut self, writer: &mut impl Write, request: Request) -> Result<(), Error> {
        let failure = |code: ErrorCode, reason: &str| {
            Message::Response(Response::new_err(
                request.id.clone(),
                code as i32,
                reason.into(),
            ))
        };
        let success = |value: Value| {
            Message::Response(Response {
                id: request.id.clone(),
                response_result: Ok(value),
            })
        };
        let message = if request.method == "initialize" {
            if self.phase != Phase::New {
                failure(ErrorCode::InvalidRequest, "initialize already completed")
            } else {
                match selection(request.params.clone(), self.limits) {
                    Err(reason) => failure(ErrorCode::InvalidParams, &reason),
                    Ok((documents, hover_format)) => {
                        self.documents = Some(documents);
                        self.hover_format = hover_format;
                        self.phase = Phase::Initializing;
                        success(
                            serde_json::to_value(InitializeResult {
                                capabilities: ServerCapabilities {
                                    position_encoding: Some(PositionEncodingKind::UTF16),
                                    text_document_sync: Some(TextDocumentSyncCapability::Options(
                                        TextDocumentSyncOptions {
                                            open_close: Some(true),
                                            change: Some(TextDocumentSyncKind::FULL),
                                            save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions { include_text: Some(true) })),
                                            ..TextDocumentSyncOptions::default()
                                        },
                                    )),
                                    document_symbol_provider: Some(OneOf::Left(true)),
                                    workspace_symbol_provider: Some(OneOf::Left(true)),
                                    definition_provider: Some(OneOf::Left(true)),
                                    references_provider: Some(OneOf::Left(true)),
                                    document_link_provider: Some(lsp_types::DocumentLinkOptions { resolve_provider: Some(false), work_done_progress_options: Default::default() }),
                                    hover_provider: Some(HoverProviderCapability::Simple(true)),
                                    completion_provider: Some(CompletionOptions {
                                        resolve_provider: Some(false),
                                        trigger_characters: Some(vec!["\"".into(), "'".into(), "=".into(), ".".into(), "[".into()]),
                                        ..Default::default()
                                    }),
                                    code_action_provider: Some(
                                        CodeActionProviderCapability::Options(CodeActionOptions {
                                            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                                            resolve_provider: Some(false),
                                            ..Default::default()
                                        }),
                                    ),
                                    experimental: Some(json!({"gazette":{"authoring":"0.1.0","sourceDiscovery":{"selectSource":"gazette/selectSource"},"context":{"method":"gazette/authoringContext","generationRequired":true},"editorSnapshot":{"manifest":"gazette/editorSnapshot","source":"gazette/editorSource","chunkBytes":65536,"nativeContextFormat":"gazette-editor-context/2"},"origin":{"method":"gazette/origin","generationRequired":true,"nativePositionEncoding":"utf-8","rangePositionEncoding":"utf-16"},"pagePackage":{"method":"gazette/pagePackage","generationRequired":true,"compiledDestinations":"compiler-proven-only"}}})),
                                    ..ServerCapabilities::default()
                                },
                                server_info: Some(ServerInfo {
                                    name: "gazette".into(),
                                    version: Some(env!("CARGO_PKG_VERSION").into()),
                                }),
                            })
                            .map_err(Error::Encoding)?,
                        )
                    }
                }
            }
        } else if matches!(self.phase, Phase::New | Phase::Initializing) {
            failure(
                ErrorCode::ServerNotInitialized,
                "initialize and initialized are required",
            )
        } else if self.phase == Phase::Shutdown {
            failure(ErrorCode::InvalidRequest, "server has shut down")
        } else if request.method == "shutdown" {
            if request.params.is_null() {
                self.phase = Phase::Shutdown;
                self.documents = None;
                success(Value::Null)
            } else {
                failure(ErrorCode::InvalidParams, "shutdown takes no parameters")
            }
        } else if request.method == "gazette/origin" {
            match self.documents.as_ref() {
                None => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                Some(state) => match crate::origin::query(
                    state,
                    request.params.clone(),
                    self.limits.output_bytes,
                ) {
                    Ok(value) => success(value),
                    Err((code, reason)) => failure(code, reason),
                },
            }
        } else if request.method == "gazette/pagePackage" {
            match self.documents.as_ref() {
                None => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                Some(state) => match crate::page_package::query(
                    state,
                    request.params.clone(),
                    self.limits.output_bytes,
                ) {
                    Ok(value) => success(value),
                    Err((code, reason)) => failure(code, reason),
                },
            }
        } else if request.method == "gazette/authoringContext" {
            match self.documents.as_ref() {
                None => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                Some(state) => match crate::context::query(
                    state,
                    request.params.clone(),
                    self.limits.output_bytes,
                ) {
                    Ok(value) => success(value),
                    Err((code, reason)) => failure(code, reason),
                },
            }
        } else if request.method == "gazette/selectSource" {
            let uri = request
                .params
                .as_object()
                .filter(|params| params.len() == 1)
                .and_then(|params| params.get("uri"))
                .and_then(Value::as_str)
                .and_then(|uri| uri.parse::<lsp_types::Uri>().ok());
            match (uri, self.documents.as_mut()) {
                (Some(uri), Some(state)) => {
                    let generation = state.generation();
                    let result = state.select_uri(&uri);
                    let watch_files = state.configuration_uris().map_err(documents::error)?;
                    let changed = generation != state.generation();
                    let generation = state.generation();
                    if changed {
                        self.publish(writer)?;
                    }
                    match result {
                        Ok(selected) => success(
                            json!({"selected":selected,"generation":generation,"watchFiles":watch_files}),
                        ),
                        Err(error) => failure(ErrorCode::InvalidParams, error.reason()),
                    }
                }
                _ => failure(
                    ErrorCode::InvalidParams,
                    "source selection requires one file URI",
                ),
            }
        } else if request.method == "gazette/editorSnapshot"
            || request.method == "gazette/editorSource"
        {
            match self.documents.as_ref() {
                None => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                Some(state) => match if request.method == "gazette/editorSnapshot" {
                    crate::snapshot::manifest(state, self.limits.output_bytes)
                } else {
                    crate::snapshot::source(state, request.params.clone(), self.limits.output_bytes)
                } {
                    Ok(value) => success(value),
                    Err((code, reason)) => failure(code, reason),
                },
            }
        } else if request.method == "workspace/symbol" {
            let params = if request.params.get("query").is_some_and(Value::is_string) {
                decode::<WorkspaceSymbolParams>(request.params.clone())
            } else {
                Err(())
            };
            match (params, self.documents.as_ref()) {
                (Err(()), _) => failure(
                    ErrorCode::InvalidParams,
                    "workspace symbol query must be a string",
                ),
                (Ok(_), None) => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                (Ok(params), Some(state)) => {
                    match symbols::query(state, &params.query, self.limits.output_bytes) {
                        Ok(value) => success(value),
                        Err((code, reason)) => failure(code, reason),
                    }
                }
            }
        } else if request.method == "textDocument/completion" {
            let named = ["textDocument", "position"]
                .iter()
                .all(|field| request.params.get(*field).is_some_and(Value::is_object))
                && request.params.get("context").is_none_or(Value::is_object);
            let params = if named {
                decode::<CompletionParams>(request.params.clone())
            } else {
                Err(())
            };
            match (params, self.documents.as_ref()) {
                (Err(()), _) => failure(ErrorCode::InvalidParams, "invalid completion parameters"),
                (Ok(_), None) => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                (Ok(params), Some(state)) => match completion::query(
                    state,
                    params.text_document_position,
                    self.limits.output_bytes,
                ) {
                    Ok(value) => success(value),
                    Err((code, reason)) => failure(code, reason),
                },
            }
        } else if request.method == "textDocument/codeAction" {
            let named = ["textDocument", "range", "context"]
                .iter()
                .all(|field| request.params.get(*field).is_some_and(Value::is_object));
            let params = if named {
                decode::<CodeActionParams>(request.params.clone())
            } else {
                Err(())
            };
            match (params, self.documents.as_ref()) {
                (Err(()), _) => failure(ErrorCode::InvalidParams, "invalid codeAction parameters"),
                (Ok(_), None) => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                (Ok(params), Some(state)) => {
                    match code_actions::query(state, params, self.limits) {
                        Ok(value) => success(value),
                        Err((code, reason)) => failure(code, reason),
                    }
                }
            }
        } else if request.method == "textDocument/hover" {
            let named = ["textDocument", "position"]
                .iter()
                .all(|field| request.params.get(*field).is_some_and(Value::is_object));
            let params = if named {
                decode::<HoverParams>(request.params.clone())
            } else {
                Err(())
            };
            match (params, self.documents.as_ref()) {
                (Err(()), _) => failure(ErrorCode::InvalidParams, "invalid hover parameters"),
                (Ok(_), None) => failure(
                    ErrorCode::InvalidRequest,
                    "selected-source session is unavailable",
                ),
                (Ok(params), Some(state)) => match hover::query(
                    state,
                    params.text_document_position_params,
                    self.limits.output_bytes,
                    self.hover_format.clone(),
                ) {
                    Ok(value) => success(value),
                    Err((code, reason)) => failure(code, reason),
                },
            }
        } else if request.method == "textDocument/documentLink" {
            match decode::<lsp_types::DocumentLinkParams>(request.params.clone()) {
                Ok(params) => match self.documents.as_ref() {
                    Some(state) => match navigation::links(
                        state,
                        params.text_document.uri.as_str(),
                        self.limits.output_bytes,
                    ) {
                        Ok(value) => success(value),
                        Err((code, reason)) => failure(code, reason),
                    },
                    None => failure(
                        ErrorCode::InvalidRequest,
                        "selected-source session is unavailable",
                    ),
                },
                Err(()) => failure(ErrorCode::InvalidParams, "invalid document-link parameters"),
            }
        } else if request.method == "textDocument/definition"
            || request.method == "textDocument/references"
        {
            let objects = ["textDocument", "position"]
                .iter()
                .all(|field| request.params.get(*field).is_some_and(Value::is_object))
                && (request.method != "textDocument/references"
                    || request.params.get("context").is_some_and(Value::is_object));
            let selected = if !objects {
                Err(())
            } else if request.method == "textDocument/definition" {
                decode::<GotoDefinitionParams>(request.params.clone())
                    .map(|params| (params.text_document_position_params, None))
            } else {
                decode::<ReferenceParams>(request.params.clone()).map(|params| {
                    (
                        params.text_document_position,
                        Some(params.context.include_declaration),
                    )
                })
            };
            match selected {
                Err(()) => failure(ErrorCode::InvalidParams, "invalid navigation parameters"),
                Ok((params, references)) => match self.documents.as_ref() {
                    None => failure(
                        ErrorCode::InvalidRequest,
                        "selected-source session is unavailable",
                    ),
                    Some(state) => {
                        match navigation::query(state, params, references, self.limits.output_bytes)
                        {
                            Ok(value) => success(value),
                            Err((code, reason)) => failure(code, reason),
                        }
                    }
                },
            }
        } else if request.method == "textDocument/documentSymbol" {
            match decode::<DocumentSymbolParams>(request.params.clone()) {
                Ok(params) => match self.documents.as_ref().and_then(|state| {
                    state
                        .files()
                        .get(params.text_document.uri.as_str())
                        .map(|doc| (state, doc))
                }) {
                    Some((state, document)) => match diagnostics::symbols(state, document) {
                        Ok(symbols) => {
                            success(serde_json::to_value(symbols).map_err(Error::Encoding)?)
                        }
                        Err(_) => failure(
                            ErrorCode::InvalidRequest,
                            "current selected-source analysis is unavailable",
                        ),
                    },
                    None => failure(ErrorCode::InvalidParams, "document is not selected"),
                },
                Err(()) => failure(
                    ErrorCode::InvalidParams,
                    "invalid documentSymbol parameters",
                ),
            }
        } else {
            failure(ErrorCode::MethodNotFound, "unsupported request")
        };
        transport::write(writer, &message, self.limits.output_bytes)
    }

    fn notification(
        &mut self,
        writer: &mut impl Write,
        notification: Notification,
    ) -> Result<(), Error> {
        if notification.method == "initialized" {
            if self.phase == Phase::Initializing
                && decode::<InitializedParams>(notification.params).is_ok()
            {
                self.phase = Phase::Ready;
                return self.publish(writer);
            }
            return self.log(
                writer,
                "lsp_lifecycle: unexpected or malformed initialized notification",
            );
        }
        if self.phase == Phase::Shutdown {
            return Ok(());
        }
        if self.phase != Phase::Ready {
            return self.log(
                writer,
                "lsp_lifecycle: notification ignored before initialization",
            );
        }
        if crate::scheduling::is_document_notification(&notification)
            && self.mutation(writer, notification)?
        {
            self.publish(writer)?;
        }
        Ok(())
    }

    fn mutation(
        &mut self,
        writer: &mut impl Write,
        notification: Notification,
    ) -> Result<bool, Error> {
        if self.phase == Phase::Shutdown {
            return Ok(false);
        }
        if self.phase != Phase::Ready {
            self.log(
                writer,
                "lsp_lifecycle: notification ignored before initialization",
            )?;
            return Ok(false);
        }
        let documents = self
            .documents
            .as_mut()
            .ok_or(Error::Protocol("missing source selection"))?;
        let changed = match notification.method.as_str() {
            "workspace/didChangeWatchedFiles" => {
                if decode::<lsp_types::DidChangeWatchedFilesParams>(notification.params).is_err() {
                    return self
                        .ignored(writer, "lsp_params: malformed watched-file change ignored");
                }
                documents.refresh().map_err(documents::error)?
            }
            "textDocument/didOpen" => {
                if !notification
                    .params
                    .get("textDocument")
                    .is_some_and(Value::is_object)
                {
                    return self
                        .ignored(writer, "lsp_params: didOpen requires a textDocument object");
                }
                let Ok(params) = decode::<DidOpenTextDocumentParams>(notification.params) else {
                    return self.ignored(writer, "lsp_params: malformed didOpen ignored");
                };
                let doc = params.text_document;
                if !documents
                    .open(doc.uri.as_str(), doc.version, doc.text)
                    .map_err(documents::error)?
                {
                    return self.ignored(
                        writer,
                        "lsp_document_open: unselected or already-open document ignored",
                    );
                }
                true
            }
            "textDocument/didSave" => {
                let Ok(params) = decode::<DidSaveTextDocumentParams>(notification.params) else {
                    return self.ignored(writer, "lsp_params: malformed didSave ignored");
                };
                let Some(text) = params.text else {
                    return self.ignored(writer, "lsp_params: didSave requires complete text");
                };
                match documents.save(params.text_document.uri.as_str(), &text) {
                    Ok(changed) => changed,
                    Err(_) => {
                        return self.ignored(
                            writer,
                            "lsp_save: saved text does not match the current selected overlay",
                        );
                    }
                }
            }
            "textDocument/didChange" => {
                if !notification
                    .params
                    .get("textDocument")
                    .is_some_and(Value::is_object)
                    || !notification
                        .params
                        .get("contentChanges")
                        .and_then(Value::as_array)
                        .is_some_and(|v| v.iter().all(Value::is_object))
                {
                    return self.ignored(
                        writer,
                        "lsp_params: didChange requires named document and change objects",
                    );
                }
                let Ok(params) = decode::<DidChangeTextDocumentParams>(notification.params) else {
                    return self.ignored(writer, "lsp_params: malformed didChange ignored");
                };
                if params.content_changes.len() != 1
                    || params.content_changes[0].range.is_some()
                    || params.content_changes[0].range_length.is_some()
                {
                    return self.ignored(
                        writer,
                        "lsp_full_text: exactly one complete replacement is required",
                    );
                }
                let text = params
                    .content_changes
                    .into_iter()
                    .next()
                    .ok_or(Error::Protocol("missing full text"))?
                    .text;
                let changed = match documents.change(
                    params.text_document.uri.as_str(),
                    params.text_document.version,
                    text,
                ) {
                    Ok(changed) => changed,
                    Err(session::Error::Limit(resource)) => {
                        return self.ignored(
                            writer,
                            &format!(
                                "lsp_document_change_limit: full replacement exceeds {resource}; edit refused"
                            ),
                        );
                    }
                    Err(error) => return Err(documents::error(error)),
                };
                if !changed {
                    return self.ignored(
                        writer,
                        "lsp_stale_version: non-increasing or unopened change ignored",
                    );
                }
                true
            }
            "textDocument/didClose" => {
                if !notification
                    .params
                    .get("textDocument")
                    .is_some_and(Value::is_object)
                {
                    return self.ignored(
                        writer,
                        "lsp_params: didClose requires a textDocument object",
                    );
                }
                let Ok(params) = decode::<DidCloseTextDocumentParams>(notification.params) else {
                    return self.ignored(writer, "lsp_params: malformed didClose ignored");
                };
                documents
                    .close(params.text_document.uri.as_str())
                    .map_err(documents::error)?
            }
            _ => false,
        };
        Ok(changed)
    }

    fn publish(&mut self, writer: &mut impl Write) -> Result<(), Error> {
        let state = self
            .documents
            .as_ref()
            .ok_or(Error::Protocol("missing source selection"))?;
        // Prepare all diagnostics before notifications. Writes remain sequential;
        // the marker proves stream delivery, not client application or publication.
        let diagnostics = diagnostics::check(state, self.limits)?;
        let current: std::collections::BTreeSet<_> = state.files().keys().cloned().collect();
        for uri in self.published.difference(&current) {
            self.send(
                writer,
                "textDocument/publishDiagnostics",
                json!({"uri":uri,"diagnostics":[]}),
            )?;
        }
        for params in diagnostics {
            self.send(writer, "textDocument/publishDiagnostics", params)?;
        }
        self.send(
            writer,
            "gazette/analysisPublished",
            json!({
                "generation":state.generation(),
                "structurallyValid":state.analysis().as_ref().is_ok_and(|a| a.structurally_valid()),
            }),
        )?;
        self.published = current;
        Ok(())
    }

    fn log(&self, writer: &mut impl Write, message: &str) -> Result<(), Error> {
        self.send(
            writer,
            "window/logMessage",
            LogMessageParams {
                typ: MessageType::WARNING,
                message: message.into(),
            },
        )
    }

    fn ignored(&self, writer: &mut impl Write, message: &str) -> Result<bool, Error> {
        self.log(writer, message)?;
        Ok(false)
    }

    fn send(
        &self,
        writer: &mut impl Write,
        method: &str,
        params: impl Serialize,
    ) -> Result<(), Error> {
        transport::write(
            writer,
            &Message::Notification(Notification {
                method: method.into(),
                params: serde_json::to_value(params).map_err(Error::Encoding)?,
            }),
            self.limits.output_bytes,
        )
    }
}

fn selection(
    value: Value,
    limits: Limits,
) -> Result<(documents::State, lsp_types::MarkupKind), String> {
    if !value.get("capabilities").is_some_and(Value::is_object) {
        return Err("invalid initialize capabilities".into());
    }
    let params = decode::<InitializeParams>(value)
        .map_err(|_| "invalid initialize parameters".to_owned())?;
    let hover_format = params
        .capabilities
        .text_document
        .as_ref()
        .and_then(|document| document.hover.as_ref())
        .and_then(|hover| hover.content_format.as_ref())
        .and_then(|formats| formats.first())
        .cloned()
        .unwrap_or(lsp_types::MarkupKind::PlainText);
    documents::select_initialization(params, limits).map(|state| (state, hover_format))
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ()> {
    if !value.is_object() {
        return Err(());
    }
    serde_json::from_value(value).map_err(|_| ())
}
