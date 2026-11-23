//! LSP projections of current SDK evidence; no parsing or semantic reconstruction.

use gazette_sdk::authoring::session::{Document, Session};
use gazette_sdk::{authoring, source::Sha256Digest};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, NumberOrString, Position,
    PublishDiagnosticsParams, Range, SymbolKind,
};
use serde_json::json;

use crate::{
    Error, Limits,
    positions::{position, range},
};

pub(super) fn check(
    state: &Session,
    limits: Limits,
) -> Result<Vec<PublishDiagnosticsParams>, Error> {
    let mut result = Vec::new();
    let mut count = 0usize;
    for (uri, document) in state.files() {
        let diagnostics = current(state, uri, document, limits)?;
        count = count
            .checked_add(diagnostics.len())
            .filter(|n| *n <= limits.diagnostics)
            .ok_or(Error::Limit("diagnostics"))?;
        result.push(PublishDiagnosticsParams {
            uri: uri
                .parse()
                .map_err(|_| Error::Protocol("selected URI is invalid"))?,
            diagnostics,
            version: document.version(),
        });
    }
    // Bound the complete set, not just each separately framed notification.
    let mut writer = gazette_sdk::render::LimitedWriter::new(std::io::sink(), limits.output_bytes);
    let encoded = serde_json::to_writer(&mut writer, &result);
    if writer.exceeded() {
        return Err(Error::Limit("diagnostic set bytes"));
    }
    encoded.map_err(Error::Encoding)?;
    Ok(result)
}

pub(super) fn current(
    state: &Session,
    id: &str,
    document: &Document,
    limits: Limits,
) -> Result<Vec<Diagnostic>, Error> {
    let source = document.text();
    let whole = Range::new(Position::new(0, 0), position(source, source.len())?);
    match state.analysis() {
        Ok(analysis) => {
            let views = match state.package_baseline(id) {
                Some(package) => authoring::package_diagnostics(
                    analysis,
                    package,
                    document.path(),
                    source,
                    limits.output_bytes,
                ),
                None => authoring::diagnostics_for(
                    analysis,
                    document.analysis_path(),
                    document.path(),
                    source,
                    limits.output_bytes,
                ),
            }
            .map_err(|error| match error {
                authoring::DiagnosticError::Limit => Error::Limit("diagnostic projection bytes"),
                authoring::DiagnosticError::InvalidSource => {
                    Error::Protocol("diagnostic source mismatch")
                }
            })?;
            let mut diagnostics = views
                .into_iter()
                .map(|view| {
                    let located = view
                        .location
                        .and_then(|location| range(source, location.span));
                    let mut data = serde_json::to_value(&view).map_err(Error::Encoding)?;
                    data["generation"] = json!(state.generation());
                    let message = view.explanation(source);
                    Ok(Diagnostic {
                        range: located.unwrap_or(whole),
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: Some(NumberOrString::String(view.code.into())),
                        source: Some("gazette".into()),
                        message,
                        data: Some(data),
                        ..Diagnostic::default()
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            if let Some(ownership) = state
                .page_package(id)
                .map_err(|_| Error::Protocol("page-package source binding unavailable"))?
                && matches!(ownership.role, authoring::PagePackageRole::Document)
            {
                let marker = Range::new(Position::new(0, 0), Position::new(0, 0));
                // Ordinary ownership is context, exposed by authoringContext and
                // pagePackage. Only actionable companion findings belong here.
                for companion in ownership.package.companions().iter().filter(|companion| {
                    companion.kind() == gazette_sdk::authored::PageCompanionKind::Unhandled
                }) {
                    diagnostics.push(Diagnostic {
                        range: marker,
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(NumberOrString::String(
                            "page_package_unhandled_companion".into(),
                        )),
                        source: Some("gazette".into()),
                        message: format!(
                            "Retained page-package companion '{}' has no implicit compiler or transport.",
                            companion.relative()
                        ),
                        data: Some(json!({
                            "generation":state.generation(),
                            "content":ownership.content,
                            "document":ownership.package.document(),
                            "companion":companion,
                        })),
                        ..Diagnostic::default()
                    });
                }
            }
            if document.analysis_path() == analysis.workspace() {
                match authoring::documentation(analysis) {
                    Ok(documentation) => {
                        for view in documentation {
                            if let authoring::DocumentationClosure::Unavailable { reason } =
                                &view.closure
                            {
                                diagnostics.push(Diagnostic {
                                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                                    severity: Some(DiagnosticSeverity::INFORMATION),
                                    code: Some(NumberOrString::String(
                                        "documentation_validation_incomplete".into(),
                                    )),
                                    source: Some("gazette".into()),
                                    message: (*reason).into(),
                                    data: Some(json!({
                                        "generation":state.generation(),
                                        "documentation":view,
                                    })),
                                    ..Diagnostic::default()
                                });
                            }
                        }
                    }
                    Err(error) => diagnostics.push(Diagnostic {
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: Some(NumberOrString::String(error.code().into())),
                        source: Some("gazette".into()),
                        message: error.to_string(),
                        data: Some(json!({
                            "generation":state.generation(),
                            "path":document.path(),
                            "documentation_preflight":true,
                            "selection_wide":true,
                        })),
                        ..Diagnostic::default()
                    }),
                }
            }
            Ok(diagnostics)
        }
        Err(authoring::Error::UnavailableSources(_)) if id != state.workspace_id() => Ok(vec![]),
        Err(error) => Ok(vec![Diagnostic {
            range: whole,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(error.code().into())),
            source: Some("gazette".into()),
            message: match error {
                authoring::Error::UnavailableSources(_) => error.to_string(),
                _ => "complete selected-source analysis is unavailable".into(),
            },
            data: Some(json!({
                "generation":state.generation(), "path":document.path(),
                "source_sha256":Sha256Digest::of(source.as_bytes()),
                "selection_wide":true, "location":null,
            })),
            ..Diagnostic::default()
        }]),
    }
}

pub(super) fn symbols(state: &Session, document: &Document) -> Result<Vec<DocumentSymbol>, Error> {
    let analysis = state
        .analysis()
        .as_ref()
        .map_err(|_| Error::Protocol("analysis unavailable"))?;
    let source = document.text();
    if let Some(declarations) = analysis
        .workspace_symbols(document.analysis_path(), source)
        .map_err(|_| Error::Protocol("current workspace analysis unavailable"))?
    {
        return declarations
            .iter()
            .map(|declaration| {
                let range = range(source, declaration.location().span)
                    .ok_or(Error::Protocol("workspace symbol coordinates unavailable"))?;
                #[allow(deprecated)]
                Ok(DocumentSymbol {
                    name: declaration.name().to_owned(),
                    detail: Some(format!(
                        "{:?} ({:?} source range)",
                        declaration.kind(),
                        declaration.location().precision
                    )),
                    kind: SymbolKind::OBJECT,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                })
            })
            .collect();
    }
    let Some(headings) = analysis
        .heading_symbols(document.analysis_path(), source)
        .map_err(|_| Error::Protocol("current heading analysis unavailable"))?
    else {
        return Ok(Vec::new());
    };
    headings
        .values
        .iter()
        .map(|heading| {
            let span = heading
                .span
                .in_parent(source, headings.body_span)
                .ok_or(Error::Protocol("heading source mismatch"))?;
            let range =
                range(source, span).ok_or(Error::Protocol("heading coordinates unavailable"))?;
            // These are flat containing constructs, not invented whole-section ranges
            // or precise rename tokens. Both ranges preserve that same limited extent.
            #[allow(deprecated)]
            Ok(DocumentSymbol {
                name: if heading.text.is_empty() {
                    heading.id.clone()
                } else {
                    heading.text.clone()
                },
                detail: Some(format!(
                    "{} ({:?} source range)",
                    heading.fragment, heading.precision
                )),
                kind: SymbolKind::NAMESPACE,
                tags: None,
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            })
        })
        .collect()
}
