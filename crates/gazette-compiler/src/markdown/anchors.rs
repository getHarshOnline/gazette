//! Compiler-owned anchor allocation, shared by HTML and semantic navigation.

use super::{AnchorPolicy, Budget, Error, ParsedMarkdown, Span};
use gazette_core::markdown::{FootnoteBinding, HeadingBinding};
use gazette_core::source::Precision;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const FRAGMENT_ESCAPE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// An emitted heading with the exact compiler-owned public anchor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Heading {
    /// Heading level from one through six.
    #[schemars(range(min = 1, max = 6))]
    pub level: u8,
    /// Text/code/alt content, without footnote labels.
    pub text: String,
    /// Explicit or allocated DOM ID, not publication identity.
    pub id: String,
    /// URL-ready fragment encoding this exact ID.
    pub fragment: String,
    /// Nearest preceding lower-level emitted heading.
    #[schemars(range(max = usize::MAX))]
    pub parent: Option<usize>,
    /// Original containing heading construct in body coordinates.
    pub span: Span,
    /// Parser location precision; a containing heading is not an exact token edit.
    pub precision: Precision,
}

pub(super) struct Anchors {
    pub headings: Vec<Heading>,
    pub bindings: Vec<HeadingBinding>,
    pub footnotes: Vec<FootnoteBinding>,
    pub continuation: Option<String>,
    ids: BTreeSet<String>,
}

impl Anchors {
    pub(super) fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    // Heading and footnote prefixes put the namespace in different positions.
    // One final coordinate map both rejects collisions and rewrites local links.
    pub(super) fn namespaced(
        &self,
        namespace: &str,
        syntax: &gazette_core::markdown::Syntax,
        limit: usize,
    ) -> Result<BTreeMap<String, String>, Error> {
        let mut ids = BTreeSet::new();
        let mut mapping = BTreeMap::new();
        let mut budget = Budget(limit, "namespaced anchor string bytes");
        let mut insert = |original: String, final_id: String, span| {
            budget.charge(original.len())?;
            budget.charge(final_id.len())?;
            if !ids.insert(final_id.clone()) || mapping.insert(original, final_id).is_some() {
                Err(Error::Anchor {
                    span,
                    reason: "embedding namespace causes an emitted anchor collision",
                })
            } else {
                Ok(())
            }
        };
        for heading in &self.headings {
            insert(
                heading.id.clone(),
                format!("{namespace}{}", heading.id),
                heading.span,
            )?;
        }
        let references = reference_index(syntax);
        for note in &self.footnotes {
            if note.occurrence.references != 0 {
                insert(
                    format!("fn-{}", note.alias),
                    format!("fn-{namespace}{}", note.alias),
                    note.occurrence.location.span,
                )?;
            }
            for reference in references
                .get(note.occurrence.name.as_str())
                .into_iter()
                .flatten()
            {
                let suffix = if reference.number > 1 {
                    format!("-{}", reference.number)
                } else {
                    String::new()
                };
                insert(
                    format!("fnref-{}{suffix}", note.alias),
                    format!("fnref-{namespace}{}{suffix}", note.alias),
                    reference.location.span,
                )?;
            }
        }
        if let Some(id) = &self.continuation {
            let span = syntax
                .html
                .first()
                .map(|h| h.location.span)
                .ok_or(Error::InvalidOutput)?;
            insert(id.clone(), format!("{namespace}{id}"), span)?;
        }
        Ok(mapping)
    }
}

fn reference_index(
    syntax: &gazette_core::markdown::Syntax,
) -> BTreeMap<&str, Vec<&gazette_core::markdown::FootnoteReference>> {
    let mut references = BTreeMap::<_, Vec<_>>::new();
    for reference in syntax.footnote_references.iter().filter(|r| r.emitted) {
        references
            .entry(reference.name.as_str())
            .or_default()
            .push(reference);
    }
    references
}

impl ParsedMarkdown<'_> {
    /// Borrow the single emitted-heading allocation shared with formatting.
    ///
    /// # Errors
    /// Rejects invalid/duplicate explicit IDs, unsupported attributes or exhausted bounds.
    pub fn headings(&self) -> Result<&[Heading], Error> {
        Ok(&self.anchor_index()?.headings)
    }

    /// Source headings allocated by their publication or manual owner.
    ///
    /// # Errors
    /// Rejects the selected owner's invalid declarations and exhausted output bounds.
    pub fn headings_with_policy(&self, policy: AnchorPolicy) -> Result<&[Heading], Error> {
        Ok(&self.anchor_index_with_policy(policy)?.headings)
    }

    /// Test a decoded fragment against actual emitted heading, footnote and reference IDs.
    ///
    /// # Errors
    /// Returns the same allocation failure as heading inspection.
    pub fn contains_anchor(&self, id: &str) -> Result<bool, Error> {
        Ok(self.anchor_index()?.ids.contains(id))
    }

    pub(crate) fn has_anchor_prefix(&self, prefix: &str) -> Result<bool, Error> {
        Ok(self
            .anchor_index()?
            .ids
            .iter()
            .any(|id| id.starts_with(prefix)))
    }

    pub(super) fn anchor_index(&self) -> Result<&Anchors, Error> {
        self.anchors
            .get_or_init(|| self.collect_anchors())
            .as_ref()
            .map_err(Clone::clone)
    }

    pub(super) fn anchor_index_with_policy(&self, policy: AnchorPolicy) -> Result<&Anchors, Error> {
        match policy {
            AnchorPolicy::Publication => self.anchor_index(),
            AnchorPolicy::Manual => self
                .manual_anchors
                .get_or_init(|| self.collect_manual_anchors())
                .as_ref()
                .map_err(Clone::clone),
        }
    }

    fn collect_manual_anchors(&self) -> Result<Anchors, Error> {
        let manual = gazette_core::documentation::projection::manual_anchors(
            self.document.syntax(),
            "manual",
        )
        .map_err(|failure| Error::Anchor {
            span: failure.location.span,
            reason: match failure.error {
                gazette_core::documentation::Error::Invalid { reason, .. } => reason,
                _ => "manual anchor declarations are invalid",
            },
        })?;
        let mut budget = Budget(self.limits.output_bytes, "manual anchor bytes");
        for id in &manual.ids {
            budget.charge(std::mem::size_of::<String>() + id.len())?;
        }
        let mut headings = Vec::<Heading>::new();
        let mut parents = Vec::<usize>::new();
        for binding in &manual.headings {
            let fact = &binding.occurrence;
            budget
                .charge(std::mem::size_of::<Heading>() + std::mem::size_of::<HeadingBinding>())?;
            budget.charge(binding.id.len() + fact.text.len())?;
            while parents
                .last()
                .is_some_and(|i| headings[*i].level >= binding.level)
            {
                parents.pop();
            }
            let mut fragment = budget.copy("#")?;
            for part in utf8_percent_encode(&binding.id, FRAGMENT_ESCAPE) {
                budget.push(&mut fragment, part)?;
            }
            headings.push(Heading {
                level: binding.level,
                text: budget.copy(&fact.text)?,
                id: budget.copy(&binding.id)?,
                fragment,
                parent: parents.last().copied(),
                span: fact.location.span,
                precision: fact.location.precision,
            });
            parents.push(headings.len() - 1);
        }
        Ok(Anchors {
            headings,
            bindings: manual.headings,
            footnotes: Vec::new(),
            continuation: None,
            ids: manual.ids,
        })
    }

    fn collect_anchors(&self) -> Result<Anchors, Error> {
        let syntax = self.document.syntax();
        let summary = self.summary_cut()?;
        let mut budget = Budget(self.limits.output_bytes, "anchor string bytes");
        let mut reserved = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for heading in &syntax.headings {
            let span = heading.location.span;
            if !heading.attributes.is_empty() || heading.classes.iter().any(|c| !valid_token(c)) {
                return Err(Error::Anchor {
                    span,
                    reason: "only valid heading IDs and classes are supported",
                });
            }
            if let Some(id) = &heading.id {
                if !valid_token(id) {
                    return Err(Error::Anchor {
                        span,
                        reason: "anchor ID must be nonempty without whitespace or controls",
                    });
                }
                if !reserved.insert(budget.copy(id)?) {
                    return Err(Error::Anchor {
                        span,
                        reason: "duplicate explicit heading ID",
                    });
                }
            }
        }
        let mut footnotes = Vec::new();
        let mut labels = BTreeSet::new();
        let references = reference_index(syntax);
        for note in &syntax.footnotes {
            if !labels.insert(note.name.as_str()) {
                return Err(Error::Anchor {
                    span: note.location.span,
                    reason: "duplicate footnote association label",
                });
            }
            // Injective UTF-8 hex avoids formatter percent/href differences without
            // excluding Unicode authored labels or reconstructing generic numbering.
            let mut alias = String::from("note-");
            budget.charge(alias.len())?;
            for byte in note.name.as_bytes() {
                use std::fmt::Write;
                budget.charge(2)?;
                write!(&mut alias, "{byte:02x}").map_err(|_| Error::InvalidOutput)?;
            }
            let mut reserve = |id: String| {
                if !reserved.insert(budget.copy(&id)?) {
                    return Err(Error::Anchor {
                        span: note.location.span,
                        reason: "explicit ID collides with a generated footnote anchor",
                    });
                }
                ids.insert(budget.copy(&id)?);
                Ok(())
            };
            if note.references != 0 {
                reserve(format!("fn-{alias}"))?;
            }
            for reference in references.get(note.name.as_str()).into_iter().flatten() {
                reserve(if reference.number > 1 {
                    format!("fnref-{alias}-{}", reference.number)
                } else {
                    format!("fnref-{alias}")
                })?;
            }
            footnotes.push(FootnoteBinding {
                occurrence: note.clone(),
                alias,
            });
        }
        let mut headings = Vec::<Heading>::new();
        let mut bindings = Vec::new();
        let mut parents = Vec::<usize>::new();
        let mut suffixes = BTreeMap::new();
        for fact in &syntax.headings {
            let id = if let Some(id) = &fact.id {
                budget.copy(id)?
            } else {
                let base = slug::slugify(&fact.text);
                allocate_id(
                    if base.is_empty() { "section" } else { &base },
                    &mut reserved,
                    &mut suffixes,
                    &mut budget,
                )?
            };
            bindings.push(HeadingBinding {
                occurrence: fact.clone(),
                id: id.clone(),
                level: fact.level,
            });
            if !fact.emitted {
                continue;
            }
            ids.insert(budget.copy(&id)?);
            while parents
                .last()
                .is_some_and(|i| headings[*i].level >= fact.level)
            {
                parents.pop();
            }
            let mut fragment = budget.copy("#")?;
            for part in utf8_percent_encode(&id, FRAGMENT_ESCAPE) {
                budget.push(&mut fragment, part)?;
            }
            headings.push(Heading {
                level: fact.level,
                text: budget.copy(&fact.text)?,
                id,
                fragment,
                parent: parents.last().copied(),
                span: fact.location.span,
                precision: fact.location.precision,
            });
            parents.push(headings.len() - 1);
        }
        let continuation = summary
            .map(|_| {
                allocate_id(
                    "continue-reading",
                    &mut reserved,
                    &mut suffixes,
                    &mut budget,
                )
            })
            .transpose()?;
        if let Some(id) = &continuation {
            ids.insert(budget.copy(id)?);
        }
        Ok(Anchors {
            headings,
            bindings,
            footnotes,
            continuation,
            ids,
        })
    }
}

fn allocate_id(
    base: &str,
    ids: &mut BTreeSet<String>,
    suffixes: &mut BTreeMap<String, usize>,
    budget: &mut Budget,
) -> Result<String, Error> {
    // Charge retained counter keys, returned IDs and membership-index copies.
    // Temporary library allocations are input-bounded, not a memory sandbox.
    let next = if let Some(next) = suffixes.get_mut(base) {
        next
    } else {
        suffixes.entry(budget.copy(base)?).or_default()
    };
    loop {
        let candidate = if *next == 0 {
            base.to_owned()
        } else {
            format!("{base}-{next}")
        };
        *next = next
            .checked_add(1)
            .ok_or(Error::Limit("anchor suffix counter"))?;
        if !ids.contains(&candidate) {
            budget.charge(candidate.len())?;
            ids.insert(budget.copy(&candidate)?);
            return Ok(candidate);
        }
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}
