//! Retained selected-document edges; compiler owns paths, syntax and anchor resolution.

use std::collections::{BTreeMap, BTreeSet};

use gazette_compiler::corpus;
use gazette_core::source::{EntryPath, Location, Span};

use super::{
    Analysis, Error, Limits, Navigation, NavigationError, SourceLocation, charge, navigation,
};

#[derive(Debug)]
pub(super) struct Reference {
    path: EntryPath,
    location: Location,
    target: EntryPath,
    definition: Location,
}

pub(super) fn retain(
    corpus: &corpus::ParsedCorpus<'_>,
    sources: &BTreeMap<EntryPath, &[u8]>,
    manuals: &BTreeSet<EntryPath>,
    limits: Limits,
    occurrences: &mut usize,
    bytes: &mut usize,
) -> Result<Result<Vec<Reference>, corpus::Error>, Error> {
    let references = match corpus.document_references_with_manuals(
        manuals,
        limits.navigation_occurrences.saturating_sub(*occurrences),
        limits.navigation_bytes.saturating_sub(*bytes),
    ) {
        Ok(references) => references,
        Err(corpus::Error::Limit(resource)) => return Err(Error::Limit(resource)),
        Err(error) => return Ok(Err(error)),
    };
    let lift = |path: &EntryPath, location: Location| -> Result<Location, Error> {
        let source = std::str::from_utf8(sources.get(path).ok_or(Error::InvalidSource)?)
            .map_err(|_| Error::InvalidSource)?;
        let mapped = corpus
            .files()
            .get(path)
            .and_then(|file| file.body_location(location))
            .ok_or(Error::InvalidSource)?;
        mapped.span.slice(source).ok_or(Error::InvalidSource)?;
        Ok(mapped)
    };
    references
        .into_iter()
        .map(|reference| {
            charge_reference(
                reference.path,
                reference.target,
                std::mem::size_of::<corpus::DocumentReference<'_>>(),
                limits,
                occurrences,
                bytes,
            )?;
            Ok(Reference {
                path: reference.path.clone(),
                location: lift(reference.path, reference.location)?,
                target: reference.target.clone(),
                definition: lift(reference.target, reference.definition)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Ok)
}

/// Consume compiler-proved full-file coordinates without a second offset lift,
/// symbolic parser or route lookup. Both endpoint preimages remain selected.
pub(super) fn append_compiler_references<'a>(
    links: &mut Vec<Reference>,
    references: impl IntoIterator<Item = (&'a EntryPath, Location, &'a EntryPath, Location)>,
    sources: &BTreeMap<EntryPath, &[u8]>,
    limits: Limits,
    occurrences: &mut usize,
    bytes: &mut usize,
) -> Result<(), Error> {
    let validate = |path: &EntryPath, location: Location| -> Result<(), Error> {
        let source = std::str::from_utf8(sources.get(path).ok_or(Error::InvalidSource)?)
            .map_err(|_| Error::InvalidSource)?;
        location.span.slice(source).ok_or(Error::InvalidSource)?;
        Ok(())
    };
    for (path, location, target, definition) in references {
        validate(path, location)?;
        validate(target, definition)?;
        charge_reference(
            path,
            target,
            std::mem::size_of::<(&EntryPath, Location, &EntryPath, Location)>(),
            limits,
            occurrences,
            bytes,
        )?;
        links.push(Reference {
            path: path.clone(),
            location,
            target: target.clone(),
            definition,
        });
    }
    links.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.location.span.cmp(&right.location.span))
            .then(left.target.cmp(&right.target))
            .then(left.definition.span.cmp(&right.definition.span))
    });
    Ok(())
}

fn charge_reference(
    path: &EntryPath,
    target: &EntryPath,
    record_bytes: usize,
    limits: Limits,
    occurrences: &mut usize,
    bytes: &mut usize,
) -> Result<(), Error> {
    charge(
        occurrences,
        1,
        limits.navigation_occurrences,
        "navigation occurrences",
    )?;
    for size in [record_bytes, path.as_str().len(), target.as_str().len()] {
        charge(bytes, size, limits.navigation_bytes, "navigation bytes")?;
    }
    Ok(())
}

impl Analysis {
    /// All compiler-resolved document and editorial links for one exact source.
    ///
    /// # Errors
    /// Refuses changed inputs, unavailable current resolution, ambiguous targets,
    /// or a complete result which exceeds the caller's byte ceiling.
    pub fn source_links<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        maximum: usize,
    ) -> Result<Vec<super::navigation::SourceLink<'a>>, NavigationError> {
        self.navigation_source(path, source, 0)?;
        let links = self
            .document_links
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?;
        let mut result = Vec::new();
        let mut bytes = 0usize;
        for link in links
            .iter()
            .chain(&self.editorial_links)
            .filter(|link| &link.path == path)
        {
            link.location
                .span
                .slice(source)
                .ok_or(NavigationError::InvalidSource)?;
            bytes = bytes
                .checked_add(std::mem::size_of::<super::navigation::SourceLink<'_>>())
                .and_then(|n| n.checked_add(link.target.as_str().len()))
                .filter(|n| *n <= maximum)
                .ok_or(NavigationError::Limit)?;
            result.push(super::navigation::SourceLink {
                occurrence: link.location,
                definition: self.source_location(&link.target, link.definition)?,
            });
        }
        result.sort_by(|a, b| {
            a.occurrence
                .span
                .cmp(&b.occurrence.span)
                .then(a.definition.path.cmp(b.definition.path))
        });
        result.dedup();
        if result.windows(2).any(|pair| {
            pair[0].occurrence == pair[1].occurrence && pair[0].definition != pair[1].definition
        }) {
            return Err(NavigationError::Ambiguous);
        }
        Ok(result)
    }

    fn source_location<'a>(
        &'a self,
        path: &EntryPath,
        location: Location,
    ) -> Result<SourceLocation<'a>, NavigationError> {
        let (path, digest) = self
            .inputs
            .get_key_value(path)
            .ok_or(NavigationError::InvalidSource)?;
        Ok(SourceLocation {
            path,
            source_sha256: *digest,
            location,
        })
    }

    pub(super) fn extend_document_references<'a>(
        &'a self,
        navigation: &mut Navigation<'a>,
    ) -> Result<(), NavigationError> {
        let links = self
            .document_links
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?;
        for link in links {
            if &link.target == navigation.definition.path
                && link.definition == navigation.definition.location
            {
                navigation
                    .references
                    .push(self.source_location(&link.path, link.location)?);
            }
        }
        order(&mut navigation.references);
        Ok(())
    }

    pub(super) fn document_navigation<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<Navigation<'a>>, NavigationError> {
        let links = self
            .document_links
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?;
        let Some(mut result) = self.reference_navigation(links, path, source, offset)? else {
            return Ok(None);
        };
        let target = result.definition.path;
        let location = result.definition.location;
        // Heading facts were checked against their complete source during analysis.
        // Rebase those retained coordinates, never read the target's native file.
        if let Some(headings) = self.headings.get(target) {
            let index = headings.values.iter().position(|heading| {
                rebase(
                    Location {
                        span: heading.span,
                        precision: heading.precision,
                    },
                    headings.body_span,
                )
                .is_ok_and(|value| value == location)
            });
            if let Some(index) = index {
                let local = self
                    .heading_links
                    .get(target)
                    .ok_or(NavigationError::Unavailable)?
                    .as_ref()
                    .map_err(|_| NavigationError::Unavailable)?;
                for link in local.iter().filter(|link| link.heading == index) {
                    result.references.push(
                        self.source_location(target, rebase(link.location, headings.body_span)?)?,
                    );
                }
            }
        }
        order(&mut result.references);
        Ok(Some(result))
    }

    pub(super) fn editorial_navigation<'a>(
        &'a self,
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<Navigation<'a>>, NavigationError> {
        self.reference_navigation(&self.editorial_links, path, source, offset)
    }

    fn reference_navigation<'a>(
        &'a self,
        links: &'a [Reference],
        path: &EntryPath,
        source: &str,
        offset: usize,
    ) -> Result<Option<Navigation<'a>>, NavigationError> {
        let (path, digest) = self.navigation_source(path, source, offset)?;
        let mut selected = None;
        for link in links {
            let target = (&link.target, link.definition);
            if &link.path == path {
                navigation::consider(&mut selected, target, link.location, offset)?;
            }
            if &link.target == path {
                navigation::consider(&mut selected, target, link.definition, offset)?;
            }
        }
        let Some(((target, location), occurrence)) = selected else {
            return Ok(None);
        };
        let definition = self.source_location(target, location)?;
        let mut result = Navigation {
            path,
            source_sha256: digest,
            occurrence,
            definition,
            references: Vec::new(),
        };
        for link in links {
            if &link.target == target && link.definition == location {
                result
                    .references
                    .push(self.source_location(&link.path, link.location)?);
            }
        }
        order(&mut result.references);
        Ok(Some(result))
    }
}

fn rebase(location: Location, body: Span) -> Result<Location, NavigationError> {
    let start = body
        .start
        .checked_add(location.span.start)
        .ok_or(NavigationError::InvalidSource)?;
    let end = body
        .start
        .checked_add(location.span.end)
        .filter(|end| *end <= body.end)
        .ok_or(NavigationError::InvalidSource)?;
    if start > end {
        return Err(NavigationError::InvalidSource);
    }
    Ok(Location {
        span: Span { start, end },
        precision: location.precision,
    })
}

fn order(locations: &mut Vec<SourceLocation<'_>>) {
    locations.sort_by(|left, right| {
        left.path
            .cmp(right.path)
            .then(left.location.span.cmp(&right.location.span))
    });
    locations.dedup();
}
