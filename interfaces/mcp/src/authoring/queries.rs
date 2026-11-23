//! Source-bound SDK evidence projected without a second parser or resolver.

use gazette_cli::sdk::{
    authoring::{
        self,
        session::{Document, Session},
    },
    source::{EntryPath, Location, Sha256Digest},
};
use rmcp::model::{CallToolResult, ErrorData};
use serde::Serialize;

use crate::tools::{encode, failure};

use super::wire::{BoundDiagnostic, BoundPrecision, Repair};

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Report<'a, T> {
    #[schemars(range(max = u64::MAX))]
    generation: u64,
    document: &'a str,
    path: &'a EntryPath,
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    version: Option<i32>,
    source_sha256: Sha256Digest,
    value: T,
}

/// Native source coordinates joined to the exact selected document and current overlay.
#[derive(Serialize, schemars::JsonSchema, PartialEq, Eq)]
pub(super) struct SelectedLocation<'a> {
    document: &'a str,
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    version: Option<i32>,
    #[serde(flatten)]
    source: authoring::SourceLocation<'a>,
}

/// An opaque retained page input selected by an authored resource scalar.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct SelectedPageInput<'a> {
    document: &'a str,
    path: &'a EntryPath,
    source_sha256: Sha256Digest,
    input: &'a EntryPath,
    compiled_destinations: Vec<&'a gazette_cli::sdk::publication::routing::RoutePath>,
}

/// One standard definition target without inventing coordinates for opaque bytes.
#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(super) enum SelectedDefinition<'a> {
    Source(SelectedLocation<'a>),
    PageInput(SelectedPageInput<'a>),
}

#[derive(Serialize, schemars::JsonSchema)]
struct File<'a> {
    document: &'a str,
    path: &'a EntryPath,
    manual: bool,
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    version: Option<i32>,
    source_sha256: Sha256Digest,
}

#[derive(Serialize, schemars::JsonSchema)]
struct PageInputFile<'a> {
    document: &'a str,
    path: &'a EntryPath,
    source_sha256: Sha256Digest,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Status<'a> {
    session: &'a str,
    #[schemars(range(max = u64::MAX))]
    generation: u64,
    structurally_valid: bool,
    analysis_error: Option<&'static str>,
    files: Vec<File<'a>>,
    page_inputs: Vec<PageInputFile<'a>>,
}

/// One diagnostic-gated proposal set which remains inert until a caller reviews
/// and submits changed source through the ordinary authoring lifecycle.
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct RepairReview<'a> {
    review_required: bool,
    diagnostic: &'a BoundDiagnostic,
    proposal: RepairProposal<'a>,
}

/// Existing shared authoring projections which can carry a reviewable repair.
#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(super) enum RepairProposal<'a> {
    Assistance(Box<authoring::Assistance<'a>>),
    Reference(ReferenceProposal<'a>),
    Heading(authoring::HeadingCompletion<'a>),
}

/// Native reference namespaces have distinct required field shapes.
/// No protocol-local parser or candidate selection occurs.
#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(super) enum ReferenceProposal<'a> {
    Workspace(authoring::ReferenceCompletion<'a>),
    Editorial(authoring::EditorialReferenceCompletion<'a>),
    Authored(authoring::AuthoredReferenceCompletion<'a>),
    Symbolic(authoring::SymbolicReferenceCompletion<'a>),
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(super) enum Symbol<'a> {
    Heading {
        heading: &'a gazette_cli::sdk::markdown::Heading,
        /// Full-file location; heading.span remains the native body-relative range.
        location: Location,
    },
    Declaration {
        declaration: &'a gazette_cli::sdk::workspace::WorkspaceSymbol,
        /// Original TOML scalar construct; not an editable unquoted token.
        location: Location,
    },
}

fn report<T: Serialize>(
    session: &Session,
    id: &str,
    document: &Document,
    value: T,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    encode(
        &Report {
            generation: session.generation(),
            document: id,
            path: document.path(),
            version: document.version(),
            source_sha256: Sha256Digest::of(document.text().as_bytes()),
            value,
        },
        false,
        maximum,
    )
}

pub(super) fn status(
    handle: &str,
    session: &Session,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    encode(
        &Status {
            session: handle,
            generation: session.generation(),
            structurally_valid: session
                .analysis()
                .as_ref()
                .is_ok_and(authoring::Analysis::structurally_valid),
            analysis_error: session
                .analysis()
                .as_ref()
                .err()
                .map(authoring::Error::code),
            files: session
                .files()
                .iter()
                .map(|(id, document)| File {
                    document: id,
                    path: document.path(),
                    manual: session.is_manual(id),
                    version: document.version(),
                    source_sha256: Sha256Digest::of(document.text().as_bytes()),
                })
                .collect(),
            page_inputs: session
                .page_inputs()
                .iter()
                .map(|(id, input)| PageInputFile {
                    document: id,
                    path: input.path(),
                    source_sha256: input.source_sha256(),
                })
                .collect(),
        },
        false,
        maximum,
    )
}

pub(super) fn page_package(
    session: &Session,
    id: &str,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let (path, version, source_sha256) = if let Some(document) = session.files().get(id) {
        (
            document.path(),
            document.version(),
            Sha256Digest::of(document.text().as_bytes()),
        )
    } else if let Some(input) = session.page_inputs().get(id) {
        (input.path(), None, input.source_sha256())
    } else {
        return unknown(maximum);
    };
    let value = match session.page_package(id) {
        Ok(value) => value,
        Err(error) => {
            return failure("authoring_page_package", &error.to_string(), maximum);
        }
    };
    encode(
        &Report {
            generation: session.generation(),
            document: id,
            path,
            version,
            source_sha256,
            value,
        },
        false,
        maximum,
    )
}

// Inspection and repair revalidation must use the same selected-source role.
fn current_diagnostics<'a>(
    session: &'a Session,
    id: &str,
    analysis: &'a authoring::Analysis,
    document: &'a Document,
    maximum: usize,
) -> Result<Vec<authoring::Diagnostic<'a>>, authoring::DiagnosticError> {
    match session.package_baseline(id) {
        Some(package) => authoring::package_diagnostics(
            analysis,
            package,
            document.path(),
            document.text(),
            maximum,
        ),
        None => authoring::diagnostics_for(
            analysis,
            document.analysis_path(),
            document.path(),
            document.text(),
            maximum,
        ),
    }
}

pub(super) fn diagnostics(
    session: &Session,
    id: &str,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Complete current selected-source analysis is unavailable; no prior result substituted.",
                maximum,
            );
        }
    };
    match current_diagnostics(session, id, analysis, document, maximum) {
        Ok(value) => report(session, id, document, value, maximum),
        Err(error) => failure("authoring_diagnostics", &error.to_string(), maximum),
    }
}

pub(super) fn symbols(
    session: &Session,
    id: &str,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Current source symbol analysis is unavailable.",
                maximum,
            );
        }
    };
    let mut symbols = Vec::new();
    match analysis.workspace_symbols(document.analysis_path(), document.text()) {
        Ok(Some(declarations)) => {
            for declaration in declarations {
                if declaration.location().span.slice(document.text()).is_none() {
                    return failure(
                        "authoring_source",
                        "Current declaration coordinates are invalid.",
                        maximum,
                    );
                }
                symbols.push(Symbol::Declaration {
                    declaration,
                    location: declaration.location(),
                });
            }
            return report(session, id, document, symbols, maximum);
        }
        Ok(None) => (),
        Err(error) => return failure("authoring_symbols_unavailable", &error.to_string(), maximum),
    }
    let headings = match analysis.heading_symbols(document.analysis_path(), document.text()) {
        Ok(headings) => headings,
        Err(error) => return failure("authoring_symbols_unavailable", &error.to_string(), maximum),
    };
    if let Some(headings) = headings {
        for heading in &headings.values {
            let Some(span) = heading.span.in_parent(document.text(), headings.body_span) else {
                return failure(
                    "authoring_source",
                    "Current heading source coordinates are invalid.",
                    maximum,
                );
            };
            symbols.push(Symbol::Heading {
                heading,
                location: Location {
                    span,
                    precision: heading.precision,
                },
            });
        }
    }
    report(session, id, document, symbols, maximum)
}

pub(super) fn search(
    session: &Session,
    query: &str,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match session.search_symbols(query, maximum) {
        Ok(result) => encode(&result, false, maximum),
        Err(error) => failure("authoring_symbols_unavailable", &error.to_string(), maximum),
    }
}

pub(super) fn hover(
    session: &Session,
    id: &str,
    offset: usize,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Current workspace identity analysis is unavailable.",
                maximum,
            );
        }
    };
    match analysis.workspace_identity(document.analysis_path(), document.text(), offset) {
        Ok(value) => report(session, id, document, value, maximum),
        Err(error) => failure("authoring_navigation", &error.to_string(), maximum),
    }
}

pub(super) fn assistance(
    session: &Session,
    id: &str,
    offset: usize,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Current authoring analysis is unavailable.",
                maximum,
            );
        }
    };
    match analysis.assistance(document.analysis_path(), document.text(), offset, maximum) {
        Ok(value) => report(session, id, document, value, maximum),
        Err(error) => failure("authoring_assistance", &error.to_string(), maximum),
    }
}

pub(super) fn component_assistance(
    session: &Session,
    id: &str,
    offset: usize,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Current component authoring analysis is unavailable.",
                maximum,
            );
        }
    };
    match analysis.component_assistance(document.analysis_path(), document.text(), offset, maximum)
    {
        Ok(value) => report(session, id, document, value, maximum),
        Err(error) => failure(
            "authoring_component_assistance",
            &error.to_string(),
            maximum,
        ),
    }
}

pub(super) fn completion(
    session: &Session,
    id: &str,
    offset: usize,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let Ok(analysis) = session.analysis() else {
        return failure(
            "authoring_navigation",
            "Current reference completion analysis is unavailable.",
            maximum,
        );
    };
    let proposal = reference_proposal(analysis, document, offset, maximum);
    match proposal {
        Ok(value) => report(session, id, document, value, maximum),
        Err(error) => failure("authoring_navigation", &error.to_string(), maximum),
    }
}

fn reference_proposal<'a>(
    analysis: &'a authoring::Analysis,
    document: &'a Document,
    offset: usize,
    maximum: usize,
) -> Result<Option<ReferenceProposal<'a>>, authoring::NavigationError> {
    let path = document.analysis_path();
    let source = document.text();
    if let Some(value) = analysis.complete_reference(path, source, offset, maximum)? {
        return Ok(Some(ReferenceProposal::Workspace(value)));
    }
    if let Some(value) = analysis.complete_editorial_reference(path, source, offset, maximum)? {
        return Ok(Some(ReferenceProposal::Editorial(value)));
    }
    if let Some(value) = analysis.complete_authored_reference(path, source, offset, maximum)? {
        return Ok(Some(ReferenceProposal::Authored(value)));
    }
    Ok(analysis
        .complete_symbolic_reference(path, source, offset, maximum)?
        .map(ReferenceProposal::Symbolic))
}

pub(super) fn repair<'a>(
    session: &'a Session,
    input: &'a Repair,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(&input.document) else {
        return unknown(maximum);
    };
    let digest = Sha256Digest::of(document.text().as_bytes());
    if document.version() != input.version || digest != input.source_sha256 {
        return failure(
            "authoring_stale_source",
            "The selected source version or digest changed; inspect current authoring status and diagnostics.",
            maximum,
        );
    }
    if !document.text().is_char_boundary(input.offset) {
        return failure(
            "authoring_diagnostic_binding",
            "The repair position is outside an exact current UTF-8 source boundary; no repair proposed.",
            maximum,
        );
    }
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Complete current selected-source analysis is unavailable; no prior result substituted.",
                maximum,
            );
        }
    };
    let diagnostics =
        match current_diagnostics(session, &input.document, analysis, document, maximum) {
            Ok(diagnostics) => diagnostics,
            Err(error) => {
                return failure("authoring_diagnostics", &error.to_string(), maximum);
            }
        };
    let Some(diagnostic) = diagnostics
        .iter()
        .find(|diagnostic| exact_diagnostic(diagnostic, &input.diagnostic))
    else {
        return failure(
            "authoring_diagnostic_binding",
            "The supplied diagnostic is not an exact current Gazette diagnostic; no repair proposed.",
            maximum,
        );
    };
    if diagnostic.selection_wide {
        return report(
            session,
            &input.document,
            document,
            Option::<RepairReview<'_>>::None,
            maximum,
        );
    }
    if diagnostic.location.is_some_and(|location| {
        !(location.span.start <= input.offset && input.offset < location.span.end)
    }) {
        return failure(
            "authoring_diagnostic_binding",
            "The repair position is outside the exact current diagnostic span; no repair proposed.",
            maximum,
        );
    }
    // Package manifests have native diagnostics, but no native repair producer.
    // Return the documented unassisted outcome only after exact revalidation.
    if session.package_baseline(&input.document).is_some() {
        return report(
            session,
            &input.document,
            document,
            Option::<RepairReview<'_>>::None,
            maximum,
        );
    }
    let proposal = match analysis.assistance(
        document.analysis_path(),
        document.text(),
        input.offset,
        maximum,
    ) {
        Ok(Some(value))
            if value.help.completion_location.is_some() && !value.help.candidates.is_empty() =>
        {
            Some(RepairProposal::Assistance(Box::new(value)))
        }
        Ok(_) => match reference_proposal(analysis, document, input.offset, maximum) {
            Ok(Some(value)) => Some(RepairProposal::Reference(value)),
            Ok(None) => match analysis.complete_heading(
                document.analysis_path(),
                document.text(),
                input.offset,
                maximum,
            ) {
                Ok(value) => value.map(RepairProposal::Heading),
                Err(error) => {
                    return failure("authoring_navigation", &error.to_string(), maximum);
                }
            },
            Err(error) => return failure("authoring_navigation", &error.to_string(), maximum),
        },
        Err(error) => return failure("authoring_assistance", &error.to_string(), maximum),
    };
    let proposal = proposal.filter(|proposal| {
        diagnostic.location.is_some()
            || matches!(
                proposal,
                RepairProposal::Reference(ReferenceProposal::Workspace(value))
                    if diagnostic.code == "workspace_reference"
                        && diagnostic
                            .key
                            .is_some_and(|identity| value.matches_unlocated_diagnostic(identity))
            )
    });
    let value = proposal.map(|proposal| RepairReview {
        review_required: true,
        diagnostic: &input.diagnostic,
        proposal,
    });
    report(session, &input.document, document, value, maximum)
}

fn exact_diagnostic(actual: &authoring::Diagnostic<'_>, expected: &BoundDiagnostic) -> bool {
    actual.path == &expected.path
        && actual.source_sha256 == &expected.source_sha256
        && actual.code == expected.code
        && actual.message == expected.message
        && actual.field == expected.field.0.as_deref()
        && actual.key.map(|key| key.as_str()) == expected.key.0.as_deref()
        && actual.source.map(|key| key.as_str()) == expected.source.0.as_deref()
        && actual.target.map(|key| key.as_str()) == expected.target.0.as_deref()
        && actual.selection_wide == expected.selection_wide
        && match (actual.location, expected.location.0) {
            (None, None) => true,
            (Some(actual), Some(expected)) => {
                actual.span.start == expected.span.start
                    && actual.span.end == expected.span.end
                    && matches!(
                        (actual.precision, expected.precision),
                        (
                            gazette_cli::sdk::source::Precision::Construct,
                            BoundPrecision::Construct
                        ) | (
                            gazette_cli::sdk::source::Precision::Exact,
                            BoundPrecision::Exact
                        )
                    )
            }
            _ => false,
        }
}

pub(super) fn heading_completion(
    session: &Session,
    id: &str,
    offset: usize,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let Ok(analysis) = session.analysis() else {
        return failure(
            "authoring_navigation",
            "Current heading completion analysis is unavailable.",
            maximum,
        );
    };
    match analysis.complete_heading(document.analysis_path(), document.text(), offset, maximum) {
        Ok(value) => report(session, id, document, value, maximum),
        Err(error) => failure("authoring_navigation", &error.to_string(), maximum),
    }
}

pub(super) fn navigation(
    session: &Session,
    id: &str,
    offset: usize,
    references: Option<bool>,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let Some(document) = session.files().get(id) else {
        return unknown(maximum);
    };
    let analysis = match session.analysis() {
        Ok(analysis) => analysis,
        Err(error) => {
            return failure(
                error.code(),
                "Current navigation analysis is unavailable.",
                maximum,
            );
        }
    };
    if references.is_none() {
        let resource = match session.page_resource_navigation(id, offset) {
            Ok(value) => value,
            Err(error) => return failure("authoring_navigation", &error.to_string(), maximum),
        };
        if let Some(resource) = resource {
            if resource.path != document.path()
                || resource.source_sha256 != Sha256Digest::of(document.text().as_bytes())
                || resource.occurrence.span.slice(document.text()).is_none()
            {
                return failure(
                    "authoring_source",
                    "Current resource-navigation source binding is invalid.",
                    maximum,
                );
            }
            let Some((target_id, target)) = session
                .page_inputs()
                .iter()
                .find(|(_, target)| target.path() == resource.companion.path())
            else {
                return failure(
                    "authoring_source",
                    "Resource-navigation target is not selected.",
                    maximum,
                );
            };
            if Some(target.source_sha256()) != resource.companion.source_sha256() {
                return failure(
                    "authoring_source",
                    "Current resource-navigation target identity is invalid.",
                    maximum,
                );
            }
            return report(
                session,
                id,
                document,
                Some(SelectedDefinition::PageInput(SelectedPageInput {
                    document: target_id,
                    path: target.path(),
                    source_sha256: target.source_sha256(),
                    input: resource.input,
                    compiled_destinations: resource.compiled_destinations,
                })),
                maximum,
            );
        }
    }
    let value = match analysis.source_navigation(document.analysis_path(), document.text(), offset)
    {
        Ok(value) => value,
        Err(error) => return failure("authoring_navigation", &error.to_string(), maximum),
    };
    let mut locations = Vec::new();
    if let Some(value) = value {
        if value.path != document.analysis_path()
            || value.source_sha256 != Sha256Digest::of(document.text().as_bytes())
            || value.occurrence.span.slice(document.text()).is_none()
        {
            return failure(
                "authoring_source",
                "Current navigation source binding is invalid.",
                maximum,
            );
        }
        if let Some(include_declaration) = references {
            locations = value.references;
            if include_declaration {
                locations.push(value.definition);
            }
        } else {
            locations.push(value.definition);
        }
    }
    let mut selected = Vec::new();
    let mut retained = 0usize;
    for location in locations {
        let Some((id, target)) = session
            .files()
            .iter()
            .find(|(_, target)| target.analysis_path() == location.path)
        else {
            return failure(
                "authoring_source",
                "Navigation target is not selected.",
                maximum,
            );
        };
        if location.source_sha256 != Sha256Digest::of(target.text().as_bytes())
            || location.location.span.slice(target.text()).is_none()
        {
            return failure(
                "authoring_source",
                "Current navigation target coordinates are invalid.",
                maximum,
            );
        }
        let Some(bytes) = retained
            .checked_add(id.len())
            .and_then(|n| n.checked_add(target.path().as_str().len()))
            .and_then(|n| n.checked_add(std::mem::size_of::<SelectedLocation<'_>>()))
            .filter(|n| *n <= maximum)
        else {
            return failure(
                "authoring_source",
                "Complete navigation output exceeds its bound.",
                maximum,
            );
        };
        retained = bytes;
        selected.push(SelectedLocation {
            document: id,
            version: target.version(),
            source: authoring::SourceLocation {
                path: target.path(),
                source_sha256: location.source_sha256,
                location: location.location,
            },
        });
    }
    selected.sort_by(|left, right| {
        left.source.path.cmp(right.source.path).then_with(|| {
            (
                left.source.location.span.start,
                left.source.location.span.end,
            )
                .cmp(&(
                    right.source.location.span.start,
                    right.source.location.span.end,
                ))
        })
    });
    selected.dedup();
    if references.is_some() {
        report(session, id, document, selected, maximum)
    } else {
        report(
            session,
            id,
            document,
            selected.into_iter().next().map(SelectedDefinition::Source),
            maximum,
        )
    }
}

fn unknown(maximum: usize) -> Result<CallToolResult, ErrorData> {
    failure(
        "authoring_unknown_document",
        "Document is not in the explicit selection; no native lookup attempted.",
        maximum,
    )
}
