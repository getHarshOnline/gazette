//! Source-bound publication crossings feed authoring without another resolver.

use gazette_core::{
    markdown::LinkForm,
    source::{EntryPath, Location, Precision, Span},
};

use super::{ProjectionError, ResolvedAuthored};
use crate::{corpus::File, markdown};

/// A proved native publication crossing, private to selected-source authoring.
/// Both locations are full-file constructs, never inferred replacement ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicationReference<'a> {
    /// Exact retained source containing the native wikilink.
    pub path: &'a EntryPath,
    /// Original full-file containing link construct.
    pub location: Location,
    /// Exact retained definition source, including an article notes companion.
    pub target: &'a EntryPath,
    /// Native full-file heading or declaration construct.
    pub definition: Location,
}

/// One current selected publication destination and its retained native definition.
/// This is an inert authoring candidate, not discovery or publication approval.
#[derive(Clone, Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SymbolicTarget {
    /// Destination-only replacement in the existing publication-selector language.
    pub replacement: String,
    /// Existing content identity; commentary anchors retain their article owner.
    pub content: gazette_core::publication::Key,
    /// Declared publication home, independent from preview addresses.
    pub site: gazette_core::publication::Key,
    /// Literal page title or native heading text, never inferred copy.
    pub title: String,
    /// Exact selected declaration/heading source, including notes companions.
    pub path: EntryPath,
    /// Whole current target preimage, not normalized Markdown.
    pub source_sha256: gazette_core::source::Sha256Digest,
    /// Full-source native definition, retaining its actual coordinate precision.
    pub definition: Location,
}

impl SymbolicTarget {
    /// Owned record and string bytes charged to the shared authoring fact budget.
    pub fn fact_bytes(&self) -> usize {
        [
            self.replacement.len(),
            self.content.as_str().len(),
            self.site.as_str().len(),
            self.title.len(),
            self.path.as_str().len(),
        ]
        .into_iter()
        .fold(std::mem::size_of::<Self>(), usize::saturating_add)
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Build complete current page/heading/notes candidates through native routing
    /// and reference-definition validation, without executing or acquiring anything.
    /// Invalid current links do not erase this independent repair substrate.
    ///
    /// # Errors
    /// Refuses ambiguous/unbound destinations, invalid native anchors and exhausted
    /// complete-result bounds. Never returns a successful prefix of the selected set.
    pub fn symbolic_completion_targets(
        &self,
        occurrence_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<SymbolicTarget>, ProjectionError> {
        let mut targets = Vec::new();
        let mut bytes = 0usize;
        let workspace = &self.content.compilation().workspace;
        let mut domains = std::collections::BTreeMap::<String, usize>::new();
        for site in workspace.sites() {
            if let Some(domain) = workspace
                .public_base_url(&site.id)
                .and_then(|base| url::Url::parse(base.as_str()).ok())
                .and_then(|url| url.host_str().map(str::to_owned))
            {
                *domains.entry(domain).or_default() += 1;
            }
        }
        for node in self.content.graph().nodes() {
            let Some(route) = &node.route else {
                continue;
            };
            let Some(base) = self
                .content
                .compilation()
                .workspace
                .public_base_url(&node.site)
            else {
                continue;
            };
            let url = url::Url::parse(base.as_str()).map_err(|_| ProjectionError::Binding {
                id: node.id.clone(),
                reason: "configured symbolic publication base is invalid",
            })?;
            let Some(domain) = url.host_str() else {
                continue;
            };
            let mut destinations = vec![markdown::IdentityReference::destination(&node.id)];
            // Identity targets remain addressable when several publications share
            // a domain. Only offer a domain selector when it names one publication.
            if domains.get(domain) == Some(&1)
                && matches!(url::Host::parse(domain), Ok(url::Host::Domain(_)))
            {
                destinations.push(format!("site:{domain}{}", route.path.as_str()));
            }
            let main = self
                .file(&node.id)
                .ok_or_else(|| ProjectionError::Binding {
                    id: node.id.clone(),
                    reason: "symbolic target has no retained document",
                })?;
            let body = main.body().ok_or_else(|| ProjectionError::Binding {
                id: node.id.clone(),
                reason: "symbolic target has no retained Markdown",
            })?;
            let mut add = |replacement: String, title: &str| -> Result<(), ProjectionError> {
                let span = Span { start: 0, end: 0 };
                let resolved = self
                    .resolve_symbolic_reference(&replacement, span)
                    .map_err(|_| ProjectionError::Binding {
                        id: node.id.clone(),
                        reason: "symbolic completion target is ambiguous or unbound",
                    })?;
                if resolved.content.as_ref() != Some(&node.id) {
                    return Err(ProjectionError::Binding {
                        id: node.id.clone(),
                        reason: "symbolic completion target resolves to another owner",
                    });
                }
                let suffix = replacement
                    .find('#')
                    .map_or("", |index| &replacement[index..]);
                let (path, definition) = self
                    .reference_definition(&node.id, suffix, span)
                    .map_err(|_| ProjectionError::Binding {
                        id: node.id.clone(),
                        reason: "symbolic completion definition is unavailable",
                    })?;
                let file =
                    self.corpus
                        .files()
                        .get(path)
                        .ok_or_else(|| ProjectionError::Binding {
                            id: node.id.clone(),
                            reason: "symbolic completion source is absent",
                        })?;
                let target = SymbolicTarget {
                    replacement,
                    content: node.id.clone(),
                    site: node.site.clone(),
                    title: title.to_owned(),
                    path: path.clone(),
                    source_sha256: file.source_sha256(),
                    definition,
                };
                if targets.len() >= occurrence_limit {
                    return Err(ProjectionError::Limit("symbolic completion targets"));
                }
                bytes = bytes
                    .checked_add(target.fact_bytes())
                    .filter(|&bytes| bytes <= byte_limit)
                    .ok_or(ProjectionError::Limit("symbolic completion target bytes"))?;
                targets.push(target);
                Ok(())
            };
            for destination in destinations {
                add(destination.clone(), &node.title)?;
                for heading in body.headings().map_err(|_| ProjectionError::Binding {
                    id: node.id.clone(),
                    reason: "symbolic completion headings are invalid",
                })? {
                    add(format!("{destination}{}", heading.fragment), &heading.text)?;
                }
                if let Some(notes) = self
                    .page_package(&node.id)
                    .and_then(|package| package.notes())
                    .and_then(|notes| self.corpus.files().get(notes.path()))
                    .and_then(File::body)
                {
                    for heading in notes.headings().map_err(|_| ProjectionError::Binding {
                        id: node.id.clone(),
                        reason: "symbolic completion notes headings are invalid",
                    })? {
                        let fragment = heading.fragment.strip_prefix('#').ok_or_else(|| {
                            ProjectionError::Binding {
                                id: node.id.clone(),
                                reason: "native heading fragment lacks its marker",
                            }
                        })?;
                        add(
                            format!("{destination}#{}{fragment}", super::notes::NAMESPACE),
                            &heading.text,
                        )?;
                    }
                }
            }
        }
        targets.sort_by(|left, right| left.replacement.cmp(&right.replacement));
        Ok(targets)
    }

    /// Validate and retain symbolic crossings from article bodies and their notes.
    /// Includes omitted source occurrences so unresolved references cannot hide in
    /// unused footnotes. No resources execute and no publication approval is supplied.
    ///
    /// # Errors
    /// Returns exact source-bound Markdown failures, complete-result ceilings or
    /// an invalid retained-source binding. Never returns a successful subset.
    pub fn publication_references(
        &self,
        occurrence_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<PublicationReference<'_>>, ProjectionError> {
        let mut references = Vec::new();
        let mut bytes = 0usize;
        let mut inspected = std::collections::BTreeSet::new();
        for node in self.content.graph().nodes() {
            let Some(path) = self.source_path(&node.id) else {
                continue;
            };
            let main = self.corpus.files().get_key_value(path);
            let notes = self
                .page_package(&node.id)
                .and_then(|package| package.notes())
                .and_then(|notes| self.corpus.files().get_key_value(notes.path()));
            for (path, file) in main.into_iter().chain(notes) {
                if !inspected.insert(path) {
                    // Multiple legal outputs borrow one source occurrence, not copies.
                    continue;
                }
                let Some(body) = file.body() else { continue };
                let failure = |error| ProjectionError::body(path, file, error);
                for link in body
                    .source_links()
                    .iter()
                    .filter(|link| link.form == LinkForm::WikiLink)
                {
                    let resolved = self
                        .resolve_symbolic_reference(&link.destination, link.location.span)
                        .map_err(&failure)?;
                    let target = resolved
                        .content
                        .as_ref()
                        .ok_or_else(|| failure(markdown::Error::InvalidOutput))?;
                    let suffix = resolved
                        .destination
                        .find('#')
                        .map_or("", |index| &resolved.destination[index..]);
                    let (target, definition) = self
                        .reference_definition(target, suffix, link.location.span)
                        .map_err(&failure)?;
                    let location = lift(file, link.location).map_err(&failure)?;
                    if references.len() >= occurrence_limit {
                        return Err(ProjectionError::Limit("publication reference occurrences"));
                    }
                    bytes = bytes
                        .checked_add(std::mem::size_of::<PublicationReference<'_>>())
                        .and_then(|n| n.checked_add(path.as_str().len()))
                        .and_then(|n| n.checked_add(target.as_str().len()))
                        .filter(|&n| n <= byte_limit)
                        .ok_or(ProjectionError::Limit("publication reference bytes"))?;
                    references.push(PublicationReference {
                        path,
                        location,
                        target,
                        definition,
                    });
                }
            }
        }
        references.sort_by(|left, right| {
            left.path
                .cmp(right.path)
                .then(left.location.span.cmp(&right.location.span))
        });
        Ok(references)
    }

    pub(super) fn reference_definition(
        &self,
        target: &gazette_core::publication::Key,
        suffix: &str,
        span: Span,
    ) -> Result<(&EntryPath, Location), markdown::Error> {
        let failure = |reason| markdown::Error::LinkResolution { span, reason };
        let path = self
            .source_path(target)
            .ok_or_else(|| failure("target has no retained source association"))?;
        let (path, file) = self
            .corpus
            .files()
            .get_key_value(path)
            .ok_or_else(|| failure("document target has no retained source"))?;
        let body = file
            .body()
            .ok_or_else(|| failure("document target has no retained Markdown body"))?;
        let Some((_, fragment)) = suffix
            .split_once('#')
            .filter(|(_, fragment)| !fragment.is_empty())
        else {
            let location = match file {
                File::Landing(landing) => Location {
                    span: landing
                        .span_at(&[crate::toml::PathSegment::Key("content".into())])
                        .ok_or(markdown::Error::InvalidOutput)?,
                    precision: Precision::Construct,
                },
                File::Document(document) => Location {
                    span: document.metadata_span(),
                    precision: Precision::Construct,
                },
                _ => lift(
                    file,
                    Location {
                        span: Span {
                            start: 0,
                            end: body.source().len(),
                        },
                        precision: Precision::Construct,
                    },
                )?,
            };
            return Ok((path, location));
        };
        let anchor = markdown::decode_reference(fragment, span)?;
        if body
            .contains_anchor(&anchor)
            .map_err(|_| failure("target document anchor index is invalid"))?
        {
            return Ok((path, anchor_location(file, &anchor)?));
        }
        let logical = notes_anchor(&anchor);
        if let Some(logical) = logical
            && let Some((path, file)) = self
                .page_package(target)
                .and_then(|package| package.notes())
                .and_then(|notes| self.corpus.files().get_key_value(notes.path()))
            && let Some(body) = file.body()
            && body
                .contains_anchor(&logical)
                .map_err(|_| failure("target notes anchor index is invalid"))?
        {
            return Ok((path, anchor_location(file, &logical)?));
        }
        Err(failure(
            "fragment is absent from the target document anchor index",
        ))
    }
}

fn notes_anchor(anchor: &str) -> Option<String> {
    anchor
        .strip_prefix(super::notes::NAMESPACE)
        .map(str::to_owned)
        .or_else(|| {
            anchor
                .strip_prefix("fn-")
                .and_then(|rest| rest.strip_prefix(super::notes::NAMESPACE))
                .map(|rest| format!("fn-{rest}"))
        })
        .or_else(|| {
            anchor
                .strip_prefix("fnref-")
                .and_then(|rest| rest.strip_prefix(super::notes::NAMESPACE))
                .map(|rest| format!("fnref-{rest}"))
        })
}

fn anchor_location(file: &File<'_>, anchor: &str) -> Result<Location, markdown::Error> {
    let body = file.body().ok_or(markdown::Error::InvalidOutput)?;
    let location = body
        .headings()?
        .iter()
        .find(|heading| heading.id == anchor)
        .map_or(
            Location {
                span: Span {
                    start: 0,
                    end: body.source().len(),
                },
                precision: Precision::Construct,
            },
            |heading| Location {
                span: heading.span,
                precision: heading.precision,
            },
        );
    lift(file, location)
}

fn lift(file: &File<'_>, location: Location) -> Result<Location, markdown::Error> {
    file.body_location(location)
        .ok_or(markdown::Error::InvalidOutput)
}
