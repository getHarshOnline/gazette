//! Source-order declaration and reference facts over the one typed workspace parser.

use std::collections::BTreeMap;

use gazette_core::source::{EntryPath, Location, Precision, Sha256Digest};
use serde::Serialize;

use super::{Declaration, DeclarationKind, Error, Key, ResourceInputName};
use crate::toml::{self, ParsedToml, PathSegment};
use std::borrow::Cow;

mod accounts;

/// One explicit workspace identity at its native TOML value construct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DeclarationSymbol {
    /// Namespace selected by the typed declaration, never a key-prefix heuristic.
    pub kind: DeclarationKind,
    /// Exact decoded identity; names and provider-owned keys are not declarations.
    pub identity: Key,
    /// Original value range, possibly containing quotes/escapes; not a rename token.
    pub location: Location,
}

/// Typed workspace symbol namespace; a logical resource alias is not a publication Key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "declaration", rename_all = "kebab-case")]
pub enum WorkspaceSymbolKind {
    /// Existing Key-based declaration namespace.
    Declaration(DeclarationKind),
    /// Workspace-local resource input name.
    ResourceInput,
}

/// One native declaration occurrence in the shared workspace navigation index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkspaceSymbol {
    /// Existing namespaced Key declaration with its original scalar location.
    Declaration {
        /// Native Key declaration; no alias-to-Key coercion occurs.
        declaration: DeclarationSymbol,
    },
    /// Plain resource input name at its explicit TOML key, not the source value.
    ResourceInput {
        /// Exact validated workspace-local name.
        name: ResourceInputName,
        /// Parser-owned key construct, including native quoting and escapes.
        location: Location,
    },
}

impl WorkspaceSymbol {
    /// Native namespace, independent of display spelling.
    pub fn kind(&self) -> WorkspaceSymbolKind {
        match self {
            Self::Declaration { declaration } => WorkspaceSymbolKind::Declaration(declaration.kind),
            Self::ResourceInput { .. } => WorkspaceSymbolKind::ResourceInput,
        }
    }
    /// Exact decoded identifier without inferred prefixes.
    pub fn name(&self) -> &str {
        match self {
            Self::Declaration { declaration } => declaration.identity.as_str(),
            Self::ResourceInput { name, .. } => name.as_str(),
        }
    }
    /// Original definition construct; keys and values retain distinct spans.
    pub fn location(&self) -> Location {
        match self {
            Self::Declaration { declaration } => declaration.location,
            Self::ResourceInput { location, .. } => *location,
        }
    }
    /// Borrow a Key declaration only when this symbol actually has that meaning.
    pub fn declaration(&self) -> Option<&DeclarationSymbol> {
        match self {
            Self::Declaration { declaration } => Some(declaration),
            Self::ResourceInput { .. } => None,
        }
    }
    /// Complete TOML string scalar for a reference, never an unquoted alias key.
    ///
    /// # Errors
    /// Refuses a definition range not contained in the exact supplied source.
    pub fn replacement<'a>(&self, source: &'a str) -> Result<Cow<'a, str>, Error> {
        self.location().span.slice(source).ok_or_else(invalid)?;
        Ok(match self {
            Self::Declaration { .. } => {
                Cow::Borrowed(self.location().span.slice(source).ok_or_else(invalid)?)
            }
            Self::ResourceInput { name, .. } => {
                // ResourceInputName admits only [a-z][a-z0-9_-]{0,63}.
                // This closed alphabet has no TOML string escapes: quote the
                // validated name directly, without enabling a general emitter.
                Cow::Owned(format!("\"{}\"", name.as_str()))
            }
        })
    }
}

/// Complete document-local association for one recognized source occurrence.
#[derive(Debug)]
pub struct WorkspaceAssociation<'a> {
    /// The exact symbol supplying the target; no copied semantic record.
    pub symbol: &'a WorkspaceSymbol,
    /// Native construct containing the queried offset.
    pub occurrence: Location,
    /// The declaration's explicit identity value.
    pub definition: Location,
    /// All explicit references in original source order, excluding the declaration.
    pub references: Vec<Location>,
}

#[derive(Debug)]
struct Reference {
    kind: WorkspaceSymbolKind,
    identity: String,
    symbol: Option<usize>,
    identity_sha256: Sha256Digest,
    location: Location,
}

/// One declaration in an explicitly selected workspace document set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DocumentDeclarationSymbol {
    /// Exact configuration document containing the declaration.
    pub document: EntryPath,
    /// Digest of that document's complete current bytes.
    pub source_sha256: Sha256Digest,
    /// Compiler-owned Key declaration or plain resource alias with local coordinates.
    pub symbol: WorkspaceSymbol,
    /// Valid TOML scalar: original Key value or natively formatted alias string.
    replacement: String,
}

impl DocumentDeclarationSymbol {
    /// Complete valid reference scalar, including TOML quotes and escapes.
    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

/// A source construct bound to its exact workspace document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct DocumentSourceLocation<'a> {
    /// Exact logical workspace document.
    pub document: &'a EntryPath,
    /// Digest of that document's complete current bytes.
    pub source_sha256: Sha256Digest,
    /// Document-local native coordinates.
    pub location: Location,
}

/// Complete cross-document association for one recognized workspace occurrence.
#[derive(Debug)]
pub struct DocumentWorkspaceAssociation<'a> {
    /// The compiler-owned declaration supplying the target.
    pub symbol: &'a DocumentDeclarationSymbol,
    /// Queried occurrence in its own document.
    pub occurrence: DocumentSourceLocation<'a>,
    /// Declaration location, which may be in another document.
    pub definition: DocumentSourceLocation<'a>,
    /// Complete explicit references in document/source order.
    pub references: Vec<DocumentSourceLocation<'a>>,
}

#[derive(Debug)]
struct DocumentReference {
    document: EntryPath,
    source_sha256: Sha256Digest,
    reference: Reference,
}

/// One typed reference slot in a modular workspace document set.
#[derive(Debug)]
pub struct DocumentWorkspaceReference<'a> {
    /// Namespace required by this field.
    pub kind: WorkspaceSymbolKind,
    /// Current declaration when resolved across the complete selected set.
    pub symbol: Option<&'a DocumentDeclarationSymbol>,
    /// Exact queried scalar in its owning document.
    pub location: DocumentSourceLocation<'a>,
    /// Exact decoded identity digest.
    pub identity_sha256: Sha256Digest,
    /// Whether this kind/identity identifies exactly one typed reference slot.
    pub unique_identity: bool,
    symbols: &'a [DocumentDeclarationSymbol],
}

impl<'a> DocumentWorkspaceReference<'a> {
    /// Same-namespace declarations in deterministic document/source order.
    pub fn candidates(&self) -> impl Iterator<Item = &'a DocumentDeclarationSymbol> + '_ {
        self.symbols
            .iter()
            .filter(|symbol| symbol.symbol.kind() == self.kind)
    }
}

/// Compiler-owned associations over one explicit non-recursive workspace document set.
#[derive(Debug)]
pub struct WorkspaceDocumentsNavigation {
    symbols: Vec<DocumentDeclarationSymbol>,
    references: Vec<DocumentReference>,
    key_bytes: usize,
}

/// One typed reference slot with a verified whole TOML string scalar.
#[derive(Debug)]
pub struct WorkspaceReference<'a> {
    /// Namespace required by this field, independent of whether its value resolves.
    pub kind: WorkspaceSymbolKind,
    /// Current declaration when resolved; an unknown key never fabricates a target.
    pub symbol: Option<&'a WorkspaceSymbol>,
    /// Exact scalar bytes, including their original delimiters and escapes.
    pub location: Location,
    /// Exact decoded identity digest, retained without reparsing the TOML scalar.
    pub identity_sha256: Sha256Digest,
    /// Whether this kind/identity identifies exactly one typed reference slot.
    pub unique_identity: bool,
    symbols: &'a [WorkspaceSymbol],
}

impl<'a> WorkspaceReference<'a> {
    /// Same-namespace declarations in native source order. These are replacement
    /// proposals, not proof that changing a reference satisfies all cross-field rules.
    pub fn candidates(&self) -> impl Iterator<Item = &'a WorkspaceSymbol> + '_ {
        self.symbols
            .iter()
            .filter(|symbol| symbol.kind() == self.kind)
    }
}

/// Compact typed source facts, distinct from complete workspace resolution.
#[derive(Debug)]
pub struct WorkspaceNavigation {
    symbols: Vec<WorkspaceSymbol>,
    declaration_paths: Vec<Option<Vec<PathSegment>>>,
    references: Vec<Reference>,
    key_bytes: usize,
}

impl WorkspaceNavigation {
    /// Locate only an existing typed reference, never declarations or arbitrary text.
    /// Typed decoding and native string validation precede this view. A proposal
    /// does not establish semantic validity. Callers bind the exact original source.
    pub fn reference_at(&self, offset: usize) -> Option<WorkspaceReference<'_>> {
        let reference = self.references.iter().find(|reference| {
            reference.location.span.start <= offset && offset < reference.location.span.end
        })?;
        Some(WorkspaceReference {
            kind: reference.kind,
            symbol: reference.symbol.map(|index| &self.symbols[index]),
            location: Location {
                precision: Precision::Exact,
                ..reference.location
            },
            identity_sha256: reference.identity_sha256,
            unique_identity: self
                .references
                .iter()
                .filter(|candidate| {
                    candidate.kind == reference.kind
                        && candidate.identity_sha256 == reference.identity_sha256
                })
                .count()
                == 1,
            symbols: &self.symbols,
        })
    }

    /// Key declarations and logical resource aliases, in original source order.
    pub fn symbols(&self) -> &[WorkspaceSymbol] {
        &self.symbols
    }

    /// Stored declaration and reference occurrences, for enclosing authoring budgets.
    pub fn occurrence_count(&self) -> usize {
        self.symbols.len() + self.references.len()
    }

    /// Decoded declaration and alias identifier bytes for enclosing authoring bounds.
    pub fn key_bytes(&self) -> usize {
        self.key_bytes
    }

    /// Resolve a byte within a declared identity/reference value. Whitespace, comments,
    /// arbitrary strings and EOF return none. Callers bind the exact source digest.
    pub fn at(&self, offset: usize) -> Option<WorkspaceAssociation<'_>> {
        let contains =
            |location: Location| location.span.start <= offset && offset < location.span.end;
        let (symbol, occurrence) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, symbol)| contains(symbol.location()))
            .map(|(index, symbol)| (index, symbol.location()))
            .or_else(|| {
                self.references
                    .iter()
                    .find(|reference| contains(reference.location))
                    .and_then(|reference| {
                        reference.symbol.map(|symbol| (symbol, reference.location))
                    })
            })?;
        let definition = &self.symbols[symbol];
        Some(WorkspaceAssociation {
            symbol: definition,
            occurrence,
            definition: definition.location(),
            references: self
                .references
                .iter()
                .filter(|reference| reference.symbol == Some(symbol))
                .map(|reference| reference.location)
                .collect(),
        })
    }

    pub(super) fn declaration_path(
        &self,
        kind: DeclarationKind,
        key: &Key,
    ) -> Option<&[PathSegment]> {
        self.symbols
            .iter()
            .position(|symbol| {
                symbol
                    .declaration()
                    .is_some_and(|symbol| symbol.kind == kind && &symbol.identity == key)
            })
            .and_then(|index| self.declaration_paths[index].as_deref())
    }

    pub(super) fn build(
        syntax: &ParsedToml<'_>,
        declaration: &Declaration,
        limits: toml::Limits,
    ) -> Result<Self, Error> {
        let mut entries = Vec::new();
        let mut key_bytes = 0usize;
        let mut add_declaration = |kind, id: &Key, path: Vec<PathSegment>| {
            if entries.len() >= limits.nodes {
                return Err(Error::Limit("workspace navigation occurrences"));
            }
            key_bytes = key_bytes
                .checked_add(id.as_str().len())
                .filter(|size| *size <= limits.source_bytes)
                .ok_or(Error::Limit("workspace navigation key bytes"))?;
            let location = string_location(syntax, &extend(&path, &[key("id")]), id)?;
            entries.push((
                WorkspaceSymbol::Declaration {
                    declaration: DeclarationSymbol {
                        kind,
                        identity: id.clone(),
                        location,
                    },
                },
                Some(path),
            ));
            Ok::<_, Error>(())
        };
        macro_rules! symbols {
            ($field:ident, $kind:ident) => {
                for (index, value) in declaration.$field.iter().enumerate() {
                    add_declaration(
                        DeclarationKind::$kind,
                        &value.id,
                        vec![key(stringify!($field)), PathSegment::Index(index)],
                    )?;
                }
            };
        }
        symbols!(brands, Brand);
        symbols!(principals, Principal);
        symbols!(accounts, Account);
        symbols!(sites, Site);
        for (index, value) in declaration.projects.iter().enumerate() {
            add_declaration(
                DeclarationKind::Project,
                value.id(),
                vec![key("projects"), PathSegment::Index(index)],
            )?;
        }
        symbols!(taxonomies, Taxonomy);
        symbols!(extensions, Extension);
        symbols!(sources, Source);
        for (index, site) in declaration.sites.iter().enumerate() {
            if site.explicit_brand
                && let Some(brand) = &site.brand
            {
                add_declaration(
                    DeclarationKind::Brand,
                    &brand.id,
                    vec![key("sites"), PathSegment::Index(index), key("brand")],
                )?;
            }
        }
        for name in declaration.design_inputs.keys() {
            if entries.len() >= limits.nodes {
                return Err(Error::Limit("workspace navigation occurrences"));
            }
            key_bytes = key_bytes
                .checked_add(name.as_str().len())
                .filter(|bytes| *bytes <= limits.source_bytes)
                .ok_or(Error::Limit("workspace navigation key bytes"))?;
            let (span, decoded) = syntax
                .key_at(&[key("design_inputs"), key(name.as_str())])
                .ok_or_else(invalid)?;
            if decoded != name.as_str() || span.start == span.end {
                return Err(invalid());
            }
            entries.push((
                WorkspaceSymbol::ResourceInput {
                    name: name.clone(),
                    location: Location {
                        span,
                        precision: Precision::Construct,
                    },
                },
                None,
            ));
        }
        entries.sort_by_key(|(symbol, _)| symbol.location().span.start);
        let (symbols, declaration_paths): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        let lookup: BTreeMap<_, _> = symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| ((symbol.kind(), symbol.name()), index))
            .collect();
        if lookup.len() != symbols.len() {
            // No first/last-wins proposal catalog from ambiguous declarations.
            return Err(invalid());
        }
        let mut references = References {
            syntax,
            lookup,
            values: Vec::new(),
            remaining: limits.nodes.saturating_sub(symbols.len()),
        };
        references.accounts()?;
        for (index, publisher) in declaration.publishers.iter().enumerate() {
            references.add(
                DeclarationKind::Principal,
                &publisher.id,
                &[key("publishers"), PathSegment::Index(index), key("id")],
            )?;
        }
        if let Some(build) = &declaration.build {
            for (index, site) in build.sites.iter().enumerate() {
                references.add(
                    DeclarationKind::Site,
                    site,
                    &[key("build"), key("sites"), PathSegment::Index(index)],
                )?;
            }
        }
        if let Some(policy) = &declaration.policy_source {
            references.add(
                DeclarationKind::Source,
                &policy.source,
                &[key("policy_source"), key("source")],
            )?;
        }
        for (name, source) in &declaration.design_inputs {
            references.add(
                DeclarationKind::Source,
                source,
                &[key("design_inputs"), key(name.as_str())],
            )?;
        }
        for (index, binding) in declaration.theme_packages.iter().enumerate() {
            for (target, mount) in &binding.resource_inputs {
                references.add_resource_input(
                    &mount.input,
                    &[
                        key("theme_packages"),
                        PathSegment::Index(index),
                        key("resource_inputs"),
                        key(target.as_str()),
                        key("input"),
                    ],
                )?;
            }
            references.add(
                DeclarationKind::Source,
                &binding.source,
                &[
                    key("theme_packages"),
                    PathSegment::Index(index),
                    key("source"),
                ],
            )?;
        }
        for (index, binding) in declaration.plugin_packages.iter().enumerate() {
            references.add(
                DeclarationKind::Source,
                &binding.source,
                &[
                    key("plugin_packages"),
                    PathSegment::Index(index),
                    key("source"),
                ],
            )?;
        }
        for (index, principal) in declaration.principals.iter().enumerate() {
            if let Some(gazette_core::publication::PrincipalWebsite::Site { site }) =
                &principal.website
            {
                references.add(
                    DeclarationKind::Site,
                    site,
                    &[
                        key("principals"),
                        PathSegment::Index(index),
                        key("website"),
                        key("site"),
                    ],
                )?;
            }
        }
        for (index, site) in declaration.sites.iter().enumerate() {
            let base = [key("sites"), PathSegment::Index(index)];
            references.add(
                DeclarationKind::Principal,
                &site.publisher,
                &extend(&base, &[key("publisher")]),
            )?;
            for (field, selected) in [
                ("rights_holder", site.rights_holder.as_ref()),
                ("default_author", site.default_author.as_ref()),
            ] {
                if let Some(selected) = selected {
                    references.add(
                        DeclarationKind::Principal,
                        selected,
                        &extend(&base, &[key(field)]),
                    )?;
                }
            }
            for (position, binding) in site.roles.iter().enumerate() {
                references.add(
                    DeclarationKind::Principal,
                    &binding.principal,
                    &extend(
                        &base,
                        &[key("roles"), PathSegment::Index(position), key("principal")],
                    ),
                )?;
            }
        }
        for (index, project) in declaration.projects.iter().enumerate() {
            if let Some(upstream) = project
                .inline()
                .and_then(|project| project.origin.as_ref())
                .and_then(|origin| origin.upstream())
            {
                references.add(
                    DeclarationKind::Project,
                    upstream,
                    &[
                        key("projects"),
                        PathSegment::Index(index),
                        key("origin"),
                        key("upstream"),
                    ],
                )?;
            }
        }
        for (index, mount) in declaration.documentation.mounts.iter().enumerate() {
            let base = [
                key("documentation"),
                key("mounts"),
                PathSegment::Index(index),
            ];
            references.add(
                DeclarationKind::Site,
                &mount.site,
                &extend(&base, &[key("site")]),
            )?;
            references.add(
                DeclarationKind::Source,
                &mount.source,
                &extend(&base, &[key("source")]),
            )?;
        }
        for (index, placement) in declaration.placements.iter().enumerate() {
            let base = [key("placements"), PathSegment::Index(index)];
            references.add(
                DeclarationKind::Site,
                &placement.site,
                &extend(&base, &[key("site")]),
            )?;
            references.add(
                DeclarationKind::Project,
                &placement.project,
                &extend(&base, &[key("project")]),
            )?;
            if let Some(binding) = &placement.documents {
                references.add(
                    DeclarationKind::Source,
                    &binding.source,
                    &extend(&base, &[key("documents"), key("source")]),
                )?;
            }
        }
        for (brand_index, brand) in declaration.brands.iter().enumerate() {
            if let Some(parent) = &brand.parent {
                references.add(
                    DeclarationKind::Brand,
                    parent,
                    &[
                        key("brands"),
                        PathSegment::Index(brand_index),
                        key("parent"),
                    ],
                )?;
            }
        }
        for (index, site) in declaration.sites.iter().enumerate() {
            if let Some(parent) = site.brand.as_ref().and_then(|brand| brand.parent.as_ref()) {
                references.add(
                    DeclarationKind::Brand,
                    parent,
                    &[
                        key("sites"),
                        PathSegment::Index(index),
                        key("brand"),
                        key("parent"),
                    ],
                )?;
            }
        }
        let publications = declaration
            .brands
            .iter()
            .enumerate()
            .flat_map(|(brand_index, brand)| {
                brand.publications.iter().enumerate().map(
                    move |(publication_index, publication)| {
                        (
                            publication,
                            vec![
                                key("brands"),
                                PathSegment::Index(brand_index),
                                key("publications"),
                                PathSegment::Index(publication_index),
                            ],
                            true,
                        )
                    },
                )
            })
            .chain(
                declaration
                    .sites
                    .iter()
                    .enumerate()
                    .flat_map(|(index, site)| {
                        site.brand.iter().flat_map(move |brand| {
                            brand.publications.iter().map(move |publication| {
                                (
                                    publication,
                                    vec![key("sites"), PathSegment::Index(index)],
                                    false,
                                )
                            })
                        })
                    }),
            );
        for (publication, base, explicit_site) in publications {
            if explicit_site {
                references.add(
                    DeclarationKind::Site,
                    &publication.site,
                    &extend(&base, &[key("site")]),
                )?;
            }
            for (binding_index, binding) in publication.authored.iter().enumerate() {
                references.add(
                    DeclarationKind::Source,
                    &binding.source,
                    &extend(
                        &base,
                        &[
                            key("authored"),
                            PathSegment::Index(binding_index),
                            key("source"),
                        ],
                    ),
                )?;
            }
            if let Some(build) = publication
                .documentation_repository
                .as_ref()
                .and_then(|repository| repository.build.as_ref())
            {
                for (module_index, module) in build.modules.iter().enumerate() {
                    let module_path = extend(
                        &base,
                        &[
                            key("documentation_repository"),
                            key("build"),
                            key("modules"),
                            PathSegment::Index(module_index),
                        ],
                    );
                    references.add(
                        DeclarationKind::Project,
                        &module.project,
                        &extend(&module_path, &[key("project")]),
                    )?;
                    if let Some(manual) = &module.manual {
                        references.add(
                            DeclarationKind::Source,
                            &manual.source,
                            &extend(&module_path, &[key("manual"), key("source")]),
                        )?;
                    }
                    for (item_index, item) in module.navigation.iter().enumerate() {
                        if let Some((_, _, input)) = item.reference() {
                            references.add(
                                DeclarationKind::Source,
                                input.source(),
                                &extend(
                                    &module_path,
                                    &[
                                        key("navigation"),
                                        PathSegment::Index(item_index),
                                        key("input"),
                                        key("source"),
                                    ],
                                ),
                            )?;
                        }
                    }
                }
            }
        }
        references
            .values
            .sort_by_key(|reference| reference.location.span.start);
        let values = references.values;
        drop(references.lookup);
        Ok(Self {
            symbols,
            declaration_paths,
            references: values,
            key_bytes,
        })
    }
}

impl WorkspaceDocumentsNavigation {
    pub(super) fn build<'s>(
        documents: impl IntoIterator<Item = (EntryPath, WorkspaceNavigation, &'s str)>,
        limits: toml::SetLimits,
    ) -> Result<Self, Error> {
        let mut symbols = Vec::new();
        let mut references = Vec::new();
        let mut key_bytes = 0usize;
        for (document, navigation, source) in documents {
            let source_sha256 = Sha256Digest::of(source.as_bytes());
            key_bytes = key_bytes
                .checked_add(navigation.key_bytes)
                .filter(|size| *size <= limits.source_bytes)
                .ok_or(Error::Limit("workspace navigation key bytes"))?;
            for symbol in navigation.symbols {
                let replacement = symbol.replacement(source)?.into_owned();
                symbols.push(DocumentDeclarationSymbol {
                    document: document.clone(),
                    source_sha256,
                    symbol,
                    replacement,
                });
            }
            references.extend(navigation.references.into_iter().map(|reference| {
                DocumentReference {
                    document: document.clone(),
                    source_sha256,
                    reference,
                }
            }));
        }
        if symbols.len().saturating_add(references.len()) > limits.nodes {
            return Err(Error::Limit("workspace navigation occurrences"));
        }
        symbols.sort_by(|left, right| {
            left.document.cmp(&right.document).then_with(|| {
                left.symbol
                    .location()
                    .span
                    .cmp(&right.symbol.location().span)
            })
        });
        let lookup: BTreeMap<_, _> = symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| {
                (
                    (symbol.symbol.kind(), symbol.symbol.name().to_owned()),
                    index,
                )
            })
            .collect();
        if lookup.len() != symbols.len() {
            return Err(invalid());
        }
        for reference in &mut references {
            reference.reference.symbol = lookup
                .get(&(
                    reference.reference.kind,
                    reference.reference.identity.clone(),
                ))
                .copied();
        }
        references.sort_by(|left, right| {
            left.document.cmp(&right.document).then_with(|| {
                left.reference
                    .location
                    .span
                    .cmp(&right.reference.location.span)
            })
        });
        Ok(Self {
            symbols,
            references,
            key_bytes,
        })
    }

    /// Locate one uniquely occurring unresolved typed identity across the current set.
    pub fn unique_unresolved_reference(&self, key: &Key) -> Option<DocumentSourceLocation<'_>> {
        let mut matches = self
            .references
            .iter()
            .filter(|value| value.reference.identity == key.as_str());
        let reference = matches.next()?;
        if matches.next().is_some() || reference.reference.symbol.is_some() {
            return None;
        }
        Some(DocumentSourceLocation {
            document: &reference.document,
            source_sha256: reference.source_sha256,
            location: reference.reference.location,
        })
    }

    /// Declarations owned by one exact selected document, in native source order.
    pub fn symbols<'a>(
        &'a self,
        document: &EntryPath,
    ) -> impl Iterator<Item = &'a WorkspaceSymbol> + 'a {
        let document = document.clone();
        self.symbols
            .iter()
            .filter(move |symbol| symbol.document == document)
            .map(|symbol| &symbol.symbol)
    }

    /// Locate one typed reference in one exact selected document.
    pub fn reference_at(
        &self,
        document: &EntryPath,
        offset: usize,
    ) -> Option<DocumentWorkspaceReference<'_>> {
        let reference = self.references.iter().find(|reference| {
            &reference.document == document
                && reference.reference.location.span.start <= offset
                && offset < reference.reference.location.span.end
        })?;
        Some(DocumentWorkspaceReference {
            kind: reference.reference.kind,
            symbol: reference.reference.symbol.map(|index| &self.symbols[index]),
            location: DocumentSourceLocation {
                document: &reference.document,
                source_sha256: reference.source_sha256,
                location: Location {
                    precision: Precision::Exact,
                    ..reference.reference.location
                },
            },
            identity_sha256: reference.reference.identity_sha256,
            unique_identity: self
                .references
                .iter()
                .filter(|candidate| {
                    candidate.reference.kind == reference.reference.kind
                        && candidate.reference.identity_sha256
                            == reference.reference.identity_sha256
                })
                .count()
                == 1,
            symbols: &self.symbols,
        })
    }

    /// Resolve a declaration/reference occurrence across the selected document set.
    pub fn at(
        &self,
        document: &EntryPath,
        offset: usize,
    ) -> Option<DocumentWorkspaceAssociation<'_>> {
        let contains =
            |location: Location| location.span.start <= offset && offset < location.span.end;
        let (symbol, occurrence) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, symbol)| &symbol.document == document && contains(symbol.symbol.location()))
            .map(|(index, symbol)| {
                (
                    index,
                    DocumentSourceLocation {
                        document: &symbol.document,
                        source_sha256: symbol.source_sha256,
                        location: symbol.symbol.location(),
                    },
                )
            })
            .or_else(|| {
                self.references
                    .iter()
                    .find(|reference| {
                        &reference.document == document && contains(reference.reference.location)
                    })
                    .and_then(|reference| {
                        reference.reference.symbol.map(|symbol| {
                            (
                                symbol,
                                DocumentSourceLocation {
                                    document: &reference.document,
                                    source_sha256: reference.source_sha256,
                                    location: reference.reference.location,
                                },
                            )
                        })
                    })
            })?;
        let definition = &self.symbols[symbol];
        Some(DocumentWorkspaceAssociation {
            symbol: definition,
            occurrence,
            definition: DocumentSourceLocation {
                document: &definition.document,
                source_sha256: definition.source_sha256,
                location: definition.symbol.location(),
            },
            references: self
                .references
                .iter()
                .filter(|reference| reference.reference.symbol == Some(symbol))
                .map(|reference| DocumentSourceLocation {
                    document: &reference.document,
                    source_sha256: reference.source_sha256,
                    location: reference.reference.location,
                })
                .collect(),
        })
    }

    /// Stored declaration and reference occurrences across the complete set.
    pub fn occurrence_count(&self) -> usize {
        self.symbols.len() + self.references.len()
    }

    /// Aggregate retained decoded declaration-key bytes.
    pub fn key_bytes(&self) -> usize {
        self.key_bytes
    }
}

struct References<'a, 's> {
    syntax: &'a ParsedToml<'s>,
    lookup: BTreeMap<(WorkspaceSymbolKind, &'a str), usize>,
    values: Vec<Reference>,
    remaining: usize,
}

impl References<'_, '_> {
    fn add(
        &mut self,
        kind: DeclarationKind,
        identity: &Key,
        path: &[PathSegment],
    ) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(Error::Limit("workspace navigation occurrences"))?;
        let kind = WorkspaceSymbolKind::Declaration(kind);
        let symbol = self.lookup.get(&(kind, identity.as_str())).copied();
        let location = string_location(self.syntax, path, identity)?;
        self.values.push(Reference {
            kind,
            identity: identity.as_str().to_owned(),
            symbol,
            identity_sha256: Sha256Digest::of(identity.as_str().as_bytes()),
            location,
        });
        Ok(())
    }

    fn add_resource_input(
        &mut self,
        name: &ResourceInputName,
        path: &[PathSegment],
    ) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(Error::Limit("workspace navigation occurrences"))?;
        let (span, decoded) = self.syntax.string_at(path).ok_or_else(invalid)?;
        if decoded != name.as_str() || span.start == span.end {
            return Err(invalid());
        }
        let kind = WorkspaceSymbolKind::ResourceInput;
        self.values.push(Reference {
            kind,
            identity: name.as_str().to_owned(),
            symbol: self.lookup.get(&(kind, name.as_str())).copied(),
            identity_sha256: Sha256Digest::of(name.as_str().as_bytes()),
            location: Location {
                span,
                precision: Precision::Construct,
            },
        });
        Ok(())
    }
}

fn key(value: &str) -> PathSegment {
    PathSegment::Key(value.to_owned())
}

fn string_location(
    syntax: &ParsedToml<'_>,
    path: &[PathSegment],
    identity: &Key,
) -> Result<Location, Error> {
    let (span, decoded) = syntax.string_at(path).ok_or_else(invalid)?;
    if decoded != identity.as_str() || span.start == span.end {
        return Err(invalid());
    }
    // Existing navigation retains construct precision; only reference replacement
    // explicitly projects the independently checked complete scalar as Exact.
    Ok(Location {
        span,
        precision: Precision::Construct,
    })
}
fn extend(base: &[PathSegment], fields: &[PathSegment]) -> Vec<PathSegment> {
    base.iter().chain(fields).cloned().collect()
}
fn invalid() -> Error {
    Error::Toml(toml::Error::Invalid {
        message: "workspace navigation has no exact native declaration binding or value range"
            .into(),
        span: None,
    })
}
