//! FAQ data borrows the selected question argument and the same admitted answer
//! fragment sent to its template. Templates cannot supply a second answer source.

use std::{collections::BTreeMap, sync::Mutex};

use gazette_core::{
    markdown::{ComponentBinding, Error, Fragment},
    source::Span,
};
use gazette_ports::markdown::ComponentRenderer;
use serde::Serialize;

use super::ComponentPresentation;
use crate::theme::ComponentSemantic;

mod text;

/// One declared FAQ group, in authored question order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Faq {
    /// Existing compiler occurrence address, absent only in unhosted utilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Questions and answers from this group's own admitted component children.
    pub questions: Vec<FaqQuestion>,
}

/// A single authored question and its resolved Markdown answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct FaqQuestion {
    /// Plain text supplied by the declared question argument.
    pub question: String,
    /// Exact admitted answer fragment, including resolved links and formatting.
    pub answer_html: String,
}

struct Group<'a> {
    binding: &'a ComponentBinding,
    items: Vec<Span>,
}
struct Item<'a> {
    binding: &'a ComponentBinding,
    question: &'a str,
}
struct Captured {
    remaining: usize,
    answers: BTreeMap<Span, String>,
}

pub(super) struct Collector<'a> {
    selected: ComponentPresentation<'a>,
    groups: BTreeMap<Span, Group<'a>>,
    items: BTreeMap<Span, Item<'a>>,
    captured: Mutex<Captured>,
}

fn failure(binding: &ComponentBinding, reason: &'static str) -> Error {
    Error::Unsupported {
        location: binding.occurrence.location,
        reason,
    }
}
fn encloses(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

impl<'a> Collector<'a> {
    pub(super) fn new(
        selected: ComponentPresentation<'a>,
        bindings: &'a [ComponentBinding],
        maximum: usize,
    ) -> Result<Self, Error> {
        let mut result = Self {
            selected,
            groups: BTreeMap::new(),
            items: BTreeMap::new(),
            captured: Mutex::new(Captured {
                remaining: maximum,
                answers: BTreeMap::new(),
            }),
        };
        let mut ordered: Vec<_> = bindings.iter().collect();
        ordered.sort_by_key(|binding| binding.occurrence.location.span);
        let mut stack: Vec<(&ComponentBinding, Option<&ComponentSemantic>)> = Vec::new();
        for binding in ordered {
            let span = binding.occurrence.location.span;
            while stack
                .last()
                .is_some_and(|(parent, _)| !encloses(parent.occurrence.location.span, span))
            {
                stack.pop();
            }
            let definition = selected
                .definitions
                .definition(&binding.name)
                .ok_or(Error::InvalidPlan)?;
            let semantic = definition.semantic.as_ref().filter(|semantic| {
                matches!(
                    semantic,
                    ComponentSemantic::Faq | ComponentSemantic::FaqItem { .. }
                )
            });
            if semantic.is_some() && stack.iter().any(|(parent, _)| parent.presentation_only) {
                return Err(failure(
                    binding,
                    "FAQ content cannot be inside a presentation-only component",
                ));
            }
            let parent = stack.iter().rev().find(|(_, semantic)| semantic.is_some());
            match semantic {
                Some(ComponentSemantic::Faq) => {
                    if parent.is_some() {
                        return Err(failure(
                            binding,
                            "FAQ groups cannot be nested inside FAQ groups or answers",
                        ));
                    }
                    result.groups.insert(
                        span,
                        Group {
                            binding,
                            items: Vec::new(),
                        },
                    );
                }
                Some(ComponentSemantic::FaqItem { question_argument }) => {
                    let Some((parent, Some(ComponentSemantic::Faq))) = parent else {
                        return Err(failure(
                            binding,
                            "FAQ items require an enclosing declared FAQ group",
                        ));
                    };
                    let question = binding
                        .arguments
                        .get(question_argument)
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| {
                            failure(
                                binding,
                                "FAQ question argument must contain nonempty plain text",
                            )
                        })?;
                    let captured = result.captured.get_mut().map_err(|_| Error::InvalidPlan)?;
                    captured.remaining = captured
                        .remaining
                        .checked_sub(question.len())
                        .ok_or(Error::Limit("FAQ projection bytes"))?;
                    result
                        .groups
                        .get_mut(&parent.occurrence.location.span)
                        .ok_or(Error::InvalidPlan)?
                        .items
                        .push(span);
                    result.items.insert(span, Item { binding, question });
                }
                _ => {}
            }
            stack.push((binding, semantic));
        }
        if let Some(group) = result.groups.values().find(|group| group.items.is_empty()) {
            return Err(failure(
                group.binding,
                "a declared FAQ group requires at least one question and answer",
            ));
        }
        Ok(result)
    }

    pub(super) fn finish(self) -> Result<BTreeMap<Span, Faq>, Error> {
        let mut captured = self.captured.into_inner().map_err(|_| Error::InvalidPlan)?;
        let mut faqs = BTreeMap::new();
        for (span, group) in self.groups {
            let mut questions = Vec::new();
            for item in group.items {
                let item = self.items.get(&item).ok_or(Error::InvalidPlan)?;
                let answer_html = captured
                    .answers
                    .remove(&item.binding.occurrence.location.span)
                    .ok_or(Error::InvalidPlan)?;
                questions.push(FaqQuestion {
                    question: item.question.into(),
                    answer_html,
                });
            }
            faqs.insert(
                span,
                Faq {
                    address: group
                        .binding
                        .context
                        .as_ref()
                        .map(|context| context.address.clone()),
                    questions,
                },
            );
        }
        if !captured.answers.is_empty() {
            return Err(Error::InvalidPlan);
        }
        Ok(faqs)
    }
}

impl ComponentRenderer for Collector<'_> {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error> {
        let output = self.selected.renderer.render(binding, body, excerpt)?;
        if !excerpt && self.items.contains_key(&binding.occurrence.location.span) {
            if body.html.trim().is_empty() {
                return Err(failure(binding, "FAQ answers require an admitted body"));
            }
            let mut captured = self.captured.lock().map_err(|_| Error::InvalidPlan)?;
            captured.remaining = captured
                .remaining
                .checked_sub(body.html.len())
                .ok_or(Error::Limit("FAQ projection bytes"))?;
            if captured
                .answers
                .insert(binding.occurrence.location.span, body.html.clone())
                .is_some()
            {
                return Err(Error::InvalidPlan);
            }
        }
        Ok(output)
    }
}

/// Check an actual FAQ template against its admitted body and declared question.
/// HTML formatting and whitespace may differ; added, removed or changed words fail.
/// This is decoded-text agreement, not browser visibility or search eligibility.
///
/// # Errors
/// Refuses malformed or oversized HTML, a missing question, or wording disagreement.
pub fn validate_faq_html(
    binding: &ComponentBinding,
    semantic: &ComponentSemantic,
    body: &Fragment,
    rendered: &str,
    limits: crate::html::Limits,
) -> Result<(), Error> {
    fn append(output: &mut String, value: &str, maximum: usize) -> Result<(), ()> {
        for word in value.split_whitespace() {
            let required = word.len().saturating_add(usize::from(!output.is_empty()));
            if required > maximum.saturating_sub(output.len()) {
                return Err(());
            }
            if !output.is_empty() {
                output.push(' ');
            }
            output.push_str(word);
        }
        Ok(())
    }
    let text = |html: &str| -> Result<String, Error> {
        let raw = text::decode(html, limits).map_err(|()| {
            failure(
                binding,
                "FAQ template text could not be checked within the HTML bounds",
            )
        })?;
        let mut text = String::new();
        append(&mut text, &raw, limits.decoded_bytes)
            .map_err(|()| Error::Limit("FAQ comparison bytes"))?;
        Ok(text)
    };
    let mut expected = String::new();
    if let ComponentSemantic::FaqItem { question_argument } = semantic {
        let question = binding
            .arguments
            .get(question_argument)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                failure(
                    binding,
                    "FAQ question argument must contain nonempty plain text",
                )
            })?;
        append(&mut expected, question, limits.decoded_bytes)
            .map_err(|()| Error::Limit("FAQ comparison bytes"))?;
    }
    append(&mut expected, &text(&body.html)?, limits.decoded_bytes)
        .map_err(|()| Error::Limit("FAQ comparison bytes"))?;
    if text(rendered)? != expected {
        return Err(failure(
            binding,
            "FAQ template wording differs from its authored question and answer",
        ));
    }
    Ok(())
}
