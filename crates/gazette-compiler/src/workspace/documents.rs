//! Explicit multi-document workspace assembly through the one workspace schema.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::source::{EntryPath, Sha256Digest, Span};
use serde::Serialize;

use super::{
    Compilation, CompositionOriginQuery, Declaration, DeclarationKind, Diagnostic, Error,
    FieldQuery, Key, OriginError, SelectionField, SelectionOrigin, SelectionScope,
    WorkspaceDocumentsNavigation, WorkspaceNavigation, resolve_declaration,
};
use crate::toml::{self, ParsedSet, PathSegment};

/// Exact retained identity of one configuration document in a successful assembly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DocumentEvidence {
    /// Root-relative logical path selected by the root document.
    pub path: EntryPath,
    /// Digest of the exact unmodified document bytes.
    pub source_sha256: Sha256Digest,
}

/// Resolved workspace plus every exact contributing configuration document.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentCompilation {
    /// One resolution of the merged typed declarations.
    pub compilation: Compilation,
    /// Root and included documents in lexical path order.
    pub documents: Vec<DocumentEvidence>,
}

/// Exact document-local field origin from the same successful assembly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DocumentFieldOrigin {
    /// Configuration document containing the declaration.
    pub document: EntryPath,
    /// Digest of that document, distinct from the complete-set digest.
    pub source_sha256: Sha256Digest,
    /// Stable resolved identity and namespace.
    pub identity: Key,
    /// Typed declaration namespace containing the identity.
    pub kind: DeclarationKind,
    /// Document-local typed syntax path.
    pub path: Vec<PathSegment>,
    /// Document-local half-open UTF-8 byte range.
    pub span: Span,
}

/// Exact document-local origin for one winning composition selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DocumentCompositionFieldOrigin {
    /// Winning declaration and inheritance origin, not an assumed file-local override.
    pub selection: SelectionOrigin,
    /// Configuration document containing the winning selection.
    pub document: EntryPath,
    /// Digest of that exact document.
    pub source_sha256: Sha256Digest,
    /// Document-local typed syntax path.
    pub path: Vec<PathSegment>,
    /// Document-local half-open UTF-8 byte range.
    pub span: Span,
}

#[derive(Clone)]
struct Location {
    document: EntryPath,
    path: Vec<PathSegment>,
}

/// Retained multi-document syntax, one resolved workspace and document-local origins.
pub struct ParsedWorkspaceDocuments<'s> {
    syntax: ParsedSet<'s>,
    compilation: Compilation,
    evidence: Vec<DocumentEvidence>,
    declarations: BTreeMap<(DeclarationKind, Key), Location>,
    workspace_composition: Option<EntryPath>,
    navigation: WorkspaceDocumentsNavigation,
}

impl<'s> ParsedWorkspaceDocuments<'s> {
    /// Explain an effective publication field across the exact contributing documents.
    ///
    /// # Errors
    /// Rejects unknown sites, absent fields and merged tables with multiple origins.
    pub fn publication_origin(
        &self,
        query: &super::PublicationOriginQuery,
    ) -> Result<super::PublicationFieldOrigin, OriginError> {
        super::publication_origins::origin(
            &self.compilation.workspace,
            self.syntax
                .documents()
                .map(|(path, syntax)| (Some(path), syntax)),
            query,
        )
    }

    /// Parse exactly the supplied root and its explicitly named, non-recursive includes.
    ///
    /// # Errors
    ///
    /// Rejects invalid syntax, incomplete or recursive document sets, duplicate
    /// singleton/declaration state, exhausted bounds and unresolved workspace meaning.
    pub fn parse<'p>(
        root: &EntryPath,
        sources: impl IntoIterator<Item = (&'p EntryPath, &'s [u8])>,
        limits: toml::SetLimits,
    ) -> Result<Self, Error> {
        let syntax = ParsedSet::parse(sources, limits).map_err(Error::TomlSet)?;
        let (compilation, evidence, declarations, workspace_composition, navigation) =
            assemble(root, &syntax, limits)?;
        Ok(Self {
            syntax,
            compilation: compilation?,
            evidence,
            declarations,
            workspace_composition,
            navigation: navigation?,
        })
    }

    /// Borrow the one resolved workspace and complete-set identity.
    pub fn compilation(&self) -> &Compilation {
        &self.compilation
    }

    /// Borrow every exact contributing document's identity.
    pub fn documents(&self) -> &[DocumentEvidence] {
        &self.evidence
    }

    /// Borrow compiler-owned declarations and typed references across every document.
    pub fn navigation(&self) -> &WorkspaceDocumentsNavigation {
        &self.navigation
    }

    /// Discard retained syntax after moving out the resolved result and evidence.
    pub fn into_compilation(self) -> DocumentCompilation {
        DocumentCompilation {
            compilation: self.compilation,
            documents: self.evidence,
        }
    }

    /// Move the resolved result, document evidence and cross-document source facts.
    pub fn into_parts(self) -> (DocumentCompilation, WorkspaceDocumentsNavigation) {
        (
            DocumentCompilation {
                compilation: self.compilation,
                documents: self.evidence,
            },
            self.navigation,
        )
    }

    /// Resolve one explicit declaration field to its owning document and local span.
    ///
    /// # Errors
    ///
    /// Rejects an unknown resolved identity or a field without an explicit source span.
    pub fn origin(&self, query: &FieldQuery) -> Result<DocumentFieldOrigin, OriginError> {
        let location = self
            .declarations
            .get(&(query.kind, query.identity.clone()))
            .ok_or(OriginError::UnknownIdentity)?;
        let syntax = self
            .syntax
            .document(&location.document)
            .ok_or(OriginError::UnknownIdentity)?;
        let path = super::site_declaration::field_origin_path(&location.path, &query.field);
        let span = syntax.span_at(&path).ok_or(OriginError::NoExplicitField)?;
        Ok(DocumentFieldOrigin {
            document: location.document.clone(),
            source_sha256: Sha256Digest::of(syntax.source().as_bytes()),
            identity: query.identity.clone(),
            kind: query.kind,
            path,
            span,
        })
    }

    /// Explain a winning composition atom without hiding which document declared it.
    ///
    /// # Errors
    ///
    /// Rejects unknown sites, absent selections and fields without explicit source spans.
    pub fn composition_origin(
        &self,
        query: &CompositionOriginQuery,
    ) -> Result<DocumentCompositionFieldOrigin, OriginError> {
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
            SelectionField::Capability { capability } => composition
                .capabilities()
                .get(capability)
                .map(|v| v.origin()),
            SelectionField::Plugin { package } => {
                composition.plugins().get(package).map(|v| v.origin())
            }
            SelectionField::Resource { resource } => {
                composition.resources().get(resource).map(|v| v.origin())
            }
        }
        .ok_or(OriginError::NoExplicitField)?;
        let (document, mut path) = match &selection.scope {
            SelectionScope::Theme(_) => return Err(OriginError::NoExplicitField),
            SelectionScope::Workspace => (
                self.workspace_composition
                    .as_ref()
                    .ok_or(OriginError::NoExplicitField)?,
                Vec::new(),
            ),
            SelectionScope::Site(site) => {
                let location = self
                    .declarations
                    .get(&(DeclarationKind::Site, site.clone()))
                    .ok_or(OriginError::UnknownIdentity)?;
                (&location.document, location.path.clone())
            }
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
            SelectionField::Capability { capability } => {
                path.push(PathSegment::Key("capabilities".into()));
                path.push(PathSegment::Key(capability.as_str().into()));
            }
            SelectionField::Plugin { package } => {
                path.push(PathSegment::Key("plugins".into()));
                path.push(PathSegment::Key(package.as_str().into()));
            }
            SelectionField::Resource { resource } => {
                path.push(PathSegment::Key("resources".into()));
                path.push(PathSegment::Key(resource.as_str().into()));
            }
        }
        path.extend_from_slice(&query.path);
        let syntax = self
            .syntax
            .document(document)
            .ok_or(OriginError::UnknownIdentity)?;
        super::site_declaration::theme_origin_path(syntax, &mut path);
        let span = syntax.span_at(&path).ok_or(OriginError::NoExplicitField)?;
        Ok(DocumentCompositionFieldOrigin {
            selection: selection.clone(),
            document: document.clone(),
            source_sha256: Sha256Digest::of(syntax.source().as_bytes()),
            path,
            span,
        })
    }
}

/// Read the root's exact include set before resolving defaults from those documents.
///
/// # Errors
///
/// Rejects invalid syntax, malformed includes, exhausted bounds and duplicate paths.
/// The assembled document set is checked against the complete workspace schema.
pub fn included_documents(source: &str, limits: toml::Limits) -> Result<Vec<EntryPath>, Error> {
    root_selection(source, limits).map(|selection| selection.includes)
}

/// Files selected by the root before native acquisition; the binding is not an included module.
#[derive(Debug, serde::Deserialize)]
pub struct RootSelection {
    /// Exact additional configuration documents.
    #[serde(default)]
    pub includes: Vec<EntryPath>,
    /// Exact machine binding, interpreted separately by the application.
    #[serde(default)]
    pub machine_binding: Option<EntryPath>,
}

/// Read the root's exact configuration and machine-binding selections.
///
/// # Errors
/// Rejects malformed paths, duplicate includes and a binding also selected as a module.
pub fn root_selection(source: &str, limits: toml::Limits) -> Result<RootSelection, Error> {
    let syntax = crate::toml::ParsedToml::parse(source, limits).map_err(Error::Toml)?;
    let declaration: RootSelection = syntax.deserialize().map_err(Error::Toml)?;
    validate_includes(&declaration.includes)?;
    if let Some(binding) = &declaration.machine_binding
        && declaration.includes.contains(binding)
    {
        return Err(invalid(
            "workspace_machine_binding_document",
            binding.as_str(),
        ));
    }
    Ok(declaration)
}

/// Compile an explicit complete document set without filesystem discovery.
///
/// # Errors
///
/// Propagates complete-set parsing, module-set and workspace-resolution failures
/// without returning a partial compilation.
pub fn compile_documents<'s, 'p>(
    root: &EntryPath,
    sources: impl IntoIterator<Item = (&'p EntryPath, &'s [u8])>,
    limits: toml::SetLimits,
) -> Result<DocumentCompilation, Error> {
    ParsedWorkspaceDocuments::parse(root, sources, limits).map(|parsed| parsed.into_compilation())
}

type Assembly = (
    Result<Compilation, Error>,
    Vec<DocumentEvidence>,
    BTreeMap<(DeclarationKind, Key), Location>,
    Option<EntryPath>,
    Result<WorkspaceDocumentsNavigation, Error>,
);

// Semantic compilation and source navigation can fail independently after
// syntax and exact document membership have been checked.
type DocumentInspectionResults = (
    Result<Compilation, Error>,
    Result<WorkspaceDocumentsNavigation, Error>,
);

/// Inspect one explicit document set without admitting invalid workspace meaning.
/// Typed current source navigation remains available for unique repair proposals.
///
/// # Errors
/// Rejects syntax/schema failures, invalid document membership and exhausted bounds.
/// Resolution and navigation failures remain independent results.
pub fn inspect_documents<'p, 's>(
    root: &EntryPath,
    sources: impl IntoIterator<Item = (&'p EntryPath, &'s [u8])>,
    limits: toml::SetLimits,
) -> Result<DocumentInspectionResults, Error> {
    let syntax = ParsedSet::parse(sources, limits).map_err(Error::TomlSet)?;
    let (compilation, _, _, _, navigation) = assemble(root, &syntax, limits)?;
    Ok((compilation, navigation))
}

fn document_error(path: &EntryPath, error: toml::Error) -> Error {
    Error::TomlSet(toml::SetError::Documents(vec![toml::DocumentError::Toml {
        path: path.clone(),
        error,
    }]))
}

fn assemble(
    root: &EntryPath,
    syntax: &ParsedSet<'_>,
    limits: toml::SetLimits,
) -> Result<Assembly, Error> {
    let Some(root_syntax) = syntax.document(root) else {
        return Err(invalid("workspace_document_root", root.as_str()));
    };
    let inheritance = super::publication_defaults::Inheritance::collect(
        syntax.documents().map(|(_, document)| document),
    )?;
    let root_declaration = inheritance.read(root_syntax).map_err(|error| match error {
        Error::Toml(error) => document_error(root, error),
        error => error,
    })?;
    validate_includes(&root_declaration.includes)?;
    if let Some(binding) = &root_declaration.machine_binding
        && (binding == root || root_declaration.includes.contains(binding))
    {
        return Err(invalid(
            "workspace_machine_binding_document",
            binding.as_str(),
        ));
    }
    if root_declaration.includes.iter().any(|path| path == root) {
        return Err(invalid("workspace_include_root", root.as_str()));
    }
    let expected: BTreeSet<_> = std::iter::once(root.clone())
        .chain(root_declaration.includes.iter().cloned())
        .collect();
    let actual: BTreeSet<_> = syntax.documents().map(|(path, _)| path.clone()).collect();
    if expected != actual {
        let field = expected
            .symmetric_difference(&actual)
            .map(EntryPath::as_str)
            .collect::<Vec<_>>()
            .join(",");
        return Err(invalid("workspace_document_set", &field));
    }

    let mut merged = Declaration::default();
    let mut singleton_owner = BTreeMap::<&'static str, EntryPath>::new();
    let mut declarations = BTreeMap::new();
    let mut evidence = Vec::new();
    let mut navigation = Vec::new();
    for (path, document) in syntax.documents() {
        let mut part = inheritance.read(document).map_err(|error| match error {
            Error::Toml(error) => document_error(path, error),
            error => error,
        })?;
        if path != root && !part.includes.is_empty() {
            return Err(invalid("workspace_include_nested", path.as_str()));
        }
        if path != root && part.machine_binding.is_some() {
            return Err(invalid("workspace_machine_binding_nested", path.as_str()));
        }
        navigation.push((
            path.clone(),
            WorkspaceNavigation::build(document, &part, limits.document)?,
            document.source(),
        ));
        evidence.push(DocumentEvidence {
            path: path.clone(),
            source_sha256: Sha256Digest::of(document.source().as_bytes()),
        });
        macro_rules! singleton {
            ($field:ident) => {
                if document.has_root_key(stringify!($field)) {
                    if singleton_owner
                        .insert(stringify!($field), path.clone())
                        .is_some()
                    {
                        return Err(invalid(
                            "workspace_document_singleton_duplicate",
                            stringify!($field),
                        ));
                    }
                    merged.$field = part.$field;
                }
            };
        }
        singleton!(build);
        singleton!(machine_binding);
        singleton!(dot_renderer);
        singleton!(social_card_renderer);
        singleton!(math_typesetter);
        singleton!(policy_check);
        singleton!(policy_output_transport);
        singleton!(policy_source);
        singleton!(composition);
        singleton!(defaults);
        merged.publishers.append(&mut part.publishers);
        for (name, source) in part.design_inputs {
            if merged.design_inputs.contains_key(&name) {
                return Err(invalid(
                    "workspace_duplicate",
                    &format!("design_inputs[{:?}]", name.as_str()),
                ));
            }
            merged.design_inputs.insert(name, source);
        }
        record(
            &mut declarations,
            path,
            DeclarationKind::Brand,
            &part.brands,
        );
        record(
            &mut declarations,
            path,
            DeclarationKind::Principal,
            &part.principals,
        );
        record(
            &mut declarations,
            path,
            DeclarationKind::Account,
            &part.accounts,
        );
        record(&mut declarations, path, DeclarationKind::Site, &part.sites);
        record(
            &mut declarations,
            path,
            DeclarationKind::Project,
            &part.projects,
        );
        record(
            &mut declarations,
            path,
            DeclarationKind::Taxonomy,
            &part.taxonomies,
        );
        record(
            &mut declarations,
            path,
            DeclarationKind::Extension,
            &part.extensions,
        );
        record(
            &mut declarations,
            path,
            DeclarationKind::Source,
            &part.sources,
        );
        for (index, site) in part.sites.iter().enumerate() {
            if site.explicit_brand
                && let Some(brand) = &site.brand
            {
                declarations
                    .entry((DeclarationKind::Brand, brand.id.clone()))
                    .or_insert(Location {
                        document: path.clone(),
                        path: vec![
                            PathSegment::Key("sites".into()),
                            PathSegment::Index(index),
                            PathSegment::Key("brand".into()),
                        ],
                    });
            }
        }
        merged.fact_sources.append(&mut part.fact_sources);
        merged.brands.append(&mut part.brands);
        merged.principals.append(&mut part.principals);
        merged.accounts.append(&mut part.accounts);
        merged.sites.append(&mut part.sites);
        merged.projects.append(&mut part.projects);
        merged.taxonomies.append(&mut part.taxonomies);
        merged.extensions.append(&mut part.extensions);
        merged.sources.append(&mut part.sources);
        merged.theme_packages.append(&mut part.theme_packages);
        merged.plugin_packages.append(&mut part.plugin_packages);
        merged.placements.append(&mut part.placements);
        merged
            .documentation
            .mounts
            .append(&mut part.documentation.mounts);
    }
    merged.includes.clear();
    let semantic_limits = toml::Limits {
        source_bytes: limits.source_bytes,
        nodes: limits.nodes,
        depth: limits.document.depth,
    };
    let compilation =
        resolve_declaration(root_syntax, merged, semantic_limits).map(|mut compilation| {
            compilation.source_sha256 = document_set_digest(syntax);
            compilation
        });
    let navigation = WorkspaceDocumentsNavigation::build(navigation, limits);
    Ok((
        compilation,
        evidence,
        declarations,
        singleton_owner.get("composition").cloned(),
        navigation,
    ))
}

trait Identified {
    fn id(&self) -> &Key;
}

macro_rules! identified {
    ($($ty:ty),+ $(,)?) => {$(
        impl Identified for $ty {
            fn id(&self) -> &Key { &self.id }
        }
    )+};
}

identified!(
    super::accounts::AccountDeclaration,
    super::Brand,
    gazette_core::publication::Principal,
    super::site_declaration::SiteDeclaration,
    gazette_core::publication::Taxonomy,
    gazette_core::publication::ExtensionSchema,
    super::Source,
);

impl Identified for super::ProjectDeclaration {
    fn id(&self) -> &Key {
        self.id()
    }
}

fn record<T: Identified>(
    locations: &mut BTreeMap<(DeclarationKind, Key), Location>,
    document: &EntryPath,
    kind: DeclarationKind,
    values: &[T],
) {
    for (index, value) in values.iter().enumerate() {
        locations
            .entry((kind, value.id().clone()))
            .or_insert(Location {
                document: document.clone(),
                path: vec![
                    PathSegment::Key(kind.table().into()),
                    PathSegment::Index(index),
                ],
            });
    }
}

fn validate_includes(includes: &[EntryPath]) -> Result<(), Error> {
    let mut unique = BTreeSet::new();
    for include in includes {
        if !unique.insert(include) {
            return Err(invalid("workspace_include_duplicate", include.as_str()));
        }
    }
    Ok(())
}

fn document_set_digest(syntax: &ParsedSet<'_>) -> Sha256Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"gazette-workspace-documents/1\0");
    for (path, document) in syntax.documents() {
        bytes.extend_from_slice(&(path.as_str().len() as u64).to_be_bytes());
        bytes.extend_from_slice(path.as_str().as_bytes());
        bytes.extend_from_slice(&(document.source().len() as u64).to_be_bytes());
        bytes.extend_from_slice(document.source().as_bytes());
    }
    Sha256Digest::of(&bytes)
}

fn invalid(code: &'static str, field: &str) -> Error {
    Error::Invalid(vec![Diagnostic {
        code,
        field: field.to_owned(),
        key: None,
    }])
}
