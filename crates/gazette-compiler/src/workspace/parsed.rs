//! A resolved workspace and its original syntax share one immutable source binding.

use std::fmt;

use gazette_core::source::Span;
use serde::{Deserialize, Serialize};

use super::{
    Compilation, Error, Key, SelectionField, SelectionOrigin, SelectionScope, Sha256Digest,
    WorkspaceInspection, WorkspaceNavigation,
};
use crate::toml::{self, ParsedToml, PathSegment};

/// Retained workspace syntax for inspection and editor queries after full resolution.
///
/// Compilation still owns the resolved values. The index records only their
/// declaration coordinates before semantic sorting; it is not another value store.
#[derive(Debug)]
pub struct ParsedWorkspace<'s> {
    syntax: ParsedToml<'s>,
    compilation: Compilation,
    navigation: WorkspaceNavigation,
}

/// Exact declaration namespace; a source binding is not a publication identity.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationKind {
    /// Brand publication settings.
    Brand,
    /// Public account with independently declared ownership.
    Account,
    /// Person or organization reference.
    Principal,
    /// Publication site.
    Site,
    /// Native project identity and state.
    Project,
    /// Explicit taxonomy catalog; term coordinates remain relative to this declaration.
    Taxonomy,
    /// Workspace-selected domain data contract, separate from presentation components.
    Extension,
    /// Configured native source binding, not the identity of its contents.
    Source,
}

impl DeclarationKind {
    pub(super) fn table(self) -> &'static str {
        match self {
            Self::Brand => "brands",
            Self::Account => "accounts",
            Self::Principal => "principals",
            Self::Site => "sites",
            Self::Project => "projects",
            Self::Taxonomy => "taxonomies",
            Self::Extension => "extensions",
            Self::Source => "sources",
        }
    }
}

/// Typed source query shared by SDK and interface transports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldQuery {
    /// Exact declaration namespace; no display-name or prefix inference.
    pub kind: DeclarationKind,
    /// Exact key within the selected declaration namespace.
    pub identity: Key,
    /// Keys and array positions relative to that object's original declaration.
    pub field: Vec<PathSegment>,
}

/// Exact source evidence for a declared field, not an inherited value or owner approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct FieldOrigin {
    /// Exact source bytes used by the same workspace compilation.
    pub source_sha256: Sha256Digest,
    /// Stable resolved object identity; never a display-name lookup.
    pub identity: Key,
    /// Namespace in which that key resolved.
    pub kind: DeclarationKind,
    /// Full syntax path in the original document, before semantic sorting.
    pub path: Vec<PathSegment>,
    /// Half-open UTF-8 byte range in that document, not a character/UTF-16 range.
    pub span: Span,
}

/// Locate the winning effective selection, then an explicit field within that atom.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompositionOriginQuery {
    /// Exact consuming site, including nondeployable sites.
    pub site: Key,
    /// Theme, plugin, capability or resource selection.
    pub field: SelectionField,
    /// Relative syntax path; empty selects the entire winning atom.
    #[serde(default)]
    pub path: Vec<PathSegment>,
}

/// Effective selection's declaration coordinates in the original workspace input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CompositionFieldOrigin {
    /// Winning declaration and inheritance origin, not an assumed local override.
    pub selection: SelectionOrigin,
    /// Exact source bytes used by the same compilation.
    pub source_sha256: Sha256Digest,
    /// Full original syntax path, including unsorted site index when applicable.
    pub path: Vec<PathSegment>,
    /// Original half-open UTF-8 range; missing fields receive no invented span.
    pub span: Span,
}

/// A source query cannot establish the requested explicit declaration location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginError {
    /// No successfully resolved object has this exact key.
    UnknownIdentity,
    /// No explicit source range exists at this path; defaults are never fabricated.
    NoExplicitField,
}

impl fmt::Display for OriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownIdentity => "origin identity is absent from the resolved workspace",
            Self::NoExplicitField => {
                "origin field has no explicit source range; check the typed key/index path"
            }
        })
    }
}

impl std::error::Error for OriginError {}

impl<'s> ParsedWorkspace<'s> {
    /// Explain a scalar or list inherited by a publication, retaining its source span.
    ///
    /// # Errors
    /// Rejects unknown sites, absent fields and merged tables with multiple origins.
    pub fn publication_origin(
        &self,
        query: &super::PublicationOriginQuery,
    ) -> Result<super::PublicationFieldOrigin, OriginError> {
        super::publication_origins::origin(
            &self.compilation.workspace,
            [(None, &self.syntax)],
            query,
        )
    }

    /// Explain a resolved selection with the existing retained AST; no reparse or merge.
    ///
    /// # Errors
    /// Rejects unknown sites, absent selections and fields without explicit source spans.
    pub fn composition_origin(
        &self,
        query: &CompositionOriginQuery,
    ) -> Result<CompositionFieldOrigin, OriginError> {
        let composition = self
            .compilation
            .workspace
            .composition_for(&query.site)
            .ok_or(OriginError::UnknownIdentity)?;
        let selection = match &query.field {
            SelectionField::Theme {} => composition.theme().map(|v| v.origin()),
            SelectionField::Chrome {} => composition
                .chrome()
                .and_then(|v| v.field_origin(&query.path)),
            SelectionField::PageTemplates {} => composition.page_templates().map(|v| v.origin()),
            SelectionField::Navigation {} => composition.navigation().map(|v| v.origin()),
            SelectionField::Footer {} => composition.footer().map(|v| v.origin()),
            SelectionField::Invitation { invitation } => composition
                .invitations()
                .get(invitation)
                .map(|v| v.origin()),
            SelectionField::Capability { capability: key } => {
                composition.capabilities().get(key).map(|v| v.origin())
            }
            SelectionField::Plugin { package } => {
                composition.plugins().get(package).map(|v| v.origin())
            }
            SelectionField::Resource { resource: key } => {
                composition.resources().get(key).map(|v| v.origin())
            }
        }
        .ok_or(OriginError::NoExplicitField)?;
        let mut path = match &selection.scope {
            SelectionScope::Theme(_) => return Err(OriginError::NoExplicitField),
            SelectionScope::Workspace => Vec::new(),
            SelectionScope::Site(site) => self
                .navigation
                .declaration_path(DeclarationKind::Site, site)
                .ok_or(OriginError::UnknownIdentity)?
                .to_vec(),
        };
        path.push(PathSegment::Key("composition".into()));
        match &selection.field {
            SelectionField::Theme {} => path.push(PathSegment::Key("theme".into())),
            SelectionField::Chrome {} => path.push(PathSegment::Key("chrome".into())),
            SelectionField::PageTemplates {} => {
                path.push(PathSegment::Key("page_templates".into()))
            }
            SelectionField::Navigation {} => path.push(PathSegment::Key("navigation".into())),
            SelectionField::Footer {} => path.push(PathSegment::Key("footer".into())),
            SelectionField::Invitation { invitation } => {
                path.push(PathSegment::Key("invitations".into()));
                path.push(PathSegment::Key(invitation.as_str().into()));
            }
            SelectionField::Capability { capability: key } => {
                path.push(PathSegment::Key("capabilities".into()));
                path.push(PathSegment::Key(key.as_str().into()));
            }
            SelectionField::Plugin { package } => {
                path.push(PathSegment::Key("plugins".into()));
                path.push(PathSegment::Key(package.as_str().into()));
            }
            SelectionField::Resource { resource: key } => {
                path.push(PathSegment::Key("resources".into()));
                path.push(PathSegment::Key(key.as_str().into()));
            }
        }
        path.extend_from_slice(&query.path);
        super::site_declaration::theme_origin_path(&self.syntax, &mut path);
        let span = self
            .syntax
            .span_at(&path)
            .ok_or(OriginError::NoExplicitField)?;
        Ok(CompositionFieldOrigin {
            selection: selection.clone(),
            source_sha256: self.compilation.source_sha256,
            path,
            span,
        })
    }
    /// Parse once, resolve the complete topology, and retain original declaration positions.
    ///
    /// # Errors
    /// Uses the same parser and whole-workspace resolver as [`super::compile`].
    /// Invalid declarations return neither a partial graph nor a usable origin index.
    pub fn parse(source: &'s str, limits: toml::Limits) -> Result<Self, Error> {
        let inspected = WorkspaceInspection::parse(source, limits)?;
        Ok(Self {
            syntax: inspected.syntax,
            compilation: inspected.compilation?,
            navigation: inspected.navigation?,
        })
    }

    /// The same immutable resolved result returned by the ordinary compiler.
    pub fn compilation(&self) -> &Compilation {
        &self.compilation
    }

    /// Borrow declaration/reference coordinates from the same successful parse.
    pub fn navigation(&self) -> &WorkspaceNavigation {
        &self.navigation
    }

    /// Move resolved values and compact source facts without retaining parser lifetimes.
    pub fn into_parts(self) -> (Compilation, WorkspaceNavigation) {
        (self.compilation, self.navigation)
    }

    /// Original bytes, including comments, Unicode, quote choice and line endings.
    pub fn source(&self) -> &str {
        self.syntax.source()
    }

    /// Resolve an object's explicit field to its original syntax range.
    ///
    /// Field indices use original source order. Relationship targets are not
    /// followed implicitly. This does not yet explain imports or inheritance;
    /// no default, inferred field or derived value receives a fabricated span.
    ///
    /// # Errors
    /// Rejects unknown identities and paths without an explicit parser-owned span.
    pub fn origin(&self, query: &FieldQuery) -> Result<FieldOrigin, OriginError> {
        let declaration = self
            .navigation
            .declaration_path(query.kind, &query.identity)
            .ok_or(OriginError::UnknownIdentity)?;
        let path = super::site_declaration::field_origin_path(declaration, &query.field);
        let span = self
            .syntax
            .span_at(&path)
            .ok_or(OriginError::NoExplicitField)?;
        Ok(FieldOrigin {
            source_sha256: self.compilation.source_sha256,
            identity: query.identity.clone(),
            kind: query.kind,
            path,
            span,
        })
    }
}
