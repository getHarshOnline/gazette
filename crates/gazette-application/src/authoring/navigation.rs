//! Native selected-source associations, independent of rendered routes or editor protocol.

use std::fmt;

use gazette_compiler::{markdown::ParsedMarkdown, workspace};
use gazette_core::{
    markdown::{Footnote, FootnoteReference},
    source::{EntryPath, Location, Sha256Digest, Span},
};

use super::{Analysis, Error, Failure, Limits, WorkspaceNavigation, charge};

#[derive(Debug)]
pub(super) struct Footnotes {
    body_span: Span,
    definitions: Vec<Footnote>,
    references: Vec<FootnoteReference>,
}

/// One current-source association, not a rename edit or publication relationship.
#[derive(Debug, PartialEq, Eq)]
pub struct Navigation<'a> {
    /// Exact queried logical document.
    pub path: &'a EntryPath,
    /// Whole original source digest, including framing and line endings.
    pub source_sha256: Sha256Digest,
    /// Smallest containing native occurrence at the requested byte position.
    pub occurrence: Location,
    /// Unique native definition, preserving its actual source precision.
    pub definition: SourceLocation<'a>,
    /// Complete recognized references in logical-path/source order.
    pub references: Vec<SourceLocation<'a>>,
}

/// A source construct with its own exact selected preimage; never an inferred URI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct SourceLocation<'a> {
    /// Selected logical source containing this construct.
    pub path: &'a EntryPath,
    /// Complete source identity, including frontmatter and line endings.
    pub source_sha256: Sha256Digest,
    /// Original full-file source coordinates and precision.
    pub location: Location,
}

/// One compiler-resolved link in a selected source, without a protocol URI guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SourceLink<'a> {
    /// Original checked full-source construct containing the reference.
    pub occurrence: Location,
    /// Exact retained target and its original source coordinates.
    pub definition: SourceLocation<'a>,
}

/// One resolved workspace identity occurrence, not field documentation or an edit.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct WorkspaceIdentity<'a> {
    /// Exact selected workspace path.
    pub path: &'a EntryPath,
    /// Complete current workspace preimage, including quotes and line endings.
    pub source_sha256: Sha256Digest,
    /// Original queried construct; precision is never promoted to an editable token.
    pub occurrence: Location,
    /// Compiler-owned Key declaration or resource alias and its distinct definition span.
    pub symbol: &'a workspace::WorkspaceSymbol,
    /// Effective site selections and their native winning layers. Absent for other
    /// identity kinds; these are configured choices, not availability or approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<&'a workspace::ResolvedComposition>,
}

/// A navigation query cannot return trustworthy current-source evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationError {
    /// The complete response exceeds its caller-owned byte ceiling.
    Limit,
    /// Unknown path, changed source or an invalid UTF-8 byte position/range.
    InvalidSource,
    /// Current workspace/corpus parsing failed or an association has no definition.
    Unavailable,
    /// More than one definition or equally precise conflicting occurrence exists.
    Ambiguous,
}

impl fmt::Display for NavigationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Limit => "complete current-source response exceeds its byte ceiling",
            Self::InvalidSource => {
                "navigation requires the exact selected source and a valid byte position"
            }
            Self::Unavailable => {
                "current source navigation is unavailable; no earlier analysis is substituted"
            }
            Self::Ambiguous => "native source association is ambiguous; no target is chosen",
        })
    }
}

impl std::error::Error for NavigationError {}

impl Analysis {
    /// Exact origin of a uniquely occurring unresolved workspace reference.
    /// Ambiguous occurrences provide no source location and cannot authorize a repair.
    pub fn workspace_reference_origin(
        &self,
        key: &gazette_core::publication::Key,
    ) -> Option<SourceLocation<'_>> {
        match self.workspace_navigation.as_ref()? {
            WorkspaceNavigation::Buffer(_) => None,
            WorkspaceNavigation::Documents(navigation) => navigation
                .unique_unresolved_reference(key)
                .map(|origin| SourceLocation {
                    path: origin.document,
                    source_sha256: origin.source_sha256,
                    location: origin.location,
                }),
        }
    }

    pub(super) fn workspace_navigation_for(
        &self,
        path: &EntryPath,
        source: &str,
    ) -> Result<Option<&WorkspaceNavigation>, NavigationError> {
        if self.inputs.get(path) != Some(&Sha256Digest::of(source.as_bytes())) {
            return Err(NavigationError::InvalidSource);
        }
        if !self.workspace_documents.contains(path) {
            return Ok(None);
        }
        self.workspace_navigation
            .as_ref()
            .map(Some)
            .ok_or(NavigationError::Unavailable)
    }

    /// Inspect a declared workspace identity or typed reference in the current source.
    ///
    /// Names, arbitrary strings, comments and selected non-workspace sources return
    /// none. Unrelated corpus failure does not erase valid workspace facts. This
    /// does not inspect arbitrary fields, infer publication permission or copy a catalog.
    ///
    /// # Errors
    /// Refuses unknown/changed inputs, invalid UTF-8 positions and unavailable
    /// current workspace analysis; an earlier valid overlay is never substituted.
    pub fn workspace_identity(
        &self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<WorkspaceIdentity<'_>>, NavigationError> {
        if !source.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        let Some(navigation) = self.workspace_navigation_for(path, source)? else {
            return Ok(None);
        };
        let (path, source_sha256) = self
            .inputs
            .get_key_value(path)
            .ok_or(NavigationError::InvalidSource)?;
        if self.workspace_compilation.is_none() {
            return Err(NavigationError::Unavailable);
        }
        let (symbol, occurrence) = match navigation {
            WorkspaceNavigation::Buffer(navigation) => {
                let Some(value) = navigation.at(offset) else {
                    return Ok(None);
                };
                (value.symbol, value.occurrence)
            }
            WorkspaceNavigation::Documents(navigation) => {
                let Some(value) = navigation.at(path, offset) else {
                    return Ok(None);
                };
                (&value.symbol.symbol, value.occurrence.location)
            }
        };
        let composition = if let Some(declaration) = symbol
            .declaration()
            .filter(|value| value.kind == workspace::DeclarationKind::Site)
        {
            Some(
                self.workspace_compilation
                    .as_ref()
                    .and_then(|compiled| compiled.workspace.composition_for(&declaration.identity))
                    .ok_or(NavigationError::Unavailable)?,
            )
        } else {
            None
        };
        Ok(Some(WorkspaceIdentity {
            path,
            source_sha256: *source_sha256,
            occurrence,
            symbol,
            composition,
        }))
    }

    pub(super) fn navigation_source<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<(&'a EntryPath, Sha256Digest), NavigationError> {
        let (path, digest) = self
            .inputs
            .get_key_value(path)
            .ok_or(NavigationError::InvalidSource)?;
        if *digest != Sha256Digest::of(source.as_bytes()) || !source.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        if self
            .failures
            .iter()
            .any(|failure| matches!(failure, Failure::Corpus(_)))
        {
            return Err(NavigationError::Unavailable);
        }
        Ok((path, *digest))
    }

    /// Follow an allocated heading or recognized same-document fragment link.
    ///
    /// The compiler's emitted-heading allocation and strict reference resolver
    /// supply every association. No URI/path grammar or slug allocation is repeated
    /// here. Invalid current fragment evidence refuses the complete reference set;
    /// headings remain independently available as structural document symbols.
    ///
    /// # Errors
    /// Rejects unknown/changed source, invalid coordinates, failed current heading
    /// or reference analysis and ambiguous containing constructs.
    pub fn heading_navigation<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<Navigation<'a>>, NavigationError> {
        let (path, digest) = self.navigation_source(path, source, offset)?;
        let Some(headings) = self.headings.get(path) else {
            return if self.footnotes.contains_key(path) {
                Err(NavigationError::Unavailable)
            } else {
                Ok(None)
            };
        };
        let links = self
            .heading_links
            .get(path)
            .ok_or(NavigationError::Unavailable)?
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?;
        let mut selected = None;
        for (index, heading) in headings.values.iter().enumerate() {
            let location = lift(
                Location {
                    span: heading.span,
                    precision: heading.precision,
                },
                source,
                headings.body_span,
            )?;
            consider(&mut selected, index, location, offset)?;
        }
        for link in links {
            let location = lift(link.location, source, headings.body_span)?;
            consider(&mut selected, link.heading, location, offset)?;
        }
        let Some((index, occurrence)) = selected else {
            return Ok(None);
        };
        let heading = headings
            .values
            .get(index)
            .ok_or(NavigationError::Unavailable)?;
        let definition = lift(
            Location {
                span: heading.span,
                precision: heading.precision,
            },
            source,
            headings.body_span,
        )?;
        let mut references = links
            .iter()
            .filter(|link| link.heading == index)
            .map(|link| lift(link.location, source, headings.body_span))
            .collect::<Result<Vec<_>, _>>()?;
        references.sort_by_key(|v| v.span);
        references.dedup();
        Ok(Some(Navigation {
            path,
            source_sha256: digest,
            occurrence,
            definition: SourceLocation {
                path,
                source_sha256: digest,
                location: definition,
            },
            references: references
                .into_iter()
                .map(|location| SourceLocation {
                    path,
                    source_sha256: digest,
                    location,
                })
                .collect(),
        }))
    }

    /// Resolve a typed workspace reference, heading/link or footnote occurrence.
    ///
    /// Workspace navigation uses the compiler's original declaration coordinates,
    /// independently of content failure. No string matching or inference from
    /// names, paths, theme IDs or provider IDs establishes a reference.
    /// Native Markdown constructs can nest: a footnote reference in a heading must
    /// resolve to the note, while a fragment link inside a note must resolve to
    /// its heading. Equal ranges with different targets are ambiguous. Unavailable
    /// heading-link evidence does not erase an independently valid footnote target.
    ///
    /// # Errors
    /// Preserves the underlying source/analysis failures and refuses equal-range
    /// conflicting targets instead of imposing a protocol-specific precedence.
    pub fn source_navigation<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<Navigation<'a>>, NavigationError> {
        if self.workspace_documents.contains(path) {
            if !source.is_char_boundary(offset) {
                return Err(NavigationError::InvalidSource);
            }
            let navigation = self
                .workspace_navigation_for(path, source)?
                .ok_or(NavigationError::Unavailable)?;
            if self.workspace_compilation.is_none() {
                return Err(NavigationError::Unavailable);
            }
            let (path, source_sha256) = self
                .inputs
                .get_key_value(path)
                .ok_or(NavigationError::InvalidSource)?;
            return Ok(match navigation {
                WorkspaceNavigation::Buffer(navigation) => {
                    navigation.at(offset).map(|value| Navigation {
                        path: &self.workspace,
                        source_sha256: self.inputs[&self.workspace],
                        occurrence: value.occurrence,
                        definition: SourceLocation {
                            path: &self.workspace,
                            source_sha256: self.inputs[&self.workspace],
                            location: value.definition,
                        },
                        references: value
                            .references
                            .into_iter()
                            .map(|location| SourceLocation {
                                path: &self.workspace,
                                source_sha256: self.inputs[&self.workspace],
                                location,
                            })
                            .collect(),
                    })
                }
                WorkspaceNavigation::Documents(navigation) => {
                    navigation.at(path, offset).map(|value| Navigation {
                        path,
                        source_sha256: *source_sha256,
                        occurrence: value.occurrence.location,
                        definition: SourceLocation {
                            path: value.definition.document,
                            source_sha256: value.definition.source_sha256,
                            location: value.definition.location,
                        },
                        references: value
                            .references
                            .iter()
                            .map(|location| SourceLocation {
                                path: location.document,
                                source_sha256: location.source_sha256,
                                location: location.location,
                            })
                            .collect(),
                    })
                }
            });
        }
        self.navigation_source(path, source, offset)?;
        let editorial = self.editorial_navigation(path, source, offset)?;
        // A proved failure of the nested link must not navigate to its broader
        // containing footnote instead. Independent footnote labels remain usable.
        for failure in &self.failures {
            if let Failure::Renderability {
                path: failed,
                body_span,
                error: gazette_compiler::markdown::Error::LinkResolution { span, .. },
            } = failure
                && failed == path
            {
                let span = span
                    .in_parent(source, *body_span)
                    .ok_or(NavigationError::InvalidSource)?;
                if span.start <= offset && offset < span.end {
                    return Err(NavigationError::Unavailable);
                }
            }
        }
        let footnote = choose(self.footnote_navigation(path, source, offset)?, editorial)?;
        let heading = match self
            .heading_navigation(path, source, offset)
            .and_then(|mut heading| {
                if let Some(value) = &mut heading {
                    self.extend_document_references(value)?;
                }
                let document = self.document_navigation(path, source, offset)?;
                choose(heading, document)
            }) {
            Ok(heading) => heading,
            Err(NavigationError::Unavailable) if footnote.is_some() => return Ok(footnote),
            Err(error) => return Err(error),
        };
        choose(footnote, heading)
    }

    /// Resolve a native footnote occurrence against this exact analysis preimage.
    ///
    /// Native labels are joined only within the selected document. No token
    /// scanning, case-folding, URL decoding or rendered-anchor reconstruction
    /// occurs. Construct precision supports navigation, never an exact edit.
    /// Workspace/content constraint failures do not erase valid source structure;
    /// a corpus syntax failure does. A valid non-occurrence returns `None`.
    ///
    /// # Errors
    /// Refuses stale/unknown source, invalid coordinates, unavailable current
    /// syntax and ambiguous native associations without a partial reference set.
    pub fn footnote_navigation<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<Navigation<'a>>, NavigationError> {
        let (path, digest) = self.navigation_source(path, source, offset)?;
        let Some(notes) = self.footnotes.get(path) else {
            return Ok(None);
        };
        let mut selected: Option<(&str, Location)> = None;
        for (name, location) in notes
            .definitions
            .iter()
            .map(|v| (v.name.as_str(), v.location))
            .chain(
                notes
                    .references
                    .iter()
                    .map(|v| (v.name.as_str(), v.location)),
            )
        {
            let location = lift(location, source, notes.body_span)?;
            consider(&mut selected, name, location, offset)?;
        }
        let Some((name, occurrence)) = selected else {
            return Ok(None);
        };
        let mut definitions = notes.definitions.iter().filter(|v| v.name == name);
        let definition = definitions.next().ok_or(NavigationError::Unavailable)?;
        if definitions.next().is_some() {
            return Err(NavigationError::Ambiguous);
        }
        let definition = lift(definition.location, source, notes.body_span)?;
        let mut references = notes
            .references
            .iter()
            .filter(|v| v.name == name)
            .map(|v| lift(v.location, source, notes.body_span))
            .collect::<Result<Vec<_>, _>>()?;
        references.sort_by_key(|v| v.span);
        references.dedup();
        Ok(Some(Navigation {
            path,
            source_sha256: digest,
            occurrence,
            definition: SourceLocation {
                path,
                source_sha256: digest,
                location: definition,
            },
            references: references
                .into_iter()
                .map(|location| SourceLocation {
                    path,
                    source_sha256: digest,
                    location,
                })
                .collect(),
        }))
    }
}

pub(super) fn consider<T: Copy + Eq>(
    selected: &mut Option<(T, Location)>,
    target: T,
    location: Location,
    offset: usize,
) -> Result<(), NavigationError> {
    if !(location.span.start <= offset && offset < location.span.end) {
        return Ok(());
    }
    if let Some((previous, prior)) = *selected {
        let width = location.span.end - location.span.start;
        let old_width = prior.span.end - prior.span.start;
        if width > old_width {
            return Ok(());
        }
        if width == old_width && previous != target {
            return Err(NavigationError::Ambiguous);
        }
    }
    *selected = Some((target, location));
    Ok(())
}

fn choose<'a>(
    left: Option<Navigation<'a>>,
    right: Option<Navigation<'a>>,
) -> Result<Option<Navigation<'a>>, NavigationError> {
    match (left, right) {
        (None, result) | (result, None) => Ok(result),
        (Some(left), Some(right)) => {
            let width =
                |value: &Navigation<'_>| value.occurrence.span.end - value.occurrence.span.start;
            match width(&left).cmp(&width(&right)) {
                std::cmp::Ordering::Less => Ok(Some(left)),
                std::cmp::Ordering::Greater => Ok(Some(right)),
                std::cmp::Ordering::Equal if left == right => Ok(Some(left)),
                std::cmp::Ordering::Equal => Err(NavigationError::Ambiguous),
            }
        }
    }
}

fn lift(location: Location, source: &str, body: Span) -> Result<Location, NavigationError> {
    Ok(Location {
        span: location
            .span
            .in_parent(source, body)
            .ok_or(NavigationError::InvalidSource)?,
        precision: location.precision,
    })
}

pub(super) fn retain(
    body: &ParsedMarkdown<'_>,
    source: &str,
    body_span: Span,
    limits: Limits,
    count: &mut usize,
    bytes: &mut usize,
) -> Result<Footnotes, Error> {
    let definitions = body.footnote_definitions();
    let references = body.footnote_references();
    for (name, enclosing, location) in definitions
        .iter()
        .map(|v| (v.name.as_str(), None, v.location))
        .chain(
            references
                .iter()
                .map(|v| (v.name.as_str(), v.definition.as_deref(), v.location)),
        )
    {
        charge(
            count,
            1,
            limits.navigation_occurrences,
            "navigation occurrences",
        )?;
        charge(
            bytes,
            name.len(),
            limits.navigation_bytes,
            "navigation bytes",
        )?;
        charge(
            bytes,
            enclosing.map_or(0, str::len),
            limits.navigation_bytes,
            "navigation bytes",
        )?;
        lift(location, source, body_span).map_err(|_| Error::InvalidSource)?;
    }
    Ok(Footnotes {
        body_span,
        definitions: definitions.to_vec(),
        references: references.to_vec(),
    })
}
