//! Associations within the exact supplied corpus, independent of publication routes.

use std::collections::BTreeSet;

use gazette_core::source::{EntryPath, Location, Precision, Span};

use super::{Error, FileError, ParsedCorpus, charge};
use crate::markdown::{AnchorPolicy, LinkForm, LinkKind, source_path};

/// A native link and definition bound to two explicitly selected documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentReference<'a> {
    /// Selected source containing the reference.
    pub path: &'a EntryPath,
    /// Body-relative containing source link construct.
    pub location: Location,
    /// Exact selected target, never a guessed public route or host path.
    pub target: &'a EntryPath,
    /// Target-body-relative heading or whole body construct.
    pub definition: Location,
}

impl ParsedCorpus<'_> {
    /// Associate nonempty relative paths with exactly selected Markdown bodies.
    ///
    /// No filesystem discovery, extension inference or URL-to-source reconstruction
    /// occurs. Images, external/root-relative links, unselected paths and valid
    /// non-heading anchors have no association. Path-empty fragment links retain
    /// their separate document-local owner. Source precision is not upgraded.
    ///
    /// # Errors
    /// Malformed/escaping source paths, invalid selected-target anchors or exhausted
    /// result bounds refuse the complete relation set, never a successful subset.
    pub fn document_references(
        &self,
        occurrence_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<DocumentReference<'_>>, Error> {
        self.document_references_with_manuals(&BTreeSet::new(), occurrence_limit, byte_limit)
    }

    /// Resolve selected source links using manual anchor rules only for declared manuals.
    /// Ordinary publication sources retain their own anchor policy.
    ///
    /// # Errors
    /// Rejects unknown manual paths and the same complete-set failures as `document_references`.
    pub fn document_references_with_manuals(
        &self,
        manuals: &BTreeSet<EntryPath>,
        occurrence_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<DocumentReference<'_>>, Error> {
        if let Some(path) = manuals
            .iter()
            .find(|path| self.files.get(*path).and_then(super::File::body).is_none())
        {
            return Err(Error::Binding(path.clone()));
        }
        let mut references = Vec::new();
        let mut occurrences = 0;
        let mut bytes = 0;
        for (path, file) in &self.files {
            let Some(body) = file.body() else { continue };
            for link in body.source_links() {
                if link.kind != LinkKind::Link || link.form != LinkForm::Markdown {
                    continue;
                }
                let end = link
                    .destination
                    .find(['?', '#'])
                    .unwrap_or(link.destination.len());
                let relative = &link.destination[..end];
                if relative.is_empty() || source_path::is_external(relative) {
                    continue;
                }
                let fail = |error| Error::File {
                    path: path.clone(),
                    error: match file {
                        super::File::Document(document) => {
                            FileError::Document(crate::document::Error::Markdown {
                                error,
                                offset: document.body_span().start,
                            })
                        }
                        super::File::Landing(_) => {
                            FileError::Landing(crate::landing::Error::Markdown(error))
                        }
                        _ => FileError::Markdown(error),
                    },
                };
                let decoded = source_path::decode(relative, link.location.span).map_err(fail)?;
                let reduced = source_path::reduce(path.as_str(), decoded, link.location.span)
                    .map_err(fail)?;
                let Some((target, target_file)) = self.files.get_key_value(reduced.as_str()) else {
                    continue;
                };
                let Some(target_body) = target_file.body() else {
                    continue;
                };
                let policy = if manuals.contains(target) {
                    AnchorPolicy::Manual
                } else {
                    AnchorPolicy::Publication
                };
                let anchor = target_body
                    .fragment_anchor_with_policy(
                        &link.destination[end..],
                        link.location.span,
                        policy,
                    )
                    .map_err(fail)?;
                let definition = if let Some(anchor) = anchor {
                    let headings = target_body.headings_with_policy(policy).map_err(fail)?;
                    let Some(heading) = headings.iter().find(|heading| heading.id == anchor) else {
                        continue;
                    };
                    Location {
                        span: heading.span,
                        precision: heading.precision,
                    }
                } else {
                    Location {
                        span: Span {
                            start: 0,
                            end: target_body.source().len(),
                        },
                        precision: Precision::Construct,
                    }
                };
                charge(
                    &mut occurrences,
                    1,
                    occurrence_limit,
                    "document reference occurrences",
                )?;
                for size in [
                    std::mem::size_of::<DocumentReference<'_>>(),
                    path.as_str().len(),
                    target.as_str().len(),
                ] {
                    charge(&mut bytes, size, byte_limit, "document reference bytes")?;
                }
                references.push(DocumentReference {
                    path,
                    location: link.location,
                    target,
                    definition,
                });
            }
        }
        Ok(references)
    }
}
