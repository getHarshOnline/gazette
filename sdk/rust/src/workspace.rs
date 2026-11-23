//! Shared workspace operations and diagnostic projections for native interfaces.

pub use gazette_compiler::theme_package::ThemePackageId;
pub use gazette_compiler::workspace::{
    AuthoringError, AuthoringField, AuthoringFieldPresence, AuthoringHelp, AuthoringLiteral,
    AuthoringLiteralCandidate, AuthoringNode, AuthoringPathSegment, AuthoringValueKind,
    Availability, Brand, BrandPublication, Build, BuildOrigin, BuildSelection, CapabilitySelection,
    Compilation, CompletionCandidate, CompletionKind, Composition, CompositionFieldOrigin,
    CompositionOriginQuery, ContentBinding, ContentSource, DeclarationKind, DeclarationSymbol,
    Diagnostic, DocumentCompilation, DocumentCompositionFieldOrigin, DocumentEvidence,
    DocumentFieldOrigin, DocumentationBuild, DocumentationInput, DocumentationManual,
    DocumentationManualAsset, DocumentationManualPage, DocumentationModule,
    DocumentationNavigation, DocumentationRepository, Error, FeedConfiguration, FeedFormat,
    FeedScope, Feeds, FieldOrigin, FieldQuery, GraphConfiguration, GraphvizRenderer, Key,
    LocalDrafts, MachineOutputs, MathTypesetter, NativeMarkdown, OriginError, ParsedWorkspace,
    ParsedWorkspaceDocuments, Placement, PluginPackageBinding, PolicySource, ProjectKind,
    ProjectLifecycle, Requirement, ResolvedAlias, ResolvedComposition, ResolvedContent,
    ResolvedMirror, ResolvedRoute, ResourceInputMount, ResourceInputName, ResourceOwner,
    ResourceRef, ResourceSelection, ResourceUse, SchemaHover, SearchConfiguration, SearchField,
    SearchSelection, Selected, SelectionField, SelectionOrigin, SelectionScope, Site, Source,
    SourceLocation, StaticAssets, TaxonomyPage, ThemePackageBinding, ThemeSelection, Visibility,
    Workspace, WorkspaceAssociation, WorkspaceAuthoring, WorkspaceInspection, WorkspaceNavigation,
    WorkspaceReference, WorkspaceSymbol, WorkspaceSymbolKind, compile, compile_documents,
    included_documents, resolve,
};
pub use gazette_compiler::workspace::{
    PublicationFieldOrigin, PublicationOriginQuery, PublicationScope,
};
pub use gazette_core::{
    plugin::PluginPackageId,
    publication::capability_runtime::{CapabilityAbsence, ProviderId, ProviderRef},
};

use crate::{source::Span, toml};
use serde::Serialize;

/// Lossless diagnostic structure for interface transports, not a partial workspace.
///
/// Semantic diagnostics name logical fields. Only parser-provided byte spans are
/// exposed; editor clients must not mistake logical fields for source positions.
/// Transport framing, size limits and protocol errors remain interface-owned.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Failure<'a> {
    /// Stable SDK failure class.
    pub code: &'static str,
    /// Native explanation; may contain user-supplied identifiers and stays private.
    pub message: String,
    /// Complete ordered semantic diagnostics, absent for syntax and origin errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<&'a [Diagnostic]>,
    /// Original half-open UTF-8 byte range, only when the parser supplied one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

impl<'a> From<&'a Error> for Failure<'a> {
    fn from(error: &'a Error) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
            diagnostics: match error {
                Error::Invalid(diagnostics) => Some(diagnostics),
                Error::TomlSet(_) => None,
                _ => None,
            },
            span: match error {
                Error::Toml(toml::Error::Invalid { span, .. }) => *span,
                Error::TomlSet(_) => None,
                _ => None,
            },
        }
    }
}

impl From<OriginError> for Failure<'_> {
    fn from(error: OriginError) -> Self {
        Self {
            code: match error {
                OriginError::UnknownIdentity => "origin_unknown_identity",
                OriginError::NoExplicitField => "origin_no_explicit_field",
            },
            message: error.to_string(),
            diagnostics: None,
            span: None,
        }
    }
}
