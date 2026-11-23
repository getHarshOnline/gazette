//! Selected-buffer authoring analysis through the shared application.

mod diagnostics;
#[cfg(feature = "native-local")]
pub(crate) use diagnostics::{authored_diagnostics, file_diagnostics, reference_diagnostics};
mod symbols;
pub use diagnostics::{
    Diagnostic, DiagnosticError, diagnostics, diagnostics_for, package_diagnostics,
};
pub use symbols::{SearchSymbol, SymbolKind, SymbolSearch};
pub mod session;

pub use gazette_application::authoring::{
    Analysis, Assistance, AssistanceScope, AuthoredReferenceCandidate, AuthoredReferenceCompletion,
    ComponentAssistance, ComponentCandidate, ComponentCompletion, ComponentCoordinates,
    ComponentHover, DocumentationAuthoring, DocumentationClosure, DocumentationManualPageAuthoring,
    EditorialCandidate, EditorialReferenceCompletion, Error, Failure, HeadingCompletion, Headings,
    Input, LandingChildren, Limits, ManualBaseline, ManualBinding, Navigation, NavigationError,
    PackageBaseline, PackageBaselineKey, PackageFailure, PagePackageOwnership, PagePackageRole,
    PageResourceNavigation, ReferenceCandidate, ReferenceCompletion, SourceLink, SourceLocation,
    SymbolicCandidate, SymbolicReferenceCompletion, WorkspaceIdentity,
};

/// Resolve documentation navigation/origins through the shared target preflight.
/// This is effect-free: it neither acquires source trees nor invokes generators.
///
/// # Errors
/// Returns the existing documentation error when the resolved Mintlify plan is
/// structurally incompatible; no partial per-site view is returned.
pub fn documentation(
    analysis: &Analysis,
) -> Result<Vec<DocumentationAuthoring>, gazette_application::build::DocumentationError> {
    analysis.documentation_authoring(&gazette_adapter_markdown::Comrak)
}

/// Analyze supplied current inputs without native reads, writes or publication.
///
/// # Errors
/// Refuses ambiguous selections and exhausted complete-result limits. Invalid
/// source/semantics remain native phase failures inside the current analysis.
pub fn analyze<'s>(
    workspace: Input<'s>,
    documents: impl IntoIterator<Item = Input<'s>>,
    limits: Limits,
) -> Result<Analysis, Error> {
    gazette_application::authoring::analyze(
        workspace,
        documents,
        limits,
        &gazette_adapter_markdown::Comrak,
    )
}

/// Analyze supplied current inputs with explicitly keyed Theme and plugin package baselines.
///
/// Workspace package bindings remain the sole owners of source and manifest coordinates;
/// callers supply only typed package identities and complete current manifest bytes.
///
/// # Errors
/// Refuses ambiguous package identities and exhausted complete-result limits. Invalid
/// source/semantics remain native phase failures inside the current analysis.
pub fn analyze_with_package_baselines<'s>(
    workspace: Input<'s>,
    documents: impl IntoIterator<Item = Input<'s>>,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
) -> Result<Analysis, Error> {
    gazette_application::authoring::analyze_with_package_baselines(
        workspace,
        documents,
        package_baselines,
        limits,
        &gazette_adapter_markdown::Comrak,
    )
}

/// Analyze one exact modular workspace document set with explicitly keyed package baselines.
///
/// # Errors
/// Refuses incomplete or ambiguous workspace/package selections and exhausted complete-result
/// limits. Invalid source/semantics remain native phase failures inside the current analysis.
pub fn analyze_workspace_documents_with_package_baselines<'s>(
    root: crate::source::EntryPath,
    workspace_documents: Vec<Input<'s>>,
    documents: impl IntoIterator<Item = Input<'s>>,
    package_baselines: impl IntoIterator<Item = PackageBaseline<'s>>,
    limits: Limits,
) -> Result<Analysis, Error> {
    gazette_application::authoring::analyze_workspace_documents_with_package_baselines(
        root,
        workspace_documents,
        documents,
        package_baselines,
        limits,
        &gazette_adapter_markdown::Comrak,
    )
}
